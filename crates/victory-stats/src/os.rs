//! Mapping of the block: Windows file mapping, or a plain heap block elsewhere (tests on Linux).
use super::{Block, SIZE};

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Memory::{
        CreateFileMappingW, FILE_MAP_READ, FILE_MAP_WRITE, MapViewOfFile, OpenFileMappingW,
        PAGE_READWRITE, UnmapViewOfFile,
    };
    use windows_sys::Win32::System::Performance::{
        QueryPerformanceCounter, QueryPerformanceFrequency,
    };

    pub enum Owner {
        Mapped { handle: HANDLE, view: *const Block },
        Heap(#[allow(dead_code)] Box<Block>),
    }

    impl Drop for Owner {
        fn drop(&mut self) {
            if let Owner::Mapped { handle, view } = self {
                // SAFETY: both came from the calls in `create` or `open` and are released once.
                unsafe {
                    UnmapViewOfFile(
                        windows_sys::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                            Value: *view as *mut _,
                        },
                    );
                    CloseHandle(*handle);
                }
            }
        }
    }

    fn wide(name: &str) -> Vec<u16> {
        name.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn create(name: &str) -> Option<(Owner, *const Block)> {
        let w = wide(name);
        // SAFETY: plain FFI with a valid zero terminated name; default security (own session only).
        unsafe {
            let handle = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                SIZE as u32,
                w.as_ptr(),
            );
            if handle.is_null() {
                return None;
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(handle);
                return None;
            }
            let view = MapViewOfFile(handle, FILE_MAP_WRITE | FILE_MAP_READ, 0, 0, SIZE);
            if view.Value.is_null() {
                CloseHandle(handle);
                return None;
            }
            let p = view.Value as *const Block;
            Some((Owner::Mapped { handle, view: p }, p))
        }
    }

    pub fn open(name: &str) -> Option<(Owner, *const u8, usize)> {
        let w = wide(name);
        // SAFETY: as above; the view is read only.
        unsafe {
            let handle = OpenFileMappingW(FILE_MAP_READ, 0, w.as_ptr());
            if handle.is_null() {
                return None;
            }
            for len in [SIZE, super::super::V1_SIZE] {
                let view = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, len);
                if !view.Value.is_null() {
                    let p = view.Value as *const Block;
                    return Some((Owner::Mapped { handle, view: p }, p.cast(), len));
                }
            }
            CloseHandle(handle);
            None
        }
    }

    pub fn heap() -> (Owner, *const Block) {
        let b = zeroed_box();
        let p: *const Block = &*b;
        (Owner::Heap(b), p)
    }

    pub fn qpc_now() -> u64 {
        let mut v = 0i64;
        // SAFETY: valid out pointer; cannot fail on Windows XP and later.
        unsafe { QueryPerformanceCounter(&mut v) };
        v as u64
    }

    pub fn qpc_freq() -> u64 {
        let mut v = 0i64;
        // SAFETY: as above.
        unsafe { QueryPerformanceFrequency(&mut v) };
        (v as u64).max(1)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub enum Owner {
        Heap(#[allow(dead_code)] Box<Block>),
    }

    pub fn create(_name: &str) -> Option<(Owner, *const Block)> {
        None
    }
    pub fn open(_name: &str) -> Option<(Owner, *const u8, usize)> {
        None
    }
    pub fn heap() -> (Owner, *const Block) {
        let b = zeroed_box();
        let p: *const Block = &*b;
        (Owner::Heap(b), p)
    }
    pub fn qpc_now() -> u64 {
        use std::sync::OnceLock;
        static T0: OnceLock<std::time::Instant> = OnceLock::new();
        T0.get_or_init(std::time::Instant::now).elapsed().as_nanos() as u64
    }
    pub fn qpc_freq() -> u64 {
        1_000_000_000
    }
}

/// A zeroed block on the heap (all atomics are valid when zero).
fn zeroed_box() -> Box<Block> {
    let layout = std::alloc::Layout::new::<Block>();
    // SAFETY: `Block` is plain atomics and padding, for which all zero bytes is a valid value.
    unsafe {
        let p = std::alloc::alloc_zeroed(layout) as *mut Block;
        if p.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Box::from_raw(p)
    }
}

pub use imp::*;
const _: usize = SIZE;
