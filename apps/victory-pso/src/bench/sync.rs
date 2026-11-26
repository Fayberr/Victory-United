//! Fences and waits: what a submission costs when nothing is in it.

use anyhow::{Result, ensure};
use windows::Win32::Graphics::Direct3D12::*;

use super::gpu::{self, Queue, Submit, qpc, us};
use super::{Bench, Row};

const COUNT: &str = "
RWByteAddressBuffer data : register(u0);
[numthreads(1, 1, 1)] void cs() { data.Store(0, data.Load(0) + 1); }
";

pub fn run(b: &mut Bench) -> Result<()> {
    let kinds = ["direct", "compute", "copy"];
    fn queues(b: &Bench) -> [&Queue; 3] {
        [&b.direct, &b.compute, &b.copy]
    }
    for (n, kind) in kinds.iter().enumerate() {
        let t = b.rounds(|| {
            let q = queues(b)[n];
            let from = qpc();
            let value = q.signal()?;
            q.wait(value)?;
            Ok(us(from, qpc()))
        })?;
        b.row(
            Row::new(
                "sync",
                &format!("{kind}: signal, wait on an event"),
                "waits",
                1,
            )
            .cpu(&t),
        );
        let t = b.rounds(|| {
            let q = queues(b)[n];
            let from = qpc();
            let value = q.signal()?;
            q.spin(value)?;
            Ok(us(from, qpc()))
        })?;
        b.row(
            Row::new(
                "sync",
                &format!("{kind}: signal, poll the fence"),
                "waits",
                1,
            )
            .cpu(&t),
        );
        let t = b.rounds(|| queues(b)[n].timed(1, Submit::Together, &mut |_, _| Ok(())))?;
        b.row(
            Row::new(
                "sync",
                &format!("{kind}: an empty list, waited for"),
                "lists",
                1,
            )
            .timed(&t),
        );
    }
    let many = if b.quick { 8 } else { 64 };
    for (name, how) in [
        ("empty lists together", Submit::Together),
        ("empty lists one by one", Submit::Each),
        ("empty lists, waiting after each", Submit::EachAndWait),
    ] {
        let t = b.rounds(|| b.direct.timed(many, how, &mut |_, _| Ok(())))?;
        b.row(Row::new("sync", &format!("direct: {many} {name}"), "lists", many).timed(&t));
    }

    // Two queues taking turns over one buffer, each waiting for the other on
    // the GPU: a hop the second did not wait for loses a count.
    let device = b.adapter.device.clone();
    let root = gpu::root_signature(&device, &[gpu::root_view(D3D12_ROOT_PARAMETER_TYPE_UAV, 0)])?;
    let pipeline = gpu::compute_pipeline(&device, &root, &gpu::compile(COUNT, "cs", "cs_5_1")?)?;
    let data = gpu::buffer(
        &device,
        D3D12_HEAP_TYPE_DEFAULT,
        256,
        D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
    )?;
    let readback = gpu::buffer(
        &device,
        D3D12_HEAP_TYPE_READBACK,
        256,
        D3D12_RESOURCE_FLAG_NONE,
    )?;
    let hops = if b.quick { 8 } else { 32 };
    let make = |kind| -> Result<(ID3D12CommandAllocator, ID3D12GraphicsCommandList)> {
        // SAFETY: recording with live objects; run only while they live.
        unsafe {
            let allocator: ID3D12CommandAllocator = device.CreateCommandAllocator(kind)?;
            let list: ID3D12GraphicsCommandList =
                device.CreateCommandList(0, kind, &allocator, &pipeline)?;
            list.SetComputeRootSignature(&root);
            list.SetComputeRootUnorderedAccessView(0, data.GetGPUVirtualAddress());
            list.Dispatch(1, 1, 1);
            list.Close()?;
            Ok((allocator, list))
        }
    };
    let lists = [
        make(D3D12_COMMAND_LIST_TYPE_DIRECT)?,
        make(D3D12_COMMAND_LIST_TYPE_COMPUTE)?,
    ];
    let mut ran = 0u32;
    let t = b.rounds(|| {
        use windows::core::Interface;
        let pair = [&b.direct, &b.compute];
        let from = qpc();
        let mut last = None;
        for hop in 0..hops {
            let q = pair[hop % 2];
            if let Some(value) = last {
                q.wait_for(pair[(hop + 1) % 2], value)?;
            }
            // SAFETY: the list lives past the last wait. Each is run again
            // only after the other queue's run, which waits for it.
            unsafe {
                q.queue
                    .ExecuteCommandLists(&[Some(lists[hop % 2].1.cast()?)])
            };
            last = Some(q.signal()?);
        }
        if let Some(value) = last {
            pair[(hops + 1) % 2].wait(value)?;
        }
        ran += hops as u32;
        Ok(us(from, qpc()))
    })?;
    b.direct.timed(1, Submit::Together, &mut |_, list| {
        // SAFETY: both buffers have 256 bytes; the queues are idle, so the
        // buffer has gone back to the common state.
        unsafe { list.CopyBufferRegion(&readback, 0, &data, 0, 4) };
        Ok(())
    })?;
    let got: Vec<u32> = gpu::read(&readback, 1)?;
    ensure!(
        got[0] == ran,
        "two queues taking turns counted {} of {ran} runs",
        got[0]
    );
    b.row(
        Row::new("sync", "direct and compute taking turns", "hops", hops)
            .cpu(&t)
            .checked("one count for every hop"),
    );
    Ok(())
}
