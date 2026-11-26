//! The command line end to end: `fixture`, `build`, `verify`, `bench`, on the software
//! adapter (WARP) and, where there is one, on an NVIDIA adapter. The inputs
//! are `fixture`'s synthetic ones; nothing of a title is read or run.

#![cfg(windows)]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn tool<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(args: I) -> Output {
    Command::new(env!("CARGO_BIN_EXE_victory-pso"))
        .args(args)
        .output()
        .expect("the tool starts")
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// `fixture`'s files, in a folder of `dir`.
fn fixtures(dir: &Path) -> PathBuf {
    let to = dir.join("fixtures");
    let o = tool([OsStr::new("fixture"), to.as_os_str()]);
    assert!(o.status.success(), "{}", text(&o));
    to
}

fn build(description: &Path, out: &Path, wave64: &str) -> Output {
    tool([
        OsStr::new("build"),
        description.as_os_str(),
        OsStr::new("--out"),
        out.as_os_str(),
        OsStr::new("--wave64"),
        OsStr::new(wave64),
    ])
}

fn verify(dir: &Path, adapter: &str, run: bool) -> Output {
    let mut args = vec![
        OsStr::new("verify"),
        dir.as_os_str(),
        OsStr::new("--adapter"),
        OsStr::new(adapter),
    ];
    if run {
        args.push(OsStr::new("--run"));
    }
    tool(args)
}

/// Builds each of these compute fixtures for `wave64` and runs it on
/// `adapter`. Returns false, having said why, when the adapter is not there.
fn compute_fixtures_pass(adapter: &str, wave64: &str, names: &[&str]) -> bool {
    let dir = tempfile::tempdir().unwrap();
    let from = fixtures(dir.path());
    for name in names {
        let out = dir.path().join(name);
        let o = build(&from.join(format!("{name}.toml")), &out, wave64);
        assert!(o.status.success(), "{name}: {}", text(&o));
        assert!(text(&o).contains("dxil.dll"), "{name}: {}", text(&o));
        let o = verify(&out, adapter, true);
        if o.status.code() == Some(3) {
            eprintln!("skipped: {}", text(&o).trim());
            return false;
        }
        assert!(o.status.success(), "{name} on {adapter}: {}", text(&o));
        assert!(text(&o).contains("result        : PASS"), "{}", text(&o));
    }
    true
}

/// `skipped-ballot` counts over a 64-lane wave: the translator has forms for
/// hosts with waves of 64 and of 32 lanes, and the software adapter's are
/// neither, so only `bindings` runs there.
#[test]
fn compute_fixtures_run_on_the_software_adapter() {
    compute_fixtures_pass("warp", "native", &["bindings"]);
}

#[test]
fn compute_fixtures_run_on_an_nvidia_adapter() {
    compute_fixtures_pass("nvidia", "wave32", &["bindings", "skipped-ballot"]);
}

/// A small run of every group that opens no window; what it measured is not
/// looked at, only that the results it checks were right. Returns false,
/// having said why, when the adapter is not there.
fn bench_passes(adapter: &str) -> bool {
    let dir = tempfile::tempdir().unwrap();
    let report = dir.path().join("report.toml");
    let o = tool([
        OsStr::new("bench"),
        OsStr::new("--adapter"),
        OsStr::new(adapter),
        OsStr::new("--quick"),
        OsStr::new("--rounds"),
        OsStr::new("1"),
        OsStr::new("--out"),
        report.as_os_str(),
    ]);
    if o.status.code() == Some(3) {
        eprintln!("skipped: {}", text(&o).trim());
        return false;
    }
    assert!(o.status.success(), "{adapter}: {}", text(&o));
    let report: toml::Table = toml::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    let rows = report["row"].as_array().unwrap();
    for group in ["draws", "upload", "sync", "pipelines", "resources"] {
        assert!(
            rows.iter().any(|r| r["group"].as_str() == Some(group)),
            "no row of `{group}` on {adapter}"
        );
    }
    true
}

#[test]
fn the_bench_runs_on_the_software_adapter() {
    assert!(bench_passes("warp"));
}

#[test]
fn the_bench_runs_on_an_nvidia_adapter() {
    bench_passes("nvidia");
}

#[test]
fn the_graphics_fixture_gets_its_pipeline_state() {
    let dir = tempfile::tempdir().unwrap();
    let from = fixtures(dir.path());
    let out = dir.path().join("triangle");
    let o = build(&from.join("triangle.toml"), &out, "wave32");
    assert!(o.status.success(), "{}", text(&o));
    // The state as D3D12 numbers: triangles, R8G8B8A8_UNORM, D32_FLOAT.
    let manifest = std::fs::read_to_string(out.join("pipeline.toml")).unwrap();
    for line in [
        "topology = 3",
        "render_targets = [28]",
        "depth_stencil = 40",
    ] {
        assert!(manifest.contains(line), "no `{line}` in:\n{manifest}");
    }
    let o = verify(&out, "warp", false);
    if o.status.code() == Some(3) {
        eprintln!("skipped: {}", text(&o).trim());
        return;
    }
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("pipeline state: created (graphics)"),
        "{}",
        text(&o)
    );
}

/// The two root constants the other way round must change what the run
/// writes: the fixture can tell a wrong binding from a right one.
#[test]
fn a_wrong_binding_shows_in_what_the_run_writes() {
    let dir = tempfile::tempdir().unwrap();
    let from = fixtures(dir.path());
    let right = std::fs::read_to_string(from.join("bindings.toml")).unwrap();
    assert!(right.contains("1 = [4660, 5]"), "{right}");
    std::fs::write(
        from.join("swapped.toml"),
        right.replace("1 = [4660, 5]", "1 = [5, 4660]"),
    )
    .unwrap();
    let out = dir.path().join("swapped");
    let o = build(&from.join("swapped.toml"), &out, "native");
    assert!(o.status.success(), "{}", text(&o));
    let o = verify(&out, "warp", true);
    if o.status.code() == Some(3) {
        eprintln!("skipped: {}", text(&o).trim());
        return;
    }
    assert_eq!(o.status.code(), Some(4), "{}", text(&o));
    assert!(text(&o).contains("output mismatch: view 2"), "{}", text(&o));
}

#[test]
fn bad_input_fails_with_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let from = fixtures(dir.path());
    let out = dir.path().join("out");
    let describe = |name: &str, text: &str| {
        let path = from.join(name);
        std::fs::write(&path, text).unwrap();
        path
    };

    // Not a shader container at all.
    std::fs::write(from.join("garbage.bin"), [7u8; 64]).unwrap();
    let o = build(
        &describe(
            "garbage.toml",
            "[pipeline]\nkind = \"compute\"\ncompute = \"garbage.bin\"\n",
        ),
        &out,
        "wave32",
    );
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(
        text(&o).contains("garbage.bin: not a DXBC container"),
        "{}",
        text(&o)
    );

    // A container cut short.
    let whole = std::fs::read(from.join("bindings.cs.bin")).unwrap();
    std::fs::write(from.join("cut.bin"), &whole[..200]).unwrap();
    let o = build(
        &describe(
            "cut.toml",
            "[pipeline]\nkind = \"compute\"\ncompute = \"cut.bin\"\n",
        ),
        &out,
        "wave32",
    );
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(
        text(&o).contains("cut.bin: malformed shader"),
        "{}",
        text(&o)
    );

    // State without a D3D12 form, and state this builder does not map.
    let triangle = std::fs::read_to_string(from.join("triangle.toml")).unwrap();
    let o = build(
        &describe(
            "rectangle.toml",
            &triangle.replace("topology = \"triangle\"", "topology = \"rectangle\""),
        ),
        &out,
        "wave32",
    );
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(
        text(&o).contains("unsupported topology `rectangle`"),
        "{}",
        text(&o)
    );
    let o = build(
        &describe(
            "stencil.toml",
            &format!("{triangle}\n[pipeline.stencil]\nenable = true\n"),
        ),
        &out,
        "wave32",
    );
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("unknown field `stencil`"), "{}", text(&o));

    // Nothing was written for any of them.
    assert!(!out.join("pipeline.toml").exists());
}
