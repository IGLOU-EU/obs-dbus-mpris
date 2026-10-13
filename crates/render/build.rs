//! OBS Flatpak runs on the glibc of its runtime (2.42), older than the
//! `acosf` that a newer host links by default (2.43), which glyph
//! rasterization calls. The example wraps it, so that it runs in the
//! sandbox where the fonts differ, and defines the wrapper.
fn main() {
    println!("cargo::rustc-link-arg-examples=-Wl,--wrap=acosf");
}
