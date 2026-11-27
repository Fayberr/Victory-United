// SPDX-License-Identifier: GPL-3.0-or-later

//! NVIDIA Streamline for the title's FidelityFX dispatch. The SDK is loaded
//! from a user-supplied folder and uses D3D12 manual hooking.

mod abi;
pub(crate) mod generation;
mod plan;
mod sm86;

pub use plan::Camera;

use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::ptr::{from_ref, null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Graphics::Direct3D12::{
    ID3D12Device, ID3D12GraphicsCommandList, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain3;
use windows::core::Interface;

const INTERPOSER: &str = "sl.interposer.dll";
const PROJECT_ID: &CStr = c"6f0d7b2e-3a41-4c1e-9b57-2d8e4f1a9c30";
const ENGINE_VERSION: &CStr = c"Victory United";
const VIEWPORT: u32 = 1;

struct Settings {
    dir: PathBuf,
    log_dir: Option<PathBuf>,
    preset: u32,
    jitter: [f32; 2],
    motion: [f32; 2],
    reactive: bool,
    quality: Option<u8>,
    framegen: bool,
    require_sr_for_fg: bool,
    addon_luid: Option<u64>,
}

impl Settings {
    fn read(quality: Option<u8>, framegen: bool) -> Result<Self, String> {
        let folder = std::env::var_os("VICTORY_STREAMLINE_DIR")
            .filter(|value| !value.is_empty())
            .ok_or("set VICTORY_STREAMLINE_DIR to the SDK's bin\\x64 folder")?;
        let dir = std::fs::canonicalize(&folder)
            .map_err(|error| format!("Streamline folder {folder:?}: {error}"))?;
        let preset = std::env::var("VICTORY_DLSS_PRESET").unwrap_or_default();
        let preset = match preset.trim().to_ascii_lowercase().as_str() {
            "" | "default" => 0,
            "j" => 10,
            "k" => 11,
            "l" => 12,
            "m" => 13,
            _ => {
                return Err(format!(
                    "VICTORY_DLSS_PRESET={preset}: expected default, j, k, l or m"
                ));
            }
        };
        for file in [INTERPOSER, "sl.common.dll"] {
            require_file(&dir, file)?;
        }
        verify_signature(&dir.join(INTERPOSER))?;
        let require_sr_for_fg = quality.is_some();
        let quality = quality.filter(|_| match require_files(&dir, &["sl.dlss.dll", "nvngx_dlss.dll"]) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, "Streamline DLSS unavailable; using the title's upscaler");
                false
            }
        });
        let framegen = framegen
            && match require_files(
                &dir,
                &[
                    "sl.dlss_g.dll",
                    "sl.reflex.dll",
                    "sl.pcl.dll",
                    "nvngx_dlssg.dll",
                ],
            ) {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(%error, "DLSS Frame Generation unavailable");
                    false
                }
            };
        let addon_luid = if framegen
            && let Some(folder) = std::env::var_os("VICTORY_DLSSG_SM86_DIR")
        {
            match std::fs::canonicalize(folder)
                .map_err(|e| e.to_string())
                .and_then(|dir| sm86::load(&dir))
            {
                Ok(proxy) => {
                    tracing::info!(proxy = %proxy.display(), "DLSS Frame Generation: loaded dlssg_for_sm86 before Streamline");
                    sm86::cuda_first_luid()
                }
                Err(error) => {
                    tracing::warn!(%error, "DLSS Frame Generation: could not load dlssg_for_sm86");
                    None
                }
            }
        } else {
            None
        };
        if quality.is_none() && !framegen {
            return Err("no requested Streamline feature can be loaded".into());
        }
        Ok(Self {
            dir,
            log_dir: std::env::var_os("VICTORY_STREAMLINE_LOG")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            preset,
            jitter: plan::pair(std::env::var("VICTORY_DLSS_JITTER").ok().as_deref()),
            motion: plan::pair(std::env::var("VICTORY_DLSS_MV").ok().as_deref()),
            reactive: std::env::var_os("VICTORY_DLSS_REACTIVE").is_none_or(|v| v != "0"),
            quality,
            framegen,
            require_sr_for_fg,
            addon_luid,
        })
    }
}

struct Library(windows_sys::Win32::Foundation::HMODULE);

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: this owns the reference LoadLibraryExW took.
        unsafe { windows_sys::Win32::Foundation::FreeLibrary(self.0) };
    }
}

// SAFETY: loading and export lookup use the process-wide module handle.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}

struct Sl {
    _library: Library,
    init: abi::SlInit,
    shutdown: abi::SlShutdown,
    supported: abi::SlIsFeatureSupported,
    version: abi::SlGetFeatureVersion,
    set_device: abi::SlSetD3DDevice,
    function: abi::SlGetFeatureFunction,
    upgrade: abi::SlUpgradeInterface,
    native: abi::SlGetNativeInterface,
    set_tag: abi::SlSetTagForFrame,
    token: abi::SlGetNewFrameToken,
    constants: abi::SlSetConstants,
    evaluate: abi::SlEvaluateFeature,
}

fn checked(operation: &str, code: i32) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("{operation}: {} ({code})", abi::result_name(code)))
    }
}

impl Sl {
    fn initialize(settings: &Settings) -> Result<Self, String> {
        let path = settings.dir.join(INTERPOSER);
        let sl = load(&path)?;
        let dir = wide(&settings.dir);
        let paths = [dir.as_ptr()];
        let log_dir = settings.log_dir.as_deref().map(wide);
        let mut features = Vec::with_capacity(4);
        if settings.quality.is_some() {
            features.push(abi::FEATURE_DLSS);
        }
        if settings.framegen {
            features.extend([abi::FEATURE_DLSS_G, abi::FEATURE_REFLEX, abi::FEATURE_PCL]);
        }
        let preferences = abi::Preferences {
            base: abi::BaseStructure::new(abi::PREFERENCES, 1),
            show_console: false,
            log_level: if log_dir.is_some() {
                abi::LOG_VERBOSE
            } else {
                abi::LOG_DEFAULT
            },
            paths_to_plugins: paths.as_ptr(),
            num_paths_to_plugins: 1,
            path_to_logs_and_data: log_dir.as_ref().map_or(null(), |wide| wide.as_ptr()),
            allocate_callback: null(),
            release_callback: null(),
            log_message_callback: Some(log_message),
            flags: abi::DISABLE_CL_STATE_TRACKING
                | abi::DISABLE_DEBUG_TEXT
                | abi::USE_MANUAL_HOOKING
                | abi::USE_FRAME_BASED_RESOURCE_TAGGING
                | if settings.framegen {
                    abi::USE_DXGI_FACTORY_PROXY
                } else {
                    0
                },
            features_to_load: features.as_ptr(),
            num_features_to_load: features.len() as u32,
            application_id: 0,
            engine: abi::ENGINE_CUSTOM,
            engine_version: ENGINE_VERSION.as_ptr(),
            project_id: PROJECT_ID.as_ptr(),
            render_api: abi::RENDER_API_D3D12,
        };
        // SAFETY: all preferences fields live through slInit, which copies them.
        checked("slInit", unsafe {
            (sl.init)(&preferences, abi::SDK_VERSION)
        })?;
        tracing::info!(library = %path.display(), "Streamline loaded (manual hooking, no OTA updates)");
        Ok(sl)
    }

    fn check_support(&self, device: &ID3D12Device, feature: u32) -> Result<(), String> {
        // SAFETY: the device is live and this query has no arguments.
        let luid = unsafe { device.GetAdapterLuid() };
        let adapter = abi::AdapterInfo {
            base: abi::BaseStructure::new(abi::ADAPTER_INFO, 1),
            device_luid: from_ref(&luid).cast(),
            device_luid_size: size_of_val(&luid) as u32,
            vk_physical_device: null_mut(),
        };
        // SAFETY: an adapter description that lives through the call.
        checked("Streamline feature support", unsafe {
            (self.supported)(feature, &adapter)
        })
    }

    fn function(&self, feature: u32, name: &CStr) -> Result<*mut c_void, String> {
        let mut function = null_mut();
        // SAFETY: a NUL-terminated name and a valid out pointer.
        checked(&name.to_string_lossy(), unsafe {
            (self.function)(feature, name.as_ptr(), &mut function)
        })?;
        if function.is_null() {
            return Err(format!(
                "Streamline returned a null {}",
                name.to_string_lossy()
            ));
        }
        Ok(function)
    }

    fn log_version(&self, feature: u32, name: &str) {
        let mut version = abi::FeatureVersion {
            base: abi::BaseStructure::new(abi::FEATURE_VERSION, 1),
            version_sl: abi::Version::default(),
            version_ngx: abi::Version::default(),
        };
        // SAFETY: valid output structure for the feature version.
        if unsafe { (self.version)(feature, &mut version) } == 0 {
            tracing::info!(streamline = %version.version_sl, ngx = %version.version_ngx, "Streamline: {name} loaded");
        }
    }
}

struct Runtime {
    sl: Sl,
    settings: &'static Settings,
    set_options: Option<abi::SlDlssSetOptions>,
    framegen: Option<generation::FrameGeneration>,
    calls: Mutex<Calls>,
}

#[derive(Default)]
struct Calls {
    frame: u32,
    last: Option<([u32; 2], [u32; 2])>,
}

impl Calls {
    fn next_frame(&mut self) -> u32 {
        self.frame = self.frame.wrapping_add(1);
        self.frame
    }
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();
static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();
static STOPPED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
pub struct Available {
    pub super_resolution: bool,
    pub frame_generation: bool,
}

pub fn framegen_requested() -> bool {
    std::env::var_os("VICTORY_FRAMEGEN").is_some_and(|v| v == "dlss")
}

/// Select features before the host creates its direct queue.
pub fn start(quality: Option<u8>, framegen: bool) -> Result<Available, String> {
    let framegen = framegen
        && if crate::queue::line_on() {
            true
        } else {
            tracing::warn!(
                "DLSS Frame Generation requires the ordered submission line (VICTORY_THIN=1 or VICTORY_ASYNC_SUBMIT=1 with late present)"
            );
            false
        };
    if quality.is_none() && !framegen {
        return Ok(Available::default());
    }
    SETTINGS
        .set(Settings::read(quality, framegen)?)
        .map_err(|_| "Streamline already configured")?;
    let device = crate::external::host_device().ok_or("no host D3D12 device")?;
    // Normally device_made initializes inside host creation. An existing
    // device can still supply SR, but its direct queue predates the proxy.
    let runtime = RUNTIME
        .get_or_init(|| initialize(&device, false))
        .as_ref()
        .map_err(Clone::clone)?;
    Ok(Available {
        super_resolution: runtime.set_options.is_some(),
        frame_generation: runtime.framegen.is_some(),
    })
}

pub(crate) fn addon_device() -> Option<ID3D12Device> {
    use windows::Win32::Foundation::LUID;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, IDXGIAdapter1, IDXGIFactory4,
    };
    use windows::Win32::Graphics::{
        Direct3D::D3D_FEATURE_LEVEL_12_0, Direct3D12::D3D12CreateDevice,
    };

    let luid = SETTINGS.get()?.addon_luid?;
    if std::env::var("VICTORY_D3D12_ADAPTER").is_ok_and(|value| !value.trim().is_empty()) {
        return None;
    }
    let luid = LUID {
        LowPart: luid as u32,
        HighPart: (luid >> 32) as i32,
    };
    // SAFETY: DXGI enumeration and device creation with valid arguments.
    unsafe {
        let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).ok()?;
        let adapter: IDXGIAdapter1 = factory.EnumAdapterByLuid(luid).ok()?;
        let mut device = None;
        D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_12_0, &mut device).ok()?;
        device
    }
}

pub(crate) fn device_made(
    device: &ID3D12Device,
) -> Option<windows::Win32::Graphics::Direct3D12::ID3D12CommandQueue> {
    SETTINGS.get()?;
    let runtime = RUNTIME
        .get_or_init(|| initialize(device, true))
        .as_ref()
        .ok()?;
    runtime.framegen.as_ref()?.native_queue()
}

fn initialize(device: &ID3D12Device, can_proxy_queue: bool) -> Result<Runtime, String> {
    let settings = SETTINGS.get().ok_or("Streamline was not configured")?;
    let sl = Sl::initialize(settings)?;
    // SAFETY: the native device lives with the host and through shutdown.
    if let Err(error) = checked("slSetD3DDevice", unsafe {
        (sl.set_device)(device.as_raw())
    }) {
        // SAFETY: initialization succeeded and no feature work has run.
        unsafe { (sl.shutdown)() };
        return Err(error);
    }
    let set_options = settings.quality.and_then(|_| {
        match sl
            .check_support(device, abi::FEATURE_DLSS)
            .and_then(|()| sl.function(abi::FEATURE_DLSS, c"slDLSSSetOptions"))
        {
            Ok(function) => {
                sl.log_version(abi::FEATURE_DLSS, "DLSS Super Resolution");
                // SAFETY: the Streamline export has the SDK signature.
                Some(unsafe { std::mem::transmute::<*mut c_void, abi::SlDlssSetOptions>(function) })
            }
            Err(error) => {
                tracing::warn!(%error, "Streamline DLSS unavailable; using the title's upscaler");
                None
            }
        }
    });
    let framegen = if settings.framegen && (!settings.require_sr_for_fg || set_options.is_some()) {
        match generation::FrameGeneration::new(&sl, settings, device, can_proxy_queue) {
            Ok(framegen) => Some(framegen),
            Err(error) => {
                tracing::warn!(%error, "DLSS Frame Generation unavailable");
                None
            }
        }
    } else {
        None
    };
    if set_options.is_none() && framegen.is_none() {
        // SAFETY: the initialized SDK recorded no feature work.
        unsafe { (sl.shutdown)() };
        return Err("no requested Streamline feature initialized".into());
    }
    Ok(Runtime {
        sl,
        settings,
        set_options,
        framegen,
        calls: Mutex::default(),
    })
}

fn require_file(dir: &Path, name: &str) -> Result<(), String> {
    if dir.join(name).is_file() {
        Ok(())
    } else {
        Err(format!("missing {name} in {}", dir.display()))
    }
}

fn require_files(dir: &Path, names: &[&str]) -> Result<(), String> {
    names.iter().try_for_each(|name| require_file(dir, name))
}

fn runtime() -> Option<&'static Runtime> {
    RUNTIME.get()?.as_ref().ok()
}

/// The preset selected before the title's FidelityFX registry was populated.
pub fn quality() -> Option<u8> {
    let runtime = runtime()?;
    runtime.set_options?;
    runtime.settings.quality
}

pub fn reactive() -> bool {
    runtime().is_some_and(|runtime| runtime.settings.reactive)
}

pub(crate) fn upgrade_swap_chain(swap_chain: IDXGISwapChain3) -> IDXGISwapChain3 {
    let Some(runtime) = runtime().filter(|runtime| runtime.set_options.is_some()) else {
        return swap_chain;
    };
    let mut raw = swap_chain.as_raw();
    // SAFETY: the swap chain remains alive during the call. If the pointer
    // changes, Streamline gives us one reference to the proxy.
    let code = unsafe { (runtime.sl.upgrade)(&mut raw) };
    if code != 0 || raw.is_null() || raw == swap_chain.as_raw() {
        tracing::warn!(code, "Streamline: could not upgrade the swap chain");
        return swap_chain;
    }
    tracing::info!("Streamline: swap chain observes presents");
    // SAFETY: this takes ownership of the proxy reference returned above.
    unsafe { IDXGISwapChain3::from_raw(raw) }
}

/// The resources are already prepared by the existing FidelityFX bridge.
pub fn upscale(
    list: &ID3D12GraphicsCommandList,
    textures: &[Option<ID3D12Resource>; 7],
    states: [u32; 7],
    mut camera: Camera,
    pre_exposure: f32,
) -> Result<u32, String> {
    let runtime = runtime().ok_or("Streamline is unavailable")?;
    let mut calls = runtime.calls.lock().unwrap_or_else(|e| e.into_inner());
    if STOPPED.load(Ordering::Acquire) {
        return Err("Streamline has shut down".into());
    }
    let set_options = runtime
        .set_options
        .ok_or("Streamline Super Resolution is unavailable")?;
    let mode = match runtime.settings.quality {
        Some(1) => abi::DLSS_MAX_QUALITY,
        Some(2) => abi::DLSS_BALANCED,
        _ => abi::DLSS_MAX_PERFORMANCE,
    };
    let mut options = abi::DlssOptions::new(mode, camera.output, runtime.settings.preset);
    options.pre_exposure = if pre_exposure.is_finite() && pre_exposure > 0.0 {
        pre_exposure
    } else {
        1.0
    };
    options.color_buffers_hdr = abi::boolean(camera.flags & plan::FFX_HIGH_DYNAMIC_RANGE != 0);
    options.use_auto_exposure = abi::boolean(textures[3].is_none());
    let viewport = abi::ViewportHandle::new(VIEWPORT);
    // SAFETY: initialized options and an engine-owned viewport.
    checked("slDLSSSetOptions", unsafe {
        set_options(&viewport, &options)
    })?;
    let frame = calls.next_frame();
    let token = frame_token(&runtime.sl, frame)?;
    let sizes = (camera.render, camera.output);
    camera.reset |= calls.last != Some(sizes);
    let constants = camera
        .planes_in_order()
        .constants(runtime.settings.jitter, runtime.settings.motion);
    // SAFETY: initialized constants for this frame and viewport.
    checked("slSetConstants", unsafe {
        (runtime.sl.constants)(&constants, token, &viewport)
    })?;
    let kinds = [
        (0, abi::BUFFER_SCALING_INPUT_COLOR, camera.render),
        (1, abi::BUFFER_DEPTH, camera.render),
        (2, abi::BUFFER_MOTION_VECTORS, camera.motion_size()),
        (3, abi::BUFFER_EXPOSURE, [1, 1]),
        (4, abi::BUFFER_BIAS_CURRENT_COLOR_HINT, camera.render),
        (6, abi::BUFFER_SCALING_OUTPUT_COLOR, camera.output),
    ];
    let mut resources = kinds.map(|(index, _, _)| {
        textures[index]
            .as_ref()
            .map(|texture| abi::Resource::texture(texture.as_raw(), states[index]))
    });
    let mut tags = resources.each_mut().map(|resource| resource.as_mut());
    let tags = std::array::from_fn::<_, 6, _>(|index| {
        let (_, kind, size) = kinds[index];
        tags[index].take().map(|resource| {
            abi::ResourceTag::new(
                resource,
                kind,
                abi::VALID_UNTIL_EVALUATE,
                abi::Extent::sized(size),
            )
        })
    });
    let mut inputs = [null(); 7];
    inputs[0] = from_ref(&viewport.base);
    let mut count = 1;
    for tag in tags.iter().flatten() {
        inputs[count] = from_ref(&tag.base);
        count += 1;
    }
    // SAFETY: an open D3D12 list and valid local tags, borrowed for evaluation only.
    checked("slEvaluateFeature", unsafe {
        (runtime.sl.evaluate)(
            abi::FEATURE_DLSS,
            token,
            inputs.as_ptr(),
            count as u32,
            list.as_raw(),
        )
    })?;
    if calls.last != Some(sizes) {
        tracing::info!(render = ?camera.render, output = ?camera.output, "Streamline DLSS: recorded the title's upscaling");
        calls.last = Some(sizes);
    }
    Ok(frame)
}

fn frame_token(sl: &Sl, frame: u32) -> Result<*mut abi::FrameToken, String> {
    let mut token = null_mut();
    // SAFETY: valid out pointer and frame index.
    checked("slGetNewFrameToken", unsafe {
        (sl.token)(&mut token, &frame)
    })?;
    if token.is_null() {
        return Err("Streamline returned a null frame token".into());
    }
    Ok(token)
}

pub fn shut_down() {
    let Some(runtime) = runtime() else { return };
    if STOPPED.swap(true, Ordering::AcqRel) {
        return;
    }
    drop(runtime.calls.lock().unwrap_or_else(|e| e.into_inner()));
    crate::external::finish_submitted();
    generation::disable();
    crate::host::release_streamline_presenter();
    let _calls = runtime.calls.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: the GPU is settled and the Streamline swap chain has been released.
    let result = unsafe { (runtime.sl.shutdown)() };
    if let Err(error) = checked("slShutdown", result) {
        tracing::warn!(%error, "Streamline shutdown failed");
    }
}

unsafe extern "C" fn log_message(kind: u32, message: *const c_char) {
    if message.is_null() {
        return;
    }
    // SAFETY: a NUL-terminated message for the call.
    let text = unsafe { CStr::from_ptr(message) }.to_string_lossy();
    let text = text.trim_end();
    // (Every plugin in the folder that was not asked for says so.)
    let noise = text.contains("since it is was not requested by the host");
    match kind {
        abi::LOG_ERROR => tracing::error!(target: "streamline", "{text}"),
        abi::LOG_WARN if !noise => tracing::warn!(target: "streamline", "{text}"),
        _ => tracing::debug!(target: "streamline", "{text}"),
    }
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain([0]).collect()
}

/// The Authenticode signature of `path` (NVIDIA signs its DLLs), checked
/// without going online.
fn verify_signature(path: &Path) -> Result<(), String> {
    use windows_sys::Win32::Security::WinTrust::*;
    let file = wide(path);
    let mut info = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: file.as_ptr(),
        hFile: std::ptr::null_mut(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut info },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: filled structures that live through both calls.
    let code = unsafe { WinVerifyTrust(std::ptr::null_mut(), &mut action, (&raw mut data).cast()) };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: as above; closes the state the first call opened.
    unsafe { WinVerifyTrust(std::ptr::null_mut(), &mut action, (&raw mut data).cast()) };
    if code == 0 {
        Ok(())
    } else {
        Err(format!(
            "{} is not validly signed ({:#x})",
            path.display(),
            code as u32
        ))
    }
}

fn load(path: &Path) -> Result<Sl, String> {
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
        LoadLibraryExW,
    };
    let file = wide(path);
    // Its own folder and System32 only, never the current folder.
    // SAFETY: a NUL-terminated full path.
    let module = unsafe {
        LoadLibraryExW(
            file.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if module.is_null() {
        return Err(format!(
            "{INTERPOSER} did not load ({})",
            std::io::Error::last_os_error()
        ));
    }
    let library = Library(module);
    let find = |name: &CStr| {
        // SAFETY: a loaded module and a NUL-terminated name.
        unsafe { GetProcAddress(module, name.as_ptr().cast()) }
            .map(|f| f as *const c_void)
            .ok_or_else(|| format!("{INTERPOSER} has no {}", name.to_string_lossy()))
    };
    macro_rules! get {
        ($name:literal, $t:ty) => {
            // SAFETY: the export of that name has the header's signature.
            unsafe { std::mem::transmute::<*const c_void, $t>(find($name)?) }
        };
    }
    Ok(Sl {
        init: get!(c"slInit", abi::SlInit),
        shutdown: get!(c"slShutdown", abi::SlShutdown),
        supported: get!(c"slIsFeatureSupported", abi::SlIsFeatureSupported),
        version: get!(c"slGetFeatureVersion", abi::SlGetFeatureVersion),
        set_device: get!(c"slSetD3DDevice", abi::SlSetD3DDevice),
        function: get!(c"slGetFeatureFunction", abi::SlGetFeatureFunction),
        upgrade: get!(c"slUpgradeInterface", abi::SlUpgradeInterface),
        native: get!(c"slGetNativeInterface", abi::SlGetNativeInterface),
        set_tag: get!(c"slSetTagForFrame", abi::SlSetTagForFrame),
        token: get!(c"slGetNewFrameToken", abi::SlGetNewFrameToken),
        constants: get!(c"slSetConstants", abi::SlSetConstants),
        evaluate: get!(c"slEvaluateFeature", abi::SlEvaluateFeature),
        _library: library,
    })
}
