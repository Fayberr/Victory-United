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
<<<<<<< HEAD
#[cfg(windows)]
mod cardmeter;
#[cfg(all(windows, feature = "diagnostics"))]
mod renderdoc;
#[cfg(all(windows, feature = "diagnostics"))]
mod shot;
#[cfg(windows)]
mod compose;
||||||| merged common ancestors
=======
#[cfg(windows)]
mod card;
>>>>>>> belobug/main
pub mod console;
#[cfg(all(windows, feature = "diagnostics"))]
mod copystat;
<<<<<<< HEAD
#[cfg(all(windows, feature = "diagnostics"))]
mod faultbook;
#[cfg(windows)]
mod cores;
||||||| merged common ancestors
=======
#[cfg(all(windows, feature = "diagnostics"))]
mod profile;
>>>>>>> belobug/main
pub mod descriptor;
pub mod device;
#[cfg(windows)]
mod drvwatch;
#[cfg(windows)]
mod fpsbox;
#[cfg(windows)]
mod spread;
#[cfg(windows)]
mod draw;
pub mod dxgi;
pub mod fence;
pub mod frame;
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
#[cfg(windows)]
mod host;
#[cfg(windows)]
mod over;
#[cfg(windows)]
mod indirect;
#[cfg(windows)]
mod inplace;
#[cfg(windows)]
mod land;
pub mod list;
<<<<<<< HEAD
#[cfg(windows)]
mod memcopy;
#[cfg(windows)]
mod memory;
||||||| merged common ancestors
=======
#[cfg(windows)]
mod memwait;
#[cfg(windows)]
mod memory;
mod timeline;
pub mod memstat;
>>>>>>> belobug/main
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
#[cfg(windows)]
mod ranges;
#[cfg(all(windows, feature = "diagnostics"))]
mod profile;
pub mod query;
#[cfg(windows)]
mod present_trace;
pub mod queue;
<<<<<<< HEAD
#[cfg(windows)]
mod queue2;
#[cfg(windows)]
mod readback;
#[cfg(windows)]
mod rebar;
#[cfg(all(windows, feature = "diagnostics"))]
pub mod replay;
||||||| merged common ancestors
=======
mod readout;
#[cfg(windows)]
mod rects;
>>>>>>> belobug/main
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
mod shadow;
#[cfg(windows)]
mod stamp;
#[cfg(windows)]
mod tiles;
pub mod signature;
mod switch;
#[cfg(all(windows, feature = "diagnostics"))]
mod ab;
#[cfg(windows)]
mod tdr;
#[cfg(windows)]
mod ambient;
#[cfg(windows)]
mod compose;
#[cfg(windows)]
mod twin;
#[cfg(windows)]
mod texpool;
#[cfg(windows)]
mod texture;
pub mod timeline;
#[cfg(windows)]
pub mod titlewrite;
#[cfg(windows)]
mod upload;
#[cfg(windows)]
mod watch;
#[cfg(windows)]
mod texback;

#[cfg(windows)]
pub use host::on_failure;
<<<<<<< HEAD

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
||||||| merged common ancestors
=======
pub use memstat::report as memory_report;
>>>>>>> belobug/main
#[cfg(windows)]
<<<<<<< HEAD
pub use census::title_memory_reserved;
#[cfg(windows)]
pub use inplace::{title_memory_freeing, title_memory_section, title_memory_section_gone, title_memory_view};
#[cfg(windows)]
pub use watch::{
    kernel_writing, title_memory_allocated, title_memory_committed, title_memory_freed,
    title_memory_reading, title_memory_remapped, title_memory_remapping, title_memory_splitting,
    title_memory_unmapping, title_memory_written, title_pages_changed, title_pages_told,
    writing as kernel_will_write,
};
||||||| merged common ancestors
pub use watch::{title_memory_remapped, writing as kernel_will_write};
=======
pub use watch::{
    title_memory_committed, title_memory_remapped, title_memory_view, title_memory_view_gone, title_memory_written,
    writing as kernel_will_write,
};
>>>>>>> belobug/main
