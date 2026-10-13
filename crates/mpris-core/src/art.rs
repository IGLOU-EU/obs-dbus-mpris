//! Thumbnail loading: `mpris:artUrl` to decoded RGBA (B-10, D-03).
//!
//! Blocking work, which the core runs on its `blocking` pool: reading and
//! decoding are never allowed in an OBS render callback (B-41).

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Cursor, Read};
// MPRIS only exists on unix desktops, where paths are raw bytes.
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use image::{ImageReader, Limits};
use percent_encoding::percent_decode_str;

use crate::model::ArtImage;

/// Where the image bytes of an art URL live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtSource {
    /// Bytes carried by a `data:` URI, already decoded from base64.
    Data(Vec<u8>),
    File(PathBuf),
}

/// Why a thumbnail could not be loaded. Every case leaves the thumbnail
/// slot empty (B-10), the distinction only serves the logs.
#[derive(Debug)]
pub enum ArtError {
    /// A scheme this loader does not read, such as `http` (out of scope
    /// for B-10). Holds the scheme in lowercase.
    Unsupported(String),
    /// Not a well-formed base64 `data:` URI or local `file:` URL.
    Malformed(&'static str),
    Io(io::Error),
    /// The file is bigger than anything a thumbnail needs.
    TooLarge {
        size: u64,
        limit: u64,
    },
    /// Not a supported image, corrupted, or over the decoding limits.
    Decode(image::ImageError),
}

impl fmt::Display for ArtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArtError::Unsupported(scheme) => write!(f, "unsupported art URL scheme `{scheme}`"),
            ArtError::Malformed(reason) => write!(f, "malformed art URL: {reason}"),
            ArtError::Io(e) => write!(f, "cannot read art: {e}"),
            ArtError::TooLarge { size, limit } => {
                write!(f, "art file of {size} bytes exceeds the {limit} byte limit")
            }
            ArtError::Decode(e) => write!(f, "cannot decode art: {e}"),
        }
    }
}

impl std::error::Error for ArtError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ArtError::Io(e) => Some(e),
            ArtError::Decode(e) => Some(e),
            ArtError::Unsupported(_) | ArtError::Malformed(_) | ArtError::TooLarge { .. } => None,
        }
    }
}

impl From<io::Error> for ArtError {
    fn from(e: io::Error) -> Self {
        ArtError::Io(e)
    }
}

/// Splits an art URL into the source of its bytes. Accepts base64 `data:`
/// URIs, `file:` URLs on the local host, and bare absolute paths.
pub fn parse_art_url(url: &str) -> Result<ArtSource, ArtError> {
    if url.starts_with('/') {
        return Ok(ArtSource::File(PathBuf::from(url)));
    }
    let (scheme, rest) =
        split_scheme(url).ok_or(ArtError::Malformed("neither a URL nor an absolute path"))?;
    if scheme.eq_ignore_ascii_case("data") {
        parse_data(rest)
    } else if scheme.eq_ignore_ascii_case("file") {
        parse_file(rest)
    } else {
        Err(ArtError::Unsupported(scheme.to_ascii_lowercase()))
    }
}

/// `(scheme, rest)` when `url` starts with a valid scheme (RFC 3986, 3.1).
fn split_scheme(url: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then_some((scheme, rest))
}

/// `[<media type>];base64,<payload>` (RFC 2397). The media type is not
/// trusted: the decoder guesses the format from the bytes.
fn parse_data(rest: &str) -> Result<ArtSource, ArtError> {
    let (header, payload) = rest
        .split_once(',')
        .ok_or(ArtError::Malformed("data URI without a comma"))?;
    let base64 = header
        .rsplit_once(';')
        .is_some_and(|(_, flag)| flag.eq_ignore_ascii_case("base64"));
    if !base64 {
        return Err(ArtError::Malformed("data URI is not base64"));
    }
    // Encoders may wrap long base64 payloads over several lines.
    let payload: Vec<u8> = payload
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    STANDARD
        .decode(payload)
        .map(ArtSource::Data)
        .map_err(|_| ArtError::Malformed("invalid base64 in data URI"))
}

/// `file://[localhost]/path` or `file:/path` (RFC 8089). `?` and `#` stay
/// part of the path: art URLs carry neither query nor fragment, and a
/// careless player may leave them unescaped in a file name.
fn parse_file(rest: &str) -> Result<ArtSource, ArtError> {
    let path = match rest.strip_prefix("//") {
        Some(after) => {
            let (host, path) = after.split_at(after.find('/').unwrap_or(after.len()));
            if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                return Err(ArtError::Malformed("file URL on a remote host"));
            }
            path
        }
        None => rest,
    };
    if !path.starts_with('/') {
        return Err(ArtError::Malformed("file URL without an absolute path"));
    }
    // A percent-encoded file name need not be UTF-8.
    let bytes: Vec<u8> = percent_decode_str(path).collect();
    Ok(ArtSource::File(PathBuf::from(OsString::from_vec(bytes))))
}

/// Reads and decodes the thumbnail of `url` into RGBA8, downscaled when a
/// side is larger than a thumbnail needs.
pub fn load_art(url: &str) -> Result<ArtImage, ArtError> {
    load_art_within(url, MAX_FILE_BYTES)
}

/// The source shows the thumbnail at most at its own height, and the
/// decoded image is shared by every source, so more pixels would only
/// waste memory.
const MAX_ART_SIDE: u32 = 1024;

/// Generous for any cover, small enough to refuse a video or a disk image
/// named by mistake.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// Larger than any real cover. A header claiming more is refused before
/// its pixel buffer is allocated.
const MAX_DECODED_SIDE: u32 = 8192;

/// Room for a [`MAX_DECODED_SIDE`] square in RGBA8.
const MAX_DECODE_ALLOC: u64 = 256 * 1024 * 1024;

/// [`load_art`] with the file size limit as a parameter, so tests can
/// reach it without writing huge files.
fn load_art_within(url: &str, max_file_bytes: u64) -> Result<ArtImage, ArtError> {
    let bytes = match parse_art_url(url)? {
        // Already in memory, and bounded by the D-Bus message size, so
        // only files need a size check.
        ArtSource::Data(bytes) => bytes,
        ArtSource::File(path) => read_file(&path, max_file_bytes)?,
    };
    decode(&bytes)
}

fn read_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>, ArtError> {
    // Checked before opening: opening a FIFO would block until a writer
    // shows up, and the core waits for every load it starts.
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(ArtError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        )));
    }
    if metadata.len() > max_bytes {
        return Err(ArtError::TooLarge {
            size: metadata.len(),
            limit: max_bytes,
        });
    }
    let mut bytes = Vec::new();
    // Bounded even if the file grows after the size check.
    File::open(path)?.take(max_bytes).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Result<ArtImage, ArtError> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    reader.limits(decode_limits());
    let image = reader.decode().map_err(ArtError::Decode)?;
    let image = if image.width() > MAX_ART_SIDE || image.height() > MAX_ART_SIDE {
        // Averages straight into the small buffer, with no full-size
        // intermediate copy of a big cover.
        image.thumbnail(MAX_ART_SIDE, MAX_ART_SIDE)
    } else {
        image
    };
    let rgba = image.into_rgba8();
    Ok(ArtImage {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// Bounds what a hostile file can make the decoder allocate.
fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_SIDE);
    limits.max_image_height = Some(MAX_DECODED_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    limits
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use image::{ImageFormat, Rgb, RgbImage};
    use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_encode};

    use super::*;

    /// Pixel `(x, y)` is `[x, y, 200]`, so decoded pixels can be checked.
    fn gradient(width: u32, height: u32) -> RgbImage {
        RgbImage::from_fn(width, height, |x, y| Rgb([x as u8, y as u8, 200]))
    }

    fn encode(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Vec::new();
        gradient(width, height)
            .write_to(&mut Cursor::new(&mut bytes), format)
            .expect("encode test image");
        bytes
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        encode(width, height, ImageFormat::Png)
    }

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        encode(width, height, ImageFormat::Jpeg)
    }

    fn data_uri(media_type: &str, bytes: &[u8]) -> String {
        format!("data:{media_type};base64,{}", STANDARD.encode(bytes))
    }

    /// Everything but unreserved characters and `/`, as a file manager
    /// would encode a path.
    const PATH: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'/')
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');

    fn file_url(path: &Path) -> String {
        format!(
            "file://{}",
            percent_encode(path.as_os_str().as_bytes(), PATH)
        )
    }

    /// A file under the system temp directory, removed on drop so a failed
    /// assertion does not leave it behind.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &[u8], contents: &[u8]) -> TempFile {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let mut file_name =
                format!("mpris-core-art-{}-{unique}-", std::process::id()).into_bytes();
            file_name.extend_from_slice(name);
            let path = std::env::temp_dir().join(OsStr::from_bytes(&file_name));
            fs::write(&path, contents).expect("write temp file");
            TempFile(path)
        }

        fn url(&self) -> String {
            file_url(&self.0)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn pixel(image: &ArtImage, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * image.width + x) * 4) as usize;
        image.rgba[at..at + 4].try_into().expect("4 bytes")
    }

    fn assert_gradient(image: &ArtImage, width: u32, height: u32) {
        assert_eq!((image.width, image.height), (width, height));
        assert_eq!(image.rgba.len(), (width * height * 4) as usize);
        for (x, y) in [
            (0, 0),
            (width - 1, 0),
            (0, height - 1),
            (width - 1, height - 1),
        ] {
            assert_eq!(
                pixel(image, x, y),
                [x as u8, y as u8, 200, 255],
                "({x}, {y})"
            );
        }
    }

    fn assert_opaque(image: &ArtImage, width: u32, height: u32) {
        assert_eq!((image.width, image.height), (width, height));
        assert_eq!(image.rgba.len(), (width * height * 4) as usize);
        assert!(image.rgba.chunks(4).all(|p| p[3] == 255));
    }

    fn assert_malformed(url: &str) {
        let result = parse_art_url(url);
        assert!(
            matches!(result, Err(ArtError::Malformed(_))),
            "{url}: {result:?}"
        );
    }

    fn assert_unsupported(url: &str, scheme: &str) {
        match parse_art_url(url) {
            Err(ArtError::Unsupported(s)) => assert_eq!(s, scheme, "{url}"),
            other => panic!("{url}: {other:?}"),
        }
    }

    fn file(path: &[u8]) -> ArtSource {
        ArtSource::File(PathBuf::from(OsStr::from_bytes(path)))
    }

    // parse_art_url: data URIs

    #[test]
    fn data_uri_payload_is_base64_decoded() {
        assert_eq!(
            parse_art_url("data:image/jpeg;base64,AAEC").ok(),
            Some(ArtSource::Data(vec![0, 1, 2]))
        );
    }

    #[test]
    fn data_uri_ignores_whitespace_in_payload() {
        assert_eq!(
            parse_art_url("data:image/png;base64,AA\r\n E\tC\n").ok(),
            Some(ArtSource::Data(vec![0, 1, 2]))
        );
    }

    #[test]
    fn data_uri_media_type_is_optional() {
        assert_eq!(
            parse_art_url("data:;base64,AAEC").ok(),
            Some(ArtSource::Data(vec![0, 1, 2]))
        );
    }

    #[test]
    fn data_uri_scheme_and_flag_ignore_case() {
        assert_eq!(
            parse_art_url("DATA:image/png;BASE64,AAEC").ok(),
            Some(ArtSource::Data(vec![0, 1, 2]))
        );
    }

    #[test]
    fn data_uri_that_is_not_base64_is_refused() {
        assert_malformed("data:image/png,AAEC");
        assert_malformed("data:text/plain;charset=utf-8,hello");
        // Without `;`, `base64` is a media type, not the encoding flag.
        assert_malformed("data:base64,AAEC");
    }

    #[test]
    fn data_uri_without_payload_separator_is_refused() {
        assert_malformed("data:image/png;base64");
    }

    #[test]
    fn invalid_base64_is_refused() {
        assert_malformed("data:image/png;base64,@@@@");
        assert_malformed("data:image/png;base64,AAE");
        assert_malformed("data:image/png;base64,AA%3D");
    }

    // parse_art_url: files

    #[test]
    fn file_url_with_empty_host() {
        assert_eq!(
            parse_art_url("file:///home/me/cover.jpg").ok(),
            Some(file(b"/home/me/cover.jpg"))
        );
    }

    #[test]
    fn file_url_with_localhost() {
        for url in ["file://localhost/c.jpg", "file://LocalHost/c.jpg"] {
            assert_eq!(parse_art_url(url).ok(), Some(file(b"/c.jpg")), "{url}");
        }
    }

    #[test]
    fn file_url_without_authority() {
        assert_eq!(parse_art_url("file:/c.jpg").ok(), Some(file(b"/c.jpg")));
    }

    #[test]
    fn file_url_scheme_ignores_case() {
        assert_eq!(parse_art_url("FILE:///c.jpg").ok(), Some(file(b"/c.jpg")));
    }

    #[test]
    fn file_url_is_percent_decoded() {
        assert_eq!(
            parse_art_url("file:///tmp/my%20music/caf%C3%A9%20%E6%97%A5%E6%9C%AC.png").ok(),
            Some(file("/tmp/my music/café 日本.png".as_bytes()))
        );
    }

    #[test]
    fn file_url_may_decode_to_non_utf8_bytes() {
        assert_eq!(
            parse_art_url("file:///tmp/%FF%FE.png").ok(),
            Some(file(b"/tmp/\xFF\xFE.png"))
        );
    }

    #[test]
    fn file_url_with_remote_host_is_refused() {
        assert_malformed("file://server/share/cover.jpg");
    }

    #[test]
    fn file_url_without_absolute_path_is_refused() {
        assert_malformed("file:cover.jpg");
        assert_malformed("file://");
        assert_malformed("file://localhost");
    }

    #[test]
    fn bare_absolute_path_is_taken_verbatim() {
        assert_eq!(
            parse_art_url("/home/me/a%20b.jpg").ok(),
            Some(file(b"/home/me/a%20b.jpg"))
        );
    }

    // parse_art_url: other schemes and garbage

    #[test]
    fn http_and_https_are_unsupported() {
        assert_unsupported("http://example.org/cover.jpg", "http");
        assert_unsupported("https://example.org/cover.jpg", "https");
        assert_unsupported("HTTPS://example.org/cover.jpg", "https");
    }

    #[test]
    fn unknown_schemes_are_unsupported() {
        assert_unsupported("ftp://example.org/cover.jpg", "ftp");
        assert_unsupported("x-spotify:image:abc", "x-spotify");
    }

    #[test]
    fn text_without_scheme_is_refused() {
        for url in ["", " ", "cover.jpg", "./cover.jpg", "://x", "1http://x"] {
            assert_malformed(url);
        }
    }

    // load_art: decoding

    #[test]
    fn png_data_uri_loads() {
        let image = load_art(&data_uri("image/png", &png(3, 2))).expect("load");
        assert_gradient(&image, 3, 2);
    }

    #[test]
    fn jpeg_data_uri_loads() {
        let image = load_art(&data_uri("image/jpeg", &jpeg(16, 8))).expect("load");
        assert_opaque(&image, 16, 8);
    }

    #[test]
    fn format_comes_from_content_not_media_type() {
        let image = load_art(&data_uri("image/jpeg", &png(3, 2))).expect("load");
        assert_gradient(&image, 3, 2);
    }

    #[test]
    fn png_file_url_loads() {
        let file = TempFile::new(b"cover.png", &png(5, 4));
        assert_gradient(&load_art(&file.url()).expect("load"), 5, 4);
    }

    #[test]
    fn jpeg_file_url_loads() {
        let file = TempFile::new(b"cover.jpg", &jpeg(16, 8));
        assert_opaque(&load_art(&file.url()).expect("load"), 16, 8);
    }

    #[test]
    fn file_name_with_spaces_and_non_ascii_loads() {
        let file = TempFile::new("my cover é 日本.png".as_bytes(), &png(3, 2));
        assert!(file.url().contains("%20"), "{}", file.url());
        assert_gradient(&load_art(&file.url()).expect("load"), 3, 2);
    }

    #[test]
    fn non_utf8_file_name_loads() {
        let file = TempFile::new(b"cover-\xFF.png", &png(3, 2));
        assert_gradient(&load_art(&file.url()).expect("load"), 3, 2);
    }

    #[test]
    fn localhost_file_url_loads() {
        let file = TempFile::new(b"cover.png", &png(3, 2));
        let url = file.url().replacen("file://", "file://localhost", 1);
        assert_gradient(&load_art(&url).expect("load"), 3, 2);
    }

    #[test]
    fn bare_path_loads() {
        let file = TempFile::new(b"cover.png", &png(3, 2));
        let path = file.0.to_str().expect("utf-8 temp path");
        assert_gradient(&load_art(path).expect("load"), 3, 2);
    }

    // load_art: refusals

    #[test]
    fn missing_file_is_an_io_error() {
        let path =
            std::env::temp_dir().join(format!("mpris-core-art-{}-missing.png", std::process::id()));
        match load_art(&file_url(&path)) {
            Err(ArtError::Io(e)) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn directory_is_an_io_error() {
        let result = load_art(&file_url(&std::env::temp_dir()));
        assert!(matches!(result, Err(ArtError::Io(_))), "{result:?}");
    }

    #[test]
    fn unsupported_url_is_not_loaded() {
        let result = load_art("https://example.org/cover.jpg");
        assert!(
            matches!(result, Err(ArtError::Unsupported(_))),
            "{result:?}"
        );
    }

    #[test]
    fn bytes_that_are_not_an_image_fail_to_decode() {
        for bytes in [&b"hello, world"[..], &[]] {
            let result = load_art(&data_uri("image/png", bytes));
            assert!(matches!(result, Err(ArtError::Decode(_))), "{result:?}");
        }
    }

    #[test]
    fn truncated_png_fails_to_decode() {
        let bytes = png(32, 32);
        let result = load_art(&data_uri("image/png", &bytes[..bytes.len() / 2]));
        assert!(matches!(result, Err(ArtError::Decode(_))), "{result:?}");
    }

    #[test]
    fn truncated_png_file_fails_to_decode() {
        let bytes = png(32, 32);
        let file = TempFile::new(b"truncated.png", &bytes[..bytes.len() / 2]);
        let result = load_art(&file.url());
        assert!(matches!(result, Err(ArtError::Decode(_))), "{result:?}");
    }

    #[test]
    fn truncated_jpeg_fails_to_decode() {
        let bytes = jpeg(64, 64);
        let result = load_art(&data_uri("image/jpeg", &bytes[..bytes.len() / 2]));
        assert!(matches!(result, Err(ArtError::Decode(_))), "{result:?}");
    }

    #[test]
    fn file_over_the_size_limit_is_refused() {
        let bytes = png(3, 2);
        let size = bytes.len() as u64;
        let file = TempFile::new(b"cover.png", &bytes);
        match load_art_within(&file.url(), size - 1) {
            Err(ArtError::TooLarge { size: s, limit }) => assert_eq!((s, limit), (size, size - 1)),
            other => panic!("{other:?}"),
        }
        assert_gradient(&load_art_within(&file.url(), size).expect("load"), 3, 2);
    }

    #[test]
    fn hostile_dimensions_are_refused_before_allocation() {
        let result = load_art(&data_uri("image/png", &png(8193, 1)));
        assert!(
            matches!(result, Err(ArtError::Decode(image::ImageError::Limits(_)))),
            "{result:?}"
        );
    }

    // load_art: downscaling

    #[test]
    fn image_at_the_maximum_side_is_kept() {
        let image = load_art(&data_uri("image/png", &png(MAX_ART_SIDE, 2))).expect("load");
        assert_eq!((image.width, image.height), (MAX_ART_SIDE, 2));
    }

    #[test]
    fn wide_image_is_downscaled_keeping_aspect_ratio() {
        let image = load_art(&data_uri("image/png", &png(2048, 16))).expect("load");
        assert_opaque(&image, 1024, 8);
    }

    #[test]
    fn tall_image_is_downscaled_keeping_aspect_ratio() {
        let image = load_art(&data_uri("image/png", &png(30, 3000))).expect("load");
        assert_opaque(&image, 10, 1024);
    }

    #[test]
    fn image_one_pixel_over_is_downscaled() {
        let image = load_art(&data_uri("image/png", &png(MAX_ART_SIDE + 1, 2))).expect("load");
        assert_eq!((image.width, image.height), (MAX_ART_SIDE, 2));
    }

    // ArtError

    #[test]
    fn errors_describe_their_cause() {
        let decode = load_art(&data_uri("image/png", b"nope")).expect_err("not an image");
        let cases = [
            (ArtError::Unsupported("http".to_owned()), "http"),
            (ArtError::Malformed("bad base64"), "bad base64"),
            (
                ArtError::Io(io::Error::other("disk on fire")),
                "disk on fire",
            ),
            (
                ArtError::TooLarge {
                    size: 2000,
                    limit: 1000,
                },
                "2000",
            ),
        ];
        for (error, needle) in cases {
            let text = error.to_string();
            assert!(text.contains(needle), "{text:?} lacks {needle:?}");
        }
        assert!(!decode.to_string().is_empty());
    }

    #[test]
    fn io_and_decode_errors_expose_their_source() {
        let io = ArtError::Io(io::Error::other("x"));
        assert!(io.source().is_some());
        let decode = load_art(&data_uri("image/png", b"nope")).expect_err("not an image");
        assert!(decode.source().is_some());
        assert!(ArtError::Malformed("x").source().is_none());
    }
}
