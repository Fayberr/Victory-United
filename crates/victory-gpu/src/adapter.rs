//! Which GPU of the machine the host device is made on.
//!
//! `D3D12CreateDevice(NULL)` picks one; it is not always the one the DLSS
//! Frame Generation add-on (`dlssg_for_sm86`) picks. Its backend makes the
//! CUDA context on CUDA device 0 and follows that card: the architecture
//! bridge it puts in front of the DLSS-G runtime (Ada/Blackwell for the game
//! and Streamline) and the SM86/SM75 kernels are chosen for it. On a PC with
//! two NVIDIA GPUs the D3D12 default can be the other one (measured: CUDA
//! device 0 the first 3090, the D3D12 default the second); the bridge then
//! lands on an adapter the device is not on, the NGX core's platform verdict
//! refuses `CreateFeature` with `0xbad00002` and no frame is generated (the
//! add-on's issue #674 — its reporter had to disable the second GPU in
//! Device Manager).
//!
//! [`prefer`] names the adapter the device must be made on instead of the
//! default: `streamline::sm86` asks for CUDA device 0's LUID before the
//! device is made. On one GPU it is the same adapter, and nothing changes.

use std::sync::OnceLock;

use windows::Win32::Foundation::LUID;

static WANTED: OnceLock<LUID> = OnceLock::new();

/// Makes the host device on `luid` from here on (once).
pub fn prefer(luid: LUID) {
    let _ = WANTED.set(luid);
}

/// The adapter the host device must be made on, if one was asked for.
pub fn wanted() -> Option<LUID> {
    WANTED.get().copied()
}

/// A LUID as the 64-bit value logs print.
pub fn key(luid: LUID) -> u64 {
    ((luid.HighPart as u64) << 32) | u64::from(luid.LowPart)
}

/// The LUID of that 64-bit value.
pub fn luid(key: u64) -> LUID {
    LUID { LowPart: key as u32, HighPart: (key >> 32) as i32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_luids_round_trip() {
        for k in [0, 1, 0x11ebc, 0x1_0000_0001, u64::MAX] {
            assert_eq!(key(luid(k)), k);
        }
    }
}
