//! Colors as OBS draws them (B-31). The formulas are those of OBS's own
//! color source and of `graphics/srgb.h`, so a color looks the same in this
//! source as in an OBS color source set to the same value.

use mpris_core::model::Rgba;

/// A solid color ready for OBS's solid effect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidFill {
    /// Whether to draw with the framebuffer's sRGB conversion on.
    pub linear: bool,
    /// Red, green, blue and alpha in `[0, 1]`, in linear light when
    /// `linear`. Alpha is never converted, it is a coverage, not a light.
    pub rgba: [f32; 4],
}

/// How to draw `color`, given whether OBS renders in linear sRGB
/// (`gs_get_linear_srgb`). A translucent color always takes the linear
/// path, the only one that blends alpha correctly.
pub fn solid_fill(color: Rgba, linear_srgb: bool) -> SolidFill {
    let linear = linear_srgb || color.a < u8::MAX;
    let [r, g, b, a] = [color.r, color.g, color.b, color.a].map(unit);
    let rgba = if linear {
        [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), a]
    } else {
        [r, g, b, a]
    };
    SolidFill { linear, rgba }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-6;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= EPSILON
    }

    fn assert_fill(actual: SolidFill, linear: bool, rgba: [f32; 4]) {
        assert_eq!(actual.linear, linear, "{actual:?}");
        for (a, e) in actual.rgba.iter().zip(rgba) {
            assert!(close(*a, e), "{actual:?} expected {rgba:?}");
        }
    }

    #[test]
    fn opaque_color_outside_linear_mode_is_drawn_as_stored() {
        assert_fill(
            solid_fill(Rgba::new(255, 51, 0, 255), false),
            false,
            [1.0, 0.2, 0.0, 1.0],
        );
    }

    #[test]
    fn linear_mode_decodes_the_color_channels() {
        assert_fill(
            solid_fill(Rgba::new(255, 128, 0, 255), true),
            true,
            [1.0, 0.215_860_5, 0.0, 1.0],
        );
    }

    #[test]
    fn translucent_color_takes_the_linear_path_in_any_mode() {
        for linear_srgb in [false, true] {
            assert_fill(
                solid_fill(Rgba::new(128, 0, 255, 153), linear_srgb),
                true,
                [0.215_860_5, 0.0, 1.0, 0.6],
            );
        }
    }

    #[test]
    fn alpha_is_never_decoded() {
        for a in [0, 1, 64, 128, 254] {
            let fill = solid_fill(Rgba::new(0, 0, 0, a), false);
            assert!(close(fill.rgba[3], f32::from(a) / 255.0), "{a}");
        }
    }

    #[test]
    fn fully_transparent_color_stays_transparent() {
        assert_fill(
            solid_fill(Rgba::new(255, 255, 255, 0), false),
            true,
            [1.0, 1.0, 1.0, 0.0],
        );
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
}
