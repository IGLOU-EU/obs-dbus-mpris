//! OBS Flatpak runs the library on the glibc of its runtime (2.42), older
//! than the `acosf` that a newer host links by default (2.43), which glyph
//! rasterization calls. The library wraps it and defines the wrapper, so
//! that the install script's glibc check passes and OBS Flatpak loads it.
fn main() {
    println!("cargo::rustc-link-arg-cdylib=-Wl,--wrap=acosf");
}
