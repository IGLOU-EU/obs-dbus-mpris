//! The `mpris_now_playing` source. For now it draws its background at the
//! size it is set to, and its properties list the players the core sees
//! (feuille-de-route.md, phase 1).

use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};

use libobs_sys::{
    OBS_EFFECT_SOLID, OBS_ICON_TYPE_MEDIA, OBS_SOURCE_CUSTOM_DRAW, OBS_SOURCE_SRGB,
    OBS_SOURCE_TYPE_INPUT, OBS_SOURCE_VIDEO, gs_draw_sprite, gs_effect_get_param_by_name,
    gs_effect_get_technique, gs_effect_set_vec4, gs_effect_t, gs_enable_framebuffer_srgb,
    gs_framebuffer_srgb_enabled, gs_get_linear_srgb, gs_technique_begin, gs_technique_begin_pass,
    gs_technique_end, gs_technique_end_pass, obs_data_t, obs_get_base_effect, obs_properties_t,
    obs_register_source_s, obs_source_info, obs_source_t, vec4, vec4__bindgen_ty_1,
};
use mpris_core::color::solid_fill;
use mpris_core::model::{Rgba, Settings};

use crate::locale::obs_module_text;
use crate::{guard, settings, snapshot};

const ID: &CStr = c"mpris_now_playing";

/// What OBS keeps for each source it creates.
struct Source {
    /// Written by `update` on the UI thread, read on the graphics thread.
    settings: Mutex<Settings>,
}

impl Source {
    fn settings(&self) -> MutexGuard<'_, Settings> {
        // The lock only guards a swap of the whole value, which a panic
        // cannot leave half done.
        self.settings.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub fn register() {
    let info = obs_source_info {
        id: ID.as_ptr(),
        type_: OBS_SOURCE_TYPE_INPUT,
        // Drawn by hand, in sRGB-aware fashion, like OBS's own color source.
        output_flags: OBS_SOURCE_VIDEO | OBS_SOURCE_CUSTOM_DRAW | OBS_SOURCE_SRGB,
        get_name: Some(get_name),
        create: Some(create),
        destroy: Some(destroy),
        get_width: Some(get_width),
        get_height: Some(get_height),
        get_defaults: Some(get_defaults),
        get_properties: Some(get_properties),
        update: Some(update),
        video_render: Some(video_render),
        icon_type: OBS_ICON_TYPE_MEDIA,
        ..Default::default()
    };
    // SAFETY: `info` is a whole `obs_source_info` of the size given, which
    // OBS copies, and its id is a static string.
    unsafe { obs_register_source_s(&info, size_of::<obs_source_info>()) };
}

/// The source behind `data`, `None` for the null OBS passes when it asks
/// about the source type rather than one source.
///
/// # Safety
///
/// `data` is null or what `create` returned, not yet given to `destroy`.
unsafe fn source<'a>(data: *mut c_void) -> Option<&'a Source> {
    // SAFETY: per the contract of this function.
    unsafe { data.cast::<Source>().as_ref() }
}

unsafe extern "C" fn get_name(_type_data: *mut c_void) -> *const c_char {
    guard("get_name", ID.as_ptr(), || {
        // SAFETY: the key is a static C string, and the translation lives as
        // long as the locale, which OBS frees after every source is gone.
        unsafe { obs_module_text(c"SourceName".as_ptr()) }
    })
}

unsafe extern "C" fn create(data: *mut obs_data_t, _source: *mut obs_source_t) -> *mut c_void {
    guard("create", ptr::null_mut(), || {
        // SAFETY: OBS passes the live settings of the new source.
        let settings = unsafe { settings::read(data) };
        let source = Box::new(Source {
            settings: Mutex::new(settings),
        });
        Box::into_raw(source).cast()
    })
}

unsafe extern "C" fn destroy(data: *mut c_void) {
    guard("destroy", (), || {
        if !data.is_null() {
            // SAFETY: `data` comes from `Box::into_raw` in `create`, and OBS
            // destroys a source once.
            drop(unsafe { Box::from_raw(data.cast::<Source>()) });
        }
    });
}

unsafe extern "C" fn update(data: *mut c_void, obs_settings: *mut obs_data_t) {
    guard("update", (), || {
        // Read before taking the lock, so the graphics thread never waits
        // on OBS reading its settings.
        // SAFETY: OBS passes what `create` returned and the live settings of
        // that source.
        let (source, settings) = unsafe { (source(data), settings::read(obs_settings)) };
        if let Some(source) = source {
            *source.settings() = settings;
        }
    });
}

unsafe extern "C" fn get_defaults(data: *mut obs_data_t) {
    guard("get_defaults", (), || {
        // SAFETY: OBS passes the live settings to fill.
        unsafe { settings::set_defaults(data) }
    });
}

unsafe extern "C" fn get_properties(data: *mut c_void) -> *mut obs_properties_t {
    guard("get_properties", ptr::null_mut(), || {
        // SAFETY: OBS passes what `create` returned, or null.
        let remembered = unsafe { source(data) }.and_then(|s| s.settings().player.clone());
        settings::properties(&snapshot().players, remembered.as_deref())
    })
}

unsafe extern "C" fn get_width(data: *mut c_void) -> u32 {
    guard("get_width", 0, || {
        // SAFETY: OBS passes what `create` returned.
        unsafe { source(data) }.map_or(0, |s| s.settings().width)
    })
}

unsafe extern "C" fn get_height(data: *mut c_void) -> u32 {
    guard("get_height", 0, || {
        // SAFETY: OBS passes what `create` returned.
        unsafe { source(data) }.map_or(0, |s| s.settings().height)
    })
}

unsafe extern "C" fn video_render(data: *mut c_void, _effect: *mut gs_effect_t) {
    guard("video_render", (), || {
        // SAFETY: OBS passes what `create` returned.
        let Some(source) = (unsafe { source(data) }) else {
            return;
        };
        let (color, width, height) = {
            let settings = source.settings();
            (settings.color_background, settings.width, settings.height)
        };
        // SAFETY: OBS calls `video_render` on its graphics thread, inside
        // the graphics context.
        unsafe { draw_rectangle(color, width, height) };
    });
}

/// Draws a `width` by `height` rectangle of `color` at the origin of the
/// source, as OBS's color source does.
///
/// # Safety
///
/// Runs inside the OBS graphics context.
unsafe fn draw_rectangle(color: Rgba, width: u32, height: u32) {
    // SAFETY: inside the graphics context per the contract of this function,
    // the solid effect is one OBS always provides, and the names are static
    // C strings.
    unsafe {
        let fill = solid_fill(color, gs_get_linear_srgb());
        let solid = obs_get_base_effect(OBS_EFFECT_SOLID);
        let param = gs_effect_get_param_by_name(solid, c"color".as_ptr());
        let technique = gs_effect_get_technique(solid, c"Solid".as_ptr());
        let value = vec4 {
            __bindgen_anon_1: vec4__bindgen_ty_1 { ptr: fill.rgba },
        };

        let previous = gs_framebuffer_srgb_enabled();
        gs_enable_framebuffer_srgb(fill.linear);
        gs_effect_set_vec4(param, &value);
        gs_technique_begin(technique);
        gs_technique_begin_pass(technique, 0);
        gs_draw_sprite(ptr::null_mut(), 0, width, height);
        gs_technique_end_pass(technique);
        gs_technique_end(technique);
        gs_enable_framebuffer_srgb(previous);
    }
}
