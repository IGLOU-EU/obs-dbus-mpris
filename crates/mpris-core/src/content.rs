//! What a source has to draw, derived from the selected player and the settings.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::model::{Art, ArtImage, PlaybackStatus, Player, Settings};
use crate::position::display_position;

/// Everything a source draws, so the OBS glue can compare it with the
/// previous frame and regenerate only what changed (B-43).
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// Fully transparent source (B-23).
    Nothing,
    /// The fallback message alone (B-22).
    Message(String),
    Track(TrackContent),
}

/// A field is `None` when its setting hides it or when the player does not
/// provide it: the layout treats both the same way.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackContent {
    /// Drives the pause indicator (B-21).
    pub paused: bool,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Whole seconds, so it changes once per second (B-43).
    pub position: Option<Duration>,
    /// Whole seconds, like `position`.
    pub duration: Option<Duration>,
    /// Exact position over length, in `[0, 1]`.
    pub progress: Option<f32>,
    pub art: Option<Arc<ArtImage>>,
}

/// What to draw at `now` for `player`, the one picked by
/// [`crate::selection::select_player`].
pub fn content(player: Option<&Player>, settings: &Settings, now: Instant) -> Content {
    match player {
        Some(player) if player.status != PlaybackStatus::Stopped => {
            Content::Track(track_content(player, settings, now))
        }
        _ => fallback(&settings.fallback_text),
    }
}

/// A blank message would still draw a frame, where the user asked for a
/// fully transparent source (B-23).
fn fallback(text: &str) -> Content {
    if text.trim().is_empty() {
        Content::Nothing
    } else {
        Content::Message(text.to_owned())
    }
}

fn track_content(player: &Player, settings: &Settings, now: Instant) -> TrackContent {
    let track = &player.track;
    let position = display_position(player, now);
    // A zero length says no more than a missing one, and cannot divide.
    let length = track.length.filter(|length| !length.is_zero());
    TrackContent {
        paused: player.status == PlaybackStatus::Paused,
        title: settings.show_title.then(|| track.title.clone()).flatten(),
        artist: settings
            .show_artist
            .then(|| names(&track.artists).or_else(|| names(&track.album_artists)))
            .flatten(),
        album: settings.show_album.then(|| track.album.clone()).flatten(),
        position: settings.show_position.then(|| whole_seconds(position)),
        duration: settings
            .show_duration
            .then(|| length.map(whole_seconds))
            .flatten(),
        progress: settings
            .show_progress
            .then(|| length.map(|length| (position.as_secs_f64() / length.as_secs_f64()) as f32))
            .flatten(),
        art: settings
            .show_art
            .then(|| match &player.art {
                Art::Ready(image) => Some(Arc::clone(image)),
                Art::None | Art::Pending | Art::Failed => None,
            })
            .flatten(),
    }
}

/// Blank names say no more than an empty list, and must not block the
/// album artist fallback (B-12).
fn names(list: &[String]) -> Option<String> {
    let names: Vec<&str> = list
        .iter()
        .map(String::as_str)
        .filter(|name| !name.trim().is_empty())
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

fn whole_seconds(duration: Duration) -> Duration {
    Duration::from_secs(duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PlayerKey, PositionAnchor, Rgba, Track};

    use PlaybackStatus::{Paused, Playing, Stopped};

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn millis(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn settings() -> Settings {
        Settings {
            player: None,
            show_art: true,
            fetch_art_online: false,
            show_title: true,
            show_artist: true,
            show_album: true,
            show_progress: true,
            show_position: true,
            show_duration: true,
            art_right: false,
            color_background: Rgba::new(0, 0, 0, 153),
            color_text: Rgba::new(255, 255, 255, 255),
            color_bar_fill: Rgba::new(255, 255, 255, 255),
            color_bar_track: Rgba::new(255, 255, 255, 64),
            width: 600,
            height: 120,
            font_size_title: 32,
            font_size_subtitle: 24,
            font_size_time: 20,
            fallback_text: "Nothing playing".into(),
        }
    }

    fn image() -> Arc<ArtImage> {
        Arc::new(ArtImage {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
        })
    }

    /// A 3:00 track at 0:45, every field filled.
    fn player(status: PlaybackStatus, at: Instant) -> Player {
        Player {
            unique_name: ":1.1".into(),
            bus_name: "org.mpris.MediaPlayer2.mpv".into(),
            key: PlayerKey("mpv".into()),
            identity: "mpv".into(),
            status,
            rate: 1.0,
            track: Track {
                id: Some("/track/1".into()),
                title: Some("Title".into()),
                artists: vec!["Artist".into()],
                album_artists: vec!["Album Artist".into()],
                album: Some("Album".into()),
                length: Some(secs(180)),
                art_url: Some("file:///cover.jpg".into()),
            },
            anchor: PositionAnchor {
                position: secs(45),
                at,
            },
            art: Art::Ready(image()),
        }
    }

    fn track_of(content: Content) -> TrackContent {
        match content {
            Content::Track(track) => track,
            other => panic!("expected a track, got {other:?}"),
        }
    }

    fn visible(track: &TrackContent) -> [bool; 7] {
        [
            track.title.is_some(),
            track.artist.is_some(),
            track.album.is_some(),
            track.position.is_some(),
            track.duration.is_some(),
            track.progress.is_some(),
            track.art.is_some(),
        ]
    }

    // Fallback (B-22, B-23)

    #[test]
    fn no_player_shows_the_fallback_message() {
        assert_eq!(
            content(None, &settings(), Instant::now()),
            Content::Message("Nothing playing".into())
        );
    }

    #[test]
    fn stopped_player_shows_the_fallback_message() {
        let t0 = Instant::now();
        assert_eq!(
            content(Some(&player(Stopped, t0)), &settings(), t0),
            Content::Message("Nothing playing".into())
        );
    }

    #[test]
    fn empty_fallback_text_draws_nothing() {
        let t0 = Instant::now();
        let mut s = settings();
        s.fallback_text = String::new();
        assert_eq!(content(None, &s, t0), Content::Nothing);
        assert_eq!(
            content(Some(&player(Stopped, t0)), &s, t0),
            Content::Nothing
        );
    }

    #[test]
    fn blank_fallback_text_draws_nothing() {
        let mut s = settings();
        s.fallback_text = " \t\n ".into();
        assert_eq!(content(None, &s, Instant::now()), Content::Nothing);
    }

    #[test]
    fn fallback_text_is_shown_as_typed() {
        let mut s = settings();
        s.fallback_text = "Aucune lecture ♪".into();
        assert_eq!(
            content(None, &s, Instant::now()),
            Content::Message("Aucune lecture ♪".into())
        );
    }

    // Playing and paused (B-20, B-21)

    #[test]
    fn playing_player_shows_every_field() {
        let t0 = Instant::now();
        let p = player(Playing, t0);
        let image = match &p.art {
            Art::Ready(image) => Arc::clone(image),
            other => panic!("unexpected art {other:?}"),
        };
        assert_eq!(
            content(Some(&p), &settings(), t0),
            Content::Track(TrackContent {
                paused: false,
                title: Some("Title".into()),
                artist: Some("Artist".into()),
                album: Some("Album".into()),
                position: Some(secs(45)),
                duration: Some(secs(180)),
                progress: Some(0.25),
                art: Some(image),
            })
        );
    }

    #[test]
    fn paused_player_shows_the_track_marked_paused() {
        let t0 = Instant::now();
        let track = track_of(content(Some(&player(Paused, t0)), &settings(), t0));
        assert!(track.paused);
        assert_eq!(visible(&track), [true; 7]);
    }

    #[test]
    fn playing_position_and_progress_advance_with_time() {
        let t0 = Instant::now();
        let p = player(Playing, t0);
        let track = track_of(content(Some(&p), &settings(), t0 + secs(45)));
        assert_eq!(track.position, Some(secs(90)));
        assert_eq!(track.progress, Some(0.5));
    }

    #[test]
    fn paused_position_and_progress_stay_frozen() {
        let t0 = Instant::now();
        let p = player(Paused, t0);
        let before = track_of(content(Some(&p), &settings(), t0));
        let after = track_of(content(Some(&p), &settings(), t0 + secs(60)));
        assert_eq!(after.position, Some(secs(45)));
        assert_eq!(after, before);
    }

    #[test]
    fn remembered_player_in_settings_does_not_change_the_content() {
        let t0 = Instant::now();
        let p = player(Playing, t0);
        let mut s = settings();
        s.player = Some(PlayerKey("vlc".into()));
        assert_eq!(
            content(Some(&p), &s, t0),
            content(Some(&p), &settings(), t0)
        );
    }

    // Toggles

    #[test]
    fn each_toggle_hides_only_its_own_field() {
        let toggles: [fn(&mut Settings) -> &mut bool; 7] = [
            |s| &mut s.show_title,
            |s| &mut s.show_artist,
            |s| &mut s.show_album,
            |s| &mut s.show_position,
            |s| &mut s.show_duration,
            |s| &mut s.show_progress,
            |s| &mut s.show_art,
        ];
        let t0 = Instant::now();
        let p = player(Playing, t0);
        for (hidden, toggle) in toggles.iter().enumerate() {
            let mut s = settings();
            *toggle(&mut s) = false;
            let mut expected = [true; 7];
            expected[hidden] = false;
            let track = track_of(content(Some(&p), &s, t0));
            assert_eq!(visible(&track), expected, "toggle {hidden}");
        }
    }

    #[test]
    fn every_toggle_off_still_shows_a_track_with_its_state() {
        let t0 = Instant::now();
        let mut s = settings();
        s.show_art = false;
        s.show_title = false;
        s.show_artist = false;
        s.show_album = false;
        s.show_progress = false;
        s.show_position = false;
        s.show_duration = false;
        let track = track_of(content(Some(&player(Paused, t0)), &s, t0));
        assert!(track.paused);
        assert_eq!(visible(&track), [false; 7]);
    }

    // Missing metadata

    #[test]
    fn missing_metadata_leaves_fields_empty() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track = Track::default();
        p.anchor.position = secs(12);
        p.art = Art::None;
        let track = track_of(content(Some(&p), &settings(), t0));
        assert_eq!(
            track,
            TrackContent {
                paused: false,
                title: None,
                artist: None,
                album: None,
                position: Some(secs(12)),
                duration: None,
                progress: None,
                art: None,
            }
        );
    }

    // Artist (B-12)

    #[test]
    fn several_artists_are_joined_with_a_comma() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.artists = vec!["A".into(), "B".into(), "C".into()];
        let track = track_of(content(Some(&p), &settings(), t0));
        assert_eq!(track.artist.as_deref(), Some("A, B, C"));
    }

    #[test]
    fn artist_falls_back_to_album_artists() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.artists = vec![];
        p.track.album_artists = vec!["X".into(), "Y".into()];
        let track = track_of(content(Some(&p), &settings(), t0));
        assert_eq!(track.artist.as_deref(), Some("X, Y"));
    }

    #[test]
    fn artists_win_over_album_artists() {
        let t0 = Instant::now();
        let track = track_of(content(Some(&player(Playing, t0)), &settings(), t0));
        assert_eq!(track.artist.as_deref(), Some("Artist"));
    }

    #[test]
    fn blank_artist_names_are_skipped() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.artists = vec!["A".into(), " ".into(), String::new(), "B".into()];
        let track = track_of(content(Some(&p), &settings(), t0));
        assert_eq!(track.artist.as_deref(), Some("A, B"));
    }

    #[test]
    fn blank_artists_fall_back_to_album_artists() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.artists = vec![String::new()];
        let track = track_of(content(Some(&p), &settings(), t0));
        assert_eq!(track.artist.as_deref(), Some("Album Artist"));
    }

    #[test]
    fn no_artist_at_all_leaves_artist_empty() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.artists = vec![];
        p.track.album_artists = vec![" ".into()];
        let track = track_of(content(Some(&p), &settings(), t0));
        assert_eq!(track.artist, None);
    }

    #[test]
    fn japanese_metadata_comes_through_untouched() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.title = Some("愛をとりもどせ!!".into());
        p.track.album = Some("愛をとりもどせ!! (MOVIE ver.)".into());
        p.track.artists = vec![];
        p.track.album_artists = vec!["CRYSTAL KING".into()];
        let track = track_of(content(Some(&p), &settings(), t0));
        assert_eq!(track.title.as_deref(), Some("愛をとりもどせ!!"));
        assert_eq!(
            track.album.as_deref(),
            Some("愛をとりもどせ!! (MOVIE ver.)")
        );
        assert_eq!(track.artist.as_deref(), Some("CRYSTAL KING"));
    }

    // Times (B-14, B-15, B-16, B-43)

    #[test]
    fn position_is_truncated_to_whole_seconds() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.anchor.position = millis(10_900);
        let at = |offset| track_of(content(Some(&p), &settings(), t0 + offset)).position;
        assert_eq!(at(Duration::ZERO), Some(secs(10)));
        assert_eq!(at(millis(50)), Some(secs(10)));
        assert_eq!(at(millis(200)), Some(secs(11)));
    }

    #[test]
    fn duration_is_truncated_to_whole_seconds() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.length = Some(millis(180_999));
        let track = track_of(content(Some(&p), &settings(), t0));
        assert_eq!(track.duration, Some(secs(180)));
    }

    #[test]
    fn progress_is_not_truncated() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.anchor.position = millis(500);
        p.track.length = Some(secs(2));
        let track = track_of(content(Some(&p), &settings(), t0));
        assert_eq!(track.position, Some(Duration::ZERO));
        assert_eq!(track.progress, Some(0.25));
    }

    #[test]
    fn progress_starts_at_zero() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.anchor.position = Duration::ZERO;
        let track = track_of(content(Some(&p), &settings(), t0));
        assert_eq!(track.progress, Some(0.0));
    }

    #[test]
    fn progress_stops_at_one_past_the_end() {
        let t0 = Instant::now();
        let p = player(Playing, t0);
        let track = track_of(content(Some(&p), &settings(), t0 + secs(3600)));
        assert_eq!(track.position, Some(secs(180)));
        assert_eq!(track.progress, Some(1.0));
    }

    #[test]
    fn unknown_length_hides_duration_and_progress_but_not_position() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.length = None;
        let track = track_of(content(Some(&p), &settings(), t0 + secs(3600)));
        assert_eq!(track.position, Some(secs(3645)));
        assert_eq!(track.duration, None);
        assert_eq!(track.progress, None);
    }

    #[test]
    fn zero_length_is_treated_as_unknown() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.length = Some(Duration::ZERO);
        let track = track_of(content(Some(&p), &settings(), t0));
        assert_eq!(track.duration, None);
        assert_eq!(track.progress, None);
    }

    // Art (B-10)

    #[test]
    fn ready_art_is_shared_not_copied() {
        let t0 = Instant::now();
        let p = player(Playing, t0);
        let track = track_of(content(Some(&p), &settings(), t0));
        match (&p.art, &track.art) {
            (Art::Ready(stored), Some(shown)) => assert!(Arc::ptr_eq(stored, shown)),
            other => panic!("unexpected art {other:?}"),
        }
    }

    #[test]
    fn art_that_is_not_ready_is_not_shown() {
        let t0 = Instant::now();
        for art in [Art::None, Art::Pending, Art::Failed] {
            let mut p = player(Playing, t0);
            p.art = art.clone();
            let track = track_of(content(Some(&p), &settings(), t0));
            assert_eq!(track.art, None, "{art:?}");
        }
    }
}
