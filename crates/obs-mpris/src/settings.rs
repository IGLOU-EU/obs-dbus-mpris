//! The source settings on the OBS side: defaults, properties and reading,
//! all walked from the single schema `mpris_core::settings::FIELDS`, so a
//! setting is declared once.

use std::ffi::{CStr, c_char, c_int};

use libobs_sys::{
    OBS_COMBO_FORMAT_STRING, OBS_COMBO_TYPE_LIST, OBS_TEXT_DEFAULT, obs_data_get_bool,
    obs_data_get_int, obs_data_get_string, obs_data_set_default_bool, obs_data_set_default_int,
    obs_data_set_default_string, obs_data_t, obs_properties_add_bool,
    obs_properties_add_color_alpha, obs_properties_add_int, obs_properties_add_list,
    obs_properties_add_text, obs_properties_create, obs_properties_t, obs_property_list_add_string,
    obs_property_t,
};
use mpris_core::model::{Player, Rgba, Settings};
use mpris_core::selection::player_choices;
use mpris_core::settings::{DefaultValue, FIELDS, Kind, SettingValue};

use crate::c_string;
use crate::locale::text;

/// Writes the default of every setting.
///
/// # Safety
///
/// `data` is a live `obs_data_t`.
pub unsafe fn set_defaults(data: *mut obs_data_t) {
    for field in FIELDS {
        let key = c_string(field.key);
        let name = key.as_ptr();
        // SAFETY: `data` is live per the contract of this function, and every
        // string is a C string alive across the call, which copies it.
        unsafe {
            match field.default {
                DefaultValue::Bool(value) => obs_data_set_default_bool(data, name, value),
                DefaultValue::Int(value) => obs_data_set_default_int(data, name, value.into()),
                DefaultValue::Color(color) => {
                    obs_data_set_default_int(data, name, color.to_obs().into());
                }
                DefaultValue::Text(value) => {
                    let value = c_string(value);
                    obs_data_set_default_string(data, name, value.as_ptr());
                }
                DefaultValue::Translated { locale_key, .. } => {
                    let value = c_string(&text(locale_key));
                    obs_data_set_default_string(data, name, value.as_ptr());
                }
            }
        }
    }
}

/// The settings stored in `data`, each read with the getter of its kind.
///
/// # Safety
///
/// `data` is a live `obs_data_t`.
pub unsafe fn read(data: *mut obs_data_t) -> Settings {
    Settings::from_lookup(|key| {
        let field = FIELDS.iter().find(|field| field.key == key)?;
        let key = c_string(key);
        let name = key.as_ptr();
        // SAFETY: `data` is live per the contract of this function, and the
        // key is a C string alive across each call.
        let value = unsafe {
            match field.kind {
                Kind::Bool => SettingValue::Bool(obs_data_get_bool(data, name)),
                Kind::Int { .. } => SettingValue::Int(obs_data_get_int(data, name)),
                // OBS keeps a color in the low 32 bits of an integer, which is
                // how its own color source reads it back.
                Kind::Color => {
                    SettingValue::Color(Rgba::from_obs(obs_data_get_int(data, name) as u32))
                }
                Kind::Text | Kind::Player => {
                    SettingValue::Text(owned(obs_data_get_string(data, name)))
                }
            }
        };
        Some(value)
    })
}

/// The properties of a source, one per schema field, each labelled by the
/// translation of its key. The player list offers `players` and the
/// `remembered` instance, present or not (B-02, B-03).
pub fn properties(players: &[Player], remembered: Option<&str>) -> *mut obs_properties_t {
    // SAFETY: no precondition, and OBS takes ownership of the result.
    let props = unsafe { obs_properties_create() };
    for field in FIELDS {
        let (key, translation) = (c_string(field.key), c_string(&text(field.key)));
        let (name, label) = (key.as_ptr(), translation.as_ptr());
        // SAFETY: `props` is the live object created above, the strings are
        // C strings alive across each call, which copies them, and the player
        // list is created of string format, as `add_players` needs.
        unsafe {
            match field.kind {
                Kind::Bool => {
                    obs_properties_add_bool(props, name, label);
                }
                Kind::Int { min, max, .. } => {
                    obs_properties_add_int(props, name, label, c_int_of(min), c_int_of(max), 1);
                }
                Kind::Color => {
                    obs_properties_add_color_alpha(props, name, label);
                }
                Kind::Text => {
                    obs_properties_add_text(props, name, label, OBS_TEXT_DEFAULT);
                }
                Kind::Player => {
                    let list = obs_properties_add_list(
                        props,
                        name,
                        label,
                        OBS_COMBO_TYPE_LIST,
                        OBS_COMBO_FORMAT_STRING,
                    );
                    add_players(list, players, remembered);
                }
            }
        }
    }
    props
}

/// Fills the player list. The first entry remembers no player, so that a
/// source without a choice shows it as such rather than a player it does
/// not follow.
///
/// # Safety
///
/// `list` is a live list property of string format.
unsafe fn add_players(list: *mut obs_property_t, players: &[Player], remembered: Option<&str>) {
    let unavailable = text("PlayerUnavailable");
    let none = (text("PlayerNone"), String::new());
    let choices = player_choices(players, remembered)
        .into_iter()
        .map(|choice| {
            let label = if choice.available {
                choice.label
            } else {
                choice.label + &unavailable
            };
            (label, choice.bus_name)
        });
    for (label, value) in std::iter::once(none).chain(choices) {
        let (label, value) = (c_string(&label), c_string(&value));
        // SAFETY: `list` is live per the contract of this function, and the
        // strings are C strings alive across the call, which copies them.
        unsafe { obs_property_list_add_string(list, label.as_ptr(), value.as_ptr()) };
    }
}

/// A schema bound as OBS takes it. The schema bounds are far below
/// `c_int::MAX`, the saturation only keeps a typo from wrapping around.
fn c_int_of(bound: u32) -> c_int {
    c_int::try_from(bound).unwrap_or(c_int::MAX)
}

/// A string OBS returned, copied. OBS answers an empty string for a
/// missing value, null is read the same way.
///
/// # Safety
///
/// `text` is null or a C string alive for the call.
unsafe fn owned(text: *const c_char) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: a non-null `text` is a live C string per the contract of this
    // function.
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}
