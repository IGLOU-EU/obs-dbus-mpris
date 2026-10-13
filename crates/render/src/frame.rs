//! The image of the source and the few ways it is painted: rounded shapes,
//! glyph masks and copied images, each clipped to the frame.

use crate::color::{Linear, decode, encode};
use crate::layout::Rect;

/// RGBA8, row-major, premultiplied in linear light then sRGB-encoded: the
/// form of OBS's image source with linear alpha (`GS_IMAGE_ALPHA_PREMULTIPLY_SRGB`),
/// sampled as sRGB and blended `ONE, INVSRCALPHA`. A translucent color then
/// looks the same here as in an OBS color source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    width: u32,
    height: u32,
    /// `width * height * 4` bytes, which every method relies on.
    pixels: Vec<u8>,
}

impl Frame {
    pub fn transparent(width: u32, height: u32) -> Frame {
        Frame {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Row-major, 4 bytes per pixel, no padding between rows.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The pixel at `(x, y)`, `None` outside the frame.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = self.index(x, y);
        self.pixels[i..i + 4].try_into().ok()
    }

    /// Paints `shape` with corners of `radius`, capped at half its shorter
    /// side. `paint` gives the color of each column, so that one shape can
    /// hold two colors, as the progress bar does.
    pub(crate) fn fill(&mut self, shape: Rect, radius: u32, paint: impl Fn(u32) -> Linear) {
        let Some(area) = intersection(shape, self.bounds()) else {
            return;
        };
        let radius = f64::from(radius.min(shape.w / 2).min(shape.h / 2));
        for y in area.y..area.bottom() {
            let dy = inset(y, shape.y, shape.bottom());
            for x in area.x..area.right() {
                let dx = inset(x, shape.x, shape.right());
                let i = self.index(x, y);
                self.blend(i, paint(x).scaled(coverage(dx, dy, radius)));
            }
        }
    }
    fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    fn index(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }

    /// Lays `src`, already scaled by its coverage, over the pixel at `i`.
    fn blend(&mut self, i: usize, src: Linear) {
        if src.a <= 0.0 {
            return;
        }
        let pixel = &mut self.pixels[i..i + 4];
        if src.a >= 1.0 {
            pixel[..3].copy_from_slice(&src.rgb.map(encode));
            pixel[3] = u8::MAX;
            return;
        }
        let keep = 1.0 - src.a;
        for (channel, light) in pixel[..3].iter_mut().zip(src.rgb) {
            *channel = encode(light + decode(*channel) * keep);
        }
        let a = src.a + f32::from(pixel[3]) / 255.0 * keep;
        pixel[3] = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
}

/// The part of `a` inside `b`, `None` when they do not overlap.
fn intersection(a: Rect, b: Rect) -> Option<Rect> {
    let (x, y) = (a.x.max(b.x), a.y.max(b.y));
    let (right, bottom) = (a.right().min(b.right()), a.bottom().min(b.bottom()));
    (x < right && y < bottom).then(|| Rect::new(x, y, right - x, bottom - y))
}

/// Distance from the center of pixel `p` to the nearer of the edges
/// `start` and `end`, positive inside.
fn inset(p: u32, start: u32, end: u32) -> f64 {
    let center = f64::from(p) + 0.5;
    (center - f64::from(start)).min(f64::from(end) - center)
}

/// Fraction of a pixel covered by a rounded shape, from the pixel center's
/// distances to the nearest vertical and horizontal edges. Within a corner
/// square the edge is the arc, elsewhere the straight side. The half pixel
/// turns a distance into a coverage that fades over one pixel.
fn coverage(dx: f64, dy: f64, radius: f64) -> f32 {
    let distance = if dx < radius && dy < radius {
        let (ox, oy) = (radius - dx, radius - dy);
        (ox * ox + oy * oy).sqrt() - radius
    } else {
        -dx.min(dy)
    };
    (0.5 - distance).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use mpris_core::model::Rgba;

    use super::*;

    const RED: Rgba = Rgba::new(255, 0, 0, 255);
    const BLUE: Rgba = Rgba::new(0, 0, 255, 255);
    const WHITE: Rgba = Rgba::new(255, 255, 255, 255);

    fn solid(color: Rgba) -> impl Fn(u32) -> Linear {
        move |_| Linear::from(color)
    }

    fn at(frame: &Frame, x: u32, y: u32) -> [u8; 4] {
        frame
            .pixel(x, y)
            .unwrap_or_else(|| panic!("({x}, {y}) outside {}x{}", frame.width(), frame.height()))
    }

    fn alpha(frame: &Frame, x: u32, y: u32) -> u8 {
        at(frame, x, y)[3]
    }

    fn filled(width: u32, height: u32, shape: Rect, radius: u32, color: Rgba) -> Frame {
        let mut frame = Frame::transparent(width, height);
        frame.fill(shape, radius, solid(color));
        frame
    }

    #[test]
    fn a_new_frame_is_transparent_and_of_its_size() {
        let frame = Frame::transparent(3, 2);
        assert_eq!((frame.width(), frame.height()), (3, 2));
        assert_eq!(frame.pixels(), vec![0; 3 * 2 * 4]);
    }

    #[test]
    fn an_empty_frame_has_no_pixel() {
        let frame = Frame::transparent(0, 0);
        assert!(frame.pixels().is_empty());
        assert_eq!(frame.pixel(0, 0), None);
    }

    #[test]
    fn pixels_outside_the_frame_do_not_exist() {
        let frame = Frame::transparent(3, 2);
        assert_eq!(frame.pixel(2, 1), Some([0; 4]));
        assert_eq!(frame.pixel(3, 0), None);
        assert_eq!(frame.pixel(0, 2), None);
    }

    #[test]
    fn square_shape_fills_exactly_its_pixels() {
        let frame = filled(6, 5, Rect::new(1, 2, 3, 2), 0, RED);
        for y in 0..5 {
            for x in 0..6 {
                let inside = (1..4).contains(&x) && (2..4).contains(&y);
                let expected = if inside { [255, 0, 0, 255] } else { [0; 4] };
                assert_eq!(at(&frame, x, y), expected, "({x}, {y})");
            }
        }
    }

    #[test]
    fn translucent_color_is_premultiplied_in_linear_light() {
        let white = filled(
            1,
            1,
            Rect::new(0, 0, 1, 1),
            0,
            Rgba::new(255, 255, 255, 128),
        );
        // Half the light of white, sRGB-encoded.
        assert_eq!(at(&white, 0, 0), [188, 188, 188, 128]);
        let black = filled(1, 1, Rect::new(0, 0, 1, 1), 0, Rgba::new(0, 0, 0, 153));
        assert_eq!(at(&black, 0, 0), [0, 0, 0, 153]);
    }

    #[test]
    fn transparent_color_leaves_the_pixels_alone() {
        let mut frame = filled(2, 2, Rect::new(0, 0, 2, 2), 0, RED);
        frame.fill(Rect::new(0, 0, 2, 2), 0, solid(Rgba::new(0, 0, 255, 0)));
        assert_eq!(at(&frame, 1, 1), [255, 0, 0, 255]);
    }

    #[test]
    fn shapes_blend_over_in_linear_light() {
        let mut frame = filled(1, 1, Rect::new(0, 0, 1, 1), 0, RED);
        frame.fill(
            Rect::new(0, 0, 1, 1),
            0,
            solid(Rgba::new(255, 255, 255, 128)),
        );
        assert_eq!(at(&frame, 0, 0), [255, 188, 188, 255]);
    }

    #[test]
    fn rounded_corners_are_cut_and_smoothed() {
        let frame = filled(20, 20, Rect::new(0, 0, 20, 20), 6, WHITE);
        assert_eq!(alpha(&frame, 0, 0), 0, "corner");
        assert_eq!(alpha(&frame, 10, 10), 255, "middle");
        assert_eq!(alpha(&frame, 0, 10), 255, "straight edge");
        assert_eq!(alpha(&frame, 10, 0), 255, "straight edge");
        let smoothed = (0..6).any(|i| (1..255).contains(&alpha(&frame, i, i)));
        assert!(smoothed, "a pixel of the arc is partly covered");
        for y in 0..20 {
            for x in 0..20 {
                let p = at(&frame, x, y);
                assert_eq!(p, at(&frame, 19 - x, y), "({x}, {y})");
                assert_eq!(p, at(&frame, x, 19 - y), "({x}, {y})");
                assert_eq!(p, at(&frame, y, x), "({x}, {y})");
            }
        }
    }

    #[test]
    fn zero_radius_keeps_square_corners() {
        let frame = filled(4, 4, Rect::new(0, 0, 4, 4), 0, WHITE);
        assert_eq!(alpha(&frame, 0, 0), 255);
        assert_eq!(alpha(&frame, 3, 3), 255);
    }

    #[test]
    fn radius_beyond_half_the_shorter_side_gives_a_pill() {
        let shape = Rect::new(0, 0, 40, 10);
        let pill = filled(40, 10, shape, 5, WHITE);
        assert_eq!(filled(40, 10, shape, 100, WHITE), pill);
        assert_eq!(filled(40, 10, shape, u32::MAX, WHITE), pill);
        assert_eq!(alpha(&pill, 20, 0), 255);
        assert_eq!(alpha(&pill, 0, 0), 0);
    }

    #[test]
    fn paint_picks_a_color_per_column() {
        let mut frame = Frame::transparent(4, 1);
        frame.fill(Rect::new(0, 0, 4, 1), 0, |x| {
            Linear::from(if x < 2 { RED } else { BLUE })
        });
        assert_eq!(at(&frame, 1, 0), [255, 0, 0, 255]);
        assert_eq!(at(&frame, 2, 0), [0, 0, 255, 255]);
    }

    #[test]
    fn shapes_are_clipped_to_the_frame() {
        let frame = filled(4, 4, Rect::new(2, 2, 100, 100), 10, WHITE);
        assert_eq!(alpha(&frame, 3, 3), 0, "the rounded corner lies far away");
        let frame = filled(4, 4, Rect::new(2, 2, 100, 100), 0, WHITE);
        assert_eq!(alpha(&frame, 3, 3), 255);
        assert_eq!(alpha(&frame, 1, 1), 0);
        let mut frame = Frame::transparent(4, 4);
        frame.fill(Rect::new(u32::MAX - 1, 0, 10, 10), 0, solid(WHITE));
        frame.fill(Rect::new(0, 0, u32::MAX, u32::MAX), 3, solid(WHITE));
        assert_eq!(alpha(&frame, 2, 2), 255);
    }

    #[test]
    fn empty_shape_paints_nothing() {
        let frame = filled(4, 4, Rect::new(1, 1, 0, 3), 2, WHITE);
        assert_eq!(frame, Frame::transparent(4, 4));
    }
}
