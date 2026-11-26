//! Picking the host adapter through DXGI and creating its D3D12 device.

use anyhow::{Result, anyhow};
use clap::ValueEnum;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::*;

use crate::Failure;

/// NVIDIA's PCI vendor id.
const NVIDIA: u32 = 0x10DE;

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Want {
    /// An NVIDIA hardware adapter; none is a result of its own (exit code 3).
    Nvidia,
    /// The software adapter (WARP).
    Warp,
    /// The first hardware adapter, fastest first.
    Any,
}

pub struct Adapter {
    pub device: ID3D12Device,
    pub name: String,
    pub vendor: u32,
    pub device_id: u32,
    pub software: bool,
    pub memory_mib: usize,
    /// The highest shader model the device takes.
    pub shader_model: (u32, u32),
    /// Lane counts of its waves, if it has wave operations.
    pub wave_lanes: Option<(u32, u32)>,
}

struct Seen {
    adapter: IDXGIAdapter1,
    name: String,
    vendor: u32,
    device_id: u32,
    software: bool,
    memory_mib: usize,
}

/// Every adapter DXGI lists, fastest first.
fn seen() -> Result<Vec<Seen>> {
    let mut out = Vec::new();
    // SAFETY: plain DXGI enumeration.
    unsafe {
        let factory: IDXGIFactory6 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
        for i in 0.. {
            let Ok(adapter) = factory.EnumAdapterByGpuPreference::<IDXGIAdapter1>(
                i,
                DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
            ) else {
                break;
            };
            let d = adapter.GetDesc1()?;
            let len = d
                .Description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(d.Description.len());
            out.push(Seen {
                adapter,
                name: String::from_utf16_lossy(&d.Description[..len]),
                vendor: d.VendorId,
                device_id: d.DeviceId,
                software: d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0,
                memory_mib: d.DedicatedVideoMemory >> 20,
            });
        }
    }
    Ok(out)
}

/// The highest shader model `device` takes, as (major, minor).
fn shader_model(device: &ID3D12Device) -> (u32, u32) {
    // The query fails for a model the runtime does not know: ask downwards.
    for minor in (0..=9).rev() {
        let mut data = D3D12_FEATURE_DATA_SHADER_MODEL {
            HighestShaderModel: D3D_SHADER_MODEL(0x60 + minor),
        };
        // SAFETY: a feature query with its own structure.
        let ok = unsafe {
            device.CheckFeatureSupport(
                D3D12_FEATURE_SHADER_MODEL,
                (&raw mut data).cast(),
                size_of_val(&data) as u32,
            )
        };
        if ok.is_ok() {
            let model = data.HighestShaderModel.0 as u32;
            return (model >> 4, model & 0xf);
        }
    }
    (5, 1)
}

/// Which of the adapters DXGI lists, each a (vendor, software) pair, `want`
/// picks: the first that fits, so the fastest.
fn choose(want: Want, adapters: &[(u32, bool)]) -> Option<usize> {
    adapters.iter().position(|&(vendor, software)| match want {
        Want::Nvidia => vendor == NVIDIA && !software,
        Want::Warp => software,
        Want::Any => !software,
    })
}

/// The adapter asked for, with a device on it.
pub fn pick(want: Want) -> Result<Adapter> {
    let seen = seen()?;
    let listed = || {
        let all: Vec<String> = seen
            .iter()
            .map(|s| {
                format!(
                    "{} (vendor {:#06x}{})",
                    s.name,
                    s.vendor,
                    if s.software { ", software" } else { "" }
                )
            })
            .collect();
        if all.is_empty() {
            "none".to_owned()
        } else {
            all.join("; ")
        }
    };
    let kinds: Vec<(u32, bool)> = seen.iter().map(|s| (s.vendor, s.software)).collect();
    let Some(s) = choose(want, &kinds).map(|i| &seen[i]) else {
        let what = match want {
            Want::Nvidia => "no NVIDIA hardware adapter",
            Want::Warp => "no software adapter",
            Want::Any => "no hardware adapter",
        };
        return Err(Failure::NoAdapter(format!("{what}; DXGI lists: {}", listed())).into());
    };
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: device creation on a live adapter.
    unsafe { D3D12CreateDevice(&s.adapter, D3D_FEATURE_LEVEL_12_0, &mut device) }.map_err(|e| {
        Failure::NoAdapter(format!(
            "{}: no D3D12 device at feature level 12.0: {}",
            s.name,
            e.message()
        ))
    })?;
    let device = device.ok_or_else(|| anyhow!("{}: no device", s.name))?;
    let mut options = D3D12_FEATURE_DATA_D3D12_OPTIONS1::default();
    // SAFETY: a feature query with its own structure.
    let waves = unsafe {
        device.CheckFeatureSupport(
            D3D12_FEATURE_D3D12_OPTIONS1,
            (&raw mut options).cast(),
            size_of_val(&options) as u32,
        )
    }
    .is_ok()
        && options.WaveOps.as_bool();
    Ok(Adapter {
        shader_model: shader_model(&device),
        wave_lanes: waves.then_some((options.WaveLaneCountMin, options.WaveLaneCountMax)),
        device,
        name: s.name.clone(),
        vendor: s.vendor,
        device_id: s.device_id,
        software: s.software,
        memory_mib: s.memory_mib,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTEL: (u32, bool) = (0x8086, false);
    const AMD: (u32, bool) = (0x1002, false);
    const GEFORCE: (u32, bool) = (NVIDIA, false);
    const WARP: (u32, bool) = (0x1414, true);

    #[test]
    fn nvidia_is_picked_wherever_dxgi_lists_it() {
        assert_eq!(choose(Want::Nvidia, &[AMD, GEFORCE, WARP]), Some(1));
        assert_eq!(choose(Want::Any, &[AMD, GEFORCE, WARP]), Some(0));
        assert_eq!(choose(Want::Warp, &[AMD, GEFORCE, WARP]), Some(2));
    }

    #[test]
    fn a_machine_without_nvidia_hardware_has_none_to_pick() {
        assert_eq!(choose(Want::Nvidia, &[INTEL, AMD, WARP]), None);
        // A software adapter is not hardware, whatever vendor it names.
        assert_eq!(choose(Want::Nvidia, &[(NVIDIA, true)]), None);
        assert_eq!(choose(Want::Any, &[WARP]), None);
    }
}
