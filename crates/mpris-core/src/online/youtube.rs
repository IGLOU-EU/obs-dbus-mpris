//! YouTube (D-09): the thumbnail of a video, derived from the address of
//! the page a browser plays (`xesam:url`), with no search. It is the
//! image the browser shows, and the only right one for a video that is
//! not music.

use crate::model::ArtImage;

/// The id of the video a YouTube page plays: the `v` parameter of a
/// watch page, or the path of a short link. `None` for any other page.
pub fn video_id(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return None;
    }
    let (authority, rest) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
    let host = authority.rsplit('@').next()?.split(':').next()?;
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let host = host.strip_prefix("m.").unwrap_or(host);
    let rest = rest.split('#').next().unwrap_or_default();
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let id = match host {
        "youtube.com" | "music.youtube.com" => query
            .split('&')
            .find_map(|pair| pair.strip_prefix("v=").filter(|id| !id.is_empty()))?,
        "youtu.be" => path.trim_matches('/').split('/').next()?,
        _ => return None,
    };
    let valid = id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| id.to_owned())
}

/// `hqdefault`, 480 by 360, exists even for the oldest videos, where
/// larger sizes may be missing (study, section 8).
pub fn thumbnail_url(id: &str) -> String {
    format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")
}

/// The central square of the 16:9 picture that a 4:3 thumbnail holds
/// between black bars, so that the bars never show in the square slot.
pub fn crop(image: ArtImage) -> ArtImage {
    let (width, height) = (image.width, image.height);
    let side = (width * 9 / 16).max(1).min(width).min(height);
    let (left, top) = ((width - side) / 2, (height - side) / 2);
    let row = |y: u32| {
        let start = ((y * width + left) * 4) as usize;
        &image.rgba[start..start + (side * 4) as usize]
    };
    let rgba = (top..top + side).flat_map(row).copied().collect();
    ArtImage {
        width: side,
        height: side,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "jNQXAC9IVRw";

    #[test]
    fn the_id_comes_from_any_page_of_a_video() {
        for url in [
            "https://www.youtube.com/watch?v=jNQXAC9IVRw",
            "https://www.youtube.com/watch?v=jNQXAC9IVRw&t=3s",
            "https://youtube.com/watch?feature=share&v=jNQXAC9IVRw",
            "https://m.youtube.com/watch?v=jNQXAC9IVRw",
            "https://music.youtube.com/watch?v=jNQXAC9IVRw&list=RD",
            "http://www.youtube.com/watch?v=jNQXAC9IVRw#t=10",
            "https://WWW.YouTube.com:443/watch?v=jNQXAC9IVRw",
            "https://youtu.be/jNQXAC9IVRw",
            "https://youtu.be/jNQXAC9IVRw?t=3",
            "https://youtu.be/jNQXAC9IVRw/",
            "https://www.youtube.com/watch?v=jNQXAC9IVRw&v=otherid1234",
        ] {
            assert_eq!(video_id(url).as_deref(), Some(ID), "{url}");
        }
    }

    #[test]
    fn other_pages_give_no_id() {
        for url in [
            "",
            "jNQXAC9IVRw",
            "https://www.youtube.com/@channel",
            "https://www.youtube.com/watch",
            "https://www.youtube.com/watch?list=RD",
            "https://www.youtube.com/watch?v=",
            "https://www.youtube.com/watch?v=short",
            "https://www.youtube.com/watch?v=jNQXAC9IVRw1",
            "https://www.youtube.com/watch?v=jNQX%2FAC9IV",
            "https://www.youtube.com/watch?v=jNQX/AC9IVR",
            "https://youtu.be/",
            "https://example.org/watch?v=jNQXAC9IVRw",
            "https://youtube.com.example.org/watch?v=jNQXAC9IVRw",
            "https://example.org/?u=https://www.youtube.com/watch?v=jNQXAC9IVRw",
            "https://user@example.org/watch?v=jNQXAC9IVRw",
            "ftp://youtube.com/watch?v=jNQXAC9IVRw",
            "file:///home/me/music.flac",
            "data:text/plain;base64,AAAA",
        ] {
            assert_eq!(video_id(url), None, "{url}");
        }
    }

    /// `hqdefault` exists even for the oldest videos, where larger sizes
    /// may be missing (study, section 8).
    #[test]
    fn the_thumbnail_is_the_high_quality_default() {
        assert_eq!(
            thumbnail_url(ID),
            "https://i.ytimg.com/vi/jNQXAC9IVRw/hqdefault.jpg"
        );
    }

    /// A 4:3 frame with a 16:9 picture between black bars, the picture red
    /// with its top left pixel green.
    fn letterboxed(width: u32, height: u32) -> ArtImage {
        let bar = (height - width * 9 / 16) / 2;
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let pixel = if y < bar || y >= height - bar {
                    [0, 0, 0, 255]
                } else if (x, y) == ((width - width * 9 / 16) / 2, bar) {
                    [0, 255, 0, 255]
                } else {
                    [255, 0, 0, 255]
                };
                rgba.extend_from_slice(&pixel);
            }
        }
        ArtImage {
            width,
            height,
            rgba,
        }
    }

    #[test]
    fn the_crop_keeps_the_central_square_of_the_picture() {
        let square = crop(letterboxed(480, 360));
        assert_eq!((square.width, square.height), (270, 270));
        assert_eq!(square.rgba.len(), 270 * 270 * 4);
        assert_eq!(square.rgba[..4], [0, 255, 0, 255]);
        assert!(square.rgba[4..].chunks(4).all(|p| p == [255, 0, 0, 255]));
    }

    #[test]
    fn the_crop_never_leaves_the_image() {
        for (width, height) in [(16, 9), (9, 16), (1, 1), (100, 10), (0, 0)] {
            let image = ArtImage {
                width,
                height,
                rgba: vec![7; (width * height * 4) as usize],
            };
            let square = crop(image);
            assert_eq!(square.width, square.height, "{width}x{height}");
            assert!(square.width <= width.min(height), "{width}x{height}");
            assert_eq!(
                square.rgba.len(),
                (square.width * square.height * 4) as usize
            );
        }
    }
}
