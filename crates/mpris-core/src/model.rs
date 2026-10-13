//! Plain data shared by the whole core. Behaviour lives in the other
//! modules, as functions over these types, so each piece of data has one
//! definition and one owner (docs/architecture.md).

use std::sync::Arc;
use std::time::{Duration, Instant};

/// Identity of a player that survives restarts: the first segment after
/// `org.mpris.MediaPlayer2.` in its bus name (B-04).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerKey(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    #[default]
    Stopped,
}

/// Track metadata as the player reports it, before any display choice.
///
/// No text field holds blank text and `length` is never zero:
/// [`crate::metadata::parse_track`] reads both as absent, so readers of a
/// track need no second check.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Track {
    /// `mpris:trackid`.
    pub id: Option<String>,
    /// `xesam:title`.
    pub title: Option<String>,
    /// `xesam:artist`.
    pub artists: Vec<String>,
    /// `xesam:albumArtist`.
    pub album_artists: Vec<String>,
    /// `xesam:album`.
    pub album: Option<String>,
    /// `mpris:length`. `None` when unknown or zero (streams).
    pub length: Option<Duration>,
    /// `mpris:artUrl`, untouched.
    pub art_url: Option<String>,
}

/// Last position read from the player, and when. MPRIS does not signal
/// position changes, so the displayed position is derived from this anchor
/// instead of being stored (B-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionAnchor {
    pub position: Duration,
    pub at: Instant,
}

/// Decoded thumbnail, ready to upload as a texture.
#[derive(Debug, PartialEq, Eq)]
pub struct ArtImage {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA8, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// Thumbnail state for the current `Track::art_url`.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Art {
    /// The track has no `art_url`.
    #[default]
    None,
    /// `art_url` is known and waits for decoding.
    Pending,
    Ready(Arc<ArtImage>),
    /// `art_url` could not be read or decoded. Kept so it is not retried
    /// before the next track.
    Failed,
}

/// One player present on the bus.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    /// Unique D-Bus name (`:1.42`). Signals come from it, not from the
    /// well-known name.
    pub unique_name: String,
    /// Well-known name, `org.mpris.MediaPlayer2.<key>[.<instance>]`.
    pub bus_name: String,
    pub key: PlayerKey,
    /// `Identity`, the human-readable name shown in the selector (B-02).
    /// Never blank: the adapter reads it with
    /// [`crate::player::read_identity`] and falls back to the key when the
    /// player gives none.
    pub identity: String,
    pub status: PlaybackStatus,
    /// `Rate`, 1.0 when the player does not report it.
    pub rate: f64,
    pub track: Track,
    pub anchor: PositionAnchor,
    pub art: Art,
}

/// Immutable view of every player, published by the core thread and read
/// by the sources.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub players: Vec<Player>,
}

/// 8-bit color with alpha, so any zone can be made transparent (B-31).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// Settings of one source (docs/besoins.md, section 5). Defaults come from
/// the schema in [`crate::settings`], never from a second definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Remembered player. `None` until the user picks one (B-07).
    pub player: Option<PlayerKey>,
    pub show_art: bool,
    pub fetch_art_online: bool,
    pub show_title: bool,
    pub show_artist: bool,
    pub show_album: bool,
    pub show_progress: bool,
    pub show_position: bool,
    pub show_duration: bool,
    pub art_right: bool,
    pub color_background: Rgba,
    pub color_text: Rgba,
    pub color_bar_fill: Rgba,
    pub color_bar_track: Rgba,
    pub width: u32,
    pub height: u32,
    pub font_size_title: u32,
    pub font_size_subtitle: u32,
    pub font_size_time: u32,
    /// Shown when nothing plays. Empty means a fully transparent source.
    pub fallback_text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(red: u8) -> Arc<ArtImage> {
        Arc::new(ArtImage {
            width: 1,
            height: 1,
            rgba: vec![red, 0, 0, 255],
        })
    }

    #[test]
    fn art_equality_follows_state_and_image_content() {
        let shared = image(255);
        assert_eq!(Art::Ready(shared.clone()), Art::Ready(shared));
        assert_eq!(Art::Ready(image(255)), Art::Ready(image(255)));
        assert_ne!(Art::Ready(image(255)), Art::Ready(image(0)));
        let states = [Art::None, Art::Pending, Art::Ready(image(255)), Art::Failed];
        for (i, a) in states.iter().enumerate() {
            for (j, b) in states.iter().enumerate() {
                assert_eq!(a == b, i == j, "{a:?} vs {b:?}");
            }
        }
    }
}
