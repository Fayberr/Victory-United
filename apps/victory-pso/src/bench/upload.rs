//! Data between the CPU and the card.

use anyhow::{Result, ensure};
use windows::Win32::Graphics::Direct3D12::*;

use super::gpu::{self, Mapped, Queue, Submit, qpc, us};
use super::{Bench, Row};

/// The value of dword `i`, so that a copy to the wrong place shows.
fn value(i: usize, round: u32) -> u32 {
    (i as u32).wrapping_mul(0x9E37_79B1) ^ round.wrapping_mul(0x0100_0193)
}

fn fill(data: &mut [u32], round: u32) {
    for (i, d) in data.iter_mut().enumerate() {
        *d = value(i, round);
    }
}

/// Reads all of `data` and adds it up: each thread a stride of its own, the
/// partial sums by atomics.
const SUM: &str = "
cbuffer Size : register(b0) { uint pieces; };
ByteAddressBuffer data : register(t0);
RWByteAddressBuffer sum : register(u0);
[numthreads(64, 1, 1)] void cs(uint i : SV_DispatchThreadID) {
    uint total = 0;
    [loop] for (uint n = 0; n < pieces; n++) {
        uint4 v = data.Load4(16 * (i + 4096 * n));
        total += v.x + v.y + v.z + v.w;
    }
    uint before;
    sum.InterlockedAdd(0, total, before);
}
";

/// Shader reads of the same data in each kind of memory a buffer can be in.
fn reads(b: &mut Bench, mib: usize, data: &[u32]) -> Result<()> {
    let device = b.adapter.device.clone();
    let bytes = size_of_val(data) as u64;
    let none = D3D12_RESOURCE_FLAG_NONE;
    let root = gpu::root_signature(
        &device,
        &[
            gpu::root_constants(0, 1),
            gpu::root_view(D3D12_ROOT_PARAMETER_TYPE_SRV, 0),
            gpu::root_view(D3D12_ROOT_PARAMETER_TYPE_UAV, 0),
        ],
    )?;
    let pipeline = gpu::compute_pipeline(&device, &root, &gpu::compile(SUM, "cs", "cs_5_1")?)?;
    let total = gpu::buffer(
        &device,
        D3D12_HEAP_TYPE_DEFAULT,
        256,
        D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
    )?;
    let readback = gpu::buffer(&device, D3D12_HEAP_TYPE_READBACK, 256, none)?;
    let staging = gpu::buffer(&device, D3D12_HEAP_TYPE_UPLOAD, bytes, none)?;
    gpu::write(&staging, data)?;
    let expected = data.iter().fold(0u32, |s, &d| s.wrapping_add(d));
    for (name, kind) in [
        ("default heap", Some(D3D12_HEAP_TYPE_DEFAULT)),
        ("upload heap", Some(D3D12_HEAP_TYPE_UPLOAD)),
        ("GPU upload heap", Some(D3D12_HEAP_TYPE_GPU_UPLOAD)),
        ("system memory (custom heap)", None),
    ] {
        let made = match kind {
            Some(kind) => gpu::buffer(&device, kind, bytes, none),
            None => gpu::system_buffer(&device, bytes),
        };
        // (not every card has every kind)
        let Ok(source) = made else { continue };
        if kind == Some(D3D12_HEAP_TYPE_DEFAULT) {
            b.direct.timed(1, Submit::Together, &mut |_, list| {
                // SAFETY: both buffers have `bytes` bytes.
                unsafe { list.CopyBufferRegion(&source, 0, &staging, 0, bytes) };
                Ok(())
            })?;
        } else {
            gpu::write(&source, data)?;
        }
        let groups = 64u32;
        let pieces = (bytes / 16 / 4096) as u32;
        let mut runs = 0u32;
        let t = b.rounds(|| {
            runs += 1;
            b.direct.timed(1, Submit::Together, &mut |_, list| {
                // SAFETY: live objects; the reads are within `source`.
                unsafe {
                    list.SetComputeRootSignature(&root);
                    list.SetPipelineState(&pipeline);
                    list.SetComputeRoot32BitConstant(0, pieces, 0);
                    list.SetComputeRootShaderResourceView(1, source.GetGPUVirtualAddress());
                    list.SetComputeRootUnorderedAccessView(2, total.GetGPUVirtualAddress());
                    list.Dispatch(groups, 1, 1);
                }
                Ok(())
            })
        })?;
        // The sum so far, taken and then set back to nothing.
        b.direct.timed(1, Submit::Together, &mut |_, list| {
            // SAFETY: both buffers have 256 bytes.
            unsafe { list.CopyBufferRegion(&readback, 0, &total, 0, 256) };
            Ok(())
        })?;
        let got: Vec<u32> = gpu::read(&readback, 1)?;
        ensure!(
            got[0] == expected.wrapping_mul(runs),
            "shader reads of a {name} added up to {:#x}, not {:#x}",
            got[0],
            expected.wrapping_mul(runs)
        );
        let zero = gpu::buffer(&device, D3D12_HEAP_TYPE_UPLOAD, 256, none)?;
        gpu::write(&zero, &[0u32; 64])?;
        b.direct.timed(1, Submit::Together, &mut |_, list| {
            // SAFETY: both buffers have 256 bytes; `zero` outlives the wait.
            unsafe { list.CopyBufferRegion(&total, 0, &zero, 0, 256) };
            Ok(())
        })?;
        b.row(
            Row::new(
                "upload",
                &format!("shader reads {mib} MiB, {name}"),
                "reads",
                1,
            )
            .timed(&t)
            .moving(bytes)
            .checked("the sum of what was read"),
        );
    }
    Ok(())
}

pub fn run(b: &mut Bench) -> Result<()> {
    let device = b.adapter.device.clone();
    let mib = if b.quick { 1 } else { 64 };
    let bytes = (mib << 20) as u64;
    let dwords = bytes as usize / 4;
    let none = D3D12_RESOURCE_FLAG_NONE;
    let upload = gpu::buffer(&device, D3D12_HEAP_TYPE_UPLOAD, bytes, none)?;
    let readback = gpu::buffer(&device, D3D12_HEAP_TYPE_READBACK, bytes, none)?;
    let card = gpu::buffer(&device, D3D12_HEAP_TYPE_DEFAULT, bytes, none)?;
    let other = gpu::buffer(&device, D3D12_HEAP_TYPE_DEFAULT, bytes, none)?;
    let mut data = vec![0u32; dwords];
    fill(&mut data, 1);

    // The CPU's side. A plain copy by the standard library, as callers do.
    let name = |what: &str| format!("{what}, {mib} MiB");
    let into = |to: &ID3D12Resource, data: &[u32]| -> Result<f64> {
        let mapped = Mapped::new(to)?;
        let from = qpc();
        // SAFETY: the buffer holds as many bytes as `data`.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), mapped.at.cast(), data.len()) };
        Ok(us(from, qpc()))
    };
    let t = b.rounds(|| into(&upload, &data))?;
    b.row(
        Row::new("upload", &name("CPU write to an upload heap"), "copies", 1)
            .cpu(&t)
            .moving(bytes),
    );
    // A card whose memory the CPU reaches directly.
    let direct = gpu::buffer(&device, D3D12_HEAP_TYPE_GPU_UPLOAD, bytes, none);
    if let Ok(direct) = &direct {
        let t = b.rounds(|| into(direct, &data))?;
        b.row(
            Row::new(
                "upload",
                &name("CPU write to a GPU upload heap"),
                "copies",
                1,
            )
            .cpu(&t)
            .moving(bytes),
        );
    }
    // Reading: a readback heap is cached, an upload heap is not. The second
    // is orders slower, so it reads a 64th.
    let out_of = |from: &ID3D12Resource, part: &mut [u32]| -> Result<f64> {
        let mapped = Mapped::new(from)?;
        let start = qpc();
        // SAFETY: `part` is no longer than the buffer.
        unsafe {
            std::ptr::copy_nonoverlapping(mapped.at.cast(), part.as_mut_ptr(), part.len());
        }
        Ok(us(start, qpc()))
    };
    let mut back = vec![0u32; dwords];
    let t = b.rounds(|| out_of(&readback, &mut back))?;
    b.row(
        Row::new("upload", &name("CPU read of a readback heap"), "copies", 1)
            .cpu(&t)
            .moving(bytes),
    );
    let part = dwords / 64;
    let t = b.rounds(|| out_of(&upload, &mut back[..part]))?;
    b.row(
        Row::new(
            "upload",
            "CPU read of an upload heap, a 64th of it",
            "copies",
            1,
        )
        .cpu(&t)
        .moving(4 * part as u64),
    );
    ensure!(
        back[..part] == data[..part],
        "an upload heap read back something else than was written"
    );

    // The GPU's side: up and back on each queue, with the result compared.
    let mut round = 1;
    for (queue, kind) in [("direct", 0), ("compute", 1), ("copy", 2)] {
        fn pick(b: &Bench, kind: i32) -> &Queue {
            match kind {
                0 => &b.direct,
                1 => &b.compute,
                _ => &b.copy,
            }
        }
        let copy = |from: &ID3D12Resource, to: &ID3D12Resource| {
            pick(b, kind).timed(1, Submit::Together, &mut |_, list| {
                // SAFETY: all of these buffers have `bytes` bytes.
                unsafe { list.CopyBufferRegion(to, 0, from, 0, bytes) };
                Ok(())
            })
        };
        round += 1;
        fill(&mut data, round);
        into(&upload, &data)?;
        let up = b.rounds(|| copy(&upload, &card))?;
        let across = b.rounds(|| copy(&card, &other))?;
        let down = b.rounds(|| copy(&other, &readback))?;
        ensure!(
            gpu::read::<u32>(&readback, dwords)? == data,
            "data copied up and back on the {queue} queue came back changed"
        );
        for (t, what) in [
            (up, "upload to the card"),
            (across, "card to card"),
            (down, "card to readback"),
        ] {
            b.row(
                Row::new(
                    "upload",
                    &name(&format!("{queue} queue, {what}")),
                    "copies",
                    1,
                )
                .timed(&t)
                .moving(bytes)
                .checked("the data after up, across and back"),
            );
        }
    }

    // The same bytes in pieces, scattered: the pieces' order is reversed, so
    // no two copies can be merged into one.
    let pieces = b.size(4096);
    let piece = 4096u64;
    let total = piece * pieces as u64;
    if total <= bytes {
        round += 1;
        fill(&mut data, round);
        into(&upload, &data)?;
        let t = b.rounds(|| {
            b.direct.timed(1, Submit::Together, &mut |_, list| {
                for n in 0..pieces as u64 {
                    let to = (pieces as u64 - 1 - n) * piece;
                    // SAFETY: within both buffers.
                    unsafe { list.CopyBufferRegion(&card, to, &upload, n * piece, piece) };
                }
                Ok(())
            })
        })?;
        b.direct.timed(1, Submit::Together, &mut |_, list| {
            // SAFETY: within both buffers.
            unsafe { list.CopyBufferRegion(&readback, 0, &card, 0, total) };
            Ok(())
        })?;
        let got: Vec<u32> = gpu::read(&readback, total as usize / 4)?;
        let each = piece as usize / 4;
        for (n, g) in got.chunks_exact(each).enumerate() {
            let from = (pieces - 1 - n) * each;
            ensure!(
                g == &data[from..from + each],
                "piece {n} of a scattered copy came back changed"
            );
        }
        b.row(
            Row::new(
                "upload",
                "direct queue, scattered 4 KiB copies",
                "copies",
                pieces,
            )
            .timed(&t)
            .moving(total)
            .checked("every piece"),
        );
    }
    reads(b, mib, &data)
}
