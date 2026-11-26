//! Shading of the kinds a title of today draws with, written by us in C,
//! compiled to RDNA2 code by a clang that has LLVM's AMD GPU back end,
//! wrapped, translated, run, and compared with the same arithmetic in Rust:
//! a physically based material in 32-bit and in 16-bit floats, hair (three
//! lobes a strand, strands blended until the pixel is full, a loop each lane
//! leaves at its own time) and the packing of a G-buffer. Nothing of a title.
//!
//! Needs `VICTORY_AMDGPU_CLANG` as `llvm.rs` does; skipped without it.

#![cfg(windows)]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const THREADS: u32 = 256;

/// The C of every case, which adds `u32 shade(u32 t)`: what thread `t`
/// stores.
const COMMON: &str = r#"
typedef unsigned u32;
#define F static inline __attribute__((always_inline)) real
F low(real a, real b) { return a < b ? a : b; }
F high(real a, real b) { return a > b ? a : b; }
F sat(real x) { return low(high(x, (real)0), (real)1); }
F root(real x) { return (real)__builtin_sqrtf((float)x); }
F power(real x, real y) { return (real)__builtin_exp2f((float)y * __builtin_log2f((float)x)); }
"#;

/// GGX, Smith's masking, Schlick's Fresnel and Lambert, a normal from two
/// numbers, then a filmic curve and a gamma.
const MATERIAL: &str = r#"
F material(real u, real v) {
    real nx = (real)1.2 * u - (real)0.6, ny = (real)1.2 * v - (real)0.6;
    real nz = root((real)1 - nx * nx - ny * ny);
    real rough = (real)0.3 + (real)0.6 * u, metal = v;
    const real vx = (real)0.3, vy = (real)0.4, vz = (real)0.8660254;
    const real lx = (real)0.48, ly = (real)-0.6, lz = (real)0.64;
    real hx = vx + lx, hy = vy + ly, hz = vz + lz;
    real hl = (real)1 / root(hx * hx + hy * hy + hz * hz);
    hx = hx * hl; hy = hy * hl; hz = hz * hl;
    real nl = sat(nx * lx + ny * ly + nz * lz);
    real nv = high(nx * vx + ny * vy + nz * vz, (real)0.05);
    real nh = sat(nx * hx + ny * hy + nz * hz);
    real vh = sat(vx * hx + vy * hy + vz * hz);
    real a = rough * rough, a2 = a * a;
    real d = nh * nh * (a2 - (real)1) + (real)1;
    real D = a2 / ((real)3.14159265 * d * d);
    real k = a * (real)0.5;
    real G = (real)0.25 / ((nl * ((real)1 - k) + k) * (nv * ((real)1 - k) + k));
    real f = (real)1 - vh, f2 = f * f, f5 = f2 * f2 * f;
    real sum = (real)0;
    for (int c = 0; c < 3; c++) {
        real base = c == 0 ? (real)0.8 : c == 1 ? (real)0.4 : (real)0.2;
        real f0 = (real)0.04 * ((real)1 - metal) + base * metal;
        real Fr = f0 + ((real)1 - f0) * f5;
        real diffuse = ((real)1 - Fr) * ((real)1 - metal) * base * (real)0.31830989;
        real x = (diffuse + D * G * Fr) * nl * (real)2 + (real)0.03 * base;
        x = sat(x * ((real)2.51 * x + (real)0.03) / (x * ((real)2.43 * x + (real)0.59) + (real)0.14));
        sum = sum + power(high(x, (real)0.001), (real)0.45454545);
    }
    return sum;
}
"#;

/// Marschner's three lobes, strand upon strand.
const HAIR: &str = r#"
F hair(u32 t) {
    float phi = 0.1f + 0.02f * (float)(t & 63), tilt = 0.15f * (float)(t >> 6);
    float left = 1.0f, sum = 0.0f;
    u32 strands = 1 + (t * 7 & 31), seed = t * 2654435761u + 1;
    for (u32 s = 0; s < strands; s++) {
        seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5;
        float alpha = 0.1f + 0.4f * (float)(seed & 255) / 255.0f;
        float strand = 0.0f;
        for (int lobe = 0; lobe < 3; lobe++) {
            float shift = lobe == 0 ? -0.07f : lobe == 1 ? 0.035f : 0.14f;
            float width = lobe == 0 ? 0.09f : lobe == 1 ? 0.045f : 0.18f;
            float theta = tilt + 0.01f * (float)s - shift;
            float M = __builtin_expf(-0.5f * theta * theta / (width * width)) / (width * 2.5066283f);
            float cosp = __builtin_cosf(phi);
            float bent = __builtin_sqrtf(high(0.0f, 0.5f + 0.5f * cosp));
            float f = 1.0f - bent, f2 = f * f;
            float Fr = 0.0465f + 0.9535f * f2 * f2 * f;
            float N = lobe == 0 ? 0.25f * bent * Fr
                    : lobe == 1 ? (1.0f - Fr) * (1.0f - Fr) * __builtin_expf(-3.65f * cosp - 3.98f)
                                : (1.0f - Fr) * (1.0f - Fr) * Fr * __builtin_expf(17.0f * cosp - 16.78f);
            strand = strand + M * N;
        }
        sum = sum + left * alpha * strand;
        left = left * (1.0f - alpha);
        if (left < 0.02f) break;
    }
    return sum + left;
}
"#;

struct Case {
    name: &'static str,
    /// The floats it computes in.
    real: &'static str,
    source: &'static str,
    /// The body of `shade`.
    shade: &'static str,
    expected: fn(u32) -> u32,
    /// How far (absolute, or of what is expected where that is above 1) the
    /// stored float may be from the expected one; exact without it.
    near: Option<f32>,
}

const CASES: &[Case] = &[
    Case {
        name: "material",
        real: "float",
        source: MATERIAL,
        shade: "float r = material(((float)(t & 15) + 0.5f) / 16.0f, ((float)(t >> 4) + 0.5f) / 16.0f);
                return __builtin_bit_cast(u32, r);",
        expected: |t| material(t).to_bits(),
        near: Some(1e-4),
    },
    Case {
        name: "material-in-halves",
        real: "_Float16",
        source: MATERIAL,
        shade: "float u = ((float)(t & 15) + 0.5f) / 16.0f, v = ((float)(t >> 4) + 0.5f) / 16.0f;
                float r = (float)material((_Float16)u, (_Float16)v);
                return __builtin_bit_cast(u32, r);",
        expected: |t| material(t).to_bits(),
        near: Some(0.04),
    },
    Case {
        name: "hair",
        real: "float",
        source: HAIR,
        shade: "float r = hair(t); return __builtin_bit_cast(u32, r);",
        expected: |t| hair(t).to_bits(),
        near: Some(1e-4),
    },
    // Eight bits a channel, as a G-buffer's albedo: t, 255 - t, 85 and 3 t
    // (clamped) of 255, each as a float and back.
    Case {
        name: "gbuffer-albedo",
        real: "float",
        source: "",
        shade: "float c[4] = { (float)t / 255.0f, 1.0f - (float)t / 255.0f, 1.0f / 3.0f, 3.0f * (float)t / 255.0f };
                u32 r = 0;
                for (int i = 0; i < 4; i++) r |= (u32)(sat(c[i]) * 255.0f + 0.5f) << 8 * i;
                return r;",
        expected: |t| t | (255 - t) << 8 | 85 << 16 | (3 * t).min(255) << 24,
        near: None,
    },
    // Two halves in a dword, as a packed normal's or velocity's.
    Case {
        name: "gbuffer-halves",
        real: "_Float16",
        source: "",
        shade: "_Float16 a = (_Float16)((float)t * 0.125f), b = (_Float16)(1.0f - (float)t);
                u32 low16 = __builtin_bit_cast(unsigned short, a), high16 = __builtin_bit_cast(unsigned short, b);
                return low16 | high16 << 16;",
        expected: |t| half(t as f32 * 0.125) | half(1.0 - t as f32) << 16,
        near: None,
    },
];

fn material(t: u32) -> f32 {
    let (u, v) = (((t & 15) as f32 + 0.5) / 16.0, ((t >> 4) as f32 + 0.5) / 16.0);
    let n = [1.2 * u - 0.6, 1.2 * v - 0.6];
    let n = [n[0], n[1], (1.0 - n[0] * n[0] - n[1] * n[1]).sqrt()];
    let (rough, metal) = (0.3 + 0.6 * u, v);
    let (e, l) = ([0.3f32, 0.4, 0.866_025_4], [0.48f32, -0.6, 0.64]);
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let h = [e[0] + l[0], e[1] + l[1], e[2] + l[2]];
    let k = 1.0 / dot(h, h).sqrt();
    let h = h.map(|x| x * k);
    let nl = dot(n, l).clamp(0.0, 1.0);
    let nv = dot(n, e).max(0.05);
    let nh = dot(n, h).clamp(0.0, 1.0);
    let vh = dot(e, h).clamp(0.0, 1.0);
    let a = rough * rough;
    let d = nh * nh * (a * a - 1.0) + 1.0;
    let spread = a * a / (std::f32::consts::PI * d * d);
    let k = a * 0.5;
    let g = 0.25 / ((nl * (1.0 - k) + k) * (nv * (1.0 - k) + k));
    let f5 = (1.0 - vh).powi(5);
    [0.8f32, 0.4, 0.2]
        .iter()
        .map(|&base| {
            let f0 = 0.04 * (1.0 - metal) + base * metal;
            let fr = f0 + (1.0 - f0) * f5;
            let diffuse = (1.0 - fr) * (1.0 - metal) * base * std::f32::consts::FRAC_1_PI;
            let x = (diffuse + spread * g * fr) * nl * 2.0 + 0.03 * base;
            let x = (x * (2.51 * x + 0.03) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0);
            x.max(0.001).powf(1.0 / 2.2)
        })
        .sum()
}

fn hair(t: u32) -> f32 {
    let (phi, tilt) = (0.1 + 0.02 * (t & 63) as f32, 0.15 * (t >> 6) as f32);
    let (mut left, mut sum) = (1.0f32, 0.0f32);
    let mut seed = t.wrapping_mul(2_654_435_761).wrapping_add(1);
    for s in 0..1 + ((t * 7) & 31) {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let alpha = 0.1 + 0.4 * (seed & 255) as f32 / 255.0;
        let mut strand = 0.0;
        for (lobe, (shift, width)) in [(-0.07f32, 0.09f32), (0.035, 0.045), (0.14, 0.18)]
            .into_iter()
            .enumerate()
        {
            let theta = tilt + 0.01 * s as f32 - shift;
            let m = (-0.5 * theta * theta / (width * width)).exp() / (width * 2.506_628_3);
            let cosp = phi.cos();
            let bent = (0.5 + 0.5 * cosp).max(0.0).sqrt();
            let fr = 0.0465 + 0.9535 * (1.0 - bent).powi(5);
            let n = match lobe {
                0 => 0.25 * bent * fr,
                1 => (1.0 - fr) * (1.0 - fr) * (-3.65 * cosp - 3.98).exp(),
                _ => (1.0 - fr) * (1.0 - fr) * fr * (17.0 * cosp - 16.78).exp(),
            };
            strand += m * n;
        }
        sum += left * alpha * strand;
        left *= 1.0 - alpha;
        if left < 0.02 {
            break;
        }
    }
    sum + left
}

/// The bits of `x` as a half; every value here is one, or zero.
fn half(x: f32) -> u32 {
    let bits = x.to_bits();
    if bits << 1 == 0 {
        return bits >> 16;
    }
    bits >> 16 & 0x8000 | ((bits >> 23 & 0xFF) - 112) << 10 | bits >> 13 & 0x3FF
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn tool(args: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_victory-pso"))
        .args(args)
        .output()
        .expect("the tool starts")
}

fn clang(clang: &Path, args: &[&str], from: &Path, to: &Path) -> Result<(), Option<String>> {
    let o = Command::new(clang)
        .args(["--target=amdgcn--", "-mcpu=gfx1030"])
        .args(args)
        .arg("-Wno-override-module")
        .arg(from)
        .arg("-o")
        .arg(to)
        .output()
        .expect("clang starts");
    if o.status.success() {
        Ok(())
    } else {
        Err(Some(text(&o)))
    }
}

/// The compute shader around `shade`, its waves of `wave` lanes.
fn main_ir(wave: u32) -> String {
    format!(
        r#"
declare void @llvm.amdgcn.raw.buffer.store.i32(i32, <4 x i32>, i32, i32, i32)

define amdgpu_cs void @main(i32 inreg %heap, i32 inreg %unused, i32 %thread) #90 {{
  %value = call i32 @shade(i32 %thread)
  %descriptor = inttoptr i32 %heap to ptr addrspace(6)
  %buffer = load <4 x i32>, ptr addrspace(6) %descriptor, align 4, !invariant.load !90
  %offset = shl i32 %thread, 2
  call void @llvm.amdgcn.raw.buffer.store.i32(i32 %value, <4 x i32> %buffer, i32 %offset, i32 0, i32 0)
  ret void
}}

attributes #90 = {{ "target-cpu"="gfx1030" "target-features"="+wavefrontsize{wave}" }}

!90 = !{{}}
"#
    )
}

/// `Err(None)`: no such adapter here.
fn run_case(
    compiler: &Path,
    dir: &Path,
    case: &Case,
    wave: u32,
    adapter: &str,
) -> Result<(), Option<String>> {
    let name = format!("{}-{wave}", case.name);
    let file = |ext: &str| dir.join(format!("{name}.{ext}"));
    let c = format!(
        "#define real {}\n{COMMON}{}\nstatic __attribute__((always_inline)) u32 shade(u32 t) {{ {} }}\n\
         u32 used(u32 t) {{ return shade(t); }}\n",
        case.real, case.source, case.shade
    );
    std::fs::write(file("c"), c).unwrap();
    // The C as IR, unoptimised, so `main` is optimised with it.
    let wavefront = format!("+wavefrontsize{wave}");
    let front = [
        "-x",
        "c",
        "-O1",
        "-fno-math-errno",
        "-ffp-contract=off",
        "-S",
        "-emit-llvm",
        "-Xclang",
        "-disable-llvm-passes",
        "-Xclang",
        "-target-feature",
        "-Xclang",
        &wavefront,
    ];
    clang(compiler, &front, &file("c"), &file("c.ll"))?;
    let mut ir = std::fs::read_to_string(file("c.ll")).unwrap();
    // (`used` kept `shade` from being left out; nothing calls it)
    ir = ir.replace("define hidden i32 @used", "define internal i32 @used");
    ir += &main_ir(wave);
    std::fs::write(file("ll"), ir).unwrap();
    clang(compiler, &["-x", "ir", "-c", "-O2"], &file("ll"), &file("o"))?;

    std::fs::write(
        file("program.toml"),
        format!(
            "code = \"{name}.o\"\nthreads = [{THREADS}, 1, 1]\nuser_sgprs = 2\nwave32 = {}\n\n\
             [[param]]\nroot = \"resource-heap\"\nslot = 0\nuser_data = 0\n",
            wave == 32
        ),
    )
    .unwrap();
    let o = tool(&[
        "wrap".as_ref(),
        file("program.toml").as_ref(),
        "--out".as_ref(),
        file("cs.bin").as_ref(),
    ]);
    if !o.status.success() {
        return Err(Some(format!("does not wrap: {}", text(&o).trim())));
    }
    let want: Vec<u32> = (0..THREADS).map(case.expected).collect();
    let near = case.near.map_or(String::new(), |n| format!("near = {n:?}\n"));
    std::fs::write(
        file("toml"),
        format!(
            "[pipeline]\nkind = \"compute\"\ncompute = \"{name}.cs.bin\"\n\n[run]\ngroups = [1, 1, 1]\n\n\
             [[run.view]]\nkind = \"raw-uav\"\ndwords = {THREADS}\n\n\
             [[run.expect]]\nview = 0\ndwords = {want:?}\n{near}"
        ),
    )
    .unwrap();
    let out = dir.join(&name);
    let form = if wave == 64 && adapter == "nvidia" { "wave32" } else { "native" };
    let o = tool(&[
        "build".as_ref(),
        file("toml").as_ref(),
        "--out".as_ref(),
        out.as_ref(),
        "--wave64".as_ref(),
        form.as_ref(),
    ]);
    if !o.status.success() {
        return Err(Some(format!("does not translate: {}", text(&o).trim())));
    }
    let o = tool(&[
        "verify".as_ref(),
        out.as_ref(),
        "--adapter".as_ref(),
        adapter.as_ref(),
        "--run".as_ref(),
    ]);
    if o.status.code() == Some(3) {
        eprintln!("skipped: {}", text(&o).trim());
        return Err(None);
    }
    if !o.status.success() || !text(&o).contains("result        : PASS") {
        let said = text(&o);
        let last = said.trim().lines().last().unwrap_or_default();
        return Err(Some(last.to_owned()));
    }
    Ok(())
}

fn shading_is_right(adapter: &str, waves: &[u32]) {
    let Some(compiler) = std::env::var_os("VICTORY_AMDGPU_CLANG").map(PathBuf::from) else {
        eprintln!("skipped: VICTORY_AMDGPU_CLANG is not set");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut failed = Vec::new();
    for case in CASES {
        for &wave in waves {
            match run_case(&compiler, dir.path(), case, wave, adapter) {
                Ok(()) => eprintln!("{} (wave{wave}, {adapter}): ok", case.name),
                Err(None) => return,
                Err(Some(why)) => failed.push(format!("{} (wave{wave}, {adapter}): {why}", case.name)),
            }
        }
    }
    assert!(failed.is_empty(), "\n{}", failed.join("\n"));
}

#[test]
fn shading_is_right_on_an_nvidia_adapter() {
    shading_is_right("nvidia", &[32, 64]);
}

#[test]
fn shading_is_right_on_the_software_adapter() {
    shading_is_right("warp", &[64]);
}
