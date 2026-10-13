//! OBS module of the "Now Playing (MPRIS)" source: the entry points OBS
//! looks up, as `OBS_DECLARE_MODULE()` declares them in C, and the one
//! MPRIS core every source reads.
//!
//! Nothing here links libobs: OBS loads this library into a process that
//! already holds libobs, and its symbols resolve there (architecture.md,
//! Builds).
//!
//! No panic may unwind into OBS (D-07, B-42). Every function OBS calls runs
//! its body through [`guard`].

mod locale;
mod logger;
mod settings;
mod source;

use std::any::Any;
use std::ffi::{CString, c_char};
use std::panic::{self, AssertUnwindSafe};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use libobs_sys::{LIBOBS_API_MAJOR_VER, LIBOBS_API_MINOR_VER, LIBOBS_API_PATCH_VER, obs_module_t};
use mpris_core::dbus::Core;
use mpris_core::model::Snapshot;

/// The libobs version the bindings were generated from, as
/// `LIBOBS_API_VER`. OBS refuses a module built for a newer libobs than
/// its own.
const API_VERSION: u32 =
    (LIBOBS_API_MAJOR_VER << 24) | (LIBOBS_API_MINOR_VER << 16) | LIBOBS_API_PATCH_VER;

/// Handle OBS gives the module, needed to load its locale files.
static MODULE: AtomicPtr<obs_module_t> = AtomicPtr::new(ptr::null_mut());

/// One core for the whole module, so the bus is watched once however many
/// sources there are (architecture.md, Fils d'exécution).
static CORE: Mutex<Option<Core>> = Mutex::new(None);

#[unsafe(no_mangle)]
pub extern "C" fn obs_module_set_pointer(module: *mut obs_module_t) {
    guard("obs_module_set_pointer", (), || {
        MODULE.store(module, Ordering::Release);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn obs_current_module() -> *mut obs_module_t {
    guard("obs_current_module", ptr::null_mut(), || {
        MODULE.load(Ordering::Acquire)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn obs_module_ver() -> u32 {
    guard("obs_module_ver", API_VERSION, || API_VERSION)
}

#[unsafe(no_mangle)]
pub extern "C" fn obs_module_load() -> bool {
    guard("obs_module_load", false, || {
        logger::install();
        // An unreachable bus is logged by the core and leaves the sources
        // showing their fallback, so it never stops the module from loading.
        *core() = Some(Core::start());
        source::register();
        log::info!("version {} loaded", env!("CARGO_PKG_VERSION"));
        true
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn obs_module_unload() {
    guard("obs_module_unload", (), || {
        // Taken out first, so the wait for the core thread does not hold
        // the lock.
        let stopped = core().take();
        drop(stopped);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn obs_module_name() -> *const c_char {
    guard("obs_module_name", ptr::null(), || {
        c"obs-dbus-mpris".as_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn obs_module_description() -> *const c_char {
    guard("obs_module_description", ptr::null(), || {
        c"Shows the track an MPRIS media player is playing, read over D-Bus.".as_ptr()
    })
}

/// Latest state of the players, empty while no core runs.
fn snapshot() -> Arc<Snapshot> {
    core().as_ref().map(Core::snapshot).unwrap_or_default()
}

fn core() -> MutexGuard<'static, Option<Core>> {
    // The lock only guards a swap of the whole value, which a panic cannot
    // leave half done.
    CORE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `body` and turns a panic into `fallback`, logged with `what`, so
/// that no panic unwinds into OBS (D-07).
fn guard<T>(what: &str, fallback: T, body: impl FnOnce() -> T) -> T {
    panic::catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|payload| {
        log::error!("{what} failed: {}", panic_message(payload.as_ref()));
        fallback
    })
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic")
}

/// `text` as a C string. NUL bytes, which C cannot carry, are dropped, so
/// that text from a player always reaches OBS.
fn c_string(text: &str) -> CString {
    CString::new(text.replace('\0', "")).unwrap_or_default()
}
