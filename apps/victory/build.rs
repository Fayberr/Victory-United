<<<<<<< HEAD
// LOCAL-ONLY (branch local/win10-agility, never pushed): exports the two
// symbols D3D12 reads from the process executable to select an app-local
// D3D12 Agility SDK runtime. Without them, Windows 10's in-box d3d12.dll
// refuses every translated pipeline's root signature
// ("Unsupported bit-flag set (root signature flags c00)").
fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/EXPORT:D3D12SDKVersion,DATA");
        println!("cargo:rustc-link-arg-bins=/EXPORT:D3D12SDKPath,DATA");
    }
}
||||||| a66ac212
=======
//! (Neptune) The diagnostics build exports the two symbols by which the D3D12 loader finds a
//! redistributable runtime and debug layer beside the executable (`D3D12\`): the debug layer
//! Windows installs is older than its D3D12 runtime on some PCs, and a device is then refused.
fn main() {
    // (ours) Only when asked for (`VICTORY_AGILITY=1` at build time): without the redistributable
    // in `D3D12\` beside the executable the exports make every device creation fail.
    println!("cargo:rerun-if-env-changed=VICTORY_AGILITY");
    if std::env::var_os("CARGO_FEATURE_DIAGNOSTICS").is_some() && std::env::var_os("VICTORY_AGILITY").is_some_and(|v| v == "1") {
        println!("cargo:rustc-link-arg-bins=/EXPORT:D3D12SDKVersion,DATA");
        println!("cargo:rustc-link-arg-bins=/EXPORT:D3D12SDKPath,DATA");
    }
}
>>>>>>> klbf/beta-2
