<<<<<<< HEAD
//! A stand-in for the indirect light the title traces rays for.
//!
//! With the ray-tracing structure builder off (`crate::pipeline::builder_runs`)
//! the title's rays find nothing. Its program that resolves the diffuse light
//! of its screen probes (`CS_ScreenProbes_TemporalResolve_*`) then leaves the
//! sky's light outdoors, nothing indoors (walls are black where no lamp or
//! sun reaches) and, in blocks of the picture's tiles, what the probes'
//! memory held. Right after that program the light is raised to an estimate
//! and bounded by a multiple of it.
//!
//! That a stand-in can be put there, and in which of the program's images,
//! is beloooo's find (VIctory-Belobug-AMD, 06031237, 4f45ab88, cd9f78ce,
//! GPL-3.0-or-later), which puts a constant there, shaded in creases from the
//! scene's depth. The estimate here is worked out from the frame instead, so
//! it follows the hour, the room and its lamps by itself: the light the
//! visible surfaces send out (the title keeps it for its own screen traces:
//! `CS_CacheScreenSpaceRadiance`, the frame before) is averaged in 64x36
//! cells, and every point gets the mean of the cells weighted by the share of
//! its surroundings they cover, as a directional value (first-order harmonics
//! at 160x90 points, taken between them by depth).
//!
//! What it assumes: what is not on screen sends out what is (with a slow
//! mean of the last seconds, so that turning into a dark corner is not black
//! at once). No occlusion but the creases'.
//!
//! `VICTORY_AMBIENT`:
//! - unset or `0`/`off`: the title's own output.
//! - a number: that share of the estimate (1: all of it).
//! - `constant:<level>`: the constant, as beloooo's fork has it.
//!
//! `VICTORY_AMBIENT_SHADE=<strength>,<radius in metres>` sets the creases.
//!
//! The air of a room is lit by the same probes: without rays they hold the
//! sky's light indoors too, a brown veil at sunset. `VICTORY_AMBIENT_AIR`
//! bounds what the air scatters by a multiple of the light around.

use std::sync::{Mutex, OnceLock};

use windows::Win32::Graphics::Direct3D12::*;
use windows::core::s;

use crate::host::{HostList, host};

/// Programs of the title the stand-in follows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Role {
    /// Resolves the diffuse light.
    Resolve,
    /// Keeps the light visible surfaces send out.
    Radiance,
    /// Adds the lamps' light to what the air scatters (the last program
    /// that writes that volume before it is summed up along the view).
    Air,
}

/// The program's name in the container, as the title starts it.
pub(crate) const PROGRAM: &[u8] = b"CS_ScreenProbes_TemporalResolve";

/// The program's part in this, by its name in the container.
pub(crate) fn role(container: &[u8]) -> Option<Role> {
    let has = |name: &[u8]| container.windows(name.len()).any(|w| w == name);
    if has(PROGRAM) {
        Some(Role::Resolve)
    } else if has(b"CS_CacheScreenSpaceRadiance") {
        Some(Role::Radiance)
    } else if has(b"CS_AccumulateLocalLightInscatteringVolume") {
        Some(Role::Air)
    } else {
        None
    }
}

/// Where the resolve writes the diffuse light (R9G9B9E5 as 32-bit integers)
/// and reads the scene's depth, and where the other writes the light of
/// visible surfaces: root parameter and place in its table, in dwords.
pub(crate) const LIGHT: (u8, u32) = (1, 176);
pub(crate) const DEPTH: (u8, u32) = (0, 464);
pub(crate) const RADIANCE: (u8, u32) = (1, 16);
/// Where the light the air scatters is.
pub(crate) const AIR: (u8, u32) = (1, 32);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Set {
    Off,
    Estimate(f32),
    Constant(f32),
}

fn parse(set: &str) -> Set {
    let set = set.trim();
    let number = |t: &str| t.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0);
    match set.strip_prefix("constant:") {
        Some(level) => number(level).map_or(Set::Off, Set::Constant),
        None => number(set).map_or(Set::Off, Set::Estimate),
    }
}

fn set() -> Set {
    static SET: OnceLock<Set> = OnceLock::new();
    *SET.get_or_init(|| parse(&std::env::var("VICTORY_AMBIENT").unwrap_or_default()))
}

/// Whether the stand-in is on: set, and the title's rays find nothing.
pub(crate) fn on() -> bool {
    set() != Set::Off && !crate::pipeline::builder_runs()
}

/// The most the air scatters, as a multiple of the light around
/// (`VICTORY_AMBIENT_AIR=<multiple>`; unset or 0: what the title has).
fn air() -> f32 {
    static SET: OnceLock<f32> = OnceLock::new();
    *SET.get_or_init(|| {
        let set = std::env::var("VICTORY_AMBIENT_AIR").unwrap_or_default();
        set.trim().parse().ok().filter(|v: &f32| v.is_finite() && *v > 0.0).unwrap_or(0.0)
    })
}

/// Strength and radius of the creases' shade.
fn shade() -> [f32; 2] {
    static SET: OnceLock<[f32; 2]> = OnceLock::new();
    *SET.get_or_init(|| {
        let mut shade = [1.0, 0.6];
        let set = std::env::var("VICTORY_AMBIENT_SHADE").unwrap_or_default();
        for (to, v) in shade.iter_mut().zip(set.split(',')) {
            if let Ok(v) = v.trim().parse() {
                *to = v;
            }
        }
        shade
    })
}

const SOURCE: &str = r#"
#define RS "RootConstants(num32BitConstants=12, b0), DescriptorTable(SRV(t0)), DescriptorTable(UAV(u0)), UAV(u1), UAV(u2)"
#define CX 64
#define CY 36
#define CELLS (CX * CY)
#define PX 160
#define PY 90
cbuffer Set : register(b0) {
    float level; float constant; float strength; float radius;
    float near; float tan_y; uint width; uint height;
    float follow; float most; float air; float unused;
};
Texture2D<float> depth : register(t0);
// Cells: the light they send out (then, at CELLS, the slow mean), from
// CELLS + 1 on where they are.
RWStructuredBuffer<float4> cells : register(u1);
// Three a point: red, green and blue, each a mean and a direction.
RWStructuredBuffer<float4> points : register(u2);
static const float3 LUMA = float3(0.2126, 0.7152, 0.0722);

// The point seen at a texel, in metres from the eye (x right, y up, z away).
float3 seen(int2 p, float2 size) {
    float d = depth.Load(int3(clamp(p, int2(0, 0), int2(size) - 1), 0));
    float z = min(near / max(d, 1e-7), 1000.0);
    float2 ndc = (float2(p) + 0.5) / size * 2.0 - 1.0;
    return float3(ndc.x * tan_y * size.x / size.y * z, -ndc.y * tan_y * z, z);
}

#ifdef KEEP
RWTexture2D<float3> radiance : register(u0);
[RootSignature(RS)]
[numthreads(8, 4, 1)]
void keep(uint3 at : SV_DispatchThreadID) {
    uint w, h;
    radiance.GetDimensions(w, h);
    float3 sum = 0;
    for (uint y = 0; y < 5; y++) for (uint x = 0; x < 5; x++) {
        uint2 p = uint2((float2(at.xy) + (float2(x, y) + 0.5) / 5.0) * float2(w, h) / float2(CX, CY));
        float3 c = max(radiance[p], 0.0);
        c = any(isnan(c)) || any(isinf(c)) ? 0.0 : c;
        // Lamps and glints count, but not for everything.
        sum += c / max(1.0, dot(c, LUMA) / 20.0);
    }
    cells[at.y * CX + at.x] = float4(sum / 25.0, 1.0);
}
[RootSignature(RS)]
[numthreads(1, 1, 1)]
void mean() {
    float3 sum = 0;
    for (uint i = 0; i < CELLS; i++) sum += cells[i].rgb;
    float4 was = cells[CELLS];
    float3 now = sum / CELLS;
    cells[CELLS] = float4(was.a > 0.0 ? lerp(was.rgb, now, follow) : now, 1.0);
}
#endif

#ifdef PLACE
[RootSignature(RS)]
[numthreads(8, 4, 1)]
void place(uint3 at : SV_DispatchThreadID) {
    float2 size = float2(width, height);
    float3 p = seen(int2((float2(at.xy) + 0.5) * size / float2(CX, CY)), size);
    cells[CELLS + 1 + at.y * CX + at.x] = float4(p, p.z * p.z);
}
#endif

#ifdef GATHER
[RootSignature(RS)]
[numthreads(8, 2, 1)]
void gather(uint3 at : SV_DispatchThreadID) {
    float2 size = float2(width, height);
    float3 p = seen(int2((float2(at.xy) + 0.5) * size / float2(PX, PY)), size);
    float r2 = 0.25 + 0.0025 * p.z * p.z;
    float whole = 0;
    float3 toward = 0, light = 0, red = 0, green = 0, blue = 0;
    for (uint i = 0; i < CELLS; i++) {
        float4 c = cells[CELLS + 1 + i];
        float3 L = cells[i].rgb;
        float3 d = c.xyz - p;
        float dd = dot(d, d);
        float w = c.w / (dd + r2);
        float3 dir = d / sqrt(dd + 0.25 * r2);
        whole += w;
        toward += w * dir;
        light += w * L;
        red += w * L.r * dir;
        green += w * L.g * dir;
        blue += w * L.b * dir;
    }
    whole = max(whole, 1e-6);
    light /= whole;
    toward /= whole;
    uint at0 = (at.y * PX + at.x) * 3;
    points[at0] = float4(red / whole - light.r * toward, light.r);
    points[at0 + 1] = float4(green / whole - light.g * toward, light.g);
    points[at0 + 2] = float4(blue / whole - light.b * toward, light.b);
}
#endif

#ifdef AIR
RWTexture3D<float3> scattered : register(u0);
// No more than a multiple of the light around: of the room as the last
// seconds showed it, and of the surface seen in this direction (towards a
// window the air outside may be as bright as the sky is).
[RootSignature(RS)]
[numthreads(8, 8, 2)]
void clear_air(uint3 at : SV_DispatchThreadID) {
    uint w, h, d;
    scattered.GetDimensions(w, h, d);
    if (at.x >= w || at.y >= h || at.z >= d) return;
    uint2 q = min(uint2((float2(at.xy) + 0.5) / float2(w, h) * float2(PX, PY)), uint2(PX - 1, PY - 1));
    uint at0 = (q.y * PX + q.x) * 3;
    float3 seen_there = float3(points[at0].w, points[at0 + 1].w, points[at0 + 2].w);
    float far = float(at.z) / float(d);
    float cap = air * lerp(dot(cells[CELLS].rgb, LUMA), dot(seen_there, LUMA), far);
    float3 c = scattered[at];
    float has = dot(c, LUMA);
    if (has > cap) scattered[at] = c * (cap / has);
}
#endif

#ifdef RAISE
RWTexture2D<uint> light : register(u0);
float3 unpack(uint v) {
    return float3(v & 511, (v >> 9) & 511, (v >> 18) & 511) * exp2(float(v >> 27) - 24.0);
}
uint pack(float3 c) {
    c = clamp(c, 0.0, 65408.0);
    float top = max(c.r, max(c.g, c.b));
    int e = clamp(int(floor(log2(max(top, 1e-9)))) + 16, 0, 31);
    float scale = exp2(float(24 - e));
    if (top * scale >= 511.5) { e = min(e + 1, 31); scale = exp2(float(24 - e)); }
    uint3 m = min(uint3(c * scale + 0.5), 511u);
    return m.r | (m.g << 9) | (m.b << 18) | (uint(e) << 27);
}
[RootSignature(RS)]
[numthreads(8, 8, 1)]
void raise(uint3 at : SV_DispatchThreadID) {
    float2 size = float2(width, height);
    if (at.x >= width || at.y >= height) return;
    int2 p = int2(at.xy);
    float3 c = seen(p, size);
    float3 l = seen(p + int2(-1, 0), size), r = seen(p + int2(1, 0), size);
    float3 u = seen(p + int2(0, -1), size), d = seen(p + int2(0, 1), size);
    float3 dx = abs(l.z - c.z) < abs(r.z - c.z) ? c - l : r - c;
    float3 dy = abs(u.z - c.z) < abs(d.z - c.z) ? c - u : d - c;
    float3 n = normalize(cross(dy, dx) + float3(0, 0, -1e-9));
    if (dot(n, c) > 0.0) n = -n;
    // How open it is (as in beloooo's cd9f78ce).
    float open = 1.0;
    float px = min(radius * size.y / (2.0 * tan_y * c.z), 128.0);
    float noise = frac(52.9829189 * frac(dot(float2(p), float2(0.06711056, 0.00583715))));
    if (strength > 0.0 && px >= 1.5 && c.z < 400.0) {
        float sum = 0.0;
        const int N = 12;
        for (int i = 0; i < N; i++) {
            float a = i * 2.399963 + noise * 6.2831853;
            float2 o = float2(cos(a), sin(a)) * (sqrt((i + 0.5) / N) * px);
            float3 v = seen(p + int2(round(o)), size) - c;
            float vv = dot(v, v);
            float over = dot(v, n) - 0.004 * c.z - 0.005;
            sum += max(over, 0.0) / sqrt(vv + 1e-6) * saturate(1.0 - vv / (radius * radius));
        }
        open = saturate(1.0 - strength * 2.0 * sum / N);
    }
    float3 found;
    if (constant > 0.0) {
        found = float3(1.0, 0.98, 0.94) * constant * open;
        light[p] = pack(found);
        return;
    }
    // Between the four points around, those at its own depth first; which
    // four is dithered a little.
    float2 g = (float2(p) + 0.5) / size * float2(PX, PY) - 0.5 + (noise - 0.5) * 0.5;
    int2 g0 = int2(floor(g));
    float2 f = g - float2(g0);
    float4 sr = 0, sg = 0, sb = 0;
    float sum = 0;
    for (int k = 0; k < 4; k++) {
        int2 q = clamp(g0 + int2(k & 1, k >> 1), int2(0, 0), int2(PX, PY) - 1);
        float z = seen(int2((float2(q) + 0.5) * size / float2(PX, PY)), size).z;
        float2 b = float2(k & 1 ? f.x : 1 - f.x, k >> 1 ? f.y : 1 - f.y);
        float w = (b.x * b.y + 1e-3) / (0.02 + abs(z - c.z) / c.z);
        uint at0 = (q.y * PX + q.x) * 3;
        sr += w * points[at0];
        sg += w * points[at0 + 1];
        sb += w * points[at0 + 2];
        sum += w;
    }
    float3 all = float3(sr.w, sg.w, sb.w) / sum;
    found = all + 1.2 * float3(dot(sr.xyz, n), dot(sg.xyz, n), dot(sb.xyz, n)) / sum;
    found = clamp(found, 0.2 * all, 2.5 * all);
    found = lerp(found, cells[CELLS].rgb, 0.2) * level * open;
    float3 was = unpack(light[p]);
    // What the program left counts as far as it can be true: the sky's light
    // outdoors, not the blocks of other memory.
    float top = max(dot(found, LUMA) * most, 1e-6);
    float has = dot(was, LUMA);
    was *= min(1.0, top / max(has, 1e-6));
    light[p] = pack(max(was, found));
}
#endif
"#;

/// Bytes of the cells' buffer and of the points'.
const CELLS: u64 = 64 * 36;
const CELL_BYTES: u64 = (2 * CELLS + 1) * 16;
const POINT_BYTES: u64 = 160 * 90 * 48;

struct Pass {
    root: ID3D12RootSignature,
    keep: ID3D12PipelineState,
    mean: ID3D12PipelineState,
    place: ID3D12PipelineState,
    gather: ID3D12PipelineState,
    raise: ID3D12PipelineState,
    air: ID3D12PipelineState,
    cells: ID3D12Resource,
    points: ID3D12Resource,
}
// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for Pass {}
// SAFETY: as above.
unsafe impl Sync for Pass {}

fn make(device: &ID3D12Device, buffer: impl Fn(u64) -> Option<ID3D12Resource>) -> windows::core::Result<Pass> {
    let mut root: Option<ID3D12RootSignature> = None;
    let mut pipeline = |part: &str, entry| -> windows::core::Result<ID3D12PipelineState> {
        let code = crate::host::compile(&format!("#define {part}\n{SOURCE}"), entry, s!("cs_5_1"))?;
        // SAFETY: the root signature is inside the shader; plain creations.
        unsafe {
            if root.is_none() {
                root = Some(device.CreateRootSignature(0, &code)?);
            }
            let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: std::mem::transmute_copy(&root),
                CS: D3D12_SHADER_BYTECODE { pShaderBytecode: code.as_ptr().cast(), BytecodeLength: code.len() },
                ..Default::default()
            };
            device.CreateComputePipelineState(&desc)
        }
    };
    let keep = pipeline("KEEP", s!("keep"))?;
    let mean = pipeline("KEEP", s!("mean"))?;
    let place = pipeline("PLACE", s!("place"))?;
    let gather = pipeline("GATHER", s!("gather"))?;
    let raise = pipeline("RAISE", s!("raise"))?;
    let air = pipeline("AIR", s!("clear_air"))?;
    let made = |bytes| buffer(bytes).ok_or_else(windows::core::Error::empty);
    Ok(Pass {
        root: root.ok_or_else(windows::core::Error::empty)?,
        keep,
        mean,
        place,
        gather,
        raise,
        air,
        cells: made(CELL_BYTES)?,
        points: made(POINT_BYTES)?,
    })
}

fn pass() -> Option<&'static Pass> {
    static PASS: OnceLock<Option<Pass>> = OnceLock::new();
    PASS.get_or_init(|| {
        let host = host()?;
        make(&host.device, |bytes| host.uav_buffer(bytes))
            .inspect_err(|e| tracing::warn!(error = %e, "no stand-in for the indirect light"))
            .ok()
    })
    .as_ref()
}

fn uav_barrier(list: &ID3D12GraphicsCommandList) {
    let barrier = D3D12_RESOURCE_BARRIER { Type: D3D12_RESOURCE_BARRIER_TYPE_UAV, ..Default::default() };
    // SAFETY: an open list; the barrier names no resource.
    unsafe { list.ResourceBarrier(&[barrier]) };
}

/// Whether the light of a frame has been kept: the estimate has something
/// to begin with.
static KEPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What the passes read and write, as views in the heap the list has set.
pub(crate) struct Views {
    /// The image the title's program just wrote.
    pub(crate) image: D3D12_GPU_DESCRIPTOR_HANDLE,
    /// The scene's depth, for [`Role::Resolve`].
    pub(crate) depth: D3D12_GPU_DESCRIPTOR_HANDLE,
    /// The size of the image, and how many slices it has.
    pub(crate) size: [u32; 2],
    pub(crate) slices: u32,
}

/// After the title's program of that role, in `list` with the program's view
/// heap set. Replaces the list's compute root signature and pipeline; false
/// if it recorded nothing.
pub(crate) fn after(list: &HostList, role: Role, views: &Views) -> bool {
    use std::sync::atomic::Ordering;
    let Some(pass) = pass() else {
        return false;
    };
    let (level, constant) = match set() {
        Set::Off => return false,
        Set::Estimate(level) => (level, 0.0f32),
        Set::Constant(level) => (0.0, level),
    };
    if role != Role::Radiance && constant == 0.0 && !KEPT.load(Ordering::Relaxed) {
        return false;
    }
    if role == Role::Air && (constant > 0.0 || air() == 0.0) {
        return false;
    }
    static SAID: Mutex<u32> = Mutex::new(0);
    {
        let mut said = SAID.lock().unwrap_or_else(|e| e.into_inner());
        if *said < 4 {
            *said += 1;
            tracing::info!(?role, size = ?views.size, level, constant, "a stand-in for the indirect light");
        }
    }
    list.keep(&pass.cells);
    list.keep(&pass.points);
    let [strength, radius] = shade();
    let floats = [level, constant, strength, radius, 0.1, 0.45];
    let mut words = [0u32; 12];
    for (to, v) in words.iter_mut().zip(floats) {
        *to = v.to_bits();
    }
    words[6] = views.size[0];
    words[7] = views.size[1];
    words[8] = 0.04f32.to_bits();
    words[9] = 4.0f32.to_bits();
    words[10] = air().to_bits();
    let l = &list.list;
    // SAFETY: an open list with the heap of the views set; the title's
    // program has just used both views, so their textures are in the states
    // these views need; buffers the list keeps.
    unsafe {
        uav_barrier(l);
        l.SetComputeRootSignature(&pass.root);
        l.SetComputeRoot32BitConstants(0, 12, words.as_ptr().cast(), 0);
        l.SetComputeRootDescriptorTable(1, views.depth);
        l.SetComputeRootDescriptorTable(2, views.image);
        l.SetComputeRootUnorderedAccessView(3, pass.cells.GetGPUVirtualAddress());
        l.SetComputeRootUnorderedAccessView(4, pass.points.GetGPUVirtualAddress());
        let run = |pso: &ID3D12PipelineState, x, y, z| {
            l.SetPipelineState(pso);
            l.Dispatch(x, y, z);
            uav_barrier(l);
        };
        match role {
            Role::Radiance => {
                run(&pass.keep, 8, 9, 1);
                run(&pass.mean, 1, 1, 1);
                KEPT.store(true, Ordering::Relaxed);
            }
            Role::Air => run(&pass.air, views.size[0].div_ceil(8), views.size[1].div_ceil(8), views.slices.div_ceil(2)),
            Role::Resolve => {
                if constant == 0.0 {
                    run(&pass.place, 8, 9, 1);
                    run(&pass.gather, 20, 45, 1);
                }
                run(&pass.raise, views.size[0].div_ceil(8), views.size[1].div_ceil(8), 1);
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings() {
        assert_eq!(parse(""), Set::Off);
        assert_eq!(parse("off"), Set::Off);
        assert_eq!(parse("0"), Set::Off);
        assert_eq!(parse("0.8"), Set::Estimate(0.8));
        assert_eq!(parse("constant:0.3"), Set::Constant(0.3));
        assert_eq!(parse("constant:nan"), Set::Off);
    }

    #[test]
    fn roles_by_name() {
        assert_eq!(role(b"..CS_ScreenProbes_TemporalResolve_a911b49c_Wrapped.."), Some(Role::Resolve));
        assert_eq!(role(b"..CS_CacheScreenSpaceRadiance_eed2fc35.."), Some(Role::Radiance));
        assert_eq!(role(b"CS_AccumulateLocalLightInscatteringVolume_7e2f5eef"), Some(Role::Air));
        assert_eq!(role(b"CS_TemporalResolve"), None);
    }

    /// The passes compile with the system's compiler and the host takes them.
    #[test]
    fn the_passes_build() {
        let Some(host) = host() else {
            return;
        };
        make(&host.device, |bytes| host.uav_buffer(bytes)).expect("the passes");
    }
}
||||||| 085ee7c4
=======
//! A least amount of indirect light (ported from Belobug/VIctory-Belobug-AMD,
//! GPL-3.0-or-later).
//!
//! The title's indirect diffuse light comes from probes it fills by tracing
//! rays, which do not run here yet: its program that resolves them
//! (`CS_ScreenProbes_TemporalResolve_*`) leaves next to nothing, and whatever
//! the sun does not reach directly is black. Right after that program, its
//! output is replaced by a constant shaded by depth occlusion from the scene's
//! depth buffer. The title's own lighting then applies it with the surfaces'
//! colors.
//!
//! `VICTORY_AMBIENT=<level>` sets it (0.6 by default, 0 for none); `off`
//! keeps the title's own output. `VICTORY_AMBIENT_TINT=r,g,b` sets the tint,
//! and `VICTORY_AMBIENT_SHADE=s,r,n,t` sets depth-occlusion shading.
//! The ambient light follows the rendered sky dome by default when available.
//! `VICTORY_AMBIENT_SKY=off` keeps the constant.
//! `VICTORY_AMBIENT_SKY=gain,least,power` tunes the response.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::s;

use crate::host::{HostList, host};
use crate::object::This;

/// The programs it follows, by the start of their name.
pub(crate) const PROGRAM: &[u8] = b"CS_ScreenProbes_TemporalResolve";
/// Where they write the indirect diffuse light (R9G9B9E5 as 32-bit
/// integers): root parameter and place in its table.
pub(crate) const LIGHT: (u8, u32) = (1, 176);

/// 0.6 looked balanced; `VICTORY_AMBIENT=off` leaves what the title's resolve
/// wrote; 0 is no indirect diffuse light at all.
const DEFAULT: f32 = 0.6;

/// The floor; `None` if off.
pub(crate) fn level() -> Option<f32> {
    static LEVEL: OnceLock<f32> = OnceLock::new();
    let level = *LEVEL.get_or_init(|| {
        let set = std::env::var("VICTORY_AMBIENT").unwrap_or_default();
        if set.trim() == "off" {
            return -1.0;
        }
        set.trim()
            .parse()
            .ok()
            .filter(|l: &f32| l.is_finite() && *l >= 0.0)
            .unwrap_or(DEFAULT)
    });
    (level >= 0.0).then_some(level)
}

/// The light's colour: warm white, or what `VICTORY_AMBIENT_TINT=r,g,b`
/// gives.
fn tint() -> [f32; 3] {
    static SET: OnceLock<[f32; 3]> = OnceLock::new();
    *SET.get_or_init(|| {
        let set: Vec<f32> = std::env::var("VICTORY_AMBIENT_TINT")
            .unwrap_or_default()
            .split(',')
            .filter_map(|v| v.trim().parse().ok())
            .collect();
        match set[..] {
            [r, g, b] => [r, g, b],
            _ => [1.0, 0.98, 0.94],
        }
    })
}

/// Indirect light and reflection settings last read from the ambient file.
#[derive(Clone, Copy, PartialEq)]
struct Live {
    level: f32,
    tint: [f32; 3],
    ceiling: f32,
    floor: f32,
}

#[derive(Clone, Copy)]
struct AmbientFile {
    constant: Option<Live>,
    sky: Option<[f32; 3]>,
}

static LIVE: std::sync::Mutex<AmbientFile> = std::sync::Mutex::new(AmbientFile {
    constant: None,
    sky: None,
});

/// The last reflection history update and the UAV it writes.
pub(crate) const REFLECTED: &[u8] = b"CS_UpdateHistory";
pub(crate) const REFLECTIONS: (u8, u32) = (1, 40);

/// The program that draws the sky as the title reflects it, by the start of
/// its name: a small image of the whole sky, in the light's own units.
pub(crate) const SKY: &[u8] = b"ps_sky_paraboloid_reflection";

/// The sky dome that program last drew into: the host copy, held, since
/// the title's object may go at any time.
///
/// The program draws twice a frame, on two lists recorded at once: the dome
/// (128x128, R16G16B16A16_FLOAT) and another image of the sky (R11G11B10).
/// Following whichever was recorded last made the light jump between the
/// two, three times apart, from frame to frame (run don1, 2026-11-25).
static SKY_IMAGE: std::sync::Mutex<Option<std::sync::Arc<crate::resource::HostTexture>>> =
    std::sync::Mutex::new(None);

pub(crate) fn sky_drawn(image: usize) {
    if level().is_none_or(|level| level == 0.0) || sky_settings().is_none() {
        return;
    }
    let Some(copy) = crate::resource::host_copy(image as This) else {
        return;
    };
    // SAFETY: a live resource.
    let d = unsafe { copy.texture.GetDesc() };
    if d.Format != DXGI_FORMAT_R16G16B16A16_FLOAT
        || d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
    {
        return;
    }
    let mut held = SKY_IMAGE.lock().unwrap_or_else(|e| e.into_inner());
    if !held
        .as_ref()
        .is_some_and(|h| std::sync::Arc::ptr_eq(h, &copy))
    {
        *held = Some(copy);
    }
}

const DEFAULT_SKY: [f32; 3] = [0.36, 0.05, 0.4];

fn sky_settings() -> Option<[f32; 3]> {
    static SETTINGS: OnceLock<Option<[f32; 3]>> = OnceLock::new();
    *SETTINGS.get_or_init(|| {
        let set = std::env::var("VICTORY_AMBIENT_SKY").unwrap_or_default();
        if set.trim() == "off" {
            return None;
        }
        let mut settings = DEFAULT_SKY;
        for (to, value) in settings.iter_mut().zip(set.split(',')) {
            if let Ok(value) = value.trim().parse::<f32>()
                && value.is_finite()
                && value >= 0.0
            {
                *to = value;
            }
        }
        Some(settings)
    })
}

/// Where the resolve reads the scene's depth (reversed, D32 with stencil):
/// root parameter and place in its table.
pub(crate) const DEPTH: (u8, u32) = (0, 464);

/// How much less of the light reaches creases and corners, worked out from
/// the depth image alone: strength (0 for none), the radius looked at in
/// metres, the near plane's distance, and the tangent of half the vertical
/// field of view. `VICTORY_AMBIENT_SHADE=s,r,n,t` sets them.
static SHADE: std::sync::Mutex<Option<[f32; 4]>> = std::sync::Mutex::new(None);

fn shade() -> [f32; 4] {
    if let Some(live) = *SHADE.lock().unwrap_or_else(|e| e.into_inner()) {
        return live;
    }
    static SET: OnceLock<[f32; 4]> = OnceLock::new();
    *SET.get_or_init(|| {
        let mut shade = [1.0, 0.6, 0.1, 0.45];
        let set = std::env::var("VICTORY_AMBIENT_SHADE").unwrap_or_default();
        for (to, v) in shade.iter_mut().zip(set.split(',')) {
            if let Ok(v) = v.trim().parse() {
                *to = v;
            }
        }
        shade
    })
}

/// Reads `VICTORY_AMBIENT_FILE` twice a second in diagnostics builds. Its
/// values are `level r g b`, optionally followed by `strength radius near
/// tan_y` and then the reflection ceiling and floor as fractions of ambient
/// light. Zero disables the corresponding reflection bound. A file with
/// two or three values sets sky gain, minimum and optional power. Add four
/// values to that form to adjust depth shading.
fn live_settings() -> AmbientFile {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        if !cfg!(feature = "diagnostics") {
            return;
        }
        let Some(file) = std::env::var_os("VICTORY_AMBIENT_FILE") else {
            return;
        };
        let _ = std::thread::Builder::new()
            .name("victory-ambient".into())
            .spawn(move || {
                loop {
                    let read: Vec<f32> = std::fs::read_to_string(&file)
                        .unwrap_or_default()
                        .split_whitespace()
                        .filter_map(|v| v.parse().ok())
                        .collect();
                    if matches!(read.len(), 2 | 3 | 6 | 7) {
                        let sky_values = if read.len() >= 6 {
                            read.len() - 4
                        } else {
                            read.len()
                        };
                        let mut settings = DEFAULT_SKY;
                        for (to, &value) in settings.iter_mut().zip(&read[..sky_values]) {
                            if value.is_finite() && value >= 0.0 {
                                *to = value;
                            }
                        }
                        if let [s, rad, n, t] = read[sky_values..] {
                            *SHADE.lock().unwrap_or_else(|e| e.into_inner()) = Some([s, rad, n, t]);
                        }
                        LIVE.lock().unwrap_or_else(|e| e.into_inner()).sky = Some(settings);
                    } else if let [level, r, g, b, ref rest @ ..] = read[..] {
                        if let [s, rad, n, t, ..] = rest[..] {
                            *SHADE.lock().unwrap_or_else(|e| e.into_inner()) = Some([s, rad, n, t]);
                        }
                        let fraction = |i| {
                            rest.get(i)
                                .copied()
                                .filter(|f: &f32| f.is_finite())
                                .unwrap_or(0.0)
                                .max(0.0)
                        };
                        let new = Some(Live {
                            level: level.max(0.0),
                            tint: [r, g, b],
                            ceiling: fraction(4),
                            floor: fraction(5),
                        });
                        let mut live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
                        if live.constant != new || live.sky.is_some() {
                            tracing::info!(level, r, g, b, "ambient light set from the file");
                            *live = AmbientFile {
                                constant: new,
                                sky: None,
                            };
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            });
    });
    *LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

const SOURCE: &str = r#"
#define RS "RootConstants(num32BitConstants=13, b0), DescriptorTable(UAV(u0), SRV(t0), SRV(t1))"
cbuffer Floor : register(b0) {
    float level; float3 tint;
    float strength; float radius; float near; float tan_y;
    float held; float floor_part;
    float gain; float least; float power;
};
Texture2DArray<float4> sky : register(t1);
// The light, from the sky's mean when it follows the sky (gain above 0).
float3 from_around() {
    if (gain <= 0.0) return tint * level;
    uint sw, sh, slices;
    sky.GetDimensions(sw, sh, slices);
    float3 sum = 0.0;
    const int N = 8;
    for (int y = 0; y < N; y++)
        for (int x = 0; x < N; x++) {
            // The sun's own texels are held down: it lights by itself.
            float3 there = sky.Load(int4((x * 2 + 1) * sw / (N * 2), (y * 2 + 1) * sh / (N * 2), 0, 0)).rgb;
            sum += clamp(there, 0.0, 8.0);
        }
    float3 mean = sum / (N * N);
    float m = dot(mean, float3(0.2126, 0.7152, 0.0722));
    // The sky's colour, half washed out (the ground lights too); a dark
    // sky has none worth following.
    const float3 NIGHT = float3(0.6, 0.75, 1.0);
    float3 hue = lerp(NIGHT, lerp(float3(1.0, 1.0, 1.0), mean / max(m, 1e-6), 0.5), saturate(m / 0.05));
    return hue * (gain * pow(max(m, 0.0), power) + least);
}
RWTexture2D<uint> light : register(u0);
Texture2D<float> depth : register(t0);
float3 seen(int2 p, float2 size) {
    float d = depth.Load(int3(clamp(p, int2(0, 0), int2(size) - 1), 0));
    float z = near / max(d, 1e-7);
    float2 ndc = (float2(p) + 0.5) / size * 2.0 - 1.0;
    return float3(ndc.x * tan_y * size.x / size.y * z, -ndc.y * tan_y * z, z);
}
float open_to_light(int2 p, float2 size) {
    float3 c = seen(p, size);
    if (c.z > 400.0) return 1.0;
    float3 l = seen(p + int2(-1, 0), size), r = seen(p + int2(1, 0), size);
    float3 u = seen(p + int2(0, -1), size), d = seen(p + int2(0, 1), size);
    float3 dx = abs(l.z - c.z) < abs(r.z - c.z) ? c - l : r - c;
    float3 dy = abs(u.z - c.z) < abs(d.z - c.z) ? c - u : d - c;
    float3 n = normalize(cross(dy, dx));
    if (dot(n, c) > 0.0) n = -n;
    float px = radius * size.y / (2.0 * tan_y * c.z);
    if (px < 1.5) return 1.0;
    px = min(px, 128.0);
    float noise = frac(52.9829189 * frac(dot(float2(p), float2(0.06711056, 0.00583715))));
    float sum = 0.0;
    const int N = 16;
    for (int i = 0; i < N; i++) {
        float a = i * 2.399963 + noise * 6.2831853;
        float2 o = float2(cos(a), sin(a)) * (sqrt((i + 0.5) / N) * px);
        float3 v = seen(p + int2(round(o)), size) - c;
        float vv = dot(v, v);
        float over = dot(v, n) - 0.004 * c.z - 0.005;
        sum += max(over, 0.0) / sqrt(vv + 1e-6) * saturate(1.0 - vv / (radius * radius));
    }
    return saturate(1.0 - strength * 2.0 * sum / N);
}
float3 unpack(uint v) {
    return float3(v & 511, (v >> 9) & 511, (v >> 18) & 511) * exp2(float(v >> 27) - 24.0);
}
uint pack(float3 c) {
    c = clamp(c, 0.0, 65408.0);
    float most = max(c.r, max(c.g, c.b));
    int e = clamp(int(floor(log2(max(most, 1e-9)))) + 16, 0, 31);
    float scale = exp2(float(24 - e));
    if (most * scale >= 511.5) { e = min(e + 1, 31); scale = exp2(float(24 - e)); }
    uint3 m = min(uint3(c * scale + 0.5), 511u);
    return m.r | (m.g << 9) | (m.b << 18) | (uint(e) << 27);
}
groupshared float3 group_ambient;
[RootSignature(RS)]
[numthreads(8, 8, 1)]
void main(uint3 at : SV_DispatchThreadID, uint lane : SV_GroupIndex) {
    float3 ambient = tint * level;
    if (gain > 0.0) {
        if (lane == 0) group_ambient = from_around();
        GroupMemoryBarrierWithGroupSync();
        ambient = group_ambient;
    }
    uint w, h;
    light.GetDimensions(w, h);
    if (at.x >= w || at.y >= h) return;
    if (held > 0.0 || floor_part > 0.0) {
        const float3 LUMA = float3(0.2126, 0.7152, 0.0722);
        float3 reflection = unpack(light[at.xy]);
        float has = dot(reflection, LUMA);
        float most = dot(ambient, LUMA) * held;
        float3 floor_light = ambient * floor_part;
        if (held > 0.0 && has > most) light[at.xy] = pack(reflection * (most / has));
        else if (has < dot(floor_light, LUMA)) light[at.xy] = pack(floor_light);
        return;
    }
    float open = strength > 0.0 ? open_to_light(int2(at.xy), float2(w, h)) : 1.0;
    light[at.xy] = pack(ambient * open);
}
"#;

/// Views, used in turn: more than the passes of every list in flight.
const SLOTS: u32 = 1024;

struct Pass {
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    views: ID3D12DescriptorHeap,
    step: u32,
    next: AtomicU32,
}
// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for Pass {}
// SAFETY: as above.
unsafe impl Sync for Pass {}

fn make(device: &ID3D12Device) -> windows::core::Result<Pass> {
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source and names live through the call; out pointers ours.
    let compiled = unsafe {
        D3DCompile(
            SOURCE.as_ptr().cast(),
            SOURCE.len(),
            s!("ambient"),
            None,
            None,
            s!("main"),
            s!("cs_5_1"),
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    let text = |b: &ID3DBlob| {
        // SAFETY: the blob's own bytes.
        unsafe { std::slice::from_raw_parts(b.GetBufferPointer().cast::<u8>(), b.GetBufferSize()) }
            .to_vec()
    };
    if let Err(e) = compiled {
        if let Some(errors) = errors {
            tracing::warn!("ambient light: {}", String::from_utf8_lossy(&text(&errors)));
        }
        return Err(e);
    }
    let code = text(&code.ok_or_else(windows::core::Error::empty)?);
    // SAFETY: the root signature is inside the shader; plain creations.
    unsafe {
        let root: ID3D12RootSignature = device.CreateRootSignature(0, &code)?;
        let pso = device.CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            CS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: code.as_ptr().cast(),
                BytecodeLength: code.len(),
            },
            ..Default::default()
        })?;
        let kind = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
        let views = device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: kind,
            NumDescriptors: SLOTS * 3,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        })?;
        Ok(Pass {
            root,
            pso,
            views,
            step: device.GetDescriptorHandleIncrementSize(kind),
            next: AtomicU32::new(0),
        })
    }
}

fn uav_barrier(list: &ID3D12GraphicsCommandList) {
    let barrier = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
        ..Default::default()
    };
    // SAFETY: an open list; the barrier names no resource.
    unsafe { list.ResourceBarrier(&[barrier]) };
}

/// Raises what `light` holds to the floor, in `list`, after the program that
/// wrote it as an image. Whether it recorded anything (the list's bindings
/// are then ours).
pub(crate) fn raise(list: &HostList, light: This, depth: Option<This>) -> bool {
    record(list, light, depth, false)
}

/// Applies the configured ambient bounds after the reflection history update.
pub(crate) fn reflect(list: &HostList, light: This) -> bool {
    record(list, light, None, true)
}

fn record(list: &HostList, light: This, depth: Option<This>, reflections: bool) -> bool {
    let (Some(level), Some(host)) = (level(), host()) else {
        return false;
    };
    let file = live_settings();
    let live = file.constant;
    let mut bounds = [0.0; 2];
    if reflections {
        let Some(live) = live else {
            return false;
        };
        let floor = if live.ceiling > 0.0 {
            live.floor.min(live.ceiling)
        } else {
            live.floor
        };
        bounds = [live.ceiling, floor];
        if bounds == [0.0; 2] {
            return false;
        }
    }
    let Some(light) = crate::resource::host_made(light) else {
        return false;
    };
    static PASS: OnceLock<Option<Pass>> = OnceLock::new();
    let Some(pass) = PASS
        .get_or_init(|| {
            make(&host.device)
                .inspect_err(|e| tracing::warn!(error = %e, "no ambient light"))
                .ok()
        })
        .as_ref()
    else {
        return false;
    };
    // SAFETY: a live resource.
    let d = unsafe { light.GetDesc() };
    static ONCE: std::sync::Once = std::sync::Once::new();
    if d.Format != DXGI_FORMAT_R32_UINT {
        ONCE.call_once(|| tracing::warn!(format = d.Format.0, "no ambient light: another format"));
        return false;
    }
    ONCE.call_once(|| {
        tracing::info!(level, width = d.Width, height = d.Height, "ambient light");
    });
    let slot = pass.next.fetch_add(1, Ordering::Relaxed) % SLOTS * 3;
    let sky = sky_settings()
        .map(|settings| file.sky.unwrap_or(settings))
        .filter(|settings| {
            level > 0.0 && settings[0] > 0.0 && (live.is_none() || file.sky.is_some())
        })
        .and_then(|settings| {
            let image = SKY_IMAGE
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()?;
            Some((image, settings))
        });
    if let Some((image, _)) = &sky {
        list.settle(
            image,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 as u32,
        );
    }
    list.keep(light);
    // The depth the resolve just read, in the state it read it in; of the
    // light's size, or the shading is left out.
    let depth = depth.and_then(crate::resource::host_made).filter(|t| {
        // SAFETY: a live resource.
        let t = unsafe { t.GetDesc() };
        t.Format == DXGI_FORMAT_R32G8X24_TYPELESS && (t.Width, t.Height) == (d.Width, d.Height)
    });
    if let Some(depth) = depth {
        list.keep(depth);
    }
    let l = &list.list;
    uav_barrier(l);
    // SAFETY: an open list; a live texture the list keeps, in the
    // unordered-access state as the program before left it; a view in a slot
    // no list in flight reads.
    unsafe {
        let cpu = pass.views.GetCPUDescriptorHandleForHeapStart();
        let gpu = pass.views.GetGPUDescriptorHandleForHeapStart();
        let at = D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: cpu.ptr + (slot * pass.step) as usize,
        };
        let view = D3D12_UNORDERED_ACCESS_VIEW_DESC {
            Format: DXGI_FORMAT_R32_UINT,
            ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        host.device
            .CreateUnorderedAccessView(light, None::<&ID3D12Resource>, Some(&view), at);
        let seen = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                },
            },
        };
        let beside = D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: at.ptr + pass.step as usize,
        };
        host.device
            .CreateShaderResourceView(depth, Some(&seen), beside);
        let around = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2DARRAY,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2DArray: D3D12_TEX2D_ARRAY_SRV {
                    MipLevels: 1,
                    ArraySize: 1,
                    ..Default::default()
                },
            },
        };
        let third = D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: at.ptr + 2 * pass.step as usize,
        };
        host.device.CreateShaderResourceView(
            sky.as_ref().map(|(image, _)| &image.texture),
            Some(&around),
            third,
        );
        l.SetDescriptorHeaps(&[Some(pass.views.clone())]);
        l.SetComputeRootSignature(&pass.root);
        l.SetPipelineState(&pass.pso);
        let [r, g, b] = live.map_or_else(tint, |live| live.tint);
        let level = live.map_or(level, |live| live.level);
        let [strength, radius, near, tan_y] = shade();
        let strength = if depth.is_some() { strength } else { 0.0 };
        let [held, floor] = bounds;
        let [gain, least, power] = sky
            .as_ref()
            .map_or([0.0, 0.0, 1.0], |(_, settings)| *settings);
        let constants = [
            level, r, g, b, strength, radius, near, tan_y, held, floor, gain, least, power,
        ];
        l.SetComputeRoot32BitConstants(0, constants.len() as u32, constants.as_ptr().cast(), 0);
        l.SetComputeRootDescriptorTable(
            1,
            D3D12_GPU_DESCRIPTOR_HANDLE {
                ptr: gpu.ptr + u64::from(slot * pass.step),
            },
        );
        l.Dispatch((d.Width as u32).div_ceil(8), d.Height.div_ceil(8), 1);
    }
    uav_barrier(l);
    true
}

#[cfg(test)]
mod tests {
    /// The pass compiles with the system's compiler and the host takes it.
    #[test]
    fn ambient_pass_builds() {
        let Some(host) = crate::host::host() else {
            return;
        };
        super::make(&host.device).expect("the ambient pass");
    }
}
>>>>>>> vitoriabr/main
