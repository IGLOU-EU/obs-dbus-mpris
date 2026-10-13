//! Drawing of the source, from its settings and what it shows to one RGBA
//! image. Nothing here depends on OBS or on D-Bus, so all of it runs under
//! `cargo test`.

pub mod color;
pub mod layout;
