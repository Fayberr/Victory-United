//! D3D12.X on host D3D12.
//!
//! D3D12.X records commands from code inlined into the title, which writes
//! packets into the command list while its put pointer is below a limit and
//! calls the list's method otherwise. Our lists keep the limits at zero, so
//! every command arrives here as a call, to be run on host D3D12. The
//! title's GPU addresses are CPU addresses (console memory is unified).
//!
//! [`device`] is the device object titles create, [`dxgi`] its adapter side,
//! [`resource`] heaps and resources, [`fence`] fences, [`queue`] command
//! queues, [`list`] command allocators and lists, [`descriptor`]
//! descriptor heaps, [`root`] root signatures, [`query`] query heaps,
//! [`signature`] command signatures, [`pipeline`] pipeline states,
//! [`frame`] frame pacing, [`object`] what all console graphics objects
//! share, [`console`] which console model the title sees. Planned:
//! `present` (swap chain).

#[cfg(windows)]
mod ambient;
#[cfg(all(windows, feature = "diagnostics"))]
mod backreads;
#[cfg(all(windows, feature = "diagnostics"))]
mod capture;
#[cfg(windows)]
mod cardmeter;
#[cfg(all(windows, feature = "diagnostics"))]
mod renderdoc;
#[cfg(all(windows, feature = "diagnostics"))]
mod shot;
#[cfg(windows)]
mod compose;
pub mod console;
#[cfg(all(windows, feature = "diagnostics"))]
mod copystat;
#[cfg(all(windows, feature = "diagnostics"))]
mod faultbook;
#[cfg(windows)]
mod cores;
pub mod descriptor;
pub mod device;
#[cfg(windows)]
<<<<<<< HEAD
mod drvwatch;
#[cfg(windows)]
mod fpsbox;
#[cfg(windows)]
mod spread;
#[cfg(windows)]
||||||| a66ac212
=======
mod ambient;
#[cfg(windows)]
mod cores;
#[cfg(windows)]
mod placement;
#[cfg(windows)]
>>>>>>> klbf/beta-2
mod draw;
pub mod dxgi;
#[cfg(windows)]
pub mod external;
pub mod fence;
pub mod frame;
<<<<<<< HEAD
#[cfg(all(windows, feature = "diagnostics"))]
mod book;
#[cfg_attr(not(windows), allow(dead_code))]
mod heap_pass;
#[cfg(windows)]
mod fx;
#[cfg(windows)]
pub mod framelog;
#[cfg(windows)]
mod heap_worker;
||||||| a66ac212
=======
pub mod frametime;
>>>>>>> klbf/beta-2
#[cfg(windows)]
mod host;
#[cfg(windows)]
<<<<<<< HEAD
mod over;
#[cfg(windows)]
||||||| a66ac212
=======
mod overlay;
#[cfg(windows)]
mod pixels;
#[cfg(windows)]
mod screenshot;
#[cfg(windows)]
>>>>>>> klbf/beta-2
mod indirect;
#[cfg(windows)]
mod inplace;
<<<<<<< HEAD
#[cfg(windows)]
mod land;
||||||| a66ac212
=======
#[cfg(windows)]
mod port;
#[cfg(windows)]
pub mod port_descriptors;
>>>>>>> klbf/beta-2
pub mod list;
#[cfg(windows)]
mod memcopy;
#[cfg(windows)]
mod memory;
pub mod object;
pub mod pipeline;
#[cfg(windows)]
mod picked;
#[cfg(windows)]
mod plaincopy;
#[cfg(windows)]
mod pool;
#[cfg(all(windows, feature = "diagnostics"))]
mod popwatch;
#[cfg(windows)]
pub mod precompile;
#[cfg(windows)]
mod primitive;
<<<<<<< HEAD
#[cfg(windows)]
mod ranges;
#[cfg(all(windows, feature = "diagnostics"))]
mod profile;
||||||| a66ac212
=======
// (ours) the sampling profiler of byartesao/victory-giga-fork (VICTORY_PROFILE=1)
#[cfg(all(windows, feature = "diagnostics"))]
mod profile;
>>>>>>> klbf/beta-2
pub mod query;
#[cfg(windows)]
mod present_trace;
pub mod queue;
#[cfg(windows)]
mod queue2;
#[cfg(windows)]
mod readback;
#[cfg(windows)]
mod rebar;
#[cfg(all(windows, feature = "diagnostics"))]
pub mod replay;
pub mod resource;
pub mod root;
#[cfg(windows)]
mod census;
#[cfg(windows)]
mod safecopy;
#[cfg(windows)]
mod sampler;
#[cfg(windows)]
pub mod shader_cache;
#[cfg(windows)]
<<<<<<< HEAD
||||||| a66ac212
mod memory;
#[cfg(windows)]
=======
mod memory;
#[cfg(windows)]
mod ramline;
#[cfg(windows)]
>>>>>>> klbf/beta-2
mod shadow;
<<<<<<< HEAD
#[cfg(windows)]
mod stamp;
#[cfg(windows)]
mod tiles;
||||||| a66ac212
=======
#[cfg(windows)]
mod windowup;
#[cfg(windows)]
mod tiles;
>>>>>>> klbf/beta-2
pub mod signature;
mod switch;
#[cfg(all(windows, feature = "diagnostics"))]
mod ab;
#[cfg(windows)]
mod tdr;
#[cfg(windows)]
mod texback;
#[cfg(windows)]
mod texture;
pub mod timeline;
#[cfg(windows)]
pub mod titlewrite;
#[cfg(windows)]
mod upload;
#[cfg(windows)]
pub mod titlemem;
#[cfg(windows)]
mod watch;
#[cfg(windows)]
mod texback;

#[cfg(windows)]
pub use host::on_failure;

/// Whether `switch` is on now, if it is one of those `VICTORY_AB` turns on and
/// off while the title runs (`ab`).
#[cfg(all(windows, feature = "diagnostics"))]
pub fn switched(switch: &str) -> Option<bool> {
    ab::pick(switch)
}

/// What the frame is at (`copystat::phase`), for the sampling profiler.
#[cfg(all(windows, feature = "diagnostics"))]
pub fn frame_phase() -> u32 {
    copystat::phase()
}
#[cfg(windows)]
pub use census::title_memory_reserved;
#[cfg(windows)]
pub use inplace::{title_memory_freeing, title_memory_section, title_memory_section_gone, title_memory_view};
#[cfg(windows)]
pub use watch::{
<<<<<<< HEAD
    kernel_writing, title_memory_allocated, title_memory_committed, title_memory_freed,
    title_memory_reading, title_memory_remapped, title_memory_remapping, title_memory_splitting,
    title_memory_unmapping, title_memory_written, title_pages_changed, title_pages_told,
    writing as kernel_will_write,
||||||| a66ac212
    title_memory_reading, title_memory_remapped, title_memory_remapping, writing as kernel_will_write,
=======
    Remapping, notify_view_writes, title_memory_reading, title_memory_reading_range, title_memory_remapped,
    title_memory_remapping, title_memory_remapping_range, title_memory_written, writing as kernel_will_write,
    written_through_view,
>>>>>>> klbf/beta-2
};

/// (ours) How many threads of this crate are inside a write-watch asking and inside a
/// change of protection now (`watch::INSIDE`).
#[cfg(windows)]
pub fn address_calls() -> [u32; 2] {
    [0, 1].map(|i| watch::INSIDE[i].load(std::sync::atomic::Ordering::Relaxed))
}
