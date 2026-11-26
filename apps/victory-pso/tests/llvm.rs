//! Test shaders written as LLVM IR, compiled to RDNA2 code by a clang that
//! has LLVM's AMD GPU back end, wrapped, translated and run: a real
//! compiler's code for shaders of our own. Nothing of a title.
//!
//! Needs `VICTORY_AMDGPU_CLANG`, the path of such a clang. The official LLVM
//! builds for Windows lack the back end; the clang AMD ships with its Radeon
//! GPU Analyzer has it (`utils\lc\opencl\bin\clang.exe`). Skipped without it.

#![cfg(windows)]

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn tool<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(args: I) -> Output {
    Command::new(env!("CARGO_BIN_EXE_victory-pso"))
        .args(args)
        .output()
        .expect("the tool starts")
}

/// The clang to compile with, if one is configured.
fn clang() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("VICTORY_AMDGPU_CLANG")?);
    assert!(
        path.is_file(),
        "VICTORY_AMDGPU_CLANG names no file: {}",
        path.display()
    );
    Some(path)
}

/// A test shader and what running it must give.
struct Case {
    name: &'static str,
    /// The body of the shader: LLVM IR computing `value` from `%thread` (the
    /// thread id, `v0`), `%heap` (`s0`) and, with constants, `%k0` and `%k1`
    /// (`s2`, `s3`). [`ir`] puts the store of `value` after it.
    body: &'static str,
    value: &'static str,
    threads: u32,
    /// The two dwords of the title's root constants (root parameter 1).
    constants: Option<[u32; 2]>,
    /// Dwords of each buffer of the view heap, and which one the shader writes.
    views: &'static [u32],
    written: usize,
    /// What thread `t` stores at dword `t`.
    expected: fn(u32) -> u32,
    /// It counts over the wave: the host must have waves of the size it was
    /// translated for (32 or 64 lanes), which the software adapter has not.
    waves: bool,
    /// Hosts of 32-lane waves have no exact form of it: it must be translated
    /// approximately and say so, and is not run there.
    inexact: bool,
}

/// The whole shader of `case`: a compute function in the calling convention
/// of AMD's shaders (`inreg` arguments arrive in SGPRs from `s0`, the others
/// in VGPRs from `v0`), ending with `value` stored at dword `thread` of the
/// raw buffer whose descriptor is number `written` of the view heap.
fn ir(case: &Case) -> String {
    let constants = if case.constants.is_some() {
        "i32 inreg %k0, i32 inreg %k1, "
    } else {
        ""
    };
    format!(
        r#"target triple = "amdgcn--"

declare i64 @llvm.amdgcn.icmp.i64.i32(i32, i32, i32)
declare i64 @llvm.ctpop.i64(i64)
declare i32 @llvm.amdgcn.readfirstlane.i32(i32)
declare void @llvm.amdgcn.raw.buffer.store.i32(i32, <4 x i32>, i32, i32, i32)

define amdgpu_cs void @main(i32 inreg %heap, i32 inreg %unused, {constants}i32 %thread) #0 {{
{body}
  %at = add i32 %heap, {at}
  %descriptor = inttoptr i32 %at to ptr addrspace(6)
  %buffer = load <4 x i32>, ptr addrspace(6) %descriptor, align 4, !invariant.load !0
  %offset = shl i32 %thread, 2
  call void @llvm.amdgcn.raw.buffer.store.i32(i32 {value}, <4 x i32> %buffer, i32 %offset, i32 0, i32 0)
  ret void
}}

attributes #0 = {{ "target-cpu"="gfx1030" "target-features"="+wavefrontsize64" }}

!0 = !{{}}
"#,
        body = case.body,
        value = case.value,
        // A view descriptor is 32 bytes on the console.
        at = 32 * case.written,
    )
}

/// The lanes of the 64-lane wave thread `t` is in, as thread ids.
fn wave(t: u32, threads: u32) -> std::ops::Range<u32> {
    let first = t & !63;
    first..(first + 64).min(threads)
}

/// How often a lane of the two loop shaders loops.
fn turns(t: u32) -> u32 {
    (t & 7).max(1)
}

/// One step of the generator `per-lane-loop` runs (xorshift32).
fn xorshift(mut x: u32) -> u32 {
    x ^= x << 13;
    x ^= x >> 17;
    x ^ x << 5
}

const CASES: &[Case] = &[
    // Root constants and a buffer that is not the heap's first: a wrong
    // heap index, or the constants the other way round, shows.
    Case {
        name: "bindings",
        body: "  %sum = add i32 %thread, %k0
  %shifted = shl i32 %sum, 8
  %value = or i32 %shifted, %k1",
        value: "%value",
        threads: 64,
        constants: Some([0x1234, 5]),
        views: &[64, 64, 64],
        written: 2,
        expected: |t| (t + 0x1234) << 8 | 5,
        waves: false,
        inexact: false,
    },
    // A wave count in an `if` the lanes below 32 take: of those lanes, the
    // ones with a thread id below 70 (36 is "unsigned less than").
    Case {
        name: "wave-count-in-if",
        body: "entry:
  %lane = and i32 %thread, 63
  %low = icmp ult i32 %lane, 32
  br i1 %low, label %then, label %end

then:
  %below = call i64 @llvm.amdgcn.icmp.i64.i32(i32 %thread, i32 70, i32 36)
  %bits = call i64 @llvm.ctpop.i64(i64 %below)
  %count = trunc i64 %bits to i32
  br label %end

end:
  %value = phi i32 [ %count, %then ], [ 0, %entry ]",
        value: "%value",
        threads: 128,
        constants: None,
        views: &[128],
        written: 0,
        expected: |t| {
            if t & 63 >= 32 {
                return 0;
            }
            wave(t, 128).filter(|l| l & 63 < 32 && *l < 70).count() as u32
        },
        waves: true,
        inexact: false,
    },
    // An `if` on a float condition every thread shares: the wave's lanes
    // above 40 when taken (34 is "unsigned greater than"), else K1.
    Case {
        name: "uniform-float-if-taken",
        body: UNIFORM_FLOAT_IF,
        value: "%value",
        threads: 128,
        constants: Some([0x3F80_0000, 7]), // 1.0
        views: &[128],
        written: 0,
        expected: |t| wave(t, 128).filter(|l| l & 63 > 40).count() as u32,
        waves: true,
        inexact: false,
    },
    Case {
        name: "uniform-float-if-skipped",
        body: UNIFORM_FLOAT_IF,
        value: "%value",
        threads: 128,
        constants: Some([0x3E80_0000, 7]), // 0.25
        views: &[128],
        written: 0,
        expected: |_| 7,
        waves: true,
        inexact: false,
    },
    // A loop each lane leaves at its own time: `turns` steps of a generator
    // no compiler folds into a formula.
    Case {
        name: "per-lane-loop",
        body: "entry:
  %turns = and i32 %thread, 7
  %seed = add i32 %thread, 1
  br label %loop

loop:
  %i = phi i32 [ 0, %entry ], [ %next, %loop ]
  %x = phi i32 [ %seed, %entry ], [ %x3, %loop ]
  %l = shl i32 %x, 13
  %x1 = xor i32 %x, %l
  %r = lshr i32 %x1, 17
  %x2 = xor i32 %x1, %r
  %l2 = shl i32 %x2, 5
  %x3 = xor i32 %x2, %l2
  %next = add i32 %i, 1
  %more = icmp ult i32 %next, %turns
  br i1 %more, label %loop, label %done

done:",
        value: "%x3",
        threads: 128,
        constants: None,
        views: &[128],
        written: 0,
        expected: |t| (0..turns(t)).fold(t + 1, |x, _| xorshift(x)),
        waves: false,
        inexact: false,
    },
    // A value every lane of the wave holds alike, fetched from the first
    // lane (what compilers use the instruction for): three times the thread
    // id of the wave's first lane, plus the lane's own.
    // (Of a value that differs between lanes the translator gives the first
    // lane of the host's wave, as `WaveReadLaneFirst` does: on a 32-lane
    // host that is not the console wave's first lane for lanes 32 to 63.)
    Case {
        name: "first-lane",
        body: "  %wave = and i32 %thread, -64
  %tripled = mul i32 %wave, 3
  %first = call i32 @llvm.amdgcn.readfirstlane.i32(i32 %tripled)
  %value = add i32 %first, %thread",
        value: "%value",
        threads: 128,
        constants: None,
        views: &[128],
        written: 0,
        expected: |t| 3 * (t & !63) + t,
        waves: false,
        inexact: false,
    },
    // A wave count in a loop the lanes leave at different times: at each
    // turn, how many of the wave's lanes are still in it (32 is "equal"). Two
    // 32-lane host waves would have to wait for each other inside the loop,
    // where the other may have left: D3D12 says nothing of such a barrier, and
    // hardware may wait for ever.
    Case {
        name: "wave-count-in-loop",
        body: "entry:
  %turns = and i32 %thread, 7
  br label %loop

loop:
  %i = phi i32 [ 0, %entry ], [ %next, %loop ]
  %sum = phi i32 [ 0, %entry ], [ %added, %loop ]
  %here = call i64 @llvm.amdgcn.icmp.i64.i32(i32 %thread, i32 %thread, i32 32)
  %bits = call i64 @llvm.ctpop.i64(i64 %here)
  %count = trunc i64 %bits to i32
  %added = add i32 %sum, %count
  %next = add i32 %i, 1
  %more = icmp ult i32 %next, %turns
  br i1 %more, label %loop, label %done

done:",
        value: "%added",
        threads: 128,
        constants: None,
        views: &[128],
        written: 0,
        expected: |t| {
            (0..turns(t))
                .map(|i| wave(t, 128).filter(|&l| turns(l) > i).count() as u32)
                .sum()
        },
        waves: true,
        inexact: true,
    },
];

const UNIFORM_FLOAT_IF: &str = "entry:
  %level = bitcast i32 %k0 to float
  %on = fcmp ogt float %level, 5.000000e-01
  br i1 %on, label %then, label %end

then:
  %lane = and i32 %thread, 63
  %above = call i64 @llvm.amdgcn.icmp.i64.i32(i32 %lane, i32 40, i32 34)
  %bits = call i64 @llvm.ctpop.i64(i64 %above)
  %count = trunc i64 %bits to i32
  br label %end

end:
  %value = phi i32 [ %count, %then ], [ %k1, %entry ]";

/// Compiles the case's shader and writes its container and pipeline
/// description into `dir`; returns the description.
fn prepare(clang: &Path, dir: &Path, case: &Case) -> PathBuf {
    let source = dir.join(format!("{}.ll", case.name));
    std::fs::write(&source, ir(case)).unwrap();
    let object = dir.join(format!("{}.o", case.name));
    let o = Command::new(clang)
        .args([
            "-x",
            "ir",
            "-c",
            "--target=amdgcn--",
            "-mcpu=gfx1030",
            "-O2",
        ])
        .arg("-Wno-override-module")
        .arg(&source)
        .arg("-o")
        .arg(&object)
        .output()
        .expect("clang starts");
    assert!(o.status.success(), "{}: {}", case.name, text(&o));

    // s0: the view heap's address; s2, s3: the root constants, if any.
    let mut program = format!(
        "code = \"{}.o\"\nthreads = [{}, 1, 1]\nuser_sgprs = {}\n\n\
         [[param]]\nroot = \"resource-heap\"\nslot = 0\nuser_data = 0\n",
        case.name,
        case.threads,
        if case.constants.is_some() { 4 } else { 2 },
    );
    if case.constants.is_some() {
        program += "\n[[param]]\nroot = \"constants\"\nslot = 1\nuser_data = 2\nsgprs = 2\n";
    }
    let described = dir.join(format!("{}.program.toml", case.name));
    std::fs::write(&described, program).unwrap();
    let container = dir.join(format!("{}.cs.bin", case.name));
    let o = tool([
        OsStr::new("wrap"),
        described.as_os_str(),
        OsStr::new("--out"),
        container.as_os_str(),
    ]);
    assert!(o.status.success(), "{}: {}", case.name, text(&o));

    let mut pipeline = format!(
        "[pipeline]\nkind = \"compute\"\ncompute = \"{}.cs.bin\"\n\n[run]\ngroups = [1, 1, 1]\n",
        case.name
    );
    if let Some(k) = case.constants {
        write!(pipeline, "\n[run.constants]\n1 = {k:?}\n").unwrap();
    }
    for dwords in case.views {
        write!(
            pipeline,
            "\n[[run.view]]\nkind = \"raw-uav\"\ndwords = {dwords}\n"
        )
        .unwrap();
    }
    // The buffer the shader writes, and every other one still zero.
    for (view, &dwords) in case.views.iter().enumerate() {
        let want: Vec<u32> = if view == case.written {
            (0..dwords).map(case.expected).collect()
        } else {
            vec![0; dwords as usize]
        };
        write!(
            pipeline,
            "\n[[run.expect]]\nview = {view}\ndwords = {want:?}\n"
        )
        .unwrap();
    }
    let path = dir.join(format!("{}.toml", case.name));
    std::fs::write(&path, pipeline).unwrap();
    path
}

/// `build`, with the translator saying which form of a 64-lane program it
/// took on a 32-lane host (`VICTORY_WAVE_WHY`).
fn build(description: &Path, out: &Path, wave64: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_victory-pso"))
        .arg("build")
        .arg(description)
        .arg("--out")
        .arg(out)
        .args(["--wave64", wave64])
        .env("VICTORY_WAVE_WHY", "1")
        .output()
        .expect("the tool starts")
}

/// The form the translator settled for, if it was one of its approximate
/// ones: each host wave counted as a wave of its own, which gives a count
/// over the console wave two half counts. Such a translation validates, runs
/// and writes other values than the console would.
fn approximate(build: &Output) -> Option<String> {
    let said = text(build);
    said.lines()
        .find(|l| l.contains("APPROXIMATE") && l.contains(": wave ") && !l.contains("Some(64)"))
        .map(str::to_owned)
}

/// What went wrong with one case built for `wave64` and run on `adapter`,
/// if anything. `Err(None)`: the adapter is not there.
fn run_case(
    clang: &Path,
    dir: &Path,
    case: &Case,
    adapter: &str,
    wave64: &str,
) -> Result<(), Option<String>> {
    let description = prepare(clang, dir, case);
    let out = dir.join(format!("{}-{wave64}", case.name));
    let o = build(&description, &out, wave64);
    if !o.status.success() {
        return Err(Some(format!("does not translate: {}", text(&o).trim())));
    }
    if !text(&o).contains("dxil.dll") {
        return Err(Some(format!("not validated: {}", text(&o).trim())));
    }
    match (approximate(&o), case.inexact && wave64 == "wave32") {
        (Some(_), true) => return Ok(()),
        (None, true) => return Err(Some("claims to be exact, and cannot be".to_owned())),
        (Some(form), false) => return Err(Some(format!("translated approximately: {form}"))),
        (None, false) => {}
    }
    let o = tool([
        OsStr::new("verify"),
        out.as_os_str(),
        OsStr::new("--adapter"),
        OsStr::new(adapter),
        OsStr::new("--run"),
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

/// The cases `wanted` picks, built for `wave64` and run on `adapter`: every
/// one is tried, and all that failed are named together.
fn compiled_shaders_pass(adapter: &str, wave64: &str, wanted: impl Fn(&Case) -> bool) {
    let Some(clang) = clang() else {
        eprintln!("skipped: VICTORY_AMDGPU_CLANG is not set");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut failed = Vec::new();
    for case in CASES.iter().filter(|c| wanted(c)) {
        match run_case(&clang, dir.path(), case, adapter, wave64) {
            Ok(()) => eprintln!("{} ({wave64}, {adapter}): ok", case.name),
            Err(None) => return,
            Err(Some(why)) => failed.push(format!("{} ({wave64}, {adapter}): {why}", case.name)),
        }
    }
    assert!(failed.is_empty(), "\n{}", failed.join("\n"));
}

/// The software adapter's waves have neither 32 nor 64 lanes, and the
/// translator has no form for other sizes: only the cases that do not count
/// over a wave run there.
#[test]
fn compiled_shaders_run_on_the_software_adapter() {
    compiled_shaders_pass("warp", "native", |c| !c.waves);
}

#[test]
fn compiled_shaders_run_on_an_nvidia_adapter() {
    compiled_shaders_pass("nvidia", "wave32", |_| true);
}
