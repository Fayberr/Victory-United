//! What every measurement here uses: queues whose submissions are timed on
//! the CPU and on the GPU, buffers, and shaders compiled from source.

use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail, ensure};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D::Fxc::*;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{Interface, PCSTR};

/// Lists one `timed` call can record.
pub const MAX_LISTS: usize = 128;
/// How long the GPU may take over one submission before it is an error.
const LIMIT: Duration = Duration::from_secs(60);

/// The CPU clock the GPU's timestamps are calibrated against, in ticks.
pub fn qpc() -> i64 {
    let mut t = 0;
    // SAFETY: fails on no supported system.
    let _ = unsafe { QueryPerformanceCounter(&mut t) };
    t
}

/// Microseconds between two `qpc` readings.
pub fn us(from: i64, to: i64) -> f64 {
    let mut frequency = 0;
    // SAFETY: fails on no supported system.
    let _ = unsafe { QueryPerformanceFrequency(&mut frequency) };
    (to - from) as f64 * 1e6 / frequency as f64
}

/// How a call's lists reach the queue.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Submit {
    /// All in one `ExecuteCommandLists`.
    Together,
    /// One `ExecuteCommandLists` each.
    Each,
    /// One each, recorded only once the CPU has waited for the one before:
    /// what code that reads the GPU's results back between passes does.
    EachAndWait,
}

/// Where one `timed` call's time went, in microseconds.
#[derive(Clone, Copy, Default)]
pub struct Timed {
    /// Recording the lists.
    pub record: f64,
    /// Inside `ExecuteCommandLists` and `Signal`.
    pub submit: f64,
    /// The CPU waiting for fences.
    pub wait: f64,
    /// All of it, first list's recording to last fence.
    pub wall: f64,
    /// Sum of the lists' own times on the GPU.
    pub gpu_busy: Option<f64>,
    /// First list's start to last list's end on the GPU, less `gpu_busy`:
    /// time the queue sat with nothing of this call to run.
    pub gpu_idle: Option<f64>,
    /// The first submission's call to the first list's start on the GPU.
    pub start_delay: Option<f64>,
    /// The last list's end on the GPU to the CPU having seen the fence.
    pub wake_delay: Option<f64>,
}

/// Ticks per second, and the timestamps' heap and readback buffer.
struct Stamps {
    frequency: f64,
    heap: ID3D12QueryHeap,
    into: ID3D12Resource,
}

/// A queue, with its fence and timestamps.
pub struct Queue {
    device: ID3D12Device,
    pub queue: ID3D12CommandQueue,
    kind: D3D12_COMMAND_LIST_TYPE,
    fence: ID3D12Fence,
    event: HANDLE,
    next: Cell<u64>,
    /// Absent where the queue has no timestamps.
    stamps: Option<Stamps>,
    lists: RefCell<Vec<(ID3D12CommandAllocator, ID3D12GraphicsCommandList)>>,
}

impl Drop for Queue {
    fn drop(&mut self) {
        // The lists and their allocators must outlive the GPU's use of them.
        let _ = self.idle();
        // SAFETY: an event this object created.
        let _ = unsafe { CloseHandle(self.event) };
    }
}

impl Queue {
    pub fn new(device: &ID3D12Device, kind: D3D12_COMMAND_LIST_TYPE) -> Result<Self> {
        // SAFETY: plain object creation.
        unsafe {
            let queue: ID3D12CommandQueue =
                device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                    Type: kind,
                    ..Default::default()
                })?;
            Ok(Self {
                stamps: stamps(device, &queue, kind)?,
                device: device.clone(),
                queue,
                kind,
                fence: device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?,
                event: CreateEventW(None, false, false, None)?,
                next: Cell::new(0),
                lists: RefCell::default(),
            })
        }
    }

    /// Signals the queue's fence behind everything submitted so far.
    pub fn signal(&self) -> Result<u64> {
        let value = self.next.get() + 1;
        self.next.set(value);
        // SAFETY: both objects are live.
        unsafe { self.queue.Signal(&self.fence, value)? };
        Ok(value)
    }

    /// Makes this queue wait on the GPU for what `other` signalled.
    pub fn wait_for(&self, other: &Queue, value: u64) -> Result<()> {
        // SAFETY: both objects are live.
        unsafe { self.queue.Wait(&other.fence, value)? };
        Ok(())
    }

    /// Waits on the CPU for a signalled value, through an event.
    pub fn wait(&self, value: u64) -> Result<()> {
        // SAFETY: the event and fence are live.
        unsafe {
            let started = Instant::now();
            while self.fence.GetCompletedValue() < value {
                self.fence.SetEventOnCompletion(value, self.event)?;
                if WaitForSingleObject(self.event, 100) != WAIT_OBJECT_0 {
                    alive(&self.device)?;
                    ensure!(
                        started.elapsed() < LIMIT,
                        "the GPU did not finish within {} seconds",
                        LIMIT.as_secs()
                    );
                }
            }
        }
        // A removed device completes every fence, with all bits set.
        alive(&self.device)
    }

    /// Waits on the CPU for a signalled value, polling.
    pub fn spin(&self, value: u64) -> Result<()> {
        let started = Instant::now();
        // SAFETY: the fence is live.
        while unsafe { self.fence.GetCompletedValue() } < value {
            std::hint::spin_loop();
            ensure!(
                started.elapsed() < LIMIT,
                "the GPU did not finish within {} seconds",
                LIMIT.as_secs()
            );
        }
        alive(&self.device)
    }

    pub fn idle(&self) -> Result<()> {
        let value = self.signal()?;
        self.wait(value)
    }

    /// List `n`, opened; made on first use. The GPU must be done with it.
    fn open(&self, n: usize) -> Result<ID3D12GraphicsCommandList> {
        let mut lists = self.lists.borrow_mut();
        // SAFETY: plain object creation, and resets of idle objects.
        unsafe {
            while lists.len() <= n {
                let allocator: ID3D12CommandAllocator =
                    self.device.CreateCommandAllocator(self.kind)?;
                let list: ID3D12GraphicsCommandList = self
                    .device
                    .CreateCommandList(0, self.kind, &allocator, None)?;
                list.Close()?;
                lists.push((allocator, list));
            }
            let (allocator, list) = &lists[n];
            allocator.Reset()?;
            list.Reset(allocator, None)?;
            Ok(list.clone())
        }
    }

    /// Records `count` lists with `record`, submits them and waits for them.
    pub fn timed(
        &self,
        count: usize,
        how: Submit,
        record: &mut dyn FnMut(usize, &ID3D12GraphicsCommandList) -> Result<()>,
    ) -> Result<Timed> {
        ensure!(
            (1..=MAX_LISTS).contains(&count),
            "{count} lists in one call"
        );
        // Made outside the timed part: a first call would pay for it.
        for n in 0..count {
            // SAFETY: the list was just opened.
            unsafe { self.open(n)?.Close()? };
        }
        let mut t = Timed::default();
        let mut first_submit = 0;
        let mut pending: Vec<Option<ID3D12CommandList>> = Vec::with_capacity(count);
        let (mut last, mut submitted) = (0, 0);
        let begin = qpc();
        // SAFETY: recording into open lists; every list, allocator and
        // resource lives until the waits below have passed.
        unsafe {
            for n in 0..count {
                let from = qpc();
                let list = self.open(n)?;
                let stamp = 2 * n as u32;
                if let Some(s) = &self.stamps {
                    list.EndQuery(&s.heap, D3D12_QUERY_TYPE_TIMESTAMP, stamp);
                }
                record(n, &list)?;
                if let Some(s) = &self.stamps {
                    list.EndQuery(&s.heap, D3D12_QUERY_TYPE_TIMESTAMP, stamp + 1);
                    list.ResolveQueryData(
                        &s.heap,
                        D3D12_QUERY_TYPE_TIMESTAMP,
                        stamp,
                        2,
                        &s.into,
                        8 * u64::from(stamp),
                    );
                }
                list.Close()?;
                pending.push(Some(list.cast()?));
                let closed = qpc();
                t.record += us(from, closed);
                if how == Submit::Together && n + 1 < count {
                    continue;
                }
                if first_submit == 0 {
                    first_submit = closed;
                }
                self.queue.ExecuteCommandLists(&pending);
                pending.clear();
                last = self.signal()?;
                submitted = qpc();
                t.submit += us(closed, submitted);
                if how == Submit::EachAndWait && n + 1 < count {
                    self.wait(last)?;
                    t.wait += us(submitted, qpc());
                }
            }
        }
        self.wait(last)?;
        let end = qpc();
        t.wait += us(submitted, end);
        t.wall = us(begin, end);

        if let Some(s) = &self.stamps {
            let ticks: Vec<u64> = read(&s.into, 2 * count)?;
            let gpu_us = |a: u64, b: u64| (b as i64 - a as i64) as f64 * 1e6 / s.frequency;
            let busy: f64 = ticks
                .chunks_exact(2)
                .map(|p| gpu_us(p[0], p[1]).max(0.0))
                .sum();
            let (first, final_) = (ticks[0], ticks[2 * count - 1]);
            t.gpu_busy = Some(busy);
            t.gpu_idle = Some((gpu_us(first, final_) - busy).max(0.0));
            // Where both clocks stood at one moment.
            let (mut gpu, mut cpu) = (0u64, 0u64);
            // SAFETY: two locals to write into.
            if unsafe { self.queue.GetClockCalibration(&mut gpu, &mut cpu) }.is_ok() {
                let cpu = cpu as i64;
                t.start_delay = Some(gpu_us(gpu, first) - us(cpu, first_submit));
                t.wake_delay = Some(us(cpu, end) - gpu_us(gpu, final_));
            }
        }
        Ok(t)
    }
}

fn stamps(
    device: &ID3D12Device,
    queue: &ID3D12CommandQueue,
    kind: D3D12_COMMAND_LIST_TYPE,
) -> Result<Option<Stamps>> {
    let copy = kind == D3D12_COMMAND_LIST_TYPE_COPY;
    // SAFETY: a feature query with its own structure, and object creation.
    unsafe {
        if copy {
            let mut o = D3D12_FEATURE_DATA_D3D12_OPTIONS3::default();
            let known = device.CheckFeatureSupport(
                D3D12_FEATURE_D3D12_OPTIONS3,
                (&raw mut o).cast(),
                size_of_val(&o) as u32,
            );
            if known.is_err() || !o.CopyQueueTimestampQueriesSupported.as_bool() {
                return Ok(None);
            }
        }
        let mut heap: Option<ID3D12QueryHeap> = None;
        device.CreateQueryHeap(
            &D3D12_QUERY_HEAP_DESC {
                Type: if copy {
                    D3D12_QUERY_HEAP_TYPE_COPY_QUEUE_TIMESTAMP
                } else {
                    D3D12_QUERY_HEAP_TYPE_TIMESTAMP
                },
                Count: 2 * MAX_LISTS as u32,
                NodeMask: 0,
            },
            &mut heap,
        )?;
        Ok(Some(Stamps {
            frequency: queue.GetTimestampFrequency()? as f64,
            heap: heap.context("no query heap")?,
            into: buffer(
                device,
                D3D12_HEAP_TYPE_READBACK,
                16 * MAX_LISTS as u64,
                D3D12_RESOURCE_FLAG_NONE,
            )?,
        }))
    }
}

/// An error naming the reason if the device is gone.
pub fn alive(device: &ID3D12Device) -> Result<()> {
    // SAFETY: a live device object, removed or not.
    unsafe { device.GetDeviceRemovedReason() }.map_err(|e| {
        anyhow!(
            "the device was removed: {} ({:#010x})",
            e.message(),
            e.code().0
        )
    })
}

const ONE_SAMPLE: DXGI_SAMPLE_DESC = DXGI_SAMPLE_DESC {
    Count: 1,
    Quality: 0,
};

pub fn buffer_desc(bytes: u64, flags: D3D12_RESOURCE_FLAGS) -> D3D12_RESOURCE_DESC {
    D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: bytes,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: ONE_SAMPLE,
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: flags,
        ..Default::default()
    }
}

pub fn texture_desc(side: u32, flags: D3D12_RESOURCE_FLAGS) -> D3D12_RESOURCE_DESC {
    D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(side),
        Height: side,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: ONE_SAMPLE,
        Flags: flags,
        ..Default::default()
    }
}

/// A committed resource in the state its heap starts in.
pub fn committed(
    device: &ID3D12Device,
    heap: D3D12_HEAP_TYPE,
    desc: &D3D12_RESOURCE_DESC,
) -> Result<ID3D12Resource> {
    let state = match heap {
        D3D12_HEAP_TYPE_UPLOAD => D3D12_RESOURCE_STATE_GENERIC_READ,
        D3D12_HEAP_TYPE_READBACK => D3D12_RESOURCE_STATE_COPY_DEST,
        _ => D3D12_RESOURCE_STATE_COMMON,
    };
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: a description that outlives the call.
    unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: heap,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            desc,
            state,
            None,
            &mut resource,
        )?;
    }
    resource.context("no resource")
}

pub fn buffer(
    device: &ID3D12Device,
    heap: D3D12_HEAP_TYPE,
    bytes: u64,
    flags: D3D12_RESOURCE_FLAGS,
) -> Result<ID3D12Resource> {
    committed(device, heap, &buffer_desc(bytes, flags))
}

/// A buffer in the system's memory that the CPU caches (a custom heap).
pub fn system_buffer(device: &ID3D12Device, bytes: u64) -> Result<ID3D12Resource> {
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: descriptions that outlive the call.
    unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_CUSTOM,
                CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_WRITE_BACK,
                MemoryPoolPreference: D3D12_MEMORY_POOL_L0,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &buffer_desc(bytes, D3D12_RESOURCE_FLAG_NONE),
            D3D12_RESOURCE_STATE_COMMON,
            None,
            &mut resource,
        )?;
    }
    resource.context("no resource")
}

/// A mapped buffer's bytes, unmapped when dropped.
pub struct Mapped<'a> {
    resource: &'a ID3D12Resource,
    pub at: *mut u8,
}

impl<'a> Mapped<'a> {
    pub fn new(resource: &'a ID3D12Resource) -> Result<Self> {
        let mut at = std::ptr::null_mut();
        // SAFETY: a buffer in a heap the CPU can reach.
        unsafe { resource.Map(0, None, Some(&mut at))? };
        Ok(Self {
            resource,
            at: at.cast(),
        })
    }
}

impl Drop for Mapped<'_> {
    fn drop(&mut self) {
        // SAFETY: mapped in `new`.
        unsafe { self.resource.Unmap(0, None) };
    }
}

/// The first `count` values of a readback buffer, which must hold them.
pub fn read<T: Copy>(from: &ID3D12Resource, count: usize) -> Result<Vec<T>> {
    let mapped = Mapped::new(from)?;
    // SAFETY: the size is the caller's to keep; a mapped buffer starts on a
    // page, so its data is aligned.
    Ok(unsafe { std::slice::from_raw_parts(mapped.at.cast::<T>(), count) }.to_vec())
}

/// Fills an upload buffer from its start, which must have the room.
pub fn write<T: Copy>(into: &ID3D12Resource, data: &[T]) -> Result<()> {
    let mapped = Mapped::new(into)?;
    // SAFETY: as `read`.
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), mapped.at.cast(), data.len()) };
    Ok(())
}

/// A state change of all of a resource. The barrier borrows `resource`
/// without counting a reference: it must not outlive it.
pub fn transition(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer, released by nobody.
                pResource: unsafe { std::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
        ..Default::default()
    }
}

/// A wait for one resource's unordered writes; borrows as `transition`.
pub fn uav_barrier(resource: &ID3D12Resource) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            UAV: std::mem::ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
                // SAFETY: as in `transition`.
                pResource: unsafe { std::mem::transmute_copy(resource) },
            }),
        },
        ..Default::default()
    }
}

pub fn bytes(blob: &ID3DBlob) -> &[u8] {
    // SAFETY: the blob owns these bytes.
    unsafe { std::slice::from_raw_parts(blob.GetBufferPointer().cast(), blob.GetBufferSize()) }
}

/// HLSL of our own, compiled with the compiler Windows ships.
pub fn compile(source: &str, entry: &str, target: &str) -> Result<Vec<u8>> {
    let (name, target) = (CString::new(entry)?, CString::new(target)?);
    let (mut code, mut errors) = (None, None);
    // SAFETY: pointers and lengths of live data.
    let done = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(name.as_ptr().cast()),
            PCSTR(target.as_ptr().cast()),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(e) = done {
        let why = errors
            .as_ref()
            .map(|b: &ID3DBlob| String::from_utf8_lossy(bytes(b)).into_owned())
            .unwrap_or_else(|| e.message());
        bail!("compiling `{entry}`: {why}");
    }
    Ok(bytes(&code.context("no code")?).to_vec())
}

pub fn root_signature(
    device: &ID3D12Device,
    parameters: &[D3D12_ROOT_PARAMETER],
) -> Result<ID3D12RootSignature> {
    let desc = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: parameters.len() as u32,
        pParameters: parameters.as_ptr(),
        ..Default::default()
    };
    let mut blob = None;
    // SAFETY: a description that outlives the call.
    unsafe {
        D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None)?;
        let blob = blob.context("no root signature")?;
        Ok(device.CreateRootSignature(0, bytes(&blob))?)
    }
}

pub fn root_constants(register: u32, dwords: u32) -> D3D12_ROOT_PARAMETER {
    D3D12_ROOT_PARAMETER {
        ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
        Anonymous: D3D12_ROOT_PARAMETER_0 {
            Constants: D3D12_ROOT_CONSTANTS {
                ShaderRegister: register,
                RegisterSpace: 0,
                Num32BitValues: dwords,
            },
        },
        ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    }
}

pub fn root_view(kind: D3D12_ROOT_PARAMETER_TYPE, register: u32) -> D3D12_ROOT_PARAMETER {
    D3D12_ROOT_PARAMETER {
        ParameterType: kind,
        Anonymous: D3D12_ROOT_PARAMETER_0 {
            Descriptor: D3D12_ROOT_DESCRIPTOR {
                ShaderRegister: register,
                RegisterSpace: 0,
            },
        },
        ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    }
}

fn code(b: &[u8]) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE {
        pShaderBytecode: b.as_ptr().cast(),
        BytecodeLength: b.len(),
    }
}

pub fn compute_pipeline(
    device: &ID3D12Device,
    root: &ID3D12RootSignature,
    cs: &[u8],
) -> Result<ID3D12PipelineState> {
    let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
        // SAFETY: a borrowed pointer the description never releases.
        pRootSignature: unsafe { std::mem::transmute_copy(root) },
        CS: code(cs),
        ..Default::default()
    };
    // SAFETY: the description points at data that outlives the call.
    Ok(unsafe { device.CreateComputePipelineState(&desc)? })
}

/// A graphics pipeline onto one R8G8B8A8 target, no depth; `cached` is the
/// blob of an earlier one, empty for none.
pub fn graphics_pipeline(
    device: &ID3D12Device,
    root: &ID3D12RootSignature,
    vs: &[u8],
    ps: &[u8],
    cached: &[u8],
) -> windows::core::Result<ID3D12PipelineState> {
    let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
        // SAFETY: a borrowed pointer the description never releases.
        pRootSignature: unsafe { std::mem::transmute_copy(root) },
        VS: code(vs),
        PS: code(ps),
        SampleMask: u32::MAX,
        RasterizerState: D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_SOLID,
            CullMode: D3D12_CULL_MODE_NONE,
            DepthClipEnable: true.into(),
            ..Default::default()
        },
        PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
        NumRenderTargets: 1,
        SampleDesc: ONE_SAMPLE,
        ..Default::default()
    };
    desc.RTVFormats[0] = DXGI_FORMAT_R8G8B8A8_UNORM;
    if !cached.is_empty() {
        desc.CachedPSO = D3D12_CACHED_PIPELINE_STATE {
            pCachedBlob: cached.as_ptr().cast(),
            CachedBlobSizeInBytes: cached.len(),
        };
    }
    // Additive, so that every drawn pixel reads the target.
    desc.BlendState.RenderTarget[0] = D3D12_RENDER_TARGET_BLEND_DESC {
        BlendEnable: true.into(),
        SrcBlend: D3D12_BLEND_ONE,
        DestBlend: D3D12_BLEND_ONE,
        BlendOp: D3D12_BLEND_OP_ADD,
        SrcBlendAlpha: D3D12_BLEND_ONE,
        DestBlendAlpha: D3D12_BLEND_ONE,
        BlendOpAlpha: D3D12_BLEND_OP_ADD,
        LogicOp: D3D12_LOGIC_OP_NOOP,
        RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
        ..Default::default()
    };
    // SAFETY: the description points at data that outlives the call.
    unsafe { device.CreateGraphicsPipelineState(&desc) }
}
