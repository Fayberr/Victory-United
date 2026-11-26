//! Link settings for the Windows executable.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Threads created without a stack size take the executable's default.
        // Host graphics drivers compiling the title's large shaders on such
        // threads (vkd3d-proton's and the drivers' own, under Wine too) overflow
        // the linker's default of 1 MiB: reserve 16 MiB instead.
        println!("cargo:rustc-link-arg-bins=/STACK:16777216");
        // LOCAL-ONLY (branch local/win10-agility, never pushed): exports the two
        // symbols D3D12 reads from the process executable to select an app-local
        // D3D12 Agility SDK runtime. Without them, Windows 10's in-box d3d12.dll
        // refuses every translated pipeline's root signature
        // ("Unsupported bit-flag set (root signature flags c00)").
        println!("cargo:rustc-link-arg-bins=/EXPORT:D3D12SDKVersion,DATA");
        println!("cargo:rustc-link-arg-bins=/EXPORT:D3D12SDKPath,DATA");
    }
}
