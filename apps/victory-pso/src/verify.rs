//! `verify`: a build folder's root signature and pipeline state created on
//! a chosen adapter and, with `--run`, the compute pipeline dispatched and
//! its buffers read back and compared.

use std::ffi::CString;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use victory_shader::emit::host::Host;
use victory_shader::emit::{HostShader, StaticSampler, UserData};
use victory_shader::{HwStage, RootType};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::{Interface, PCSTR};

use crate::Failure;
use crate::adapter::{self, Want};
use crate::manifest::{Graphics, MANIFEST, Manifest, Run, Stage};

fn root_type(name: &str) -> Result<RootType> {
    Ok(match name {
        "constants" => RootType::Constants,
        "descriptor" => RootType::Descriptor,
        "table" => RootType::Table,
        "immediate-sampler" => RootType::ImmediateSampler,
        "resource-heap" => RootType::ResourceHeap,
        "sampler-heap" => RootType::SamplerHeap,
        other => bail!("the manifest names a root type this build does not know: `{other}`"),
    })
}

/// A stage of the manifest, as the translator handed it out.
fn load(dir: &Path, s: &Stage) -> Result<HostShader> {
    let path = dir.join(&s.dxil);
    let dxil = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let stage = match s.stage.as_str() {
        "compute" => HwStage::Cs,
        "vertex" => HwStage::Vs,
        "pixel" => HwStage::Ps,
        other => bail!("the manifest names a stage this build does not know: `{other}`"),
    };
    let user_data = s
        .user_data
        .iter()
        .map(|u| {
            Ok(UserData {
                index: u.index,
                slot: u.slot,
                root: root_type(&u.root)?,
                dword: u.dword,
            })
        })
        .collect::<Result<_>>()?;
    let static_samplers = s
        .samplers
        .iter()
        .zip(&s.comparison_samplers)
        .map(|(&words, &compares)| {
            let mut sampler = StaticSampler::from_words(words);
            if compares {
                sampler.filter |= 0x80;
            }
            sampler
        })
        .collect();
    Ok(HostShader {
        written_root_uavs: Vec::new(),
        writes_guest_memory: false,
        stage,
        dxil,
        user_data,
        samplers: s.samplers.clone(),
        static_samplers,
        root_cbvs: s.root_cbvs.clone(),
        root_srvs: s.root_srvs.clone(),
        root_uavs: s.root_uavs.clone(),
        // What the runtime resolves when it binds a draw: no part of the
        // root signature or of the pipeline state.
        root_indexed_views: Vec::new(),
        half_views: Vec::new(),
    })
}

/// The graphics pipeline state for `g` with the translated shaders.
fn graphics_pipeline(
    device: &ID3D12Device,
    root: &ID3D12RootSignature,
    vs: &[u8],
    ps: &[u8],
    g: &Graphics,
) -> Result<ID3D12PipelineState> {
    let names = g
        .input
        .iter()
        .map(|e| CString::new(e.semantic.as_str()))
        .collect::<Result<Vec<_>, _>>()
        .context("an input semantic")?;
    let layout: Vec<D3D12_INPUT_ELEMENT_DESC> = g
        .input
        .iter()
        .zip(&names)
        .map(|(e, name)| D3D12_INPUT_ELEMENT_DESC {
            SemanticName: PCSTR(name.as_ptr().cast()),
            SemanticIndex: e.index,
            Format: DXGI_FORMAT(e.format),
            InputSlot: e.slot,
            AlignedByteOffset: e.offset.unwrap_or(D3D12_APPEND_ALIGNED_ELEMENT),
            InputSlotClass: if e.per_instance {
                D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA
            } else {
                D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA
            },
            InstanceDataStepRate: e.step,
        })
        .collect();
    let code = |b: &[u8]| D3D12_SHADER_BYTECODE {
        pShaderBytecode: b.as_ptr().cast(),
        BytecodeLength: b.len(),
    };
    let keep = D3D12_DEPTH_STENCILOP_DESC {
        StencilFailOp: D3D12_STENCIL_OP_KEEP,
        StencilDepthFailOp: D3D12_STENCIL_OP_KEEP,
        StencilPassOp: D3D12_STENCIL_OP_KEEP,
        StencilFunc: D3D12_COMPARISON_FUNC_ALWAYS,
    };
    let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
        // SAFETY: a borrowed pointer the description never releases.
        pRootSignature: unsafe { std::mem::transmute_copy(root) },
        VS: code(vs),
        PS: code(ps),
        SampleMask: u32::MAX,
        RasterizerState: D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE(g.fill),
            CullMode: D3D12_CULL_MODE(g.cull),
            FrontCounterClockwise: g.front_counter_clockwise.into(),
            DepthBias: g.depth_bias,
            DepthClipEnable: g.depth_clip.into(),
            ..Default::default()
        },
        DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
            DepthEnable: g.depth_enable.into(),
            DepthWriteMask: if g.depth_write {
                D3D12_DEPTH_WRITE_MASK_ALL
            } else {
                D3D12_DEPTH_WRITE_MASK_ZERO
            },
            DepthFunc: D3D12_COMPARISON_FUNC(g.depth_func),
            StencilReadMask: 0xff,
            StencilWriteMask: 0xff,
            FrontFace: keep,
            BackFace: keep,
            ..Default::default()
        },
        InputLayout: D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: layout.as_ptr(),
            NumElements: layout.len() as u32,
        },
        PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE(g.topology),
        NumRenderTargets: g.render_targets.len() as u32,
        DSVFormat: DXGI_FORMAT(g.depth_stencil),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: g.sample_count,
            Quality: 0,
        },
        ..Default::default()
    };
    for (n, &f) in g.render_targets.iter().enumerate() {
        desc.RTVFormats[n] = DXGI_FORMAT(f);
    }
    desc.BlendState.IndependentBlendEnable = (g.blend.len() > 1).into();
    for (n, target) in desc.BlendState.RenderTarget.iter_mut().enumerate() {
        // D3D12's defaults, then what the state says of this target.
        *target = D3D12_RENDER_TARGET_BLEND_DESC {
            SrcBlend: D3D12_BLEND_ONE,
            DestBlend: D3D12_BLEND_ZERO,
            BlendOp: D3D12_BLEND_OP_ADD,
            SrcBlendAlpha: D3D12_BLEND_ONE,
            DestBlendAlpha: D3D12_BLEND_ZERO,
            BlendOpAlpha: D3D12_BLEND_OP_ADD,
            LogicOp: D3D12_LOGIC_OP_NOOP,
            RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
            ..Default::default()
        };
        if let Some(b) = g.blend.get(n) {
            target.BlendEnable = b.enable.into();
            target.SrcBlend = D3D12_BLEND(b.src);
            target.DestBlend = D3D12_BLEND(b.dst);
            target.BlendOp = D3D12_BLEND_OP(b.op);
            target.SrcBlendAlpha = D3D12_BLEND(b.src_alpha);
            target.DestBlendAlpha = D3D12_BLEND(b.dst_alpha);
            target.BlendOpAlpha = D3D12_BLEND_OP(b.op_alpha);
            target.RenderTargetWriteMask = b.write_mask;
        }
    }
    // SAFETY: the description points at data that outlives the call.
    unsafe { device.CreateGraphicsPipelineState(&desc) }
        .map_err(|e| anyhow!("graphics pipeline state: {}", e.message()))
}

/// The stage's user-data block for a run: each dword from the title root
/// parameter that fills it.
fn user_data(shader: &HostShader, run: &Run) -> Result<Vec<u32>> {
    let mut data = vec![0u32; shader.user_data_dwords().max(1) as usize];
    for u in &shader.user_data {
        let slot = u.slot.to_string();
        data[u.index as usize] = match u.root {
            RootType::Constants => *run
                .constants
                .get(&slot)
                .and_then(|c| c.get(u.dword as usize))
                .with_context(|| {
                    format!(
                        "the shader reads dword {} of the title's root constants {}: \
                         give it under [run.constants]",
                        u.dword, u.slot
                    )
                })?,
            // A table is the byte offset of its first descriptor in the
            // heap, in the console's 32-byte view descriptors.
            RootType::Table => {
                32 * *run.tables.get(&slot).with_context(|| {
                    format!(
                        "the shader reads the title's descriptor table {}: \
                         give its first view-heap index under [run.tables]",
                        u.slot
                    )
                })?
            }
            // The guard slot: the null view kept after the bound ones, which
            // every heap index at or above it reads instead.
            RootType::ResourceHeap => run.view.len() as u32,
            RootType::SamplerHeap => 0,
            other => bail!("unsupported: user data from a {other:?} root parameter"),
        };
    }
    Ok(data)
}

/// Runs the queue's work to the end, or reports why it did not finish.
fn finish(device: &ID3D12Device, queue: &ID3D12CommandQueue) -> Result<()> {
    // SAFETY: plain fence calls on live objects.
    unsafe {
        let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
        queue.Signal(&fence, 1)?;
        let started = Instant::now();
        while fence.GetCompletedValue() < 1 {
            if let Err(e) = device.GetDeviceRemovedReason() {
                bail!("the device was removed: {}", e.message());
            }
            if started.elapsed() > Duration::from_secs(60) {
                bail!("the GPU did not finish within 60 seconds");
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(())
}

/// Dispatches the compute pipeline with `run`'s bindings and returns each
/// view's dwords afterwards, in heap order.
fn dispatch(
    device: &ID3D12Device,
    root: &ID3D12RootSignature,
    pso: &ID3D12PipelineState,
    user: &[u32],
    run: &Run,
) -> Result<Vec<Vec<u32>>> {
    // SAFETY: D3D12 calls with descriptions that outlive them; every
    // resource lives until the queue has finished.
    unsafe {
        let queue: ID3D12CommandQueue = device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            ..Default::default()
        })?;
        let allocator: ID3D12CommandAllocator =
            device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
        let list: ID3D12GraphicsCommandList =
            device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, pso)?;
        let buffer = |bytes: u64, flags| D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: bytes,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: flags,
            ..Default::default()
        };
        let make = |heap, desc: &D3D12_RESOURCE_DESC, state| -> Result<ID3D12Resource> {
            let mut resource: Option<ID3D12Resource> = None;
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
            resource.context("no resource")
        };
        let heap = |kind, count: usize| -> Result<ID3D12DescriptorHeap> {
            Ok(device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                Type: kind,
                NumDescriptors: count as u32,
                Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                ..Default::default()
            })?)
        };
        // The views in order, then the null guard slot.
        let views = heap(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, run.view.len() + 1)?;
        let samplers = heap(D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER, 1)?;
        let step = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV)
            as usize;
        let slot = |index: usize| {
            let mut at = views.GetCPUDescriptorHandleForHeapStart();
            at.ptr += index * step;
            at
        };
        let null = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                },
            },
        };
        device.CreateShaderResourceView(None, Some(&null), slot(run.view.len()));
        // (buffer, dwords, where it is read back): buffers start zeroed.
        let mut buffers = Vec::new();
        for (index, v) in run.view.iter().enumerate() {
            if v.kind != "raw-uav" {
                bail!("unsupported view kind `{}` (bound: raw-uav)", v.kind);
            }
            let bytes = u64::from(v.dwords) * 4;
            let written = make(
                D3D12_HEAP_TYPE_DEFAULT,
                &buffer(bytes, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS),
                D3D12_RESOURCE_STATE_COMMON,
            )?;
            let desc = D3D12_UNORDERED_ACCESS_VIEW_DESC {
                Format: DXGI_FORMAT_R32_TYPELESS,
                ViewDimension: D3D12_UAV_DIMENSION_BUFFER,
                Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 {
                    Buffer: D3D12_BUFFER_UAV {
                        NumElements: v.dwords,
                        Flags: D3D12_BUFFER_UAV_FLAG_RAW,
                        ..Default::default()
                    },
                },
            };
            device.CreateUnorderedAccessView(&written, None, Some(&desc), slot(index));
            let read = make(
                D3D12_HEAP_TYPE_READBACK,
                &buffer(bytes, D3D12_RESOURCE_FLAG_NONE),
                D3D12_RESOURCE_STATE_COPY_DEST,
            )?;
            buffers.push((written, v.dwords, read));
        }
        list.SetDescriptorHeaps(&[Some(views.clone()), Some(samplers.clone())]);
        list.SetComputeRootSignature(root);
        // The compute stage's user data is root parameter 0.
        list.SetComputeRoot32BitConstants(0, user.len() as u32, user.as_ptr().cast(), 0);
        let [x, y, z] = run.groups;
        list.Dispatch(x, y, z);
        list.Close()?;
        queue.ExecuteCommandLists(&[Some(list.cast()?)]);
        finish(device, &queue)?;
        // Buffers are back in the common state once a list has run, so the
        // copies need no barrier whether the shader wrote a buffer or not.
        allocator.Reset()?;
        list.Reset(&allocator, None)?;
        for (written, dwords, read) in &buffers {
            list.CopyBufferRegion(read, 0, written, 0, u64::from(*dwords) * 4);
        }
        list.Close()?;
        queue.ExecuteCommandLists(&[Some(list.cast()?)]);
        finish(device, &queue)?;
        let mut out = Vec::new();
        for (_, dwords, read) in &buffers {
            let mut data = std::ptr::null_mut();
            read.Map(0, None, Some(&mut data))?;
            out.push(std::slice::from_raw_parts(data.cast::<u32>(), *dwords as usize).to_vec());
            read.Unmap(0, None);
        }
        Ok(out)
    }
}

/// Whether a dword is the expected one, or a float as near it as `near`.
fn alike(got: u32, want: u32, near: Option<f32>) -> bool {
    let (a, b) = (f32::from_bits(got), f32::from_bits(want));
    got == want || near.is_some_and(|n| (a - b).abs() <= n * b.abs().max(1.0))
}

/// What the run left in the views against what the description expects.
fn compare(run: &Run, got: &[Vec<u32>]) -> Result<()> {
    if run.expect.is_empty() {
        bail!("nothing to compare: the description's [run] has no [[run.expect]]");
    }
    for e in &run.expect {
        let view = got
            .get(e.view)
            .with_context(|| format!("[[run.expect]] names view {}, which is not bound", e.view))?;
        if view.len() != e.dwords.len() {
            bail!(
                "[[run.expect]] gives {} dwords for view {}, which has {}",
                e.dwords.len(),
                e.view,
                view.len()
            );
        }
        let wrong: Vec<usize> = (0..view.len())
            .filter(|&i| !alike(view[i], e.dwords[i], e.near))
            .collect();
        if let Some(&first) = wrong.first() {
            return Err(Failure::Mismatch(format!(
                "view {}: {} of {} dwords differ; the first is dword {first}: {:#x}, expected {:#x}{}",
                e.view,
                wrong.len(),
                view.len(),
                view[first],
                e.dwords[first],
                match e.near {
                    Some(_) => format!(
                        " ({} for {})",
                        f32::from_bits(view[first]),
                        f32::from_bits(e.dwords[first])
                    ),
                    None => String::new(),
                }
            ))
            .into());
        }
        println!("  view {}: {} dwords as expected", e.view, view.len());
    }
    Ok(())
}

pub fn run(dir: &Path, want: Want, execute: bool) -> Result<()> {
    let path = dir.join(MANIFEST);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let m: Manifest = toml::from_str(&text)
        .map_err(|e| anyhow!("{}: not a build manifest: {e}", path.display()))?;
    let shaders = m
        .stages
        .iter()
        .map(|s| load(dir, s))
        .collect::<Result<Vec<_>>>()?;

    let a = adapter::pick(want)?;
    println!(
        "adapter       : {} (vendor {:#06x}, device {:#06x}, {} MiB, {})",
        a.name,
        a.vendor,
        a.device_id,
        a.memory_mib,
        if a.software { "software" } else { "hardware" }
    );
    println!("shader model  : {}.{}", a.shader_model.0, a.shader_model.1);
    match a.wave_lanes {
        Some((min, max)) => println!("wave lanes    : {min} to {max}"),
        None => println!("wave lanes    : no wave operations"),
    }
    if a.shader_model < (6, 6) {
        return Err(Failure::NoAdapter(format!(
            "{} takes shader model {}.{} at most; translated shaders are 6.6",
            a.name, a.shader_model.0, a.shader_model.1
        ))
        .into());
    }
    // A shader built for a wave size the adapter lacks may declare it.
    let lanes = |n: u32| a.wave_lanes.is_some_and(|(min, max)| min <= n && n <= max);
    match m.wave64.as_str() {
        "native" if !lanes(64) => println!(
            "note          : built with --wave64 native, and this adapter has no 64-lane waves; \
             a shader that declares them will be refused (build with wave32)"
        ),
        "wave32" if !lanes(32) => println!(
            "note          : built with --wave64 wave32, and this adapter has no 32-lane waves; \
             a shader that declares them will be refused"
        ),
        _ => {}
    }

    let host = Host {
        device: a.device.clone(),
    };
    let stages: Vec<(&HostShader, D3D12_SHADER_VISIBILITY)> = shaders
        .iter()
        .map(|s| {
            let visibility = match s.stage {
                HwStage::Vs => D3D12_SHADER_VISIBILITY_VERTEX,
                HwStage::Ps => D3D12_SHADER_VISIBILITY_PIXEL,
                _ => D3D12_SHADER_VISIBILITY_ALL,
            };
            (s, visibility)
        })
        .collect();
    let root = host
        .root_signature_for(&stages)
        .map_err(|e| anyhow!("root signature: {e}"))?;
    let samplers: usize = shaders.iter().map(|s| s.static_samplers.len()).sum();
    println!(
        "root signature: created ({} parameters, {samplers} static samplers)",
        m.root.len()
    );
    let pso = match (m.kind.as_str(), &shaders[..]) {
        ("compute", [cs]) => host
            .compute_pipeline_with(&root, &cs.dxil)
            .map_err(|e| anyhow!("compute pipeline state: {e}"))?,
        ("graphics", [vs, ps]) => {
            let g = m
                .graphics
                .as_ref()
                .context("the manifest of a graphics pipeline has no state")?;
            graphics_pipeline(&a.device, &root, &vs.dxil, &ps.dxil, g)?
        }
        (kind, _) => bail!("the manifest's `{kind}` pipeline does not match its stages"),
    };
    println!("pipeline state: created ({})", m.kind);

    if execute {
        let (Some(run), [cs]) = (&m.run, &shaders[..]) else {
            bail!("nothing to run: only a compute pipeline whose description has [run] is run");
        };
        let user = user_data(cs, run)?;
        let got = dispatch(&a.device, &root, &pso, &user, run)?;
        println!("run           : dispatched {:?} on {}", run.groups, a.name);
        compare(run, &got)?;
        println!("result        : PASS");
    }
    Ok(())
}
