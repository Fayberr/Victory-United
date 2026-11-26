//! (Neptune) The diagnostics build exports the two symbols by which the D3D12 loader finds a
//! redistributable runtime and debug layer beside the executable (`D3D12\`): the debug layer
//! Windows installs is older than its D3D12 runtime on some PCs, and a device is then refused.
//! Also needed by the local Windows 10 Agility build (branch local/win10-agility): without the
//! exports, Windows 10's in-box d3d12.dll refuses every translated pipeline's root signature
//! ("Unsupported bit-flag set (root signature flags c00)").
fn main() {
    // (ours) Only when asked for (`VICTORY_AGILITY=1` at build time): without the redistributable
    // in `D3D12\` beside the executable the exports make every device creation fail.
    println!("cargo:rerun-if-env-changed=VICTORY_AGILITY");
    if std::env::var_os("VICTORY_AGILITY").is_some_and(|v| v == "1") {
        println!("cargo:rustc-link-arg-bins=/EXPORT:D3D12SDKVersion,DATA");
        println!("cargo:rustc-link-arg-bins=/EXPORT:D3D12SDKPath,DATA");
    }
}
