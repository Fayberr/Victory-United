//! Pipeline state creation. Every shader here carries a number no run
//! before had, so a driver's disk cache cannot make a cold pipeline warm.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use windows::Win32::Graphics::Direct3D12::*;

use super::gpu::{self, qpc, us};
use super::{Bench, Row};

/// A pixel shader with enough in it that a driver has to work: a loop, math
/// and reads. `SALT` makes it a shader of its own.
const SOURCE: &str = "
cbuffer Draw : register(b0) { uint number; uint steps; };
ByteAddressBuffer data : register(t0);

float4 vs(uint id : SV_VertexID) : SV_Position {
    return float4(float2((id << 1) & 2, id & 2) * 2 - 1, 0, 1);
}
float4 ps(float4 at : SV_Position) : SV_Target {
    float4 sum = SALT;
    for (uint i = 0; i < steps; i++) {
        float4 v = asfloat(data.Load4(16 * (i + (uint)at.x)));
        sum += sin(v * at.yxyx + SALT) * sqrt(abs(v.wzyx) + i);
        if (dot(sum, v) > number) sum = sum.yzwx * 0.5 + normalize(v + 1);
    }
    return sum / (1 + abs(sum));
}
";

struct Shaders {
    vs: Vec<u8>,
    ps: Vec<u8>,
}

/// `count` pairs no run before had.
fn shaders(next: &mut u64, count: usize) -> Result<Vec<Shaders>> {
    (0..count)
        .map(|_| {
            *next += 1;
            // Two floats, as one has too few bits to hold the number.
            let salt = format!(
                "float4({}, {}, 1, 2)",
                *next & 0x3f_ffff,
                *next >> 22 & 0x3f_ffff
            );
            let source = SOURCE.replace("SALT", &salt);
            Ok(Shaders {
                vs: gpu::compile(&source, "vs", "vs_5_1")?,
                ps: gpu::compile(&source, "ps", "ps_5_1")?,
            })
        })
        .collect()
}

pub fn run(b: &mut Bench) -> Result<()> {
    let device = b.adapter.device.clone();
    let count = if b.quick { 4 } else { 64 };
    let root = gpu::root_signature(
        &device,
        &[
            gpu::root_constants(0, 2),
            gpu::root_view(D3D12_ROOT_PARAMETER_TYPE_SRV, 0),
        ],
    )?;
    let mut next = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("the clock is before 1970")?
        .as_millis() as u64
        * 1000;
    let one = |s: &Shaders, cached: &[u8]| {
        let from = qpc();
        let made = gpu::graphics_pipeline(&device, &root, &s.vs, &s.ps, cached);
        (us(from, qpc()), made)
    };

    // One of each is enough to see a cold one: they are different shaders.
    let from = qpc();
    let set = shaders(&mut next, count)?;
    let compiled = us(from, qpc());
    let mut cold = Vec::new();
    let mut kept = Vec::new();
    for s in &set {
        let (time, made) = one(s, &[]);
        cold.push(time);
        kept.push(made?);
    }
    let row = |case: &str, t: &[f64]| Row::new("pipelines", case, "pipelines", 1).cpu(t);
    b.row(
        Row::new(
            "pipelines",
            "(HLSL compiled, not the driver)",
            "shaders",
            2 * count,
        )
        .cpu(&[compiled]),
    );
    b.row(row("cold, one thread", &cold));
    let again = set
        .iter()
        .map(|s| {
            let (time, made) = one(s, &[]);
            made.map(|_| time)
        })
        .collect::<Result<Vec<_>, _>>()?;
    b.row(row("again, while the first is kept", &again));
    // SAFETY: a query on a live pipeline.
    let blobs = kept
        .iter()
        .map(|p| unsafe { p.GetCachedBlob() })
        .collect::<Result<Vec<_>, _>>()?;
    drop(kept);
    let again = set
        .iter()
        .map(|s| {
            let (time, made) = one(s, &[]);
            made.map(|_| time)
        })
        .collect::<Result<Vec<_>, _>>()?;
    b.row(row("again, after the first is released", &again));
    let from_blob = set
        .iter()
        .zip(&blobs)
        .map(|(s, blob)| {
            let (time, made) = one(s, gpu::bytes(blob));
            made.map(|_| time)
        })
        .collect::<Result<Vec<_>, _>>()?;
    b.row(row("from its cached blob", &from_blob));

    // Cold again, on threads: the time for all of them.
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let mut threads: Vec<usize> = vec![1, 4, cores];
    threads.sort_unstable();
    threads.dedup();
    for &n in threads.iter().filter(|&&n| n <= cores) {
        let set = shaders(&mut next, count)?;
        let at = AtomicUsize::new(0);
        let from = qpc();
        std::thread::scope(|scope| -> Result<()> {
            let workers: Vec<_> = (0..n)
                .map(|_| {
                    scope.spawn(|| -> windows::core::Result<()> {
                        while let Some(s) = set.get(at.fetch_add(1, Ordering::Relaxed)) {
                            gpu::graphics_pipeline(&device, &root, &s.vs, &s.ps, &[])?;
                        }
                        Ok(())
                    })
                })
                .collect();
            for w in workers {
                w.join()
                    .map_err(|_| anyhow::anyhow!("a pipeline thread panicked"))??;
            }
            Ok(())
        })?;
        let all = us(from, qpc());
        b.row(
            Row::new(
                "pipelines",
                &format!("cold, {n} threads"),
                "pipelines",
                count,
            )
            .cpu(&[all]),
        );
    }
    Ok(())
}
