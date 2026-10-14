//! What a source has to draw, derived from the selected player and the settings.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::model::{Art, ArtImage, PlaybackStatus, Player, Service, Settings, Track};
use crate::position::display_position;
use crate::time::format_time;

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

/// A text or time field is `None` when its setting hides it or when the
/// player does not provide it: the layout treats both the same way.
///
/// `art` is the exception. The thumbnail slot is decided by
/// `settings.show_art` alone, and `art: None` only means nothing to draw in
/// it, so the layout does not jump while a cover is pending or after it
/// failed (B-10). The player's own cover comes first, then the one the
/// service the source picked found online (B-18).
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
    pub right_time: Option<RightTime>,
    /// Exact position over length, in `[0, 1]`.
    pub progress: Option<f32>,
    /// What to draw in the thumbnail slot, never whether the slot exists.
    pub art: Option<Arc<ArtImage>>,
}

/// The time at the right end of the time row (B-16, B-19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightTime {
    Duration(Duration),
    /// The duration minus the position, both as shown, so that it counts
    /// down at the moment the position counts up and the source draws once
    /// per second (B-43).
    Remaining(Duration),
}

impl RightTime {
    /// `m:ss` for the duration, `-m:ss` for the remaining time (B-19).
    pub fn text(self) -> String {
        match self {
            RightTime::Duration(duration) => format_time(duration),
            RightTime::Remaining(left) => format!("-{}", format_time(left)),
        }
    }
}

/// What to draw at `now` for `player`, the one picked by
/// [`crate::selection::select`].
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
    let shown_position = whole_seconds(position);
    let right_time = |length| {
        let duration = whole_seconds(length);
        if settings.show_remaining {
            RightTime::Remaining(duration.saturating_sub(shown_position))
        } else {
            RightTime::Duration(duration)
        }
    };
    TrackContent {
        paused: player.status == PlaybackStatus::Paused,
        title: settings.show_title.then(|| track.title.clone()).flatten(),
        artist: settings.show_artist.then(|| artist(track)).flatten(),
        album: settings.show_album.then(|| track.album.clone()).flatten(),
        position: settings.show_position.then_some(shown_position),
        right_time: settings
            .show_duration
            .then(|| track.length.map(right_time))
            .flatten(),
        progress: settings
            .show_progress
            .then(|| {
                track
                    .length
                    .map(|length| (position.as_secs_f64() / length.as_secs_f64()) as f32)
            })
            .flatten(),
        art: settings
            .show_art
            .then(|| match &player.art {
                Art::Ready(image) => Some(Arc::clone(image)),
                Art::None | Art::Failed => settings
                    .art_online
                    .and_then(|service| found_online(player, service)),
                Art::Pending => None,
            })
            .flatten(),
    }
}

/// The cover `service` found for the player's track, if it found one.
fn found_online(player: &Player, service: Service) -> Option<Arc<ArtImage>> {
    player
        .online
        .iter()
        .find(|(found_by, _)| *found_by == service)
        .map(|(_, image)| Arc::clone(image))
}

/// The artists of `track` as a source shows them, which online searches
/// ask for too (B-12).
pub fn artist(track: &Track) -> Option<String> {
    names(&track.artists).or_else(|| names(&track.album_artists))
}

/// An empty list gives `None`, so the album artist fallback applies (B-12).
fn names(list: &[String]) -> Option<String> {
    (!list.is_empty()).then(|| list.join(", "))
}

fn whole_seconds(duration: Duration) -> Duration {
    Duration::from_secs(duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PlayerKey, PositionAnchor, Track};

    use PlaybackStatus::{Paused, Playing, Stopped};

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn millis(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn with_fallback(text: &str) -> Settings {
        Settings {
            fallback_text: text.into(),
            ..Settings::default()
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
                url: None,
            },
            anchor: PositionAnchor {
                position: secs(45),
                at,
            },
            art: Art::Ready(image()),
            online: Vec::new(),
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
            track.right_time.is_some(),
            track.progress.is_some(),
            track.art.is_some(),
        ]
    }

    // Fallback (B-22, B-23)

    #[test]
    fn no_player_shows_the_fallback_message() {
        let s = Settings::default();
        assert_eq!(
            content(None, &s, Instant::now()),
            Content::Message(s.fallback_text.clone())
        );
    }

    #[test]
    fn stopped_player_shows_the_fallback_message() {
        let t0 = Instant::now();
        let s = Settings::default();
        assert_eq!(
            content(Some(&player(Stopped, t0)), &s, t0),
            Content::Message(s.fallback_text.clone())
        );
    }

    #[test]
    fn empty_fallback_text_draws_nothing() {
        let t0 = Instant::now();
        let s = with_fallback("");
        assert_eq!(content(None, &s, t0), Content::Nothing);
        assert_eq!(
            content(Some(&player(Stopped, t0)), &s, t0),
            Content::Nothing
        );
    }

    #[test]
    fn blank_fallback_text_draws_nothing() {
        let s = with_fallback(" \t\n ");
        assert_eq!(content(None, &s, Instant::now()), Content::Nothing);
    }

    #[test]
    fn fallback_text_is_shown_as_typed() {
        let s = with_fallback("Aucune lecture ♪");
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
            content(Some(&p), &Settings::default(), t0),
            Content::Track(TrackContent {
                paused: false,
                title: Some("Title".into()),
                artist: Some("Artist".into()),
                album: Some("Album".into()),
                position: Some(secs(45)),
                right_time: Some(RightTime::Duration(secs(180))),
                progress: Some(0.25),
                art: Some(image),
            })
        );
    }

    #[test]
    fn paused_player_shows_the_track_marked_paused() {
        let t0 = Instant::now();
        let track = track_of(content(Some(&player(Paused, t0)), &Settings::default(), t0));
        assert!(track.paused);
        assert_eq!(visible(&track), [true; 7]);
    }

    #[test]
    fn playing_position_and_progress_advance_with_time() {
        let t0 = Instant::now();
        let p = player(Playing, t0);
        let track = track_of(content(Some(&p), &Settings::default(), t0 + secs(45)));
        assert_eq!(track.position, Some(secs(90)));
        assert_eq!(track.progress, Some(0.5));
    }

    #[test]
    fn paused_position_and_progress_stay_frozen() {
        let t0 = Instant::now();
        let p = player(Paused, t0);
        let before = track_of(content(Some(&p), &Settings::default(), t0));
        let after = track_of(content(Some(&p), &Settings::default(), t0 + secs(60)));
        assert_eq!(after.position, Some(secs(45)));
        assert_eq!(after, before);
    }

    #[test]
    fn remembered_player_in_settings_does_not_change_the_content() {
        let t0 = Instant::now();
        let p = player(Playing, t0);
        let s = Settings {
            player: Some("org.mpris.MediaPlayer2.vlc".into()),
            ..Settings::default()
        };
        assert_eq!(
            content(Some(&p), &s, t0),
            content(Some(&p), &Settings::default(), t0)
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
            let mut s = Settings::default();
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
        let s = Settings {
            show_art: false,
            show_title: false,
            show_artist: false,
            show_album: false,
            show_progress: false,
            show_position: false,
            show_duration: false,
            ..Settings::default()
        };
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
        let track = track_of(content(Some(&p), &Settings::default(), t0));
        assert_eq!(
            track,
            TrackContent {
                paused: false,
                title: None,
                artist: None,
                album: None,
                position: Some(secs(12)),
                right_time: None,
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
        let track = track_of(content(Some(&p), &Settings::default(), t0));
        assert_eq!(track.artist.as_deref(), Some("A, B, C"));
    }

    #[test]
    fn artist_falls_back_to_album_artists() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.artists = vec![];
        p.track.album_artists = vec!["X".into(), "Y".into()];
        let track = track_of(content(Some(&p), &Settings::default(), t0));
        assert_eq!(track.artist.as_deref(), Some("X, Y"));
    }

    #[test]
    fn artists_win_over_album_artists() {
        let t0 = Instant::now();
        let track = track_of(content(
            Some(&player(Playing, t0)),
            &Settings::default(),
            t0,
        ));
        assert_eq!(track.artist.as_deref(), Some("Artist"));
    }

    #[test]
    fn no_artist_at_all_leaves_artist_empty() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.artists = vec![];
        p.track.album_artists = vec![];
        let track = track_of(content(Some(&p), &Settings::default(), t0));
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
        let track = track_of(content(Some(&p), &Settings::default(), t0));
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
        let at = |offset| track_of(content(Some(&p), &Settings::default(), t0 + offset)).position;
        assert_eq!(at(Duration::ZERO), Some(secs(10)));
        assert_eq!(at(millis(50)), Some(secs(10)));
        assert_eq!(at(millis(200)), Some(secs(11)));
    }

    #[test]
    fn duration_is_truncated_to_whole_seconds() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.length = Some(millis(180_999));
        let track = track_of(content(Some(&p), &Settings::default(), t0));
        assert_eq!(track.right_time, Some(RightTime::Duration(secs(180))));
    }

    #[test]
    fn progress_is_not_truncated() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.anchor.position = millis(500);
        p.track.length = Some(secs(2));
        let track = track_of(content(Some(&p), &Settings::default(), t0));
        assert_eq!(track.position, Some(Duration::ZERO));
        assert_eq!(track.progress, Some(0.25));
    }

    #[test]
    fn progress_starts_at_zero() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.anchor.position = Duration::ZERO;
        let track = track_of(content(Some(&p), &Settings::default(), t0));
        assert_eq!(track.progress, Some(0.0));
    }

    #[test]
    fn progress_stops_at_one_past_the_end() {
        let t0 = Instant::now();
        let p = player(Playing, t0);
        let track = track_of(content(Some(&p), &Settings::default(), t0 + secs(3600)));
        assert_eq!(track.position, Some(secs(180)));
        assert_eq!(track.progress, Some(1.0));
    }

    #[test]
    fn unknown_length_hides_duration_and_progress_but_not_position() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.length = None;
        let track = track_of(content(Some(&p), &Settings::default(), t0 + secs(3600)));
        assert_eq!(track.position, Some(secs(3645)));
        assert_eq!(track.right_time, None);
        assert_eq!(track.progress, None);
    }

    // Remaining time (B-19)

    fn remaining() -> Settings {
        Settings {
            show_remaining: true,
            ..Settings::default()
        }
    }

    #[test]
    fn remaining_time_replaces_the_duration() {
        let t0 = Instant::now();
        let track = track_of(content(Some(&player(Playing, t0)), &remaining(), t0));
        assert_eq!(track.right_time, Some(RightTime::Remaining(secs(135))));
        assert_eq!(track.position, Some(secs(45)));
    }

    #[test]
    fn remaining_time_counts_down_as_the_position_plays() {
        let t0 = Instant::now();
        let p = player(Playing, t0);
        let track = track_of(content(Some(&p), &remaining(), t0 + secs(60)));
        assert_eq!(track.right_time, Some(RightTime::Remaining(secs(75))));
    }

    #[test]
    fn remaining_time_ticks_with_the_position_shown() {
        // A length with a fraction of a second, so that an exact difference
        // would tick at another moment than the position.
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.length = Some(millis(180_900));
        p.anchor.position = millis(45_500);
        let at = |offset| {
            let track = track_of(content(Some(&p), &remaining(), t0 + offset));
            (track.position, track.right_time)
        };
        let shown = |position, left| (Some(secs(position)), Some(RightTime::Remaining(secs(left))));
        assert_eq!(at(Duration::ZERO), shown(45, 135));
        assert_eq!(at(millis(450)), shown(45, 135));
        assert_eq!(at(millis(550)), shown(46, 134));
    }

    #[test]
    fn remaining_time_is_zero_at_the_end() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.anchor.position = secs(180);
        let track = track_of(content(Some(&p), &remaining(), t0));
        assert_eq!(track.right_time, Some(RightTime::Remaining(Duration::ZERO)));
    }

    #[test]
    fn remaining_time_stays_zero_past_the_length() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.anchor.position = secs(500);
        for later in [Duration::ZERO, secs(3600)] {
            let track = track_of(content(Some(&p), &remaining(), t0 + later));
            assert_eq!(track.right_time, Some(RightTime::Remaining(Duration::ZERO)));
        }
    }

    #[test]
    fn remaining_time_stays_frozen_while_paused() {
        let t0 = Instant::now();
        let p = player(Paused, t0);
        let track = track_of(content(Some(&p), &remaining(), t0 + secs(60)));
        assert_eq!(track.right_time, Some(RightTime::Remaining(secs(135))));
    }

    #[test]
    fn remaining_time_needs_the_duration_shown() {
        let t0 = Instant::now();
        let s = Settings {
            show_duration: false,
            ..remaining()
        };
        let track = track_of(content(Some(&player(Playing, t0)), &s, t0));
        assert_eq!(track.right_time, None);
    }

    #[test]
    fn remaining_time_needs_a_known_length() {
        let t0 = Instant::now();
        let mut p = player(Playing, t0);
        p.track.length = None;
        let track = track_of(content(Some(&p), &remaining(), t0));
        assert_eq!(track.right_time, None);
        assert_eq!(track.position, Some(secs(45)));
    }

    #[test]
    fn remaining_time_does_not_need_the_position_shown() {
        let t0 = Instant::now();
        let s = Settings {
            show_position: false,
            ..remaining()
        };
        let track = track_of(content(Some(&player(Playing, t0)), &s, t0));
        assert_eq!(track.position, None);
        assert_eq!(track.right_time, Some(RightTime::Remaining(secs(135))));
    }

    #[test]
    fn right_time_text_marks_the_remaining_time_with_a_minus() {
        assert_eq!(RightTime::Duration(secs(116)).text(), "1:56");
        assert_eq!(RightTime::Remaining(secs(35)).text(), "-0:35");
        assert_eq!(RightTime::Remaining(Duration::ZERO).text(), "-0:00");
        assert_eq!(RightTime::Remaining(secs(3600)).text(), "-1:00:00");
    }

    // Art (B-10)

    #[test]
    fn ready_art_is_shared_not_copied() {
        let t0 = Instant::now();
        let p = player(Playing, t0);
        let track = track_of(content(Some(&p), &Settings::default(), t0));
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
            let track = track_of(content(Some(&p), &Settings::default(), t0));
            assert_eq!(track.art, None, "{art:?}");
        }
    }

    // Online cover (B-18)

    fn image_of(red: u8) -> Arc<ArtImage> {
        Arc::new(ArtImage {
            width: 1,
            height: 1,
            rgba: vec![red, 0, 0, 255],
        })
    }

    fn looking_up(service: Service) -> Settings {
        Settings {
            art_online: Some(service),
            ..Settings::default()
        }
    }

    /// A player whose own cover is `art`, and for which MusicBrainz found
    /// another.
    fn found_online(art: Art, at: Instant) -> (Player, Arc<ArtImage>) {
        let online = image_of(9);
        let mut p = player(Playing, at);
        p.art = art;
        p.online = vec![(Service::MusicBrainz, online.clone())];
        (p, online)
    }

    #[test]
    fn a_missing_or_unreadable_cover_gives_way_to_the_one_found_online() {
        let t0 = Instant::now();
        for art in [Art::None, Art::Failed] {
            let (p, online) = found_online(art.clone(), t0);
            let track = track_of(content(Some(&p), &looking_up(Service::MusicBrainz), t0));
            match track.art {
                Some(shown) => assert!(Arc::ptr_eq(&shown, &online), "{art:?}"),
                None => panic!("no art for {art:?}"),
            }
        }
    }

    #[test]
    fn the_players_own_cover_comes_first() {
        let t0 = Instant::now();
        let own = image_of(1);
        let (p, _) = found_online(Art::Ready(own.clone()), t0);
        let track = track_of(content(Some(&p), &looking_up(Service::MusicBrainz), t0));
        assert!(Arc::ptr_eq(&track.art.expect("art"), &own));
    }

    #[test]
    fn a_cover_found_online_shows_only_for_the_service_the_source_picked() {
        let t0 = Instant::now();
        let (p, _) = found_online(Art::None, t0);
        for settings in [Settings::default(), looking_up(Service::Wikipedia)] {
            let track = track_of(content(Some(&p), &settings, t0));
            assert_eq!(track.art, None, "{:?}", settings.art_online);
        }
    }

    #[test]
    fn a_cover_found_online_waits_for_the_players_own() {
        let t0 = Instant::now();
        let (p, _) = found_online(Art::Pending, t0);
        let track = track_of(content(Some(&p), &looking_up(Service::MusicBrainz), t0));
        assert_eq!(track.art, None);
    }

    #[test]
    fn a_hidden_thumbnail_hides_the_cover_found_online() {
        let t0 = Instant::now();
        let (p, _) = found_online(Art::None, t0);
        let settings = Settings {
            show_art: false,
            ..looking_up(Service::MusicBrainz)
        };
        assert_eq!(track_of(content(Some(&p), &settings, t0)).art, None);
    }
}
