//! Sends the records of the `log` facade, the core's included (D-02), to
//! the OBS log, the one place users and bug reports look.

use std::ffi::c_int;

use libobs_sys::{LOG_DEBUG, LOG_ERROR, LOG_INFO, LOG_WARNING, blog};
use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::c_string;

/// Tells this plugin's lines apart in a log shared by every module.
const PREFIX: &str = "[obs-dbus-mpris]";

struct ObsLog;

static LOGGER: ObsLog = ObsLog;

pub fn install() {
    // Debug lines are kept: OBS itself writes them only when started with
    // --verbose, which is when they are wanted.
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
}

impl Log for ObsLog {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let level = match record.level() {
            Level::Error => LOG_ERROR,
            Level::Warn => LOG_WARNING,
            Level::Info => LOG_INFO,
            Level::Debug | Level::Trace => LOG_DEBUG,
        };
        let message = c_string(&format!("{PREFIX} {}", record.args()));
        // SAFETY: the "%s" format reads exactly the one C string given, so a
        // message carrying text from a player is never read as a format.
        unsafe { blog(level as c_int, c"%s".as_ptr(), message.as_ptr()) };
    }

    fn flush(&self) {}
}
