//! The thumbnail as drawn (B-10, B-37): cropped to its central square,
//! since VLC gives 16:9 covers, scaled to the slot, corners rounded, and
//! kept from one frame to the next, so that a new second on the clock does
//! not scale the cover again (B-43).

use std::sync::Arc;

use mpris_core::model::{ArtImage, Rgba};

use crate::color::Linear;
use crate::frame::Frame;
use crate::layout::Rect;

/// The last thumbnail prepared, for the image, slot side and corner radius
/// it was prepared for.
#[derive(Debug, Default)]
pub struct ArtCache {
    last: Option<Prepared>,
}

#[derive(Debug)]
struct Prepared {
    /// Compared by address: the core shares one decoded image for as long
    /// as the track keeps its cover.
    image: Arc<ArtImage>,
    side: u32,
    radius: u32,
    frame: Frame,
}

impl ArtCache {
    pub fn new() -> ArtCache {
        ArtCache::default()
    }

    /// `image` as a `side` pixel square with corners of `radius`, prepared
    /// again only when one of them changes.
    pub(crate) fn get(&mut self, image: &Arc<ArtImage>, side: u32, radius: u32) -> &Frame {
        let prepared = match self.last.take() {
            Some(last)
                if Arc::ptr_eq(&last.image, image)
                    && (last.side, last.radius) == (side, radius) =>
            {
                last
            }
            _ => Prepared {
                image: Arc::clone(image),
                side,
                radius,
                frame: thumbnail(image, side, radius),
            },
        };
        &self.last.insert(prepared).frame
    }
}

/// `image` cropped to its central square, scaled to `side` pixels and given
/// corners of `radius`. An image with fewer pixels than its size says draws
/// nothing.
///
/// Each output pixel averages the source pixels it covers, in proportion to
/// the area covered and in linear light, so that a cover several times the
/// slot size neither aliases nor darkens.
pub(crate) fn thumbnail(image: &ArtImage, side: u32, radius: u32) -> Frame {
    let mut frame = Frame::transparent(side, side);
    let crop = image.width.min(image.height);
    let needed = image.width as usize * image.height as usize * 4;
    if crop == 0 || side == 0 || image.rgba.len() < needed {
        return frame;
    }
    let (left, top) = ((image.width - crop) / 2, (image.height - crop) / 2);
    let source = |x: usize, y: usize| {
        let i = ((top as usize + y) * image.width as usize + left as usize + x) * 4;
        let [r, g, b, a] = [0, 1, 2, 3].map(|c| image.rgba[i + c]);
        Linear::from(Rgba::new(r, g, b, a))
    };
    let spans = spans(crop, side);
    let side = side as usize;
    // One axis at a time, so that each output pixel reads a few values
    // rather than a whole block of the source.
    let narrow: Vec<Linear> = (0..crop as usize)
        .flat_map(|y| {
            spans
                .iter()
                .map(move |span| average(span, |x| source(x, y)))
        })
        .collect();
    let scaled: Vec<Linear> = spans
        .iter()
        .flat_map(|span| (0..side).map(|x| average(span, |y| narrow[y * side + x])))
        .collect();
    let all = Rect::new(0, 0, frame.width(), frame.height());
    frame.fill(all, radius, |x, y| scaled[y as usize * side + x as usize]);
    frame
}

/// For each of `to` output pixels along an axis, the `from` source pixels
/// it covers, each with the fraction of the output pixel it fills.
fn spans(from: u32, to: u32) -> Vec<Vec<(usize, f32)>> {
    let step = f64::from(from) / f64::from(to);
    (0..to)
        .map(|i| {
            let (start, end) = (f64::from(i) * step, f64::from(i + 1) * step);
            let first = start.floor() as usize;
            let last = (end.ceil() as usize).min(from as usize);
            (first..last)
                .map(|j| {
                    let covered = end.min(j as f64 + 1.0) - start.max(j as f64);
                    (j, (covered / step) as f32)
                })
                .filter(|(_, weight)| *weight > 0.0)
                .collect()
        })
        .collect()
}

fn average(span: &[(usize, f32)], pixel: impl Fn(usize) -> Linear) -> Linear {
    span.iter()
        .fold(Linear::from(Rgba::new(0, 0, 0, 0)), |sum, &(j, weight)| {
            let p = pixel(j).scaled(weight);
            Linear {
                rgb: [0, 1, 2].map(|c| sum.rgb[c] + p.rgb[c]),
                a: sum.a + p.a,
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    /// A `width` by `height` image whose pixel at `(x, y)` is `color(x, y)`.
    fn image(width: u32, height: u32, color: impl Fn(u32, u32) -> [u8; 4]) -> ArtImage {
        let rgba = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| color(x, y))
            .collect();
        ArtImage {
            width,
            height,
            rgba,
        }
    }

    fn pixels(frame: &Frame) -> Vec<[u8; 4]> {
        frame
            .pixels()
            .chunks(4)
            .map(|p| p.try_into().expect("4 bytes"))
            .collect()
    }

    fn at(frame: &Frame, x: u32, y: u32) -> [u8; 4] {
        frame.pixel(x, y).expect("inside")
    }

    #[test]
    fn thumbnail_is_a_square_of_the_slot_side() {
        let frame = thumbnail(&image(30, 20, |_, _| RED), 12, 0);
        assert_eq!((frame.width(), frame.height()), (12, 12));
        assert!(pixels(&frame).iter().all(|p| *p == RED));
    }

    #[test]
    fn wide_image_keeps_its_central_square() {
        let stripes = image(30, 10, |x, _| match x {
            0..10 => RED,
            10..20 => GREEN,
            _ => BLUE,
        });
        let frame = thumbnail(&stripes, 10, 0);
        assert!(
            pixels(&frame).iter().all(|p| *p == GREEN),
            "{:?}",
            pixels(&frame)
        );
    }

    #[test]
    fn tall_image_keeps_its_central_square() {
        let stripes = image(10, 30, |_, y| match y {
            0..10 => RED,
            10..20 => GREEN,
            _ => BLUE,
        });
        let frame = thumbnail(&stripes, 5, 0);
        assert!(pixels(&frame).iter().all(|p| *p == GREEN));
    }

    #[test]
    fn odd_margins_crop_from_the_center() {
        // 5 wide, 2 tall: the central square is columns 1 and 2.
        let marked = image(5, 2, |x, _| if x == 1 || x == 2 { GREEN } else { RED });
        let frame = thumbnail(&marked, 2, 0);
        assert!(pixels(&frame).iter().all(|p| *p == GREEN));
    }

    #[test]
    fn downscaling_averages_in_linear_light() {
        let checker = image(2, 2, |x, y| {
            if (x + y) % 2 == 0 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        assert_eq!(at(&thumbnail(&checker, 1, 0), 0, 0), [188, 188, 188, 255]);
    }

    #[test]
    fn upscaling_keeps_the_colors() {
        let frame = thumbnail(&image(1, 1, |_, _| BLUE), 8, 0);
        assert!(pixels(&frame).iter().all(|p| *p == BLUE));
    }

    #[test]
    fn scaling_by_an_uneven_factor_keeps_a_uniform_image_uniform() {
        let frame = thumbnail(&image(7, 7, |_, _| [10, 200, 30, 255]), 3, 0);
        assert!(pixels(&frame).iter().all(|p| *p == [10, 200, 30, 255]));
        let frame = thumbnail(&image(3, 3, |_, _| [10, 200, 30, 255]), 7, 0);
        assert!(pixels(&frame).iter().all(|p| *p == [10, 200, 30, 255]));
    }

    #[test]
    fn transparent_parts_stay_transparent() {
        let frame = thumbnail(&image(4, 4, |_, _| [255, 255, 255, 0]), 2, 0);
        assert!(pixels(&frame).iter().all(|p| *p == [0; 4]));
        let half = thumbnail(&image(1, 1, |_, _| [255, 255, 255, 128]), 1, 0);
        assert_eq!(at(&half, 0, 0), [188, 188, 188, 128]);
    }

    #[test]
    fn corners_are_rounded() {
        let frame = thumbnail(&image(4, 4, |_, _| RED), 20, 6);
        assert_eq!(at(&frame, 0, 0), [0; 4]);
        assert_eq!(at(&frame, 19, 19), [0; 4]);
        assert_eq!(at(&frame, 10, 10), RED);
        assert_eq!(at(&frame, 0, 10), RED);
    }

    #[test]
    fn degenerate_images_draw_a_transparent_square() {
        let empty = ArtImage {
            width: 0,
            height: 0,
            rgba: vec![],
        };
        assert_eq!(thumbnail(&empty, 4, 2), Frame::transparent(4, 4));
        let short = ArtImage {
            width: 2,
            height: 2,
            rgba: vec![255; 15],
        };
        assert_eq!(thumbnail(&short, 4, 2), Frame::transparent(4, 4));
        let line = image(5, 0, |_, _| RED);
        assert_eq!(thumbnail(&line, 3, 0), Frame::transparent(3, 3));
    }

    #[test]
    fn zero_side_gives_an_empty_frame() {
        assert_eq!(
            thumbnail(&image(2, 2, |_, _| RED), 0, 4),
            Frame::transparent(0, 0)
        );
    }

    #[test]
    fn longer_pixels_than_the_size_needs_are_tolerated() {
        let mut long = image(1, 1, |_, _| RED);
        long.rgba.extend([0; 8]);
        assert_eq!(at(&thumbnail(&long, 2, 0), 1, 1), RED);
    }

    // Cache

    /// Address of the pixels of the thumbnail the cache hands out: a new
    /// preparation allocates new pixels while the previous ones are alive.
    fn prepared(cache: &mut ArtCache, image: &Arc<ArtImage>, side: u32, radius: u32) -> usize {
        cache.get(image, side, radius).pixels().as_ptr() as usize
    }

    #[test]
    fn cache_hands_out_the_thumbnail_of_its_input() {
        let red = Arc::new(image(3, 3, |_, _| RED));
        let mut cache = ArtCache::new();
        assert_eq!(*cache.get(&red, 5, 2), thumbnail(&red, 5, 2));
        let blue = Arc::new(image(3, 3, |_, _| BLUE));
        assert_eq!(*cache.get(&blue, 5, 2), thumbnail(&blue, 5, 2));
    }

    #[test]
    fn cache_prepares_once_for_the_same_input() {
        let red = Arc::new(image(3, 3, |_, _| RED));
        let mut cache = ArtCache::new();
        let first = prepared(&mut cache, &red, 5, 2);
        assert_eq!(prepared(&mut cache, &Arc::clone(&red), 5, 2), first);
    }

    #[test]
    fn cache_prepares_again_when_an_input_changes() {
        let red = Arc::new(image(3, 3, |_, _| RED));
        let other = Arc::new(image(3, 3, |_, _| RED));
        let mut cache = ArtCache::new();
        let mut last = prepared(&mut cache, &red, 5, 2);
        for (image, side, radius) in [(&red, 6, 2), (&red, 6, 3), (&other, 6, 3)] {
            let now = prepared(&mut cache, image, side, radius);
            assert_ne!(now, last, "{side} {radius}");
            last = now;
        }
    }
}
