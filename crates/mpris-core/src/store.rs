//! The single writer of player state: events in, snapshots out.
//!
//! A pure reducer. The D-Bus adapter turns bus signals into [`Event`]s and
//! every time comes from the event, so the store reads no clock and does no
//! I/O: each rule here stays deterministic and testable without a bus.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::model::{
    Art, ArtImage, PlaybackStatus, Player, PlayerKey, PositionAnchor, Snapshot, Track,
};
use crate::position::display_position;

/// Something that happened on the bus, already read and typed by the adapter.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A well-known name got an owner: a new player, or a restarted one
    /// under the same name.
    Appeared {
        unique_name: String,
        bus_name: String,
        key: PlayerKey,
        identity: String,
        status: PlaybackStatus,
        rate: f64,
        track: Track,
        position: Duration,
        at: Instant,
    },
    /// A well-known name lost its owner.
    Vanished { bus_name: String },
    /// `PropertiesChanged` from a player process. `None` means not reported.
    Changed {
        unique_name: String,
        status: Option<PlaybackStatus>,
        rate: Option<f64>,
        track: Option<Track>,
        identity: Option<String>,
        at: Instant,
    },
    /// A position read at `at`: `Seeked`, a periodic poll, or the re-read
    /// the adapter makes after every status change and every metadata
    /// change, where the store can only guess the position (B-17).
    PositionRead {
        unique_name: String,
        position: Duration,
        at: Instant,
    },
    /// Outcome of loading `url`. `None` when it could not be read or decoded.
    ArtLoaded {
        unique_name: String,
        url: String,
        image: Option<Arc<ArtImage>>,
    },
}

/// Every player present on the bus.
#[derive(Debug, Default)]
pub struct Store {
    /// Keyed by well-known name, since every instance of a player shares its
    /// key (B-04) and still needs its own entry. Ordered, so the same state
    /// always gives the same snapshot.
    players: BTreeMap<String, Player>,
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one event. Returns `true` when the state changed, so the
    /// adapter publishes a new snapshot only then.
    pub fn apply(&mut self, event: Event) -> bool {
        match event {
            Event::Appeared {
                unique_name,
                bus_name,
                key,
                identity,
                status,
                rate,
                track,
                position,
                at,
            } => {
                let player = Player {
                    unique_name,
                    bus_name,
                    key,
                    identity,
                    status,
                    rate: finite(rate).unwrap_or(1.0),
                    art: art_for(&track),
                    track,
                    anchor: PositionAnchor { position, at },
                };
                if self.players.get(&player.bus_name) == Some(&player) {
                    return false;
                }
                self.players.insert(player.bus_name.clone(), player);
                true
            }
            Event::Vanished { bus_name } => self.players.remove(&bus_name).is_some(),
            Event::Changed {
                unique_name,
                status,
                rate,
                track,
                identity,
                at,
            } => self.update(&unique_name, |player| {
                change(
                    player,
                    status,
                    rate,
                    track.as_ref(),
                    identity.as_deref(),
                    at,
                )
            }),
            Event::PositionRead {
                unique_name,
                position,
                at,
            } => self.update(&unique_name, |player| {
                player.anchor = PositionAnchor { position, at }
            }),
            Event::ArtLoaded {
                unique_name,
                url,
                image,
            } => self.update(&unique_name, |player| load_art(player, &url, image.clone())),
        }
    }

    /// Immutable copy of every player, ordered by bus name. Decoded images
    /// are shared, but each `art_url` is copied, and mpv puts its whole
    /// cover there as a base64 `data:` URI. One such string copy per
    /// published change is acceptable at the rate bus events arrive.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            players: self.players.values().cloned().collect(),
        }
    }

    /// Art the adapter has to load, as `(unique_name, url)`. Each pair comes
    /// once even when a process owns several names, since one
    /// [`Event::ArtLoaded`] serves all of them.
    pub fn pending_art(&self) -> Vec<(String, String)> {
        let mut pending: Vec<_> = self
            .players
            .values()
            .filter(|player| matches!(player.art, Art::Pending))
            .filter_map(|player| Some((player.unique_name.clone(), player.track.art_url.clone()?)))
            .collect();
        pending.sort();
        pending.dedup();
        pending
    }

    /// Edits every player owned by `unique_name`, since signals come from
    /// the process and one process may own several MPRIS names. Comparing
    /// whole players keeps the answer exact without each rule tracking what
    /// it touched.
    fn update(&mut self, unique_name: &str, mut edit: impl FnMut(&mut Player)) -> bool {
        let mut changed = false;
        for player in self.players.values_mut() {
            if player.unique_name == unique_name {
                let before = player.clone();
                edit(player);
                changed |= *player != before;
            }
        }
        changed
    }
}

fn change(
    player: &mut Player,
    status: Option<PlaybackStatus>,
    rate: Option<f64>,
    track: Option<&Track>,
    identity: Option<&str>,
    at: Instant,
) {
    let status = status.filter(|status| *status != player.status);
    let rate = rate.and_then(finite).filter(|rate| *rate != player.rate);
    // The anchor only holds for the status and rate it was taken under:
    // re-anchoring at the event time freezes the position on pause and keeps
    // it continuous across a rate change (B-17). A stop anchors at zero
    // because MPRIS restarts the track on the next Play.
    if status.is_some() || rate.is_some() {
        let position = match status {
            Some(PlaybackStatus::Stopped) => Duration::ZERO,
            _ => display_position(player, at),
        };
        player.anchor = PositionAnchor { position, at };
    }
    if let Some(status) = status {
        player.status = status;
    }
    if let Some(rate) = rate {
        player.rate = rate;
    }
    if let Some(identity) = identity {
        player.identity = identity.to_owned();
    }
    if let Some(track) = track {
        set_track(player, track, at);
    }
}

fn set_track(player: &mut Player, track: &Track, at: Instant) {
    if *track == player.track {
        return;
    }
    let new_track = is_new_track(&player.track, track);
    // The real position is unknown until the adapter re-reads it, and a
    // track starts at zero far more often than not.
    if new_track {
        player.anchor = PositionAnchor {
            position: Duration::ZERO,
            at,
        };
    }
    // A new track reloads its art even under the same url: some players
    // write each cover to one fixed path, and a cover that failed for the
    // previous track deserves another try.
    if new_track || track.art_url != player.track.art_url {
        player.art = art_for(track);
    }
    player.track = track.clone();
}

/// Players send metadata updates for the playing track too (length found,
/// art written to disk), and those must not reset the position. The id
/// alone cannot tell: some players keep one `mpris:trackid` for every track
/// (Firefox sends `/org/mpris/MediaPlayer2/firefox`), so the tags count too.
fn is_new_track(old: &Track, new: &Track) -> bool {
    old.id != new.id
        || old.title != new.title
        || old.album != new.album
        || old.artists != new.artists
}

/// Only an image for the url the player shows now is kept: a slow load for
/// a previous url must not overwrite the current one. A load started for
/// the previous track under the same url looks the same and is taken, a
/// race as short as one decode.
fn load_art(player: &mut Player, url: &str, image: Option<Arc<ArtImage>>) {
    if matches!(player.art, Art::Pending) && player.track.art_url.as_deref() == Some(url) {
        player.art = image.map_or(Art::Failed, Art::Ready);
    }
}

fn art_for(track: &Track) -> Art {
    match track.art_url {
        Some(_) => Art::Pending,
        None => Art::None,
    }
}

/// A non-finite rate is a player bug. Storing it would also make the player
/// unequal to itself (NaN), so every event would look like a change.
fn finite(rate: f64) -> Option<f64> {
    rate.is_finite().then_some(rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIQUE: &str = ":1.42";
    const BUS: &str = "org.mpris.MediaPlayer2.mpv";

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn track(id: &str, art_url: Option<&str>) -> Track {
        Track {
            id: Some(id.into()),
            title: Some(format!("Title of {id}")),
            artists: vec!["Artist".into()],
            album: Some("Album".into()),
            art_url: art_url.map(Into::into),
            ..Track::default()
        }
    }

    fn appeared(
        unique_name: &str,
        bus_name: &str,
        status: PlaybackStatus,
        track: Track,
        position: Duration,
        at: Instant,
    ) -> Event {
        Event::Appeared {
            unique_name: unique_name.into(),
            bus_name: bus_name.into(),
            key: PlayerKey("mpv".into()),
            identity: "mpv".into(),
            status,
            rate: 1.0,
            track,
            position,
            at,
        }
    }

    /// A playing mpv at 10 s on t0, with art to load.
    fn store_playing(t0: Instant) -> Store {
        let mut store = Store::new();
        let event = appeared(
            UNIQUE,
            BUS,
            PlaybackStatus::Playing,
            track("a", Some("file:///a.png")),
            secs(10),
            t0,
        );
        assert!(store.apply(event));
        store
    }

    fn changed(at: Instant) -> Event {
        Event::Changed {
            unique_name: UNIQUE.into(),
            status: None,
            rate: None,
            track: None,
            identity: None,
            at,
        }
    }

    fn status_changed(status: PlaybackStatus, at: Instant) -> Event {
        Event::Changed {
            unique_name: UNIQUE.into(),
            status: Some(status),
            rate: None,
            track: None,
            identity: None,
            at,
        }
    }

    fn rate_changed(rate: f64, at: Instant) -> Event {
        Event::Changed {
            unique_name: UNIQUE.into(),
            status: None,
            rate: Some(rate),
            track: None,
            identity: None,
            at,
        }
    }

    fn track_changed(track: Track, at: Instant) -> Event {
        Event::Changed {
            unique_name: UNIQUE.into(),
            status: None,
            rate: None,
            track: Some(track),
            identity: None,
            at,
        }
    }

    fn position_read(position: Duration, at: Instant) -> Event {
        Event::PositionRead {
            unique_name: UNIQUE.into(),
            position,
            at,
        }
    }

    fn art_loaded(url: &str, image: Option<Arc<ArtImage>>) -> Event {
        Event::ArtLoaded {
            unique_name: UNIQUE.into(),
            url: url.into(),
            image,
        }
    }

    fn image() -> Arc<ArtImage> {
        Arc::new(ArtImage {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
        })
    }

    fn player(store: &Store, bus_name: &str) -> Player {
        store
            .snapshot()
            .players
            .into_iter()
            .find(|p| p.bus_name == bus_name)
            .unwrap_or_else(|| panic!("no player {bus_name}"))
    }

    fn anchor(position: Duration, at: Instant) -> PositionAnchor {
        PositionAnchor { position, at }
    }

    // Construction and snapshot.

    #[test]
    fn new_store_is_empty() {
        let store = Store::new();
        assert_eq!(store.snapshot(), Snapshot::default());
        assert!(store.pending_art().is_empty());
        assert_eq!(Store::default().snapshot(), store.snapshot());
    }

    #[test]
    fn snapshot_is_ordered_by_bus_name() {
        let t0 = Instant::now();
        let mut store = Store::new();
        for bus in [
            "org.mpris.MediaPlayer2.vlc",
            "org.mpris.MediaPlayer2.chromium",
            "org.mpris.MediaPlayer2.mpv",
        ] {
            let event = appeared(
                bus,
                bus,
                PlaybackStatus::Paused,
                Track::default(),
                secs(0),
                t0,
            );
            store.apply(event);
        }
        let names: Vec<_> = store
            .snapshot()
            .players
            .into_iter()
            .map(|p| p.bus_name)
            .collect();
        assert_eq!(
            names,
            [
                "org.mpris.MediaPlayer2.chromium",
                "org.mpris.MediaPlayer2.mpv",
                "org.mpris.MediaPlayer2.vlc",
            ]
        );
    }

    #[test]
    fn snapshot_shares_images_instead_of_copying_them() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let image = image();
        store.apply(art_loaded("file:///a.png", Some(image.clone())));
        match player(&store, BUS).art {
            Art::Ready(shared) => assert!(Arc::ptr_eq(&shared, &image)),
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn snapshot_does_not_follow_later_events() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let before = store.snapshot();
        store.apply(status_changed(PlaybackStatus::Paused, t0 + secs(1)));
        assert_eq!(before.players[0].status, PlaybackStatus::Playing);
    }

    // Appeared.

    #[test]
    fn appeared_inserts_player_anchored_on_its_position() {
        let t0 = Instant::now();
        let store = store_playing(t0);
        let p = player(&store, BUS);
        assert_eq!(p.unique_name, UNIQUE);
        assert_eq!(p.key, PlayerKey("mpv".into()));
        assert_eq!(p.identity, "mpv");
        assert_eq!(p.status, PlaybackStatus::Playing);
        assert_eq!(p.rate, 1.0);
        assert_eq!(p.track, track("a", Some("file:///a.png")));
        assert_eq!(p.anchor, anchor(secs(10), t0));
        assert_eq!(p.art, Art::Pending);
    }

    #[test]
    fn appeared_without_art_url_has_no_art() {
        let t0 = Instant::now();
        let mut store = Store::new();
        store.apply(appeared(
            UNIQUE,
            BUS,
            PlaybackStatus::Paused,
            track("a", None),
            secs(0),
            t0,
        ));
        assert_eq!(player(&store, BUS).art, Art::None);
        assert!(store.pending_art().is_empty());
    }

    #[test]
    fn appeared_with_known_bus_name_replaces_the_player() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let restarted = appeared(
            ":1.99",
            BUS,
            PlaybackStatus::Paused,
            track("b", None),
            secs(3),
            t0 + secs(20),
        );
        assert!(store.apply(restarted));
        let snapshot = store.snapshot();
        assert_eq!(snapshot.players.len(), 1);
        let p = &snapshot.players[0];
        assert_eq!(p.unique_name, ":1.99");
        assert_eq!(p.status, PlaybackStatus::Paused);
        assert_eq!(p.track, track("b", None));
        assert_eq!(p.anchor, anchor(secs(3), t0 + secs(20)));
        assert_eq!(p.art, Art::None);
    }

    #[test]
    fn appeared_twice_identically_is_a_no_op() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let again = appeared(
            UNIQUE,
            BUS,
            PlaybackStatus::Playing,
            track("a", Some("file:///a.png")),
            secs(10),
            t0,
        );
        assert!(!store.apply(again));
    }

    #[test]
    fn appeared_with_non_finite_rate_falls_back_to_normal_speed() {
        let t0 = Instant::now();
        for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut store = Store::new();
            let event = Event::Appeared {
                unique_name: UNIQUE.into(),
                bus_name: BUS.into(),
                key: PlayerKey("mpv".into()),
                identity: "mpv".into(),
                status: PlaybackStatus::Playing,
                rate,
                track: Track::default(),
                position: secs(0),
                at: t0,
            };
            store.apply(event.clone());
            assert_eq!(player(&store, BUS).rate, 1.0, "rate {rate}");
            assert!(!store.apply(event), "rate {rate}");
        }
    }

    // Vanished.

    #[test]
    fn vanished_removes_the_player() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        assert!(store.apply(Event::Vanished {
            bus_name: BUS.into()
        }));
        assert!(store.snapshot().players.is_empty());
        assert!(store.pending_art().is_empty());
    }

    #[test]
    fn vanished_unknown_bus_name_is_a_no_op() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let before = store.snapshot();
        let event = Event::Vanished {
            bus_name: "org.mpris.MediaPlayer2.vlc".into(),
        };
        assert!(!store.apply(event));
        assert_eq!(store.snapshot(), before);
    }

    // Signals from an unknown process.

    #[test]
    fn events_from_unknown_unique_name_are_ignored() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let before = store.snapshot();
        let stranger = ":1.7".to_string();
        let events = [
            Event::Changed {
                unique_name: stranger.clone(),
                status: Some(PlaybackStatus::Paused),
                rate: Some(2.0),
                track: Some(track("b", None)),
                identity: Some("vlc".into()),
                at: t0 + secs(1),
            },
            Event::PositionRead {
                unique_name: stranger.clone(),
                position: secs(99),
                at: t0 + secs(1),
            },
            Event::ArtLoaded {
                unique_name: stranger,
                url: "file:///a.png".into(),
                image: Some(image()),
            },
        ];
        for event in events {
            assert!(!store.apply(event.clone()), "{event:?}");
        }
        assert_eq!(store.snapshot(), before);
    }

    // Changed: status and rate.

    #[test]
    fn pause_freezes_position_at_the_pause_time() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        assert!(store.apply(status_changed(PlaybackStatus::Paused, t0 + secs(5))));
        let p = player(&store, BUS);
        assert_eq!(p.status, PlaybackStatus::Paused);
        assert_eq!(p.anchor, anchor(secs(15), t0 + secs(5)));
        assert_eq!(display_position(&p, t0 + secs(60)), secs(15));
    }

    #[test]
    fn resume_continues_from_the_frozen_position() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        store.apply(status_changed(PlaybackStatus::Paused, t0 + secs(5)));
        assert!(store.apply(status_changed(PlaybackStatus::Playing, t0 + secs(30))));
        let p = player(&store, BUS);
        assert_eq!(p.anchor, anchor(secs(15), t0 + secs(30)));
        assert_eq!(display_position(&p, t0 + secs(32)), secs(17));
    }

    #[test]
    fn play_after_stop_restarts_the_track() {
        let t0 = Instant::now();
        let mut store = Store::new();
        store.apply(appeared(
            UNIQUE,
            BUS,
            PlaybackStatus::Playing,
            track("a", None),
            secs(100),
            t0,
        ));
        assert!(store.apply(status_changed(PlaybackStatus::Stopped, t0 + secs(5))));
        assert_eq!(player(&store, BUS).anchor, anchor(secs(0), t0 + secs(5)));
        assert!(store.apply(status_changed(PlaybackStatus::Playing, t0 + secs(60))));
        let p = player(&store, BUS);
        assert_eq!(p.anchor, anchor(secs(0), t0 + secs(60)));
        assert_eq!(display_position(&p, t0 + secs(62)), secs(2));
    }

    #[test]
    fn position_read_after_stop_wins_over_the_restart_guess() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        store.apply(status_changed(PlaybackStatus::Stopped, t0 + secs(5)));
        store.apply(position_read(secs(7), t0 + secs(6)));
        assert!(!store.apply(status_changed(PlaybackStatus::Stopped, t0 + secs(9))));
        assert!(!store.apply(rate_changed(1.0, t0 + secs(9))));
        assert_eq!(player(&store, BUS).anchor, anchor(secs(7), t0 + secs(6)));
    }

    #[test]
    fn rate_change_keeps_position_continuous() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        assert!(store.apply(rate_changed(2.0, t0 + secs(4))));
        let p = player(&store, BUS);
        assert_eq!(p.rate, 2.0);
        assert_eq!(p.anchor, anchor(secs(14), t0 + secs(4)));
        assert_eq!(display_position(&p, t0 + secs(6)), secs(18));
    }

    #[test]
    fn non_finite_rate_is_ignored() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let before = store.snapshot();
        for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                !store.apply(rate_changed(rate, t0 + secs(4))),
                "rate {rate}"
            );
        }
        assert_eq!(store.snapshot(), before);
    }

    #[test]
    fn unchanged_status_or_rate_keeps_the_anchor() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        assert!(!store.apply(status_changed(PlaybackStatus::Playing, t0 + secs(4))));
        assert!(!store.apply(rate_changed(1.0, t0 + secs(4))));
        assert_eq!(player(&store, BUS).anchor, anchor(secs(10), t0));
    }

    #[test]
    fn empty_change_is_a_no_op() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        assert!(!store.apply(changed(t0 + secs(4))));
    }

    // Changed: identity.

    #[test]
    fn identity_is_applied_when_given() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let rename = |identity: &str| Event::Changed {
            unique_name: UNIQUE.into(),
            status: None,
            rate: None,
            track: None,
            identity: Some(identity.into()),
            at: t0,
        };
        assert!(store.apply(rename("mpv Media Player")));
        assert_eq!(player(&store, BUS).identity, "mpv Media Player");
        assert!(!store.apply(rename("mpv Media Player")));
    }

    // Changed: track.

    #[test]
    fn new_track_resets_anchor_and_waits_for_its_art() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        store.apply(art_loaded("file:///a.png", Some(image())));
        let next = track("b", Some("file:///b.png"));
        assert!(store.apply(track_changed(next.clone(), t0 + secs(30))));
        let p = player(&store, BUS);
        assert_eq!(p.track, next);
        assert_eq!(p.anchor, anchor(Duration::ZERO, t0 + secs(30)));
        assert_eq!(p.art, Art::Pending);
        assert_eq!(
            store.pending_art(),
            [(UNIQUE.to_string(), "file:///b.png".to_string())]
        );
    }

    #[test]
    fn new_track_without_art_url_has_no_art() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        store.apply(track_changed(track("b", None), t0 + secs(30)));
        assert_eq!(player(&store, BUS).art, Art::None);
        assert!(store.pending_art().is_empty());
    }

    #[test]
    fn new_track_sharing_the_art_url_reloads_its_art() {
        let t0 = Instant::now();
        for loaded in [Some(image()), None] {
            let mut store = store_playing(t0);
            store.apply(art_loaded("file:///a.png", loaded.clone()));
            assert!(store.apply(track_changed(
                track("b", Some("file:///a.png")),
                t0 + secs(30),
            )));
            let p = player(&store, BUS);
            assert_eq!(p.anchor, anchor(Duration::ZERO, t0 + secs(30)));
            assert_eq!(p.art, Art::Pending, "after {loaded:?}");
            assert_eq!(
                store.pending_art(),
                [(UNIQUE.to_string(), "file:///a.png".to_string())]
            );
        }
    }

    #[test]
    fn same_track_with_the_same_art_url_does_not_retry_failed_art() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        store.apply(art_loaded("file:///a.png", None));
        let longer = Track {
            length: Some(secs(180)),
            ..track("a", Some("file:///a.png"))
        };
        assert!(store.apply(track_changed(longer, t0 + secs(30))));
        assert_eq!(player(&store, BUS).art, Art::Failed);
        assert!(store.pending_art().is_empty());
    }

    #[test]
    fn constant_track_id_still_tells_a_new_track_by_its_title() {
        let t0 = Instant::now();
        let firefox = |title: &str| Track {
            id: Some("/org/mpris/MediaPlayer2/firefox".into()),
            title: Some(title.into()),
            ..Track::default()
        };
        let mut store = Store::new();
        store.apply(appeared(
            UNIQUE,
            BUS,
            PlaybackStatus::Playing,
            firefox("A"),
            secs(200),
            t0,
        ));
        assert!(store.apply(track_changed(firefox("B"), t0 + secs(30))));
        assert_eq!(
            player(&store, BUS).anchor,
            anchor(Duration::ZERO, t0 + secs(30))
        );
    }

    #[test]
    fn title_album_or_artists_tell_a_new_track_with_or_without_ids() {
        let t0 = Instant::now();
        for id in [Some("a".to_string()), None] {
            let base = Track {
                id,
                ..track("a", None)
            };
            let variants = [
                Track {
                    title: Some("Other title".into()),
                    ..base.clone()
                },
                Track {
                    album: Some("Other album".into()),
                    ..base.clone()
                },
                Track {
                    artists: vec!["Other artist".into()],
                    ..base.clone()
                },
            ];
            for next in variants {
                let mut store = Store::new();
                store.apply(appeared(
                    UNIQUE,
                    BUS,
                    PlaybackStatus::Playing,
                    base.clone(),
                    secs(10),
                    t0,
                ));
                assert!(store.apply(track_changed(next.clone(), t0 + secs(30))));
                assert_eq!(
                    player(&store, BUS).anchor,
                    anchor(Duration::ZERO, t0 + secs(30)),
                    "{next:?}"
                );
            }
        }
    }

    #[test]
    fn id_appearing_or_disappearing_tells_a_new_track() {
        let t0 = Instant::now();
        let with_id = track("a", None);
        let without_id = Track {
            id: None,
            ..with_id.clone()
        };
        for (from, to) in [(&with_id, &without_id), (&without_id, &with_id)] {
            let mut store = Store::new();
            store.apply(appeared(
                UNIQUE,
                BUS,
                PlaybackStatus::Playing,
                from.clone(),
                secs(10),
                t0,
            ));
            store.apply(track_changed(to.clone(), t0 + secs(30)));
            assert_eq!(
                player(&store, BUS).anchor,
                anchor(Duration::ZERO, t0 + secs(30)),
                "{from:?} -> {to:?}"
            );
        }
    }

    #[test]
    fn same_track_update_keeps_anchor_and_art() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let image = image();
        store.apply(art_loaded("file:///a.png", Some(image.clone())));
        let longer = Track {
            length: Some(secs(180)),
            ..track("a", Some("file:///a.png"))
        };
        assert!(store.apply(track_changed(longer.clone(), t0 + secs(30))));
        let p = player(&store, BUS);
        assert_eq!(p.track, longer);
        assert_eq!(p.anchor, anchor(secs(10), t0));
        assert_eq!(p.art, Art::Ready(image));
    }

    #[test]
    fn same_track_without_ids_keeps_anchor() {
        let t0 = Instant::now();
        let base = Track {
            id: None,
            ..track("a", None)
        };
        let mut store = Store::new();
        store.apply(appeared(
            UNIQUE,
            BUS,
            PlaybackStatus::Playing,
            base.clone(),
            secs(10),
            t0,
        ));
        let longer = Track {
            length: Some(secs(180)),
            album_artists: vec!["Band".into()],
            ..base
        };
        assert!(store.apply(track_changed(longer, t0 + secs(30))));
        assert_eq!(player(&store, BUS).anchor, anchor(secs(10), t0));
    }

    #[test]
    fn art_url_arriving_on_same_track_waits_for_its_art() {
        let t0 = Instant::now();
        let mut store = Store::new();
        store.apply(appeared(
            UNIQUE,
            BUS,
            PlaybackStatus::Playing,
            track("a", None),
            secs(10),
            t0,
        ));
        assert!(store.apply(track_changed(
            track("a", Some("file:///a.png")),
            t0 + secs(2)
        )));
        let p = player(&store, BUS);
        assert_eq!(p.anchor, anchor(secs(10), t0));
        assert_eq!(p.art, Art::Pending);
    }

    #[test]
    fn art_url_changing_on_same_track_reloads_the_art() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        store.apply(art_loaded("file:///a.png", Some(image())));
        store.apply(track_changed(
            track("a", Some("file:///a-big.png")),
            t0 + secs(2),
        ));
        assert_eq!(player(&store, BUS).art, Art::Pending);
        assert_eq!(
            store.pending_art(),
            [(UNIQUE.to_string(), "file:///a-big.png".to_string())]
        );
    }

    #[test]
    fn art_url_removal_drops_the_art() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        store.apply(art_loaded("file:///a.png", Some(image())));
        assert!(store.apply(track_changed(track("a", None), t0 + secs(2))));
        assert_eq!(player(&store, BUS).art, Art::None);
        assert!(store.pending_art().is_empty());
    }

    #[test]
    fn identical_track_is_a_no_op() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let same = track("a", Some("file:///a.png"));
        assert!(!store.apply(track_changed(same, t0 + secs(30))));
        assert_eq!(player(&store, BUS).anchor, anchor(secs(10), t0));
    }

    #[test]
    fn status_and_new_track_together_start_the_track_at_zero() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let event = Event::Changed {
            unique_name: UNIQUE.into(),
            status: Some(PlaybackStatus::Paused),
            rate: None,
            track: Some(track("b", None)),
            identity: None,
            at: t0 + secs(30),
        };
        assert!(store.apply(event));
        let p = player(&store, BUS);
        assert_eq!(p.status, PlaybackStatus::Paused);
        assert_eq!(p.anchor, anchor(Duration::ZERO, t0 + secs(30)));
    }

    // PositionRead.

    #[test]
    fn position_read_moves_the_anchor() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        assert!(store.apply(position_read(secs(42), t0 + secs(3))));
        assert_eq!(player(&store, BUS).anchor, anchor(secs(42), t0 + secs(3)));
        assert!(!store.apply(position_read(secs(42), t0 + secs(3))));
    }

    // ArtLoaded.

    #[test]
    fn art_loaded_makes_the_art_ready() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let image = image();
        assert!(store.apply(art_loaded("file:///a.png", Some(image.clone()))));
        assert_eq!(player(&store, BUS).art, Art::Ready(image));
        assert!(store.pending_art().is_empty());
        assert!(!store.apply(art_loaded("file:///a.png", Some(self::image()))));
    }

    #[test]
    fn art_load_failure_is_kept_and_not_retried() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        assert!(store.apply(art_loaded("file:///a.png", None)));
        assert_eq!(player(&store, BUS).art, Art::Failed);
        assert!(store.pending_art().is_empty());
        assert!(!store.apply(art_loaded("file:///a.png", Some(image()))));
        assert_eq!(player(&store, BUS).art, Art::Failed);
    }

    #[test]
    fn stale_art_for_a_previous_track_is_dropped() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        store.apply(track_changed(
            track("b", Some("file:///b.png")),
            t0 + secs(30),
        ));
        assert!(!store.apply(art_loaded("file:///a.png", Some(image()))));
        assert_eq!(player(&store, BUS).art, Art::Pending);
        assert!(!store.apply(art_loaded("file:///a.png", None)));
        assert_eq!(player(&store, BUS).art, Art::Pending);
    }

    #[test]
    fn art_loaded_for_a_player_without_art_url_is_dropped() {
        let t0 = Instant::now();
        let mut store = Store::new();
        store.apply(appeared(
            UNIQUE,
            BUS,
            PlaybackStatus::Playing,
            track("a", None),
            secs(0),
            t0,
        ));
        assert!(!store.apply(art_loaded("file:///a.png", Some(image()))));
        assert_eq!(player(&store, BUS).art, Art::None);
    }

    // One process owning several MPRIS names.

    fn store_with_two_names(t0: Instant) -> Store {
        let mut store = store_playing(t0);
        let second = appeared(
            UNIQUE,
            "org.mpris.MediaPlayer2.mpv.instance2",
            PlaybackStatus::Playing,
            track("a", Some("file:///a.png")),
            secs(10),
            t0,
        );
        store.apply(second);
        store
    }

    #[test]
    fn signals_reach_every_name_of_the_process() {
        let t0 = Instant::now();
        let mut store = store_with_two_names(t0);
        assert!(store.apply(status_changed(PlaybackStatus::Paused, t0 + secs(5))));
        assert!(store.apply(position_read(secs(20), t0 + secs(6))));
        let image = image();
        assert!(store.apply(art_loaded("file:///a.png", Some(image.clone()))));
        for p in store.snapshot().players {
            assert_eq!(p.status, PlaybackStatus::Paused, "{}", p.bus_name);
            assert_eq!(p.anchor, anchor(secs(20), t0 + secs(6)), "{}", p.bus_name);
            assert_eq!(p.art, Art::Ready(image.clone()), "{}", p.bus_name);
        }
    }

    #[test]
    fn pending_art_lists_each_load_once_per_process() {
        let t0 = Instant::now();
        let store = store_with_two_names(t0);
        assert_eq!(
            store.pending_art(),
            [(UNIQUE.to_string(), "file:///a.png".to_string())]
        );
    }

    #[test]
    fn pending_art_lists_only_players_waiting_for_art() {
        let t0 = Instant::now();
        let mut store = store_playing(t0);
        let others = [
            (":1.50", "org.mpris.MediaPlayer2.vlc", Some("file:///v.png")),
            (":1.60", "org.mpris.MediaPlayer2.chromium", None),
        ];
        for (unique, bus, art_url) in others {
            let event = appeared(
                unique,
                bus,
                PlaybackStatus::Paused,
                track("x", art_url),
                secs(0),
                t0,
            );
            store.apply(event);
        }
        store.apply(art_loaded("file:///a.png", Some(image())));
        assert_eq!(
            store.pending_art(),
            [(":1.50".to_string(), "file:///v.png".to_string())]
        );
    }

    #[test]
    fn vanishing_one_name_keeps_the_others_of_the_process() {
        let t0 = Instant::now();
        let mut store = store_with_two_names(t0);
        store.apply(Event::Vanished {
            bus_name: BUS.into(),
        });
        let snapshot = store.snapshot();
        assert_eq!(snapshot.players.len(), 1);
        assert_eq!(
            snapshot.players[0].bus_name,
            "org.mpris.MediaPlayer2.mpv.instance2"
        );
        assert!(store.apply(status_changed(PlaybackStatus::Paused, t0 + secs(1))));
    }
}
