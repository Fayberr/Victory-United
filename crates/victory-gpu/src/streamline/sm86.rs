//! DLSS Frame Generation on Ampere and Turing: the `dlssg_for_sm86` add-on.
//!
//! Streamline's `sl.dlss_g` plugin decides whether this GPU is new enough for
//! frame generation inside `slInit`, from NVAPI's architecture answer, and
//! drops itself on Ampere or Turing (`Ignoring plugin 'sl.dlss_g' since it is
//! not supported on this platform`); the DLSS-G runtime it would load is made
//! of kernels those GPUs cannot run either.
//!
//! `dlssg_for_sm86` (<https://github.com/sdli1995/dlssg_for_sm86>, MIT; its
//! kernel images are NVIDIA's and not covered by that license; it is not part
//! of this repository) is a Windows proxy DLL which, loaded into the process
//! *before* `slInit`:
//!
//! - redirects the export-table entry of `NvAPI_GPU_GetArchInfo`, so that the
//!   game and Streamline hear Ada on Ampere (Blackwell on Turing), while
//!   NVIDIA's own modules keep the real answer;
//! - hands the game's — here Streamline's — `nvngx_dlssg.dll` load to its own
//!   bundled runtime, and installs its SM86/SM75 kernel images in it.
//!
//! Both happen after `slInit` is too late, so this is called from
//! [`super::load_and_init`] before the library is loaded. The proxy resolves
//! `dlssg_sm86.ini` and the `dlssg_sm86\` folder against **its own** folder:
//! the three stay together.

use std::ffi::CStr;
use std::path::{Path, PathBuf};

/// The proxy (the release's `version.dll`; the alternatives in its
/// `alternatives\` folder are the same loader under another name).
pub const PROXY: &str = "version.dll";
/// The add-on's settings, beside the proxy.
pub const INI: &str = "dlssg_sm86.ini";
/// The backend, the only file checked to tell a complete installation from a
/// proxy copied on its own.
const BACKEND: &str = r"dlssg_sm86\backend\sm86_backend.dll";

/// The folder to load the add-on from: `VICTORY_DLSSG_SM86_DIR` when given
/// (whatever it holds; [`load`] says what is missing), else the first of the
/// working directory, the executable's folder and a build's repository root
/// (two folders above `target\release`) that holds a proxy and its INI.
pub fn find(asked: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = asked {
        return Some(dir.to_owned());
    }
    let mut places = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        places.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        places.push(dir.to_owned());
        if let Some(root) = dir.parent().and_then(Path::parent) {
            places.push(root.to_owned());
        }
    }
    places.into_iter().find(|dir| dir.join(PROXY).is_file() && dir.join(INI).is_file())
}

/// Loads the proxy from `dir` into this process and leaves it there for the
/// process's life (it hooks while it is loaded). The errors say which piece
/// is missing.
pub fn load(dir: &Path) -> Result<PathBuf, String> {
    let proxy = dir.join(PROXY);
    if !proxy.is_file() {
        return Err(format!("no {PROXY} in that folder"));
    }
    if !dir.join(INI).is_file() {
        return Err(format!("no {INI} beside {PROXY} (it reads its settings and its folders from its own folder)"));
    }
    if !dir.join(BACKEND).is_file() {
        return Err(format!(
            "no {BACKEND} (the whole dlssg_sm86 folder is needed beside the proxy, not just {PROXY})"
        ));
    }
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };
    let file = super::wide(&proxy);
    // Its own folder and System32, never the current folder (as for
    // Streamline itself).
    // SAFETY: a NUL-terminated full path.
    let module = unsafe {
        LoadLibraryExW(
            file.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if module.is_null() {
        return Err(format!("{PROXY} did not load ({})", std::io::Error::last_os_error()));
    }
    // The two exports every one of the add-on's proxies has: another DLL
    // that happens to be named `version.dll` is not it. Its exports are not
    // called: what it does, it does from its DllMain and its hooks.
    let has = |name: &CStr| {
        // SAFETY: a loaded module and a NUL-terminated name.
        unsafe { GetProcAddress(module, name.as_ptr().cast()) }.is_some()
    };
    if !has(c"DlssgProxy_Name") || !has(c"DlssgProxy_Role") {
        // SAFETY: nothing of it has been called.
        unsafe { windows_sys::Win32::Foundation::FreeLibrary(module) };
        return Err(format!("{PROXY} is not dlssg_for_sm86's proxy (no DlssgProxy_Name)"));
    }
    Ok(proxy)
}

/// The LUID of CUDA device 0, which is the GPU the add-on's backend makes its
/// CUDA context on (its *selected* adapter, whatever device the host made).
/// `None` when the CUDA driver is not there or answers nothing.
pub fn cuda_first_luid() -> Option<u64> {
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };
    let name = super::wide(Path::new("nvcuda.dll"));
    // SAFETY: a NUL-terminated file name and System32 only.
    let module = unsafe { LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if module.is_null() {
        return None;
    }
    // SAFETY: the CUDA driver's three exports of the signatures used here.
    unsafe {
        let find = |name: &CStr| GetProcAddress(module, name.as_ptr().cast());
        let init: unsafe extern "system" fn(u32) -> i32 = std::mem::transmute(find(c"cuInit")?);
        let count: unsafe extern "system" fn(*mut i32) -> i32 = std::mem::transmute(find(c"cuDeviceGetCount")?);
        let luid_of: unsafe extern "system" fn(*mut u8, *mut u32, i32) -> i32 =
            std::mem::transmute(find(c"cuDeviceGetLuid")?);
        if init(0) != 0 {
            return None;
        }
        let mut devices = 0i32;
        if count(&mut devices) != 0 || devices < 1 {
            return None;
        }
        let mut luid = [0u8; 8];
        let mut node = 0u32;
        if luid_of(luid.as_mut_ptr(), &mut node, 0) != 0 {
            return None;
        }
        Some(u64::from_le_bytes(luid))
    }
}
