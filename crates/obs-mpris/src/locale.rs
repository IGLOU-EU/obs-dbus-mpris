//! Translations of the module (B-46), the functions that
//! `OBS_MODULE_USE_DEFAULT_LOCALE("obs-dbus-mpris", "en-US")` declares in C.
//! OBS hands over its locale when it opens the module, and the strings come
//! from `data/locale/<locale>.ini`, with en-US for what a locale lacks.

use std::ffi::{CStr, c_char};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

use libobs_sys::{lookup_t, obs_module_load_locale, text_lookup_destroy, text_lookup_getstr};

use crate::{c_string, guard, obs_current_module};

const DEFAULT_LOCALE: &CStr = c"en-US";

/// Strings of the current locale, null until OBS sets one. Atomic because
/// OBS reads translations from its UI and graphics threads alike.
static LOOKUP: AtomicPtr<lookup_t> = AtomicPtr::new(ptr::null_mut());

/// # Safety
///
/// `locale` is a C string, and OBS has set the module pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn obs_module_set_locale(locale: *const c_char) {
    guard("obs_module_set_locale", (), || {
        // SAFETY: the locale name and the module pointer come from OBS, per
        // the contract of this function, and the default locale is static.
        let lookup = unsafe {
            obs_module_load_locale(obs_current_module(), DEFAULT_LOCALE.as_ptr(), locale)
        };
        replace(lookup);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn obs_module_free_locale() {
    guard("obs_module_free_locale", (), || replace(ptr::null_mut()));
}

/// Translation of `key`, or `key` itself when the locale has none.
///
/// # Safety
///
/// `key` is a C string. The translation stays valid until the locale is
/// replaced or freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn obs_module_text(key: *const c_char) -> *const c_char {
    guard("obs_module_text", key, || {
        let mut out = key;
        // SAFETY: `key` is a C string per the contract of this function, the
        // lookup is null or alive, and a null one finds nothing.
        unsafe { text_lookup_getstr(LOOKUP.load(Ordering::Acquire), key, &mut out) };
        out
    })
}

/// Whether the locale translates `key`, the translation then in `out`.
///
/// # Safety
///
/// `key` is a C string and `out` points to writable storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn obs_module_get_string(
    key: *const c_char,
    out: *mut *const c_char,
) -> bool {
    guard("obs_module_get_string", false, || {
        // SAFETY: as in `obs_module_text`, and `out` is writable per the
        // contract of this function.
        unsafe { text_lookup_getstr(LOOKUP.load(Ordering::Acquire), key, out) }
    })
}

/// Translation of `key`, copied, so it outlives a later change of locale.
pub fn text(key: &str) -> String {
    let key = c_string(key);
    // SAFETY: `key` is a C string alive across the call, and the
    // translation is read at once, before any later locale change.
    unsafe { CStr::from_ptr(obs_module_text(key.as_ptr())) }
        .to_string_lossy()
        .into_owned()
}

fn replace(lookup: *mut lookup_t) {
    let previous = LOOKUP.swap(lookup, Ordering::AcqRel);
    // SAFETY: the previous lookup came from obs_module_load_locale or is
    // null, which OBS accepts, and the swap made this call its only owner.
    unsafe { text_lookup_destroy(previous) };
}
