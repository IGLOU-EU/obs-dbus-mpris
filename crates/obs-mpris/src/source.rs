//! The `mpris_now_playing` source: the track its player plays, drawn as one
//! frame by the render worker, and shown as a texture the way OBS's own
//! image source shows its image. A text too long for its line comes as a
//! strip of its own, slid under the line at each frame (B-36).

use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use libobs_sys::{
    GS_BLEND_INVSRCALPHA, GS_BLEND_ONE, GS_RGBA, OBS_ICON_TYPE_MEDIA, OBS_SOURCE_SRGB,
    OBS_SOURCE_TYPE_INPUT, OBS_SOURCE_VIDEO, gs_blend_function, gs_blend_state_pop,
    gs_blend_state_push, gs_draw_sprite, gs_draw_sprite_subregion, gs_effect_get_param_by_name,
    gs_effect_set_texture_srgb, gs_effect_t, gs_enable_framebuffer_srgb,
    gs_framebuffer_srgb_enabled, gs_matrix_pop, gs_matrix_push, gs_matrix_translate3f,
    gs_texture_create, gs_texture_destroy, gs_texture_t, obs_data_t, obs_enter_graphics,
    obs_leave_graphics, obs_properties_t, obs_register_source_s, obs_source_info, obs_source_t,
};
use mpris_core::content::content;
use mpris_core::dbus::ArtWish;
use mpris_core::model::Settings;
use mpris_core::online::wish;
use mpris_core::selection::{choice_changed, select};
use render::draw::{Picture, RedrawKey, redraw_key};
use render::frame::Frame;
use render::layout::layout;
use render::scroll::Scroll;
use render::strip::Strip;
use render::worker::Painter;

use crate::locale::obs_module_text;
use crate::{art_wish, guard, painter, settings, snapshot};

const ID: &CStr = c"mpris_now_playing";

/// What OBS keeps for each source it creates.
struct Source {
    /// Shared by the threads OBS calls a source from: `create` and
    /// `get_properties` run on the UI thread, while OBS defers `update` of a
    /// video source to the graphics thread, where `video_tick` and
    /// `video_render` run.
    settings: Mutex<Settings>,
    /// `None` when the render worker could not start, and then the source
    /// stays empty.
    painter: Option<Painter>,
    /// Touched by the graphics thread, and by `destroy` once OBS no longer
    /// draws the source.
    view: Mutex<View>,
}

/// What the source last asked the worker for, and what it shows.
#[derive(Default)]
struct View {
    /// Bus name of the player the source keeps to (B-08), `None` until one
    /// shows. Held in memory only, so it starts over with OBS, and when the
    /// choice of player changes.
    followed: Option<String>,
    /// Tells the core which online cover the source wants, if any (B-18).
    /// `None` without a core. Dropped with the source, which withdraws it.
    art_wish: Option<ArtWish>,
    /// `None` until the first request.
    asked: Option<RedrawKey>,
    texture: Option<Texture>,
    /// The scrolling texts, laid over the frame.
    strips: Vec<StripTexture>,
    cycle: Cycle,
}

/// A strip and the texture that holds it.
struct StripTexture {
    /// Kept to tell a new strip from the one shown, and for its line and
    /// period.
    strip: Arc<Strip>,
    texture: Texture,
}

/// The cycle the strips scroll in, which starts again with its pause when
/// the strips or their motion change (B-36).
struct Cycle {
    started: Instant,
    speed: u32,
    pause: Duration,
}

impl Default for Cycle {
    fn default() -> Cycle {
        Cycle {
            started: Instant::now(),
            speed: 0,
            pause: Duration::ZERO,
        }
    }
}

impl Cycle {
    /// Starts again from the pause when the speed or the pause changes, so
    /// that a text never takes up a new motion halfway through its line.
    fn follow(&mut self, speed: u32, pause: Duration) {
        if (self.speed, self.pause) != (speed, pause) {
            *self = Cycle {
                started: Instant::now(),
                speed,
                pause,
            };
        }
    }

    fn restart(&mut self) {
        self.started = Instant::now();
    }
}

/// A texture of the graphics subsystem, which only the graphics thread, or
/// a thread inside `obs_enter_graphics`, may create or destroy.
struct Texture {
    raw: *mut gs_texture_t,
    width: u32,
    height: u32,
}

impl Source {
    fn settings(&self) -> MutexGuard<'_, Settings> {
        // Both locks only guard a swap of a whole value, which a panic cannot
        // leave half done.
        self.settings.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn view(&self) -> MutexGuard<'_, View> {
        self.view.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub fn register() {
    let info = obs_source_info {
        id: ID.as_ptr(),
        type_: OBS_SOURCE_TYPE_INPUT,
        // Without custom drawing, OBS hands `video_render` the default
        // effect, as it does for its own image source.
        output_flags: OBS_SOURCE_VIDEO | OBS_SOURCE_SRGB,
        get_name: Some(get_name),
        create: Some(create),
        destroy: Some(destroy),
        get_width: Some(get_width),
        get_height: Some(get_height),
        get_defaults: Some(get_defaults),
        get_properties: Some(get_properties),
        update: Some(update),
        video_tick: Some(video_tick),
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
        let view = View {
            art_wish: art_wish(),
            ..View::default()
        };
        let source = Box::new(Source {
            settings: Mutex::new(settings),
            painter: painter(),
            view: Mutex::new(view),
        });
        Box::into_raw(source).cast()
    })
}

unsafe extern "C" fn destroy(data: *mut c_void) {
    guard("destroy", (), || {
        if data.is_null() {
            return;
        }
        // SAFETY: `data` comes from `Box::into_raw` in `create`, and OBS
        // destroys a source once.
        let source = unsafe { Box::from_raw(data.cast::<Source>()) };
        let view = source
            .view
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        let strips = view.strips.into_iter().map(|strip| strip.texture);
        let textures: Vec<Texture> = view.texture.into_iter().chain(strips).collect();
        if textures.is_empty() {
            return;
        }
        // SAFETY: OBS may destroy a source outside the graphics thread,
        // hence the graphics context entered around the textures' end.
        unsafe {
            obs_enter_graphics();
            for texture in textures {
                release(texture);
            }
            obs_leave_graphics();
        }
    });
}

unsafe extern "C" fn update(data: *mut c_void, obs_settings: *mut obs_data_t) {
    guard("update", (), || {
        // Read before taking the lock, so it is held only for the swap.
        // SAFETY: OBS passes what `create` returned and the live settings of
        // that source.
        let (source, settings) = unsafe { (source(data), settings::read(obs_settings)) };
        let Some(source) = source else {
            return;
        };
        let rechosen = {
            let mut current = source.settings();
            let rechosen = choice_changed(&current, &settings);
            *current = settings;
            rechosen
        };
        if rechosen {
            source.view().followed = None;
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
        unsafe { source(data) }.map_or(0, |s| layout(&s.settings()).width)
    })
}

unsafe extern "C" fn get_height(data: *mut c_void) -> u32 {
    guard("get_height", 0, || {
        // SAFETY: OBS passes what `create` returned.
        unsafe { source(data) }.map_or(0, |s| layout(&s.settings()).height)
    })
}

/// Asks the worker for a new frame when what the source shows changed
/// (B-43): a new second, a pixel more of progress, a new track or setting.
/// Only the motion of the scrolling texts needs no new frame.
unsafe extern "C" fn video_tick(data: *mut c_void, _seconds: f32) {
    guard("video_tick", (), || {
        // SAFETY: OBS passes what `create` returned.
        let Some(source) = (unsafe { source(data) }) else {
            return;
        };
        let Some(painter) = &source.painter else {
            return;
        };
        let settings = source.settings().clone();
        let snapshot = snapshot();
        let mut view = source.view();
        let selection = select(&snapshot.players, &settings, view.followed.as_deref());
        view.followed = selection.followed;
        if let Some(art_wish) = &mut view.art_wish {
            art_wish.set(wish(selection.player, &settings));
        }
        let shown = content(selection.player, &settings, Instant::now());
        let layout = layout(&settings);
        let key = redraw_key(&layout, &shown, &settings);
        view.cycle
            .follow(settings.scroll_speed, settings.scroll_pause);
        if view.asked.as_ref() != Some(&key) {
            painter.request(layout, shown, settings);
            view.asked = Some(key);
        }
    });
}

unsafe extern "C" fn video_render(data: *mut c_void, effect: *mut gs_effect_t) {
    guard("video_render", (), || {
        // SAFETY: OBS passes what `create` returned.
        let Some(source) = (unsafe { source(data) }) else {
            return;
        };
        let mut view = source.view();
        if let Some(picture) = source.painter.as_ref().and_then(Painter::take) {
            // SAFETY: OBS calls `video_render` inside the graphics context.
            unsafe { show(&mut view, picture) };
        }
        if !effect.is_null() {
            // SAFETY: inside the graphics context, with the effect OBS passed.
            unsafe { draw_view(&view, effect) };
        }
    });
}

/// Shows `picture` in place of the previous one. The strips go to new
/// textures only when they changed, which starts their cycle again (B-36,
/// B-43).
///
/// # Safety
///
/// Runs inside the OBS graphics context.
unsafe fn show(view: &mut View, picture: Picture) {
    // SAFETY: inside the graphics context per the contract of this
    // function, and the textures replaced are no longer referenced.
    unsafe {
        if let Some(old) = view.texture.take() {
            release(old);
        }
        view.texture = upload(&picture.frame);
        if same_strips(&view.strips, &picture.strips) {
            return;
        }
        for shown in view.strips.drain(..) {
            release(shown.texture);
        }
        for strip in picture.strips {
            if let Some(texture) = upload(&strip.image) {
                view.strips.push(StripTexture { strip, texture });
            }
        }
    }
    view.cycle.restart();
}

/// Whether `shown` holds exactly `strips`, by address: the worker hands the
/// same strip back for as long as what it shows does not change.
fn same_strips(shown: &[StripTexture], strips: &[Arc<Strip>]) -> bool {
    shown.len() == strips.len()
        && shown
            .iter()
            .zip(strips)
            .all(|(shown, strip)| Arc::ptr_eq(&shown.strip, strip))
}

/// `frame` in a new texture, `None` for an empty frame or when OBS cannot
/// create one.
///
/// # Safety
///
/// Runs inside the OBS graphics context.
unsafe fn upload(frame: &Frame) -> Option<Texture> {
    let (width, height) = (frame.width(), frame.height());
    if width == 0 || height == 0 {
        return None;
    }
    let mut planes = [frame.pixels().as_ptr()];
    // SAFETY: inside the graphics context, and the single plane holds the
    // `width * height` RGBA pixels the format asks for, which OBS copies.
    let raw = unsafe { gs_texture_create(width, height, GS_RGBA, 1, planes.as_mut_ptr(), 0) };
    (!raw.is_null()).then_some(Texture { raw, width, height })
}

/// # Safety
///
/// Runs inside the OBS graphics context, and nothing uses `texture` after.
unsafe fn release(texture: Texture) {
    // SAFETY: per the contract of this function.
    unsafe { gs_texture_destroy(texture.raw) };
}

/// Draws the frame, premultiplied and sRGB-encoded, as OBS's image source
/// draws its image, then over it the window of each strip that the time
/// elapsed in the cycle gives. Only arithmetic and draw calls run here.
///
/// # Safety
///
/// Runs inside the OBS graphics context, `effect` being the one OBS passed
/// to `video_render`.
unsafe fn draw_view(view: &View, effect: *mut gs_effect_t) {
    let Some(frame) = &view.texture else {
        return;
    };
    let cycle = &view.cycle;
    let periods = view.strips.iter().map(|shown| shown.strip.period);
    let scroll = Scroll::new(periods, cycle.speed, cycle.pause);
    let elapsed = cycle.started.elapsed();
    // SAFETY: inside the graphics context per the contract of this function,
    // with live textures and effect, and a static parameter name. Each
    // window lies within its strip, `period + line.w` wide for an offset of
    // at most `period`. The blend state, the framebuffer conversion and the
    // matrix are put back as found.
    unsafe {
        let previous = gs_framebuffer_srgb_enabled();
        gs_enable_framebuffer_srgb(true);
        gs_blend_state_push();
        gs_blend_function(GS_BLEND_ONE, GS_BLEND_INVSRCALPHA);
        let image = gs_effect_get_param_by_name(effect, c"image".as_ptr());
        gs_effect_set_texture_srgb(image, frame.raw);
        gs_draw_sprite(frame.raw, 0, frame.width, frame.height);
        for shown in &view.strips {
            let (line, period) = (shown.strip.line, shown.strip.period);
            let offset = scroll.offset(elapsed, period);
            gs_effect_set_texture_srgb(image, shown.texture.raw);
            gs_matrix_push();
            gs_matrix_translate3f(line.x as f32, line.y as f32, 0.0);
            gs_draw_sprite_subregion(shown.texture.raw, 0, offset, 0, line.w, line.h);
            gs_matrix_pop();
        }
        gs_blend_state_pop();
        gs_enable_framebuffer_srgb(previous);
    }
}
