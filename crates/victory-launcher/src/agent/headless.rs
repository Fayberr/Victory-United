//! `--headless`: the launcher and its agent endpoint with no window. It never creates a window and
//! does not look at the graphics card until a tool needs to know it.
use super::{Backend, Signal, auth, http};
use std::{
    io::Write,
    sync::{Arc, OnceLock},
};

/// Set once the endpoint is gone, so that a closing console can wait for the cleanup.
static STOPPING: OnceLock<(Arc<Backend>, Signal)> = OnceLock::new();

pub fn run() -> i32 {
    attach_console();
    let (backend, done) = STOPPING.get_or_init(|| (Backend::standalone(), Signal::default()));
    watch_for_interrupt();
    let server = match http::Server::start(backend.clone()) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("victory-launcher: {error:#}");
            return 1;
        }
    };
    let mut out = std::io::stdout();
    let _ = writeln!(out, "port: {}", server.port());
    let _ = writeln!(
        out,
        "token file: {}",
        auth::endpoint_file(&backend.dirs).display()
    );
    let _ = writeln!(out, "Stop with Ctrl+C or the shutdown tool.");
    let _ = out.flush();
    backend.stop.wait();
    drop(server);
    backend.finish_jobs();
    done.set();
    0
}

fn interrupted() {
    if let Some((backend, _)) = STOPPING.get() {
        backend.stop.set();
    }
}

/// The launcher is built without a console of its own. When started from one, join it, so that output
/// shows up and Ctrl+C arrives. Pipes and files the caller passed in stay as they are.
#[cfg(windows)]
fn attach_console() {
    use windows::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
        STD_OUTPUT_HANDLE, SetStdHandle,
    };
    let kinds = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
    // SAFETY: plain console calls; the saved handles are the ones this process was started with.
    unsafe {
        let given = kinds.map(|kind| {
            GetStdHandle(kind)
                .ok()
                .filter(|h| !h.is_invalid() && !h.0.is_null())
        });
        if AttachConsole(ATTACH_PARENT_PROCESS).is_ok() {
            for (kind, handle) in kinds.into_iter().zip(given) {
                if let Some(handle) = handle {
                    let _ = SetStdHandle(kind, handle);
                }
            }
        }
    }
}

#[cfg(not(windows))]
fn attach_console() {}

#[cfg(windows)]
fn watch_for_interrupt() {
    use windows::{
        Win32::System::Console::{CTRL_C_EVENT, SetConsoleCtrlHandler},
        core::BOOL,
    };
    unsafe extern "system" fn handler(event: u32) -> BOOL {
        interrupted();
        // Closing the console or logging off ends the process when this returns: wait for the cleanup.
        if event != CTRL_C_EVENT
            && let Some((_, done)) = STOPPING.get() {
                done.wait_for(std::time::Duration::from_secs(4));
            }
        true.into()
    }
    // SAFETY: the handler only touches synchronised state that lives as long as the process.
    unsafe {
        // A parent may have switched Ctrl+C off for its children.
        let _ = SetConsoleCtrlHandler(None, false);
        let _ = SetConsoleCtrlHandler(Some(handler), true);
    }
}

/// A signal handler may do very little, so it only writes one byte to a pipe a thread waits on.
#[cfg(unix)]
fn watch_for_interrupt() {
    use std::sync::atomic::{AtomicI32, Ordering};
    static PIPE: AtomicI32 = AtomicI32::new(-1);
    extern "C" fn handler(_: libc::c_int) {
        // SAFETY: write is async-signal-safe.
        unsafe { libc::write(PIPE.load(Ordering::Relaxed), [1u8].as_ptr().cast(), 1) };
    }
    let mut ends = [0; 2];
    // SAFETY: pipe fills the two descriptors; the handler is installed after the write end is stored.
    unsafe {
        if libc::pipe(ends.as_mut_ptr()) != 0 {
            return;
        }
        PIPE.store(ends[1], Ordering::Relaxed);
        let handler = handler as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
    let read = ends[0];
    std::thread::spawn(move || {
        let mut byte = [0u8; 1];
        // SAFETY: reads into a one byte buffer.
        if unsafe { libc::read(read, byte.as_mut_ptr().cast(), 1) } == 1 {
            interrupted();
        }
    });
}
