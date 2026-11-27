//! Hardware check: GPU, VRAM, driver, mesh shader support, RAM, CPU and Windows build, with warnings in
//! plain words. The D3D12 device is created once on a worker thread and dropped right away; nothing stays
//! alive afterwards. The warning rules are pure functions so they can be tested without a GPU.
use serde::Serialize;

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
    pub ram_mb: u64,
    pub cpu_name: String,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub windows_build: String,
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
    /// Short multi-line text for the bug report and the GitHub issue.
    pub fn summary(&self) -> String {
        let mesh = match self.mesh_shaders {
            Some(true) => "yes",
            Some(false) => "no",
            None => "unknown",
        };
        format!(
            "GPU: {} ({} MB VRAM)\nDriver: {}\nMesh shaders: {}\nRAM: {} MB\nCPU: {} ({} cores, {} threads)\nWindows build: {}\n",
            self.gpu_name, self.vram_mb, self.driver, mesh, self.ram_mb, self.cpu_name, self.cpu_cores, self.cpu_threads, self.windows_build
        )
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
            ram_mb: 65_536,
            cpu_name: "AMD Ryzen 7 5700X3D 8-Core Processor".into(),
            cpu_cores: 8,
            cpu_threads: 16,
            windows_build: "26100.2605".into(),
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
        if best.as_ref().is_none_or(|(v, _)| vram > *v) {
            hw.gpu_name = wide_to_string(&desc.Description);
            hw.vendor_id = desc.VendorId;
            hw.vram_mb = vram >> 20;
            best = Some((vram, adapter));
        }
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
    hw.mesh_shaders = mesh_shader_support(&adapter);
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

/// Creates a D3D12 device on the adapter, asks about mesh shaders, and drops the device again.
#[cfg(windows)]
fn mesh_shader_support(adapter: &windows::Win32::Graphics::Dxgi::IDXGIAdapter1) -> Option<bool> {
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0;
    use windows::Win32::Graphics::Direct3D12::{
        D3D12CreateDevice, ID3D12Device, D3D12_FEATURE_D3D12_OPTIONS7, D3D12_FEATURE_DATA_D3D12_OPTIONS7,
        D3D12_MESH_SHADER_TIER_NOT_SUPPORTED,
    };
    let mut device: Option<ID3D12Device> = None;
    unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_12_0, &mut device) }.ok()?;
    let device = device?;
    let mut opts = D3D12_FEATURE_DATA_D3D12_OPTIONS7::default();
    unsafe {
        device
            .CheckFeatureSupport(
                D3D12_FEATURE_D3D12_OPTIONS7,
                &mut opts as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<D3D12_FEATURE_DATA_D3D12_OPTIONS7>() as u32,
            )
            .ok()?;
    }
    Some(opts.MeshShaderTier != D3D12_MESH_SHADER_TIER_NOT_SUPPORTED)
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
