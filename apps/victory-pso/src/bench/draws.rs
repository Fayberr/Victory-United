//! Draws: what each kind of state change between them costs, and what the
//! way they are split over lists and submissions does.

use anyhow::{Context, Result, ensure};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use super::gpu::{self, MAX_LISTS, Submit, Timed};
use super::{Bench, Row};

const SIDE: u32 = 1024;
const TABLES: usize = 64;
const PIPELINES: usize = 8;

/// A small triangle at a place its number gives; `fullscreen` covers all.
/// The pixel shader reads all three ways of binding something.
const SOURCE: &str = "
cbuffer Draw : register(b0) { uint number; uint fullscreen; };
cbuffer View : register(b1) { float4 tint; };
Buffer<float4> data : register(t0);

float4 vs(uint id : SV_VertexID) : SV_Position {
    float2 corner = float2((id << 1) & 2, id & 2);
    if (fullscreen) return float4(corner * 2 - 1, 0, 1);
    float2 place = float2(number % 128, number / 128 % 128) / 64 - 1;
    return float4(place + corner / 128, 0, 1);
}
float4 ps() : SV_Target {
    return tint + data[0] * (SALT + 1) / 4096;
}
";

struct Scene {
    root: ID3D12RootSignature,
    pipelines: Vec<ID3D12PipelineState>,
    target: D3D12_CPU_DESCRIPTOR_HANDLE,
    constants: ID3D12Resource,
    views: ID3D12DescriptorHeap,
    step: u64,
    indirect: ID3D12CommandSignature,
    arguments: ID3D12Resource,
    queries: ID3D12QueryHeap,
    statistics: ID3D12Resource,
    _kept: (ID3D12Resource, ID3D12DescriptorHeap, ID3D12Resource),
}

#[derive(Clone, Copy, Default)]
struct Change {
    constants: bool,
    buffer: bool,
    table: bool,
    pipeline: bool,
}

impl Scene {
    fn new(b: &Bench, most: usize) -> Result<Scene> {
        let device = &b.adapter.device;
        let range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            ..Default::default()
        };
        let root = gpu::root_signature(
            device,
            &[
                gpu::root_constants(0, 2),
                gpu::root_view(D3D12_ROOT_PARAMETER_TYPE_CBV, 1),
                D3D12_ROOT_PARAMETER {
                    ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                    Anonymous: D3D12_ROOT_PARAMETER_0 {
                        DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                            NumDescriptorRanges: 1,
                            pDescriptorRanges: &range,
                        },
                    },
                    ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
                },
            ],
        )?;
        let pipelines = (0..PIPELINES)
            .map(|n| {
                let source = SOURCE.replace("SALT", &n.to_string());
                let vs = gpu::compile(&source, "vs", "vs_5_1")?;
                let ps = gpu::compile(&source, "ps", "ps_5_1")?;
                Ok(gpu::graphics_pipeline(device, &root, &vs, &ps, &[])?)
            })
            .collect::<Result<_>>()?;
        // SAFETY: plain object creation, with descriptions that outlive it.
        unsafe {
            let mut picture: Option<ID3D12Resource> = None;
            let mut clear = D3D12_CLEAR_VALUE {
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                ..Default::default()
            };
            clear.Anonymous.Color = [0.0; 4];
            device.CreateCommittedResource(
                &D3D12_HEAP_PROPERTIES {
                    Type: D3D12_HEAP_TYPE_DEFAULT,
                    ..Default::default()
                },
                D3D12_HEAP_FLAG_NONE,
                &gpu::texture_desc(SIDE, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET),
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                Some(&clear),
                &mut picture,
            )?;
            let picture = picture.context("no target")?;
            let targets: ID3D12DescriptorHeap =
                device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
                    NumDescriptors: 1,
                    ..Default::default()
                })?;
            let target = targets.GetCPUDescriptorHandleForHeapStart();
            device.CreateRenderTargetView(&picture, None, target);

            let constants = gpu::buffer(
                device,
                D3D12_HEAP_TYPE_UPLOAD,
                256 * most as u64,
                D3D12_RESOURCE_FLAG_NONE,
            )?;
            gpu::write(&constants, &vec![0.0f32; 64 * most])?;
            let data = gpu::buffer(
                device,
                D3D12_HEAP_TYPE_UPLOAD,
                16 * TABLES as u64,
                D3D12_RESOURCE_FLAG_NONE,
            )?;
            gpu::write(&data, &vec![1.0f32; 4 * TABLES])?;
            let views: ID3D12DescriptorHeap =
                device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                    NumDescriptors: TABLES as u32,
                    Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                    ..Default::default()
                })?;
            let step =
                device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
            for n in 0..TABLES {
                let mut at = views.GetCPUDescriptorHandleForHeapStart();
                at.ptr += n * step as usize;
                device.CreateShaderResourceView(
                    &data,
                    Some(&D3D12_SHADER_RESOURCE_VIEW_DESC {
                        Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
                        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                            Buffer: D3D12_BUFFER_SRV {
                                FirstElement: n as u64,
                                NumElements: 1,
                                ..Default::default()
                            },
                        },
                    }),
                    at,
                );
            }
            let draw = D3D12_INDIRECT_ARGUMENT_DESC {
                Type: D3D12_INDIRECT_ARGUMENT_TYPE_DRAW,
                ..Default::default()
            };
            let mut indirect: Option<ID3D12CommandSignature> = None;
            device.CreateCommandSignature(
                &D3D12_COMMAND_SIGNATURE_DESC {
                    ByteStride: size_of::<D3D12_DRAW_ARGUMENTS>() as u32,
                    NumArgumentDescs: 1,
                    pArgumentDescs: &draw,
                    NodeMask: 0,
                },
                None,
                &mut indirect,
            )?;
            let arguments = gpu::buffer(
                device,
                D3D12_HEAP_TYPE_UPLOAD,
                (size_of::<D3D12_DRAW_ARGUMENTS>() * most) as u64,
                D3D12_RESOURCE_FLAG_NONE,
            )?;
            let one = D3D12_DRAW_ARGUMENTS {
                VertexCountPerInstance: 3,
                InstanceCount: 1,
                StartVertexLocation: 0,
                StartInstanceLocation: 0,
            };
            gpu::write(&arguments, &vec![one; most])?;
            let mut queries: Option<ID3D12QueryHeap> = None;
            device.CreateQueryHeap(
                &D3D12_QUERY_HEAP_DESC {
                    Type: D3D12_QUERY_HEAP_TYPE_PIPELINE_STATISTICS,
                    Count: MAX_LISTS as u32,
                    NodeMask: 0,
                },
                &mut queries,
            )?;
            Ok(Scene {
                root,
                pipelines,
                target,
                constants,
                step: u64::from(step),
                views,
                indirect: indirect.context("no command signature")?,
                arguments,
                queries: queries.context("no query heap")?,
                statistics: gpu::buffer(
                    device,
                    D3D12_HEAP_TYPE_READBACK,
                    (size_of::<D3D12_QUERY_DATA_PIPELINE_STATISTICS>() * MAX_LISTS) as u64,
                    D3D12_RESOURCE_FLAG_NONE,
                )?,
                _kept: (picture, targets, data),
            })
        }
    }

    /// The state every list starts from.
    fn start(&self, list: &ID3D12GraphicsCommandList, fullscreen: bool) {
        // SAFETY: recording with live objects of the scene.
        unsafe {
            list.SetDescriptorHeaps(&[Some(self.views.clone())]);
            list.SetGraphicsRootSignature(&self.root);
            list.SetPipelineState(&self.pipelines[0]);
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            list.RSSetViewports(&[D3D12_VIEWPORT {
                Width: SIDE as f32,
                Height: SIDE as f32,
                MaxDepth: 1.0,
                ..Default::default()
            }]);
            list.RSSetScissorRects(&[RECT {
                left: 0,
                top: 0,
                right: SIDE as i32,
                bottom: SIDE as i32,
            }]);
            list.OMSetRenderTargets(1, Some(&self.target), false, None);
            list.SetGraphicsRoot32BitConstants(0, 2, [0u32, fullscreen.into()].as_ptr().cast(), 0);
            list.SetGraphicsRootConstantBufferView(1, self.constants.GetGPUVirtualAddress());
            list.SetGraphicsRootDescriptorTable(2, self.views.GetGPUDescriptorHandleForHeapStart());
        }
    }

    fn draws(
        &self,
        list: &ID3D12GraphicsCommandList,
        numbers: std::ops::Range<usize>,
        change: Change,
    ) {
        // SAFETY: as `start`; `numbers` is within what the buffers hold.
        unsafe {
            let buffer = self.constants.GetGPUVirtualAddress();
            let table = self.views.GetGPUDescriptorHandleForHeapStart();
            for n in numbers {
                if change.pipeline {
                    list.SetPipelineState(&self.pipelines[n % PIPELINES]);
                }
                if change.constants {
                    list.SetGraphicsRoot32BitConstant(0, n as u32, 0);
                }
                if change.buffer {
                    list.SetGraphicsRootConstantBufferView(1, buffer + 256 * n as u64);
                }
                if change.table {
                    let mut at = table;
                    at.ptr += (n % TABLES) as u64 * self.step;
                    list.SetGraphicsRootDescriptorTable(2, at);
                }
                list.DrawInstanced(3, 1, 0, 0);
            }
        }
    }

    /// `record` between the statistics' start and end.
    fn counted(
        &self,
        n: usize,
        list: &ID3D12GraphicsCommandList,
        fullscreen: bool,
        record: impl FnOnce(),
    ) {
        let kind = D3D12_QUERY_TYPE_PIPELINE_STATISTICS;
        let bytes = size_of::<D3D12_QUERY_DATA_PIPELINE_STATISTICS>() as u64;
        // SAFETY: as `start`.
        unsafe {
            self.start(list, fullscreen);
            list.BeginQuery(&self.queries, kind, n as u32);
            record();
            list.EndQuery(&self.queries, kind, n as u32);
            list.ResolveQueryData(
                &self.queries,
                kind,
                n as u32,
                1,
                &self.statistics,
                bytes * n as u64,
            );
        }
    }

    /// The triangles `lists` lists drew, and the pixels.
    fn drawn(&self, lists: usize) -> Result<(u64, u64)> {
        let all: Vec<D3D12_QUERY_DATA_PIPELINE_STATISTICS> = gpu::read(&self.statistics, lists)?;
        Ok((
            all.iter().map(|s| s.IAPrimitives).sum(),
            all.iter().map(|s| s.PSInvocations).sum(),
        ))
    }
}

pub fn run(b: &mut Bench) -> Result<()> {
    let sizes = [1_000, 5_000, 20_000].map(|n| b.size(n));
    let most = sizes[2];
    let scene = Scene::new(b, most)?;
    let none = Change::default();
    let all = Change {
        constants: true,
        buffer: true,
        table: true,
        pipeline: true,
    };
    let checked = "every triangle counted by the GPU";
    let count = |lists: usize, want: usize| -> Result<()> {
        let (triangles, _) = scene.drawn(lists)?;
        ensure!(
            triangles == want as u64,
            "{want} triangles were drawn and the GPU counted {triangles}"
        );
        Ok(())
    };

    for n in sizes {
        let t = b.rounds(|| {
            let t = b.direct.timed(1, Submit::Together, &mut |i, list| {
                scene.counted(i, list, false, || scene.draws(list, 0..n, none));
                Ok(())
            })?;
            count(1, n)?;
            Ok(t)
        })?;
        b.row(
            Row::new("draws", "no change between draws", "draws", n)
                .timed(&t)
                .checked(checked),
        );
    }
    let n = sizes[1];
    let cases = [
        (
            "root constants each draw",
            Change {
                constants: true,
                ..none
            },
        ),
        (
            "constant buffer address each draw",
            Change {
                buffer: true,
                ..none
            },
        ),
        (
            "descriptor table each draw",
            Change {
                table: true,
                ..none
            },
        ),
        (
            "pipeline each draw (of 8)",
            Change {
                pipeline: true,
                ..none
            },
        ),
        ("all four each draw", all),
    ];
    for (name, change) in cases {
        let t = b.rounds(|| {
            let t = b.direct.timed(1, Submit::Together, &mut |i, list| {
                scene.counted(i, list, false, || scene.draws(list, 0..n, change));
                Ok(())
            })?;
            count(1, n)?;
            Ok(t)
        })?;
        b.row(
            Row::new("draws", name, "draws", n)
                .timed(&t)
                .checked(checked),
        );
    }
    let t = b.rounds(|| {
        let t = b.direct.timed(1, Submit::Together, &mut |i, list| {
            scene.counted(i, list, false, || {
                // SAFETY: the buffer holds `n` draws' arguments.
                unsafe {
                    list.ExecuteIndirect(
                        &scene.indirect,
                        n as u32,
                        &scene.arguments,
                        0,
                        None::<&ID3D12Resource>,
                        0,
                    );
                }
            });
            Ok(())
        })?;
        count(1, n)?;
        Ok(t)
    })?;
    b.row(
        Row::new("draws", "one ExecuteIndirect", "draws", n)
            .timed(&t)
            .checked(checked),
    );

    // The same draws, split.
    let lists = if b.quick { 4 } else { 64 };
    let split = |b: &Bench, how: Submit| -> Result<Vec<Timed>> {
        b.rounds(|| {
            let t = b.direct.timed(lists, how, &mut |i, list| {
                let part = n * i / lists..n * (i + 1) / lists;
                scene.counted(i, list, false, || scene.draws(list, part, all));
                Ok(())
            })?;
            count(lists, n)?;
            Ok(t)
        })
    };
    for (name, how) in [
        ("lists submitted together", Submit::Together),
        ("lists submitted one by one", Submit::Each),
        ("lists, the CPU waiting after each", Submit::EachAndWait),
    ] {
        let t = split(b, how)?;
        b.row(
            Row::new("draws", &format!("{lists} {name}"), "draws", n)
                .timed(&t)
                .checked(checked),
        );
    }

    // Work that is the GPU's rather than the driver's.
    let layers = b.size(64);
    let t = b.rounds(|| {
        let t = b.direct.timed(1, Submit::Together, &mut |i, list| {
            scene.counted(i, list, true, || scene.draws(list, 0..layers, none));
            Ok(())
        })?;
        let (_, pixels) = scene.drawn(1)?;
        let want = layers as u64 * u64::from(SIDE) * u64::from(SIDE);
        ensure!(
            pixels >= want,
            "{want} pixels were covered and the GPU counted {pixels} pixel shader runs"
        );
        Ok(t)
    })?;
    b.row(
        Row::new("draws", "1024x1024 blended layers", "layers", layers)
            .timed(&t)
            .checked("every pixel counted by the GPU"),
    );
    Ok(())
}
