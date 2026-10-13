//! Colors as OBS blends them (B-31): decoded from sRGB to linear light and
//! premultiplied there. The formulas are those of OBS's `graphics/srgb.h`,
//! so that a translucent color looks the same in this source as in an OBS
//! color source set to the same value.

use std::array;
use std::sync::LazyLock;

use mpris_core::model::Rgba;

/// An 8-bit channel in `[0, 1]`.
fn unit(channel: u8) -> f32 {
    f32::from(channel) / 255.0
}

/// Decodes an sRGB channel in `[0, 1]` to linear light.
fn srgb_to_linear(u: f32) -> f32 {
    if u <= 0.04045 {
        u / 12.92
    } else {
        ((u + 0.055) / 1.055).powf(2.4)
    }
}

/// Encodes linear light in `[0, 1]` to an sRGB channel in `[0, 1]`.
fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Steps of the encoding table, enough for every 8-bit value to survive
/// a decode and an encode.
const ENCODE_STEPS: usize = 4096;

// Tables, since drawing a frame converts every blended pixel and `powf` would
// cost more than the rest of the drawing.
static DECODE: LazyLock<[f32; 256]> =
    LazyLock::new(|| array::from_fn(|c| srgb_to_linear(c as f32 / 255.0)));
static ENCODE: LazyLock<[u8; ENCODE_STEPS]> = LazyLock::new(|| {
    let last = (ENCODE_STEPS - 1) as f32;
    array::from_fn(|i| (linear_to_srgb(i as f32 / last) * 255.0).round() as u8)
});

/// An sRGB channel, in linear light.
pub(crate) fn decode(channel: u8) -> f32 {
    DECODE[usize::from(channel)]
}

/// Linear light to an sRGB channel, clamped to `[0, 1]` first.
pub(crate) fn encode(linear: f32) -> u8 {
    let last = (ENCODE_STEPS - 1) as f32;
    // A NaN casts to 0.
    ENCODE[nearest(linear.clamp(0.0, 1.0) * last) as usize]
}

/// `value` rounded to the nearest integer, for a value that is not
/// negative. `f32::round` is a call into libm on x86-64, too slow for a
/// function that runs for every pixel drawn.
pub(crate) fn nearest(value: f32) -> f32 {
    (value + 0.5).trunc()
}

/// A color in linear light, premultiplied by its alpha: the form in which
/// colors blend correctly, as OBS blends them in its linear sRGB pipeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Linear {
    pub rgb: [f32; 3],
    pub a: f32,
}

impl Linear {
    /// This color over a `coverage` fraction of a pixel.
    pub fn scaled(self, coverage: f32) -> Linear {
        Linear {
            rgb: self.rgb.map(|c| c * coverage),
            a: self.a * coverage,
        }
    }
}

impl From<Rgba> for Linear {
    fn from(color: Rgba) -> Linear {
        let a = unit(color.a);
        Linear {
            rgb: [color.r, color.g, color.b].map(|c| decode(c) * a),
            a,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-6;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= EPSILON
    }

    #[test]
    fn srgb_decoding_keeps_both_ends() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!(close(srgb_to_linear(1.0), 1.0));
    }

    #[test]
    fn srgb_decoding_is_linear_up_to_the_threshold() {
        for u in [0.001, 0.02, 0.04045] {
            assert!(close(srgb_to_linear(u), u / 12.92), "{u}");
        }
    }

    #[test]
    fn srgb_decoding_follows_the_curve_above_the_threshold() {
        assert!(close(srgb_to_linear(0.5), 0.214_041_14));
        assert!(close(srgb_to_linear(0.0405), 0.003_134_745));
    }

    #[test]
    fn every_channel_value_decodes_within_bounds_and_in_order() {
        let decoded: Vec<f32> = (0..=255u8)
            .map(|c| srgb_to_linear(f32::from(c) / 255.0))
            .collect();
        assert!(decoded.iter().all(|v| (0.0..=1.0 + EPSILON).contains(v)));
        assert!(decoded.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn srgb_encoding_keeps_both_ends() {
        assert_eq!(linear_to_srgb(0.0), 0.0);
        assert!(close(linear_to_srgb(1.0), 1.0));
    }

    #[test]
    fn srgb_encoding_undoes_the_decoding() {
        for c in 0..=255u8 {
            let u = f32::from(c) / 255.0;
            assert!((linear_to_srgb(srgb_to_linear(u)) - u).abs() < 1e-5, "{c}");
        }
    }

    #[test]
    fn every_channel_survives_decode_then_encode() {
        for c in 0..=255u8 {
            assert_eq!(encode(decode(c)), c, "{c}");
        }
    }

    #[test]
    fn encode_clamps_what_lies_outside_the_unit_range() {
        assert_eq!(encode(-1.0), 0);
        assert_eq!(encode(2.0), 255);
        assert_eq!(encode(f32::NAN), 0);
    }

    #[test]
    fn linear_color_is_premultiplied_in_linear_light() {
        let white = Linear::from(Rgba::new(255, 255, 255, 128));
        let a = 128.0 / 255.0;
        assert!(close(white.a, a));
        for c in white.rgb {
            assert!(close(c, a));
        }
        let mid = Linear::from(Rgba::new(128, 0, 255, 255));
        assert!(close(mid.rgb[0], 0.215_860_5));
        assert!(close(mid.rgb[1], 0.0));
        assert!(close(mid.rgb[2], 1.0));
        let clear = Linear::from(Rgba::new(9, 9, 9, 0));
        assert_eq!((clear.rgb, clear.a), ([0.0; 3], 0.0));
    }

    #[test]
    fn linear_color_scales_with_coverage() {
        let c = Linear::from(Rgba::new(255, 255, 255, 255)).scaled(0.25);
        assert!(close(c.a, 0.25) && close(c.rgb[0], 0.25));
    }
}
