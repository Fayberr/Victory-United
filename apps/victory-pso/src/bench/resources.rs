//! Resources, views and barriers.

use anyhow::{Context, Result, ensure};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use super::gpu::{self, Submit, qpc, us};
use super::{Bench, Row};

const ADD: &str = "
RWByteAddressBuffer data : register(u0);
[numthreads(64, 1, 1)] void cs(uint i : SV_DispatchThreadID) {
    data.Store(4 * i, data.Load(4 * i) + 1);
}
";
const THREADS: usize = 64;

pub fn run(b: &mut Bench) -> Result<()> {
    let device = b.adapter.device.clone();
    let default = D3D12_HEAP_TYPE_DEFAULT;
    let count = b.size(256);

    for (name, heap, desc) in [
        (
            "committed 64 KiB buffer, made and released",
            default,
            gpu::buffer_desc(1 << 16, D3D12_RESOURCE_FLAG_NONE),
        ),
        (
            "committed 64 KiB upload buffer",
            D3D12_HEAP_TYPE_UPLOAD,
            gpu::buffer_desc(1 << 16, D3D12_RESOURCE_FLAG_NONE),
        ),
        (
            "committed 256x256 texture",
            default,
            gpu::texture_desc(256, D3D12_RESOURCE_FLAG_NONE),
        ),
        (
            "committed 2048x2048 render target",
            default,
            gpu::texture_desc(2048, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET),
        ),
    ] {
        let t = b.rounds(|| {
            let from = qpc();
            for _ in 0..count {
                gpu::committed(&device, heap, &desc)?;
            }
            Ok(us(from, qpc()))
        })?;
        b.row(Row::new("resources", name, "resources", count).cpu(&t));
    }

    let each = u64::from(D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT);
    let mut heap: Option<ID3D12Heap> = None;
    // SAFETY: a description that outlives the call.
    unsafe {
        device.CreateHeap(
            &D3D12_HEAP_DESC {
                SizeInBytes: each * count as u64,
                Properties: D3D12_HEAP_PROPERTIES {
                    Type: default,
                    ..Default::default()
                },
                Flags: D3D12_HEAP_FLAG_ALLOW_ONLY_BUFFERS,
                ..Default::default()
            },
            &mut heap,
        )?;
    }
    let heap = heap.context("no heap")?;
    let desc = gpu::buffer_desc(each, D3D12_RESOURCE_FLAG_NONE);
    let t = b.rounds(|| {
        let from = qpc();
        for n in 0..count as u64 {
            let mut made: Option<ID3D12Resource> = None;
            // SAFETY: within the heap; a description that outlives the call.
            unsafe {
                device.CreatePlacedResource(
                    &heap,
                    n * each,
                    &desc,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut made,
                )?;
            }
        }
        Ok(us(from, qpc()))
    })?;
    b.row(
        Row::new(
            "resources",
            "placed 64 KiB buffer in a heap",
            "resources",
            count,
        )
        .cpu(&t),
    );

    // Views, written and copied.
    let views = b.size(10_000);
    let make = |visible| -> Result<ID3D12DescriptorHeap> {
        // SAFETY: plain object creation.
        Ok(unsafe {
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                NumDescriptors: views as u32,
                Flags: if visible {
                    D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE
                } else {
                    D3D12_DESCRIPTOR_HEAP_FLAG_NONE
                },
                ..Default::default()
            })?
        })
    };
    let (staged, visible) = (make(false)?, make(true)?);
    // SAFETY: plain queries on live objects.
    let (step, staged_at, visible_at) = unsafe {
        (
            device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV)
                as usize,
            staged.GetCPUDescriptorHandleForHeapStart(),
            visible.GetCPUDescriptorHandleForHeapStart(),
        )
    };
    let slot = |at: D3D12_CPU_DESCRIPTOR_HANDLE, n: usize| D3D12_CPU_DESCRIPTOR_HANDLE {
        ptr: at.ptr + n * step,
    };
    let source = gpu::buffer(&device, default, 1 << 16, D3D12_RESOURCE_FLAG_NONE)?;
    let srv = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32_UINT,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV {
                NumElements: 1 << 14,
                ..Default::default()
            },
        },
    };
    let t = b.rounds(|| {
        let from = qpc();
        for n in 0..views {
            // SAFETY: a slot of the heap; nothing on the GPU reads it.
            unsafe { device.CreateShaderResourceView(&source, Some(&srv), slot(staged_at, n)) };
        }
        Ok(us(from, qpc()))
    })?;
    b.row(Row::new("resources", "shader resource view written", "views", views).cpu(&t));
    for (name, one) in [
        ("view copied to the visible heap, one by one", true),
        ("views copied to the visible heap at once", false),
    ] {
        let t = b.rounds(|| {
            let from = qpc();
            let kind = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
            // SAFETY: slots of both heaps; nothing on the GPU reads them.
            unsafe {
                if one {
                    for n in 0..views {
                        device.CopyDescriptorsSimple(
                            1,
                            slot(visible_at, n),
                            slot(staged_at, n),
                            kind,
                        );
                    }
                } else {
                    device.CopyDescriptorsSimple(views as u32, visible_at, staged_at, kind);
                }
            }
            Ok(us(from, qpc()))
        })?;
        b.row(Row::new("resources", name, "views", views).cpu(&t));
    }

    // Barriers: between states, and between dispatches that need them.
    let many = b.size(1000) & !1;
    let target = gpu::buffer(&device, default, 1 << 16, D3D12_RESOURCE_FLAG_NONE)?;
    let (a, c) = (
        D3D12_RESOURCE_STATE_COPY_DEST,
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
    );
    let image = gpu::committed(
        &device,
        default,
        &gpu::texture_desc(1024, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET),
    )?;
    let (rt, ps) = (
        D3D12_RESOURCE_STATE_RENDER_TARGET,
        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
    );
    let common = D3D12_RESOURCE_STATE_COMMON;
    for (name, resource, x, y) in [
        ("buffer transitions, one by one", &target, a, c),
        ("render target transitions, one by one", &image, rt, ps),
    ] {
        let t = b.rounds(|| {
            b.direct.timed(1, Submit::Together, &mut |_, list| {
                let there = [gpu::transition(resource, x, y)];
                let back = [gpu::transition(resource, y, x)];
                // SAFETY: the resource outlives the recorded barriers and
                // is in the states they name.
                unsafe {
                    list.ResourceBarrier(&[gpu::transition(resource, common, x)]);
                    for _ in 0..many / 2 {
                        list.ResourceBarrier(&there);
                        list.ResourceBarrier(&back);
                    }
                    list.ResourceBarrier(&[gpu::transition(resource, x, common)]);
                }
                Ok(())
            })
        })?;
        b.row(Row::new("resources", name, "barriers", many).timed(&t));
    }

    let root = gpu::root_signature(&device, &[gpu::root_view(D3D12_ROOT_PARAMETER_TYPE_UAV, 0)])?;
    let pipeline = gpu::compute_pipeline(&device, &root, &gpu::compile(ADD, "cs", "cs_5_1")?)?;
    let bytes = 4 * THREADS as u64;
    let data = gpu::buffer(
        &device,
        default,
        256 * many as u64,
        D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
    )?;
    let readback = gpu::buffer(
        &device,
        D3D12_HEAP_TYPE_READBACK,
        bytes,
        D3D12_RESOURCE_FLAG_NONE,
    )?;
    let dispatches = |ordered: bool| {
        b.direct.timed(1, Submit::Together, &mut |_, list| {
            // SAFETY: live objects; each dispatch writes 256 bytes of the
            // buffer, its own where no barrier orders it.
            unsafe {
                list.SetComputeRootSignature(&root);
                list.SetPipelineState(&pipeline);
                let at = data.GetGPUVirtualAddress();
                let wait = [gpu::uav_barrier(&data)];
                for n in 0..many as u64 {
                    let own = if ordered { 0 } else { 256 * n };
                    list.SetComputeRootUnorderedAccessView(0, at + own);
                    list.Dispatch(1, 1, 1);
                    if ordered {
                        list.ResourceBarrier(&wait);
                    }
                }
            }
            Ok(())
        })
    };
    let free = b.rounds(|| dispatches(false))?;
    let mut runs = 0u32;
    let ordered = b.rounds(|| {
        runs += 1;
        dispatches(true)
    })?;
    b.direct.timed(1, Submit::Together, &mut |_, list| {
        // SAFETY: the sizes match; the buffer is back in the common state.
        unsafe { list.CopyBufferRegion(&readback, 0, &data, 0, bytes) };
        Ok(())
    })?;
    // The free rounds ran dispatch 0 on the same place once each.
    let want = runs * many as u32 + runs;
    let got: Vec<u32> = gpu::read(&readback, THREADS)?;
    ensure!(
        got.iter().all(|&g| g == want),
        "ordered dispatches counted {:?}, not {want} each",
        &got[..4]
    );
    b.row(
        Row::new(
            "resources",
            "dispatches, free to overlap",
            "dispatches",
            many,
        )
        .timed(&free),
    );
    b.row(
        Row::new(
            "resources",
            "dispatches, a UAV barrier after each",
            "dispatches",
            many,
        )
        .timed(&ordered)
        .checked("every dispatch saw the one before"),
    );
    Ok(())
}
