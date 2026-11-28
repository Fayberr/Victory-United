//! On-demand system samples. The worker blocks between explicit requests from the launcher.
use super::monitor::Live;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub pid: u32,
    pub luid: Option<u64>,
}

pub struct Sampler {
    request: SyncSender<Target>,
    reply: Receiver<(Target, Live)>,
}

impl Sampler {
    pub fn start(wake: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        let (request, input) = sync_channel(1);
        let (output, reply) = sync_channel(1);
        std::thread::Builder::new()
            .name("victory-monitor-sensors".into())
            .spawn(move || {
                let mut sampler = platform::State::default();
                while let Ok(target) = input.recv() {
                    let sample = sampler.read(target);
                    if output.send((target, sample)).is_err() {
                        break;
                    }
                    wake();
                }
            })?;
        Ok(Self { request, reply })
    }

    pub fn request(&self, target: Target) {
        let _ = self.request.try_send(target);
    }

    pub fn take(&self, target: Target) -> Option<Live> {
        self.reply
            .try_iter()
            .filter_map(|(for_target, data)| (for_target == target).then_some(data))
            .last()
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    #[derive(Default)]
    pub struct State;

    impl State {
        pub fn read(&mut self, _target: Target) -> Live {
            Live::default()
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        ffi::c_void,
        mem::{MaybeUninit, size_of},
    };
    use windows::{
        Win32::{
            Foundation::{FILETIME, FreeLibrary, HMODULE, LUID},
            Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory4},
            System::{
                LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
                Performance::*,
                SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
                Threading::GetSystemTimes,
            },
        },
        core::{PCSTR, PCWSTR, w},
    };

    #[derive(Default)]
    pub struct State {
        target: Option<Target>,
        nvml: Option<Nvml>,
        pdh: Option<Pdh>,
        cpu: Option<(u64, u64)>,
    }

    impl State {
        pub fn read(&mut self, target: Target) -> Live {
            if self.target != Some(target) {
                self.nvml = target.luid.and_then(Nvml::open);
                self.pdh = None;
                self.cpu = None;
                self.target = Some(target);
            }
            let (nv_load, gpu_temp) = self.nvml.as_ref().map_or((None, None), Nvml::read);
            let load = nv_load.or_else(|| {
                if self.pdh.is_none() {
                    self.pdh = Pdh::open();
                }
                self.pdh.as_mut()?.read(target)
            });
            let cpu_load = cpu_times().and_then(|now| {
                let (idle, total) = self.cpu.replace(now)?;
                let elapsed = now.1.checked_sub(total)?;
                let active = elapsed.saturating_sub(now.0.saturating_sub(idle));
                (elapsed > 0).then(|| (100.0 * active as f64 / elapsed as f64) as f32)
            });
            let mut mem = MEMORYSTATUSEX {
                dwLength: size_of::<MEMORYSTATUSEX>() as u32,
                ..Default::default()
            };
            // SAFETY: Windows fills this sized structure.
            let system_ram = unsafe { GlobalMemoryStatusEx(&mut mem) }.ok().map(|()| {
                (
                    mem.ullTotalPhys.saturating_sub(mem.ullAvailPhys),
                    mem.ullTotalPhys,
                )
            });
            Live {
                gpu_load: load,
                gpu_temp,
                cpu_load,
                system_ram,
                nvml_load: nv_load.is_some(),
            }
        }
    }

    fn cpu_times() -> Option<(u64, u64)> {
        let [mut idle, mut kernel, mut user] = [FILETIME::default(); 3];
        // SAFETY: valid output pointers. Kernel time includes idle time.
        unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
        let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        Some((ticks(idle), ticks(kernel).saturating_add(ticks(user))))
    }

    struct Pdh {
        query: PDH_HQUERY,
        counter: PDH_HCOUNTER,
        collected: bool,
    }

    impl Pdh {
        fn open() -> Option<Self> {
            let mut query = PDH_HQUERY::default();
            // SAFETY: PDH writes the handle; no log file requested.
            if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
                return None;
            }
            let mut pdh = Self {
                query,
                counter: PDH_HCOUNTER::default(),
                collected: false,
            };
            let status = unsafe {
                PdhAddEnglishCounterW(
                    query,
                    w!(r"\GPU Engine(*)\Utilization Percentage"),
                    0,
                    &mut pdh.counter,
                )
            };
            (status == 0).then_some(pdh)
        }

        fn read(&mut self, target: Target) -> Option<f32> {
            // SAFETY: a query/counter we own, used on this worker only.
            if unsafe { PdhCollectQueryData(self.query) } != 0 {
                return None;
            }
            if !std::mem::replace(&mut self.collected, true) {
                return None; // Rate counters need two samples.
            }
            let format = PDH_FMT_DOUBLE;
            let (mut bytes, mut count) = (0, 0);
            unsafe {
                PdhGetFormattedCounterArrayW(self.counter, format, &mut bytes, &mut count, None)
            };
            if bytes == 0 || bytes > 8 << 20 {
                return None;
            }
            let mut buffer = vec![
                MaybeUninit::<PDH_FMT_COUNTERVALUE_ITEM_W>::uninit();
                (bytes as usize)
                    .div_ceil(size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>())
            ];
            let ptr = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            if unsafe {
                PdhGetFormattedCounterArrayW(
                    self.counter,
                    format,
                    &mut bytes,
                    &mut count,
                    Some(ptr),
                )
            } != 0
                || count as usize > buffer.len()
            {
                return None;
            }
            // SAFETY: PDH returned count initialized records and the names in the same buffer.
            let items = unsafe { std::slice::from_raw_parts(ptr, count as usize) };
            items
                .iter()
                .filter_map(|item| {
                    if item.szName.is_null() || item.FmtValue.CStatus > 1 {
                        return None;
                    }
                    let name = unsafe { item.szName.to_string() }.ok()?;
                    if !super::matches_instance(&name, target) {
                        return None;
                    }
                    let usage = unsafe { item.FmtValue.Anonymous.doubleValue };
                    usage.is_finite().then_some(usage.clamp(0.0, 100.0) as f32)
                })
                .max_by(f32::total_cmp)
        }
    }

    impl Drop for Pdh {
        fn drop(&mut self) {
            // SAFETY: closing the query releases its counter.
            unsafe { PdhCloseQuery(self.query) };
        }
    }

    type Device = *mut c_void;
    type Status = i32;
    type Init = unsafe extern "C" fn() -> Status;
    type GetCount = unsafe extern "C" fn(*mut u32) -> Status;
    type GetHandle = unsafe extern "C" fn(u32, *mut Device) -> Status;
    type GetPci = unsafe extern "C" fn(Device, *mut Pci) -> Status;
    type GetTemp = unsafe extern "C" fn(Device, u32, *mut u32) -> Status;
    type GetLoad = unsafe extern "C" fn(Device, *mut Usage) -> Status;

    #[repr(C)]
    #[derive(Default)]
    struct Pci {
        bus_legacy: [u8; 16],
        domain: u32,
        bus: u32,
        device: u32,
        pci_device_id: u32,
        pci_subsystem_id: u32,
        bus_id: [u8; 32],
    }

    #[repr(C)]
    #[derive(Default)]
    struct Usage {
        gpu: u32,
        memory: u32,
    }

    struct Library(HMODULE);
    impl Drop for Library {
        fn drop(&mut self) {
            // SAFETY: owned LoadLibraryExW reference.
            let _ = unsafe { FreeLibrary(self.0) };
        }
    }

    struct Nvml {
        device: Device,
        shutdown: Init,
        load: GetLoad,
        temp: GetTemp,
        _library: Library,
    }

    impl Nvml {
        fn open(luid: u64) -> Option<Self> {
            let luid = LUID {
                LowPart: luid as u32,
                HighPart: (luid >> 32) as i32,
            };
            // SAFETY: DXGI returns the description for the actual engine adapter.
            let desc = unsafe {
                let factory: IDXGIFactory4 = CreateDXGIFactory1().ok()?;
                factory
                    .EnumAdapterByLuid::<IDXGIAdapter1>(luid)
                    .ok()?
                    .GetDesc1()
                    .ok()?
            };
            if desc.VendorId != 0x10de {
                return None;
            }
            // Only load driver-installed NVML from System32.
            let dll = Library(
                unsafe { LoadLibraryExW(w!("nvml.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
                    .ok()?,
            );
            macro_rules! symbol {
                ($name:literal, $type:ty) => {{
                    // SAFETY: NVML's exported C signatures are declared above.
                    let address =
                        unsafe { GetProcAddress(dll.0, PCSTR(concat!($name, "\0").as_ptr())) }?;
                    unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, $type>(address) }
                }};
            }
            let init = symbol!("nvmlInit_v2", Init);
            let shutdown = symbol!("nvmlShutdown", Init);
            let count = symbol!("nvmlDeviceGetCount_v2", GetCount);
            let at = symbol!("nvmlDeviceGetHandleByIndex_v2", GetHandle);
            let pci = symbol!("nvmlDeviceGetPciInfo_v3", GetPci);
            let load = symbol!("nvmlDeviceGetUtilizationRates", GetLoad);
            let temp = symbol!("nvmlDeviceGetTemperature", GetTemp);
            if unsafe { init() } != 0 {
                return None;
            }
            let mut nvml = Self {
                device: std::ptr::null_mut(),
                shutdown,
                load,
                temp,
                _library: dll,
            };
            let mut devices = 0;
            if unsafe { count(&mut devices) } != 0 {
                return None;
            }
            let mut matches = (0..devices).filter_map(|i| {
                let mut handle = std::ptr::null_mut();
                let mut info = Pci::default();
                if unsafe { at(i, &mut handle) } != 0
                    || handle.is_null()
                    || unsafe { pci(handle, &mut info) } != 0
                {
                    return None;
                }
                (info.pci_device_id == (desc.DeviceId << 16 | desc.VendorId)
                    && info.pci_subsystem_id == desc.SubSysId)
                    .then_some(handle)
            });
            nvml.device = matches.next()?;
            // Identical cards cannot be reliably distinguished by PCI IDs alone.
            matches.next().is_none().then_some(nvml)
        }

        fn read(&self) -> (Option<f32>, Option<u32>) {
            let mut usage = Usage::default();
            let mut temperature = 0;
            // SAFETY: device and function pointers belong to the open NVML session.
            unsafe {
                (
                    ((self.load)(self.device, &mut usage) == 0)
                        .then_some(usage.gpu.min(100) as f32),
                    ((self.temp)(self.device, 0, &mut temperature) == 0).then_some(temperature),
                )
            }
        }
    }

    impl Drop for Nvml {
        fn drop(&mut self) {
            // SAFETY: owns one successful nvmlInit_v2 call.
            unsafe { (self.shutdown)() };
        }
    }
}

#[cfg(any(test, windows))]
fn matches_instance(name: &str, target: Target) -> bool {
    let mut parts = name.split('_');
    if parts.next() != Some("pid")
        || parts.next().and_then(|pid| pid.parse().ok()) != Some(target.pid)
    {
        return false;
    }
    let Some(luid) = target.luid else { return true };
    if parts.next() != Some("luid") {
        return false;
    }
    let hex = |s: &str| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok();
    match (parts.next().and_then(hex), parts.next().and_then(hex)) {
        (Some(high), Some(low)) => (u64::from(high) << 32 | u64::from(low)) == luid,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_pdh_instances_to_the_right_process_and_adapter() {
        let name = "pid_42_luid_0x00000004_0x00000201_phys_0_eng_0_engtype_3D";
        assert!(matches_instance(
            name,
            Target {
                pid: 42,
                luid: Some((4 << 32) | 513)
            }
        ));
        assert!(matches_instance(
            name,
            Target {
                pid: 42,
                luid: None
            }
        ));
        assert!(!matches_instance(name, Target { pid: 4, luid: None }));
        assert!(!matches_instance(
            name,
            Target {
                pid: 42,
                luid: Some(513)
            }
        ));
    }
}
