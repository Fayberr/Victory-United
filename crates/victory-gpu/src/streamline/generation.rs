// SPDX-License-Identifier: GPL-3.0-or-later

//! Streamline's DLSS-G presenter and the inputs submitted ahead of each present.

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::{HRESULT, IUnknown, Interface};

use crate::host::HostList;
use crate::object::This;
use crate::resource::HostTexture;

use super::{Camera, Settings, Sl, abi, checked, frame_token, runtime};

const VIEWPORT: abi::ViewportHandle = abi::ViewportHandle::new(0);
const UI_COMPOSITE: [&str; 2] = ["0602c2384acbb3b2", "a78d088fa55aa495"];
static HUDLESS: AtomicBool = AtomicBool::new(false);

pub(super) struct FrameGeneration {
    proxies: Proxies,
    options: abi::SlDlssgSetOptions,
    state: abi::SlDlssgGetState,
    marker: abi::SlPclSetMarker,
    presentation: Mutex<Presentation>,
}

struct Proxies {
    // Keep these with the initialized runtime for the life of the process.
    queue: ID3D12CommandQueue,
    native: ID3D12CommandQueue,
    _device: ID3D12Device,
}

#[derive(Default)]
struct Presentation {
    options: Option<([u32; 2], bool)>,
    stats: Option<(Instant, u64, u64)>,
    status: Option<u32>,
}

impl FrameGeneration {
    pub(super) fn new(
        sl: &Sl,
        settings: &Settings,
        device: &ID3D12Device,
        can_proxy: bool,
    ) -> Result<Self, String> {
        if !can_proxy {
            return Err("the host queue already exists".into());
        }
        if let Some(expected) = settings.addon_luid {
            // SAFETY: a plain query to the live host device.
            let actual = unsafe { device.GetAdapterLuid() };
            let actual = u64::from(actual.LowPart) | (u64::from(actual.HighPart as u32) << 32);
            if actual != expected {
                return Err(format!(
                    "dlssg_for_sm86 uses CUDA device 0 ({expected:#x}), host adapter is {actual:#x}"
                ));
            }
        }
        for (id, name) in [
            (abi::FEATURE_DLSS_G, "DLSS Frame Generation"),
            (abi::FEATURE_REFLEX, "Reflex"),
            (abi::FEATURE_PCL, "PC Latency"),
        ] {
            sl.check_support(device, id)
                .map_err(|error| format!("{name}: {error}"))?;
        }
        macro_rules! function {
            ($feature:expr, $name:literal, $signature:ty) => {{
                let function = sl.function($feature, $name)?;
                // SAFETY: the Streamline export named above has this SDK signature.
                unsafe { std::mem::transmute::<*mut c_void, $signature>(function) }
            }};
        }
        let options = function!(
            abi::FEATURE_DLSS_G,
            c"slDLSSGSetOptions",
            abi::SlDlssgSetOptions
        );
        let state = function!(
            abi::FEATURE_DLSS_G,
            c"slDLSSGGetState",
            abi::SlDlssgGetState
        );
        let marker = function!(abi::FEATURE_PCL, c"slPCLSetMarker", abi::SlPclSetMarker);
        let reflex = function!(
            abi::FEATURE_REFLEX,
            c"slReflexSetOptions",
            abi::SlReflexSetOptions
        );
        let reflex_options = abi::ReflexOptions {
            base: abi::BaseStructure::new(abi::REFLEX_OPTIONS, 1),
            mode: abi::REFLEX_LOW_LATENCY,
            frame_limit_us: 0,
            use_markers_to_optimize: false,
            virtual_key: 0,
            id_thread: 0,
        };
        // SAFETY: the Reflex plugin is loaded and the options are initialized.
        checked("slReflexSetOptions", unsafe { reflex(&reflex_options) })?;
        let raw = proxy(sl, device)?;
        // SAFETY: Streamline returned its typed D3D12 device proxy.
        let proxy_device = unsafe { ID3D12Device::from_raw(raw) };
        let desc = D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            ..Default::default()
        };
        // SAFETY: a valid direct queue description and a live Streamline proxy.
        let queue: ID3D12CommandQueue = unsafe { proxy_device.CreateCommandQueue(&desc) }
            .map_err(|error| format!("Streamline direct queue: {error}"))?;
        let mut native = null_mut();
        // SAFETY: a live queue and a valid COM output location.
        checked("slGetNativeInterface", unsafe {
            (sl.native)(queue.as_raw(), &mut native)
        })?;
        if native.is_null() || native == queue.as_raw() {
            if !native.is_null() {
                // SAFETY: slGetNativeInterface added one reference even when unchanged.
                drop(unsafe { ID3D12CommandQueue::from_raw(native) });
            }
            return Err("Streamline did not create a queue proxy".into());
        }
        // SAFETY: one native command queue reference returned by slGetNativeInterface.
        let native = unsafe { ID3D12CommandQueue::from_raw(native) };
        sl.log_version(abi::FEATURE_DLSS_G, "DLSS Frame Generation");
        tracing::info!("Streamline: created the direct queue through the device proxy");
        Ok(Self {
            proxies: Proxies {
                queue,
                native,
                _device: proxy_device,
            },
            options,
            state,
            marker,
            presentation: Mutex::default(),
        })
    }

    pub(super) fn native_queue(&self) -> Option<ID3D12CommandQueue> {
        Some(self.proxies.native.clone())
    }

    fn set_options(&self, presentation: &mut Presentation, size: [u32; 2], on: bool) -> bool {
        if presentation.options == Some((size, on)) {
            return true;
        }
        let mut options = abi::DlssgOptions::new(if on { abi::DLSSG_ON } else { abi::DLSSG_OFF });
        options.flags = abi::DLSSG_RETAIN_RESOURCES_WHEN_OFF;
        options.num_back_buffers = 2;
        [options.color_width, options.color_height] = size;
        options.on_error_callback = Some(present_error);
        // SAFETY: valid options and the presenter owns viewport 0.
        match checked("slDLSSGSetOptions", unsafe {
            (self.options)(&VIEWPORT, &options)
        }) {
            Ok(()) => {
                presentation.options = Some((size, on));
                tracing::info!(on, ?size, "DLSS Frame Generation switched");
                true
            }
            Err(error) => {
                tracing::warn!(%error, on, "DLSS Frame Generation could not switch");
                false
            }
        }
    }

    fn statistics(&self, presentation: &mut Presentation) {
        let mut state = abi::DlssgState::default();
        // SAFETY: the output lives through the call, and no VRAM estimate is requested.
        if unsafe { (self.state)(&VIEWPORT, &mut state, null()) } != 0 {
            return;
        }
        if presentation.status != Some(state.status) {
            presentation.status = Some(state.status);
            tracing::info!(status = state.status, "DLSS Frame Generation status");
        }
        let (start, rendered, shown) = presentation
            .stats
            .get_or_insert_with(|| (Instant::now(), 0, 0));
        *rendered += 1;
        *shown += u64::from(state.num_frames_actually_presented);
        if start.elapsed() >= Duration::from_secs(1) {
            tracing::info!(
                rendered = *rendered,
                shown = *shown,
                "DLSS Frame Generation: rendered presents and total frames shown"
            );
            presentation.stats = None;
        }
    }
}

/// Request the proxy through Streamline's manual hook without replacing the native reference.
fn proxy<T: Interface>(sl: &Sl, original: &T) -> Result<*mut c_void, String> {
    let mut raw = original.as_raw();
    // SAFETY: a live native interface, held by the caller.
    checked("slUpgradeInterface", unsafe { (sl.upgrade)(&mut raw) })?;
    if raw.is_null() || raw == original.as_raw() {
        return Err("Streamline did not upgrade the native interface".into());
    }
    Ok(raw)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

pub fn wants_inputs() -> bool {
    !super::STOPPED.load(Ordering::Acquire)
        && !FAILED_SWAP_CHAIN.load(Ordering::Acquire)
        && runtime().is_some_and(|runtime| runtime.framegen.is_some())
}

pub(crate) fn swap_chain(
    factory: &IDXGIFactory2,
    hwnd: HWND,
    desc: &DXGI_SWAP_CHAIN_DESC1,
) -> Option<IDXGISwapChain3> {
    let runtime = runtime()?;
    let generation = runtime.framegen.as_ref()?;
    if !crate::queue::line_on() || FAILED_SWAP_CHAIN.load(Ordering::Acquire) {
        return None;
    }
    let create = || -> Result<IDXGISwapChain3, String> {
        let queue = &generation.proxies.queue;
        let raw = proxy(&runtime.sl, factory)?;
        // SAFETY: Streamline created a new proxy of the factory above.
        let factory = unsafe { IDXGIFactory2::from_raw(raw) };
        // SAFETY: a live queue and window with a valid swap-chain description.
        let chain = unsafe { factory.CreateSwapChainForHwnd(queue, hwnd, desc, None, None) }
            .map_err(|error| error.to_string())?;
        // The factory proxy must return a swap-chain proxy for DLSS-G.
        let mut native = null_mut();
        // SAFETY: a live swap chain and COM output location.
        checked("slGetNativeInterface", unsafe {
            (runtime.sl.native)(chain.as_raw(), &mut native)
        })?;
        if native.is_null() {
            return Err("Streamline returned a null swap-chain interface".into());
        }
        // Compare COM identity; queried interface pointers can differ.
        // SAFETY: slGetNativeInterface added one reference.
        let native = unsafe { IUnknown::from_raw(native) };
        if native.cast::<IUnknown>().ok() == chain.cast::<IUnknown>().ok() {
            return Err("Streamline did not create a swap-chain proxy".into());
        }
        chain
            .cast::<IDXGISwapChain3>()
            .map_err(|error| error.to_string())
    };
    match create() {
        Ok(chain) => {
            HUDLESS.store(
                std::env::var_os("VICTORY_FRAMEGEN_HUDLESS").is_none_or(|v| v != "0"),
                Ordering::Release,
            );
            tracing::info!("DLSS Frame Generation: Streamline swap-chain proxy is active");
            Some(chain)
        }
        Err(error) => {
            abandon_swap_chain();
            tracing::warn!(%error, "DLSS Frame Generation: plain presentation after swap-chain proxy failure");
            None
        }
    }
}

static FAILED_SWAP_CHAIN: AtomicBool = AtomicBool::new(false);

pub(crate) fn abandon_swap_chain() {
    FAILED_SWAP_CHAIN.store(true, Ordering::Release);
    HUDLESS.store(false, Ordering::Release);
    drop(lock(&SUBMITTED).take());
    lock(&HUD_SUBMITTED).clear();
}

#[derive(Clone)]
pub(crate) struct Preparation {
    pub depth: Arc<HostTexture>,
    pub motion: Arc<HostTexture>,
    pub camera: Camera,
    pub frame: Option<u32>,
}

static SUBMITTED: Mutex<Option<Preparation>> = Mutex::new(None);

pub(crate) fn submitted(preparation: &Preparation) {
    *lock(&SUBMITTED) = Some(preparation.clone());
}

pub(crate) fn disable() {
    drop(lock(&SUBMITTED).take());
    let Some(generation) = runtime().and_then(|runtime| runtime.framegen.as_ref()) else {
        return;
    };
    let mut presentation = lock(&generation.presentation);
    if let Some((size, true)) = presentation.options {
        generation.set_options(&mut presentation, size, false);
    }
    presentation.options = None;
}

pub(crate) fn present(
    chain: &IDXGISwapChain3,
    scene: (&ID3D12Resource, u32),
    overlay: Option<(&ID3D12Resource, u32)>,
    size: [u32; 2],
) -> HRESULT {
    // SAFETY: a live swap chain whose back buffer was just submitted.
    let present = || unsafe { chain.Present(0, DXGI_PRESENT(0)) };
    let Some(runtime) = runtime() else {
        return present();
    };
    let Some(generation) = runtime.framegen.as_ref() else {
        return present();
    };
    let mut calls = lock(&runtime.calls);
    let mut state = lock(&generation.presentation);
    let inputs = lock(&SUBMITTED).take();
    let hudless = hudless_for(scene.0);
    // Reuse the token of the submitted SR evaluation. Native FSR frames
    // receive a token here from the same sequence.
    let frame = inputs
        .as_ref()
        .and_then(|input| input.frame)
        .unwrap_or_else(|| calls.next_frame());
    let token = frame_token(&runtime.sl, frame);
    let tagged = if let (Ok(token), Some(input)) = (&token, inputs.as_ref())
        && !input.camera.reset
        && crate::queue::line_on()
    {
        match tag(
            &runtime.sl,
            runtime.settings,
            *token,
            input,
            scene,
            overlay,
            hudless.as_deref(),
            size,
        ) {
            Ok(()) => true,
            Err(error) => {
                static ONCE: std::sync::Once = std::sync::Once::new();
                ONCE.call_once(
                    || tracing::warn!(%error, "DLSS Frame Generation: frame tags rejected"),
                );
                false
            }
        }
    } else {
        false
    };
    if !tagged && let Ok(token) = token {
        let tags = [
            abi::BUFFER_DEPTH,
            abi::BUFFER_MOTION_VECTORS,
            abi::BUFFER_HUDLESS_COLOR,
            abi::BUFFER_UI_COLOR_AND_ALPHA,
        ]
        .map(|kind| {
            abi::ResourceTag::new(
                null_mut(),
                kind,
                abi::VALID_UNTIL_PRESENT,
                abi::Extent::default(),
            )
        });
        // SAFETY: empty tags clear this viewport's retained inputs.
        unsafe {
            (runtime.sl.set_tag)(
                token,
                &VIEWPORT,
                tags.as_ptr(),
                tags.len() as u32,
                null_mut(),
            )
        };
    }
    let on = generation.set_options(&mut state, size, tagged) && tagged;
    if let Ok(token) = token {
        let mark = |marker| {
            // SAFETY: the token is held by Streamline through the present.
            unsafe { (generation.marker)(marker, token) };
        };
        for marker in [
            abi::SIMULATION_START,
            abi::SIMULATION_END,
            abi::RENDER_SUBMIT_START,
            abi::RENDER_SUBMIT_END,
            abi::PRESENT_START,
        ] {
            mark(marker);
        }
        let result = present();
        mark(abi::PRESENT_END);
        if result == S_OK && on {
            generation.statistics(&mut state);
        }
        result
    } else {
        present()
    }
}

fn tag(
    sl: &Sl,
    settings: &Settings,
    token: *mut abi::FrameToken,
    input: &Preparation,
    scene: (&ID3D12Resource, u32),
    overlay: Option<(&ID3D12Resource, u32)>,
    hudless: Option<&HudlessSurface>,
    size: [u32; 2],
) -> Result<(), String> {
    let camera = input.camera.planes_in_order();
    let mut resources = [
        abi::Resource::texture(
            input.depth.texture.as_raw(),
            input.depth.state.load(Ordering::Acquire),
        ),
        abi::Resource::texture(
            input.motion.texture.as_raw(),
            input.motion.state.load(Ordering::Acquire),
        ),
        abi::Resource::texture(null_mut(), 0),
        abi::Resource::texture(null_mut(), 0),
    ];
    if let Some((ui, state)) = overlay.filter(|(ui, _)| {
        // The display composer may scale a UI plane that differs from the back buffer.
        // SAFETY: the present owns a reference to the plane.
        let desc = unsafe { ui.GetDesc() };
        desc.Width == u64::from(size[0]) && desc.Height == size[1]
    }) {
        resources[2] = abi::Resource::texture(scene.0.as_raw(), scene.1);
        resources[3] = abi::Resource::texture(ui.as_raw(), state);
    } else if let Some(hudless) = hudless {
        resources[2] = abi::Resource::texture(
            hudless.texture.as_raw(),
            D3D12_RESOURCE_STATE_COPY_DEST.0 as u32,
        );
    }
    let extents = [camera.render, camera.motion_size(), size, size];
    let kinds = [
        abi::BUFFER_DEPTH,
        abi::BUFFER_MOTION_VECTORS,
        abi::BUFFER_HUDLESS_COLOR,
        abi::BUFFER_UI_COLOR_AND_ALPHA,
    ];
    let tags = std::array::from_fn::<_, 4, _>(|i| {
        let resource = &mut resources[i];
        let extent = if resource.native.is_null() {
            abi::Extent::default()
        } else {
            abi::Extent::sized(extents[i])
        };
        let raw = if resource.native.is_null() {
            null_mut()
        } else {
            resource
        };
        abi::ResourceTag::new(raw, kinds[i], abi::VALID_UNTIL_PRESENT, extent)
    });
    let constants = camera.constants(settings.jitter, settings.motion);
    // SAFETY: valid constants and tags for the current Streamline frame and viewport.
    checked("slSetConstants", unsafe {
        (sl.constants)(&constants, token, &VIEWPORT)
    })?;
    // SAFETY: all the tagged resources are held through the present and their states
    // are the states left by ordered submissions.
    checked("slSetTagForFrame", unsafe {
        (sl.set_tag)(
            token,
            &VIEWPORT,
            tags.as_ptr(),
            tags.len() as u32,
            null_mut(),
        )
    })?;
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| tracing::info!(render = ?camera.render, output = ?camera.output, interface_plane = overlay.is_some(),
        "DLSS Frame Generation: tagged the first submitted depth and motion"));
    Ok(())
}

unsafe extern "C" fn present_error(error: *const i32) {
    // SAFETY: Streamline's callback argument, valid for this call when non-null.
    if let Some(error) = unsafe { error.as_ref() } {
        tracing::warn!(
            error = format_args!("{error:#x}"),
            "DLSS Frame Generation present callback"
        );
    }
}

#[derive(Clone)]
pub(crate) struct HudlessCopy {
    target: ID3D12Resource,
    surface: Arc<HudlessSurface>,
}

struct HudlessSurface {
    texture: ID3D12Resource,
    desc: D3D12_RESOURCE_DESC,
}

static PRESENTED_TEXTURES: Mutex<Vec<ID3D12Resource>> = Mutex::new(Vec::new());
static HUD_SUBMITTED: Mutex<Vec<HudlessCopy>> = Mutex::new(Vec::new());
static HUD_POOL: Mutex<Vec<Arc<HudlessSurface>>> = Mutex::new(Vec::new());

pub(crate) fn hudless_wanted() -> bool {
    HUDLESS.load(Ordering::Acquire)
}

pub(crate) fn hudless_submitted(copy: &HudlessCopy) {
    let mut submitted = lock(&HUD_SUBMITTED);
    if !submitted
        .iter()
        .any(|existing| existing.target.as_raw() == copy.target.as_raw())
    {
        if submitted.len() == 4 {
            submitted.remove(0);
        }
        submitted.push(copy.clone());
    }
}

fn hudless_for(scene: &ID3D12Resource) -> Option<Arc<HudlessSurface>> {
    if !hudless_wanted() {
        return None;
    }
    let mut seen = lock(&PRESENTED_TEXTURES);
    if !seen
        .iter()
        .any(|texture| texture.as_raw() == scene.as_raw())
    {
        if seen.len() == 4 {
            seen.remove(0);
        }
        seen.push(scene.clone());
    }
    drop(seen);
    let mut submitted = lock(&HUD_SUBMITTED);
    let index = submitted
        .iter()
        .position(|copy| copy.target.as_raw() == scene.as_raw())?;
    Some(submitted.remove(index).surface)
}

pub(crate) fn before_target_draw(
    list: &HostList,
    target: This,
    pipeline: &crate::pipeline::host::HostPipeline,
) {
    if !UI_COMPOSITE.iter().all(|hash| {
        pipeline
            .stages
            .iter()
            .any(|stage| stage.shader.hash == *hash)
    }) {
        return;
    }
    let Some(texture) = crate::resource::host_resource(target) else {
        return;
    };
    if !lock(&PRESENTED_TEXTURES)
        .iter()
        .any(|seen| seen.as_raw() == texture.as_raw())
        || list.has_hudless_copy()
    {
        return;
    }
    let Some(surface) = acquire_surface(texture) else {
        return;
    };
    crate::list::ensure_state(list, target, D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32);
    // SAFETY: compatible textures in COPY_SOURCE and COPY_DEST on an open list.
    unsafe { list.list.CopyResource(&surface.texture, texture) };
    crate::list::ensure_state(list, target, D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32);
    list.keep_hudless_copy(HudlessCopy {
        target: texture.clone(),
        surface,
    });
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| {
        tracing::info!("DLSS Frame Generation: copied Series X scene before the HUD composite")
    });
}

fn acquire_surface(texture: &ID3D12Resource) -> Option<Arc<HudlessSurface>> {
    // SAFETY: a live texture and a plain query.
    let mut desc = unsafe { texture.GetDesc() };
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.DepthOrArraySize != 1
        || desc.MipLevels != 1
        || desc.SampleDesc.Count != 1
    {
        return None;
    }
    desc.Flags = D3D12_RESOURCE_FLAG_NONE;
    desc.Format = crate::host::swap_format(desc.Format)?;
    let mut pool = lock(&HUD_POOL);
    let same = |surface: &HudlessSurface| {
        surface.desc.Width == desc.Width
            && surface.desc.Height == desc.Height
            && surface.desc.Format == desc.Format
    };
    if let Some(surface) = pool
        .iter()
        .find(|surface| Arc::strong_count(surface) == 1 && same(surface))
    {
        return Some(surface.clone());
    }
    pool.retain(|surface| Arc::strong_count(surface) > 1 || same(surface));
    if pool.len() >= 4 {
        return None;
    }
    let device = &crate::host::host()?.device;
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let mut texture = None;
    // SAFETY: compatible 2D texture description and initialized arguments.
    unsafe {
        device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
            &mut texture,
        )
    }
    .ok()?;
    let surface = Arc::new(HudlessSurface {
        texture: texture?,
        desc,
    });
    pool.push(surface.clone());
    Some(surface)
}
