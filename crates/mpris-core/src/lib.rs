//! Core of the OBS MPRIS source: player state, the rules over it, and the
//! D-Bus adapter that feeds it. Nothing here depends on OBS, so all of it
//! runs under `cargo test`.
//!
//! The data lives in [`model`]. Every other module is a set of
//! transformations over that data, except [`store`] (the only writer of
//! player state) and [`dbus`] (the only module that talks to the bus).

pub mod art;
pub mod content;
pub mod dbus;
pub mod metadata;
pub mod model;
pub mod online;
pub mod player;
pub mod position;
pub mod selection;
pub mod settings;
pub mod store;
pub mod time;
