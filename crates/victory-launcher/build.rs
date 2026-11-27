//! Embed the existing icon in the Windows executable.
use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=../../branding/vu-icon.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    match compile_icon() {
        Ok(path) => println!(
            "cargo:rustc-link-arg-bin=victory-launcher={}",
            path.display()
        ),
        Err(error) => println!("cargo:warning=Windows launcher icon resource unavailable: {error}"),
    }
}

fn compile_icon() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let target = env::var("TARGET")?;
    let branding = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?)
        .join("../../branding")
        .canonicalize()?;
    let out = PathBuf::from(env::var("OUT_DIR")?);
    let script = out.join("victory-icon.rc");
    fs::write(&script, "1 ICON \"vu-icon.ico\"\n")?;
    let result = if target.ends_with("-msvc") {
        let resource = out.join("victory-icon.res");
        let mut rc = cc::windows_registry::find_tool(&target, "rc.exe").map_or_else(
            || Command::new(if cfg!(windows) { "rc.exe" } else { "llvm-rc" }),
            |tool| tool.to_command(),
        );
        let compiled = rc
            .current_dir(&branding)
            .arg("/nologo")
            .arg("/fo")
            .arg(&resource)
            .arg(&script)
            .status()?;
        (resource, compiled)
    } else {
        let object = out.join("victory-icon.o");
        let compiler = env::var_os("WINDRES").unwrap_or_else(|| "windres".into());
        let compiled = Command::new(compiler)
            .current_dir(branding)
            .args(["-O", "coff"])
            .arg(&script)
            .arg(&object)
            .status()?;
        (object, compiled)
    };
    if !result.1.success() {
        return Err(format!("resource compiler exited with {}", result.1).into());
    }
    Ok(result.0)
}
