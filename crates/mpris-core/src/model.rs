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
    /// `xesam:url`, the page a browser plays, which tells a YouTube video
    /// whose thumbnail can stand in for a cover (B-18).
    pub url: Option<String>,
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

/// A service a source may look a missing cover up with (B-18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Service {
    MusicBrainz,
    Wikipedia,
    YouTube,
}

impl Service {
    pub const ALL: [Service; 3] = [Service::MusicBrainz, Service::Wikipedia, Service::YouTube];
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
    /// Full bus name of the remembered instance (B-03), `None` until the
    /// user picks one (B-07). Its key is derived with
    /// [`crate::player::player_key`] when needed, never stored twice (D-06).
    pub player: Option<String>,
    /// Whether the source may follow another player than the remembered
    /// instance (B-05 steps 2 to 4). Off, a source dedicated to one player
    /// never switches to another.
    pub auto_select: bool,
    pub show_art: bool,
    /// Where to look a missing cover up, `None` for nowhere (B-18).
    pub art_online: Option<Service>,
    pub show_title: bool,
    pub show_artist: bool,
    pub show_album: bool,
    pub show_progress: bool,
    pub show_position: bool,
    pub show_duration: bool,
    /// The remaining time in place of the duration, only where the duration
    /// shows (B-19).
    pub show_remaining: bool,
    pub art_right: bool,
    /// The thumbnail drawn as a disc, whatever `corner_radius` (B-38).
    pub art_round: bool,
    /// Pixels between the thumbnail and the panel, counted in `width` (B-39).
    pub art_gap: u32,
    pub color_background: Rgba,
    pub color_text: Rgba,
    pub color_bar_fill: Rgba,
    pub color_bar_track: Rgba,
    /// Total width of the source, thumbnail included. The height follows
    /// from the font sizes and the shown elements (B-33).
    pub width: u32,
    pub font_size_title: u32,
    pub font_size_subtitle: u32,
    pub font_size_time: u32,
    /// Corners of the panel and the thumbnail, 0 for square ones (B-38).
    pub corner_radius: u32,
    /// Thickness of the progress bar, which the source height follows
    /// (B-50).
    pub bar_height: u32,
    /// Whether a text too long for its line scrolls, rather than ends with
    /// an ellipsis (B-36).
    pub scroll_enabled: bool,
    /// Pixels per second at which a text too long for its line scrolls
    /// (B-36).
    pub scroll_speed: u32,
    /// The stop of a scrolling text at its start, before each round (B-36).
    pub scroll_pause: Duration,
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
