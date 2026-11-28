//! Hardware check: GPU, VRAM, driver, mesh shader support, RAM, CPU and Windows build, with warnings in
//! plain words. The D3D12 device is created once on a worker thread and dropped right away; nothing stays
//! alive afterwards. The warning rules are pure functions so they can be tested without a GPU.
use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize)]
pub struct Capabilities {
    pub shader_model: Option<String>,
    pub resource_binding_tier: Option<u32>,
    pub mesh_shader_tier: Option<u32>,
    pub ray_tracing_tier: Option<u32>,
    pub enhanced_barriers: Option<bool>,
    pub gpu_upload_heap: Option<bool>,
    pub wave_lanes: Option<[u32; 2]>,
    pub unified_memory: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Adapter {
    pub name: String,
    pub luid: u64,
    pub capabilities: Capabilities,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Hardware {
    pub gpu_name: String,
    /// PCI vendor id: 0x10DE NVIDIA, 0x1002 AMD, 0x8086 Intel.
    pub vendor_id: u32,
    pub vram_mb: u64,
    /// Driver version as the user knows it (for NVIDIA like 546.01), else the WDDM style four numbers.
    pub driver: String,
    /// The four numbers of the display driver (WDDM version first), when known.
    pub driver_parts: [u16; 4],
    /// `Some(true)` when the card supports mesh shaders, `None` when the query failed.
    pub mesh_shaders: Option<bool>,
    pub adapters: Vec<Adapter>,
    pub checked_adapter: Option<u64>,
    pub ram_mb: u64,
    pub cpu_name: String,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub windows_build: String,
    /// How many screens the desktop is shown on (0 when unknown).
    pub screens: u32,
    /// Set when the probe could not run (non-Windows, no adapter).
    pub note: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Warning,
    /// The game cannot run on this machine.
    Blocker,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub severity: Severity,
    pub text: String,
}

/// Plain-word warnings. Limits are a judgment, not measured: 16 GB RAM and 6 GB VRAM follow the launcher
/// brief; the driver rule only flags drivers from before the Windows 11 era display model.
pub fn warnings(hw: &Hardware) -> Vec<Warning> {
    let mut out = Vec::new();
    let mut add = |severity, text: &str| out.push(Warning { severity, text: text.to_string() });
    if hw.mesh_shaders == Some(false) {
        add(Severity::Blocker, "This graphics card does not support mesh shaders, so the game cannot run on it.");
    }
    if hw.ram_mb > 0 && hw.ram_mb < 15_500 {
        add(Severity::Warning, "Less than 16 GB of memory. The game may stutter or run out of memory.");
    }
    if hw.vram_mb > 0 && hw.vram_mb < 6_000 {
        add(Severity::Warning, "Less than 6 GB of video memory. Expect low frame rates and texture problems.");
    }
    if driver_is_old(hw) {
        add(Severity::Warning, "The graphics driver looks old. Install the newest driver from your card's maker.");
    }
    out
}

fn driver_is_old(hw: &Hardware) -> bool {
    let wddm_major = hw.driver_parts[0];
    if wddm_major == 0 {
        return false;
    }
    // WDDM 3.0 arrived with Windows 11 and drivers of 2021; anything below is old for this title.
    if wddm_major < 30 {
        return true;
    }
    // NVIDIA numbers like 546.01: older than 531 (spring 2023) is flagged.
    if hw.vendor_id == 0x10DE {
        if let Some(major) = hw.driver.split('.').next().and_then(|n| n.parse::<u32>().ok()) {
            return major < 531;
        }
    }
    false
}

/// The NVIDIA style number out of the Windows driver version `a.b.c.d` (for 31.0.15.4601: 546.01).
pub fn nvidia_user_version(parts: [u16; 4]) -> String {
    let [_, _, c, d] = parts;
    format!("{}.{:02}", (c % 10) as u32 * 100 + (d / 100) as u32, d % 100)
}

impl Hardware {
    pub fn capabilities(&self, active: Option<u64>) -> Vec<(&'static str, String)> {
        use crate::i18n::tr;
        let checked = self.checked_adapter.and_then(|id| self.adapters.iter().find(|a| a.luid == id));
        let active = active.and_then(|id| self.adapters.iter().find(|a| a.luid == id));
        let caps = active.or(checked).map(|a| &a.capabilities);
        let unavailable = || tr("monitor.unavailable").to_owned();
        let toggle = |value: Option<bool>| match value {
            Some(true) => tr("monitor.supported").into(),
            Some(false) => tr("monitor.unsupported").into(),
            None => unavailable(),
        };
        let tier = |value: Option<u32>, version: bool| match value {
            Some(0) => tr("monitor.unsupported").into(),
            Some(n) if version => format!("{}.{}", n / 10, n % 10),
            Some(n) => n.to_string(),
            None => unavailable(),
        };
        vec![
            ("monitor.adapter_checked", checked.map_or_else(unavailable, |a| a.name.clone())),
            ("monitor.adapter_rendering", active.map_or_else(unavailable, |a| a.name.clone())),
            ("monitor.shader_model", caps.and_then(|c| c.shader_model.clone()).unwrap_or_else(unavailable)),
            ("monitor.binding_tier", tier(caps.and_then(|c| c.resource_binding_tier), false)),
            ("monitor.mesh_tier", tier(caps.and_then(|c| c.mesh_shader_tier), true)),
            ("monitor.ray_tier", tier(caps.and_then(|c| c.ray_tracing_tier), true)),
            ("monitor.enhanced_barriers", toggle(caps.and_then(|c| c.enhanced_barriers))),
            ("monitor.gpu_upload_heap", toggle(caps.and_then(|c| c.gpu_upload_heap))),
            ("monitor.wave_width", caps.and_then(|c| c.wave_lanes).map_or_else(unavailable, |[min, max]| format!("{min} to {max}"))),
        ]
    }

    pub fn hybrid_adapters(&self) -> bool {
        self.adapters.iter().any(|a| a.capabilities.unified_memory == Some(true))
            && self.adapters.iter().any(|a| a.capabilities.unified_memory == Some(false))
    }

    /// Short multi-line text for the bug report and the GitHub issue.
    pub fn summary(&self) -> String {
        self.summary_with_adapter(None)
    }

    pub fn summary_with_adapter(&self, active: Option<u64>) -> String {
        let mesh = match self.mesh_shaders {
            Some(true) => "yes",
            Some(false) => "no",
            None => "unknown",
        };
        let mut summary = format!(
            "GPU: {} ({} MB VRAM)\nDriver: {}\nMesh shaders: {}\nRAM: {} MB\nCPU: {} ({} cores, {} threads)\nWindows build: {}\n",
            self.gpu_name, self.vram_mb, self.driver, mesh, self.ram_mb, self.cpu_name, self.cpu_cores, self.cpu_threads, self.windows_build
        );
        use std::fmt::Write;
        for (key, value) in self.capabilities(active) {
            let _ = writeln!(summary, "{}: {value}", crate::i18n::tr(key));
        }
        summary
    }

    /// Demo machine for screenshots.
    pub fn demo() -> Self {
        Self {
            gpu_name: "NVIDIA GeForce RTX 3080 Ti".into(),
            vendor_id: 0x10DE,
            vram_mb: 12_288,
            driver: "546.01".into(),
            driver_parts: [31, 0, 15, 4601],
            mesh_shaders: Some(true),
            checked_adapter: Some(1),
            adapters: vec![Adapter {
                name: "NVIDIA GeForce RTX 3080 Ti".into(),
                luid: 1,
                capabilities: Capabilities {
                    shader_model: Some("6.7".into()),
                    resource_binding_tier: Some(3),
                    mesh_shader_tier: Some(10),
                    ray_tracing_tier: Some(11),
                    enhanced_barriers: Some(true),
                    gpu_upload_heap: Some(true),
                    wave_lanes: Some([32, 32]),
                    unified_memory: Some(false),
                },
            }],
            ram_mb: 65_536,
            cpu_name: "AMD Ryzen 7 5700X3D 8-Core Processor".into(),
            cpu_cores: 8,
            cpu_threads: 16,
            windows_build: "26100.2605".into(),
            screens: 2,
            note: String::new(),
        }
    }
}

#[cfg(not(windows))]
pub fn probe() -> Hardware {
    Hardware {
        cpu_threads: std::thread::available_parallelism().map_or(0, |n| n.get() as u32),
        note: "Hardware check is only available on Windows.".into(),
        ..Hardware::default()
    }
}

#[cfg(windows)]
pub fn probe() -> Hardware {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    let mut hw = Hardware::default();
    // SAFETY: a plain query of the system.
    hw.screens = unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CMONITORS) }.max(0) as u32;
    hw.cpu_threads = std::thread::available_parallelism().map_or(0, |n| n.get() as u32);
    hw.cpu_cores = physical_cores().unwrap_or(hw.cpu_threads);
    hw.ram_mb = total_ram_mb();
    hw.cpu_name = reg_string(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString").unwrap_or_default().trim().to_string();
    let nt = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let build = reg_string(nt, "CurrentBuildNumber").unwrap_or_default();
    let ubr = reg_dword(nt, "UBR");
    hw.windows_build = match ubr {
        Some(u) if !build.is_empty() => format!("{build}.{u}"),
        _ => build,
    };
    let factory: windows::core::Result<IDXGIFactory1> = unsafe { CreateDXGIFactory1() };
    let Ok(factory) = factory else {
        hw.note = "No DXGI factory: the graphics stack looks unusable.".into();
        return hw;
    };
    let mut best: Option<(u64, windows::Win32::Graphics::Dxgi::IDXGIAdapter1)> = None;
    let mut i = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(i) } {
        i += 1;
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else { continue };
        if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue;
        }
        let vram = desc.DedicatedVideoMemory as u64;
        let luid = (u64::from(desc.AdapterLuid.HighPart as u32) << 32) | u64::from(desc.AdapterLuid.LowPart);
        let capabilities = probe_capabilities(&adapter).unwrap_or_default();
        if best.as_ref().is_none_or(|(v, _)| vram > *v) {
            hw.gpu_name = wide_to_string(&desc.Description);
            hw.vendor_id = desc.VendorId;
            hw.vram_mb = vram >> 20;
            hw.checked_adapter = Some(luid);
            hw.mesh_shaders = capabilities.mesh_shader_tier.map(|tier| tier > 0);
            best = Some((vram, adapter));
        }
        hw.adapters.push(Adapter { name: wide_to_string(&desc.Description), luid, capabilities });
    }
    let Some((_, adapter)) = best else {
        hw.note = "No graphics adapter found.".into();
        return hw;
    };
    hw.driver_parts = driver_parts(&adapter);
    hw.driver = if hw.vendor_id == 0x10DE {
        nvidia_user_version(hw.driver_parts)
    } else {
        hw.driver_parts.map(|p| p.to_string()).join(".")
    };
    hw
}

#[cfg(windows)]
fn wide_to_string(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end]).trim().to_string()
}

#[cfg(windows)]
fn driver_parts(adapter: &windows::Win32::Graphics::Dxgi::IDXGIAdapter1) -> [u16; 4] {
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    match unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID) } {
        Ok(v) => [(v >> 48) as u16, (v >> 32) as u16, (v >> 16) as u16, v as u16],
        Err(_) => [0; 4],
    }
}

/// Probes a device and drops it before returning; unsupported queries stay absent.
#[cfg(windows)]
fn probe_capabilities(adapter: &windows::Win32::Graphics::Dxgi::IDXGIAdapter1) -> Option<Capabilities> {
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0;
    use windows::Win32::Graphics::Direct3D12::*;
    fn check<T>(device: &ID3D12Device, feature: D3D12_FEATURE, mut data: T) -> Option<T> {
        // SAFETY: each call pairs a feature with its documented structure.
        unsafe { device.CheckFeatureSupport(feature, (&raw mut data).cast(), size_of::<T>() as u32) }.ok()?;
        Some(data)
    }
    let mut device: Option<ID3D12Device> = None;
    unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_12_0, &mut device) }.ok()?;
    let device = device?;
    let base = check(&device, D3D12_FEATURE_D3D12_OPTIONS, D3D12_FEATURE_DATA_D3D12_OPTIONS::default());
    let wave = check(&device, D3D12_FEATURE_D3D12_OPTIONS1, D3D12_FEATURE_DATA_D3D12_OPTIONS1::default());
    let ray = check(&device, D3D12_FEATURE_D3D12_OPTIONS5, D3D12_FEATURE_DATA_D3D12_OPTIONS5::default());
    let mesh = check(&device, D3D12_FEATURE_D3D12_OPTIONS7, D3D12_FEATURE_DATA_D3D12_OPTIONS7::default());
    let barrier = check(&device, D3D12_FEATURE_D3D12_OPTIONS12, D3D12_FEATURE_DATA_D3D12_OPTIONS12::default());
    let upload = check(&device, D3D12_FEATURE_D3D12_OPTIONS16, D3D12_FEATURE_DATA_D3D12_OPTIONS16::default());
    let arch = check(&device, D3D12_FEATURE_ARCHITECTURE1, D3D12_FEATURE_DATA_ARCHITECTURE1::default());
    let shader_model = [
        D3D_SHADER_MODEL_6_9, D3D_SHADER_MODEL_6_8, D3D_SHADER_MODEL_6_7, D3D_SHADER_MODEL_6_6,
        D3D_SHADER_MODEL_6_5, D3D_SHADER_MODEL_6_4, D3D_SHADER_MODEL_6_3, D3D_SHADER_MODEL_6_2,
        D3D_SHADER_MODEL_6_1, D3D_SHADER_MODEL_6_0, D3D_SHADER_MODEL_5_1,
    ].into_iter().find_map(|model| {
        check(&device, D3D12_FEATURE_SHADER_MODEL, D3D12_FEATURE_DATA_SHADER_MODEL { HighestShaderModel: model })
            .map(|value| format!("{}.{}", value.HighestShaderModel.0 >> 4, value.HighestShaderModel.0 & 15))
    });
    Some(Capabilities {
        shader_model,
        resource_binding_tier: base.map(|o| o.ResourceBindingTier.0 as u32),
        mesh_shader_tier: mesh.map(|o| o.MeshShaderTier.0 as u32),
        ray_tracing_tier: ray.map(|o| o.RaytracingTier.0 as u32),
        enhanced_barriers: barrier.map(|o| o.EnhancedBarriersSupported.as_bool()),
        gpu_upload_heap: upload.map(|o| o.GPUUploadHeapSupported.as_bool()),
        wave_lanes: wave.and_then(|o| o.WaveOps.as_bool().then_some([o.WaveLaneCountMin, o.WaveLaneCountMax])),
        unified_memory: arch.map(|o| o.UMA.as_bool()),
    })
}

#[cfg(windows)]
fn total_ram_mb() -> u64 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() { m.ullTotalPhys >> 20 } else { 0 }
}

#[cfg(windows)]
fn physical_cores() -> Option<u32> {
    use windows::Win32::System::SystemInformation::{GetLogicalProcessorInformation, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION};
    let mut len = 0u32;
    let _ = unsafe { GetLogicalProcessorInformation(None, &mut len) };
    let n = len as usize / std::mem::size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>();
    if n == 0 {
        return None;
    }
    let mut buf = vec![SYSTEM_LOGICAL_PROCESSOR_INFORMATION::default(); n];
    unsafe { GetLogicalProcessorInformation(Some(buf.as_mut_ptr()), &mut len) }.ok()?;
    Some(buf.iter().filter(|i| i.Relationship == RelationProcessorCore).count() as u32)
}

#[cfg(windows)]
fn reg_string(subkey: &str, name: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let key: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
    let val: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut buf = vec![0u16; 256];
    let mut bytes = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(HKEY_LOCAL_MACHINE, PCWSTR(key.as_ptr()), PCWSTR(val.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut bytes))
    };
    rc.is_ok().then(|| wide_to_string(&buf))
}

#[cfg(windows)]
fn reg_dword(subkey: &str, name: &str) -> Option<u32> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
    let key: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
    let val: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut out = 0u32;
    let mut bytes = 4u32;
    let rc = unsafe {
        RegGetValueW(HKEY_LOCAL_MACHINE, PCWSTR(key.as_ptr()), PCWSTR(val.as_ptr()), RRF_RT_REG_DWORD, None, Some(&mut out as *mut u32 as *mut _), Some(&mut bytes))
    };
    rc.is_ok().then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_version_from_wddm_parts() {
        assert_eq!(nvidia_user_version([31, 0, 15, 4601]), "546.01");
        assert_eq!(nvidia_user_version([32, 0, 15, 6094]), "560.94");
    }

    #[test]
    fn warnings_follow_the_rules() {
        let ok = Hardware::demo();
        assert!(warnings(&ok).is_empty());
        let mut hw = ok.clone();
        hw.mesh_shaders = Some(false);
        assert_eq!(warnings(&hw)[0].severity, Severity::Blocker);
        hw = ok.clone();
        hw.ram_mb = 8_000;
        hw.vram_mb = 4_096;
        assert_eq!(warnings(&hw).len(), 2);
        hw = ok.clone();
        hw.driver = "472.12".into();
        hw.driver_parts = [30, 0, 14, 7212];
        assert_eq!(warnings(&hw).len(), 1);
        hw = ok;
        hw.mesh_shaders = None; // unknown is not a blocker
        assert!(warnings(&hw).is_empty());
    }
}
