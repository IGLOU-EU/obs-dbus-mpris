//! Which player a source shows (B-05 to B-08) and the selector entries
//! (B-02, B-03).

use std::cmp::Ordering;

use crate::model::{PlaybackStatus, Player, Settings};
use crate::player::{key_str, player_key};

/// What a source shows on one frame, and the player it follows from then on.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection<'a> {
    /// `None` for the fallback message (B-23).
    pub player: Option<&'a Player>,
    /// Bus name of the followed player (B-08), handed back on the next frame.
    pub followed: Option<String>,
}

/// What a source shows on this frame (B-05), given `followed`, what the
/// previous frame returned, `None` for a new source or one whose choice
/// just changed (B-08). The shown player is never stored: callers
/// re-evaluate it on every frame, which is what lets a source find its
/// player again once it comes back under a new instance name (B-06).
///
/// Automatic selection serves the start, when nothing is chosen yet or the
/// chosen player is not launched yet. Once a player shows, the source keeps
/// to it, so that another player starting to play, a video opened by
/// accident for one, never takes over the overlay. The remembered instance
/// wins whatever its status, because an explicit choice outranks the
/// activity of other players. With automatic selection off it is the only
/// candidate, so a source dedicated to one player never shows another.
pub fn select<'a>(
    players: &'a [Player],
    settings: &Settings,
    followed: Option<&str>,
) -> Selection<'a> {
    let player = shown(players, settings, followed);
    // Kept while nothing shows, so that the relaunch of the followed player
    // is found (step 3), and no other player is latched meanwhile (step 4).
    let followed = player.map(|p| p.bus_name.as_str()).or(followed);
    Selection {
        player,
        followed: followed.map(str::to_owned),
    }
}

/// Whether going from `before` to `after` changes the choice of player,
/// which starts the followed player over, as for a new source (B-08).
pub fn choice_changed(before: &Settings, after: &Settings) -> bool {
    (&before.player, before.auto_select) != (&after.player, after.auto_select)
}

/// The steps of B-05, in their order.
fn shown<'a>(
    players: &'a [Player],
    settings: &Settings,
    followed: Option<&str>,
) -> Option<&'a Player> {
    let remembered = settings.player.as_deref();
    if !settings.auto_select {
        return remembered.and_then(|name| exact(players, name));
    }
    let chosen = remembered.and_then(|name| itself_or_relaunch(players, name));
    match followed {
        Some(followed) => chosen.or_else(|| itself_or_relaunch(players, followed)),
        // A paused or stopped player is never latched: only playing tells
        // which player the user means.
        None => chosen.or_else(|| first(players.iter().filter(|p| is_playing(p)))),
    }
}

fn exact<'a>(players: &'a [Player], bus_name: &str) -> Option<&'a Player> {
    players.iter().find(|p| p.bus_name == bus_name)
}

/// The instance named `bus_name`, or once it is gone, the instance that
/// stands for it: one of the same key, playing preferably (B-06).
fn itself_or_relaunch<'a>(players: &'a [Player], bus_name: &str) -> Option<&'a Player> {
    exact(players, bus_name).or_else(|| {
        let key = key_str(bus_name)?;
        let same_key = players.iter().filter(|p| p.key.0 == key);
        first(same_key.clone().filter(|p| is_playing(p))).or_else(|| first(same_key))
    })
}

fn first<'a>(players: impl Iterator<Item = &'a Player>) -> Option<&'a Player> {
    players.min_by(|a, b| by_rank(&a.bus_name, &b.bus_name))
}

/// What "first" means in B-05, for the selection and the selector alike.
/// Keys come first so the order stays the same from one session to the
/// next, and the bus name settles the rest so the order players appeared in
/// never matters. Case is ignored because players capitalize their names
/// at will, and a capital must not move a player ahead of the others. The
/// exact bytes only break the tie between names that differ by case alone,
/// so the order stays fixed.
///
/// Both are read from the bus name alone, so a remembered instance gone
/// from the bus still ranks among the others, and a remembered text that is
/// not an MPRIS name, having no key, ranks first.
fn by_rank(a: &str, b: &str) -> Ordering {
    let (key_a, key_b) = (key_str(a), key_str(b));
    key_a
        .is_some()
        .cmp(&key_b.is_some())
        .then_with(|| ignoring_case(key_a.unwrap_or_default(), key_b.unwrap_or_default()))
        .then_with(|| ignoring_case(a, b))
        .then_with(|| a.cmp(b))
}

fn ignoring_case(a: &str, b: &str) -> Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

fn is_playing(player: &Player) -> bool {
    player.status == PlaybackStatus::Playing
}

/// One entry of the player selector (B-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// What the setting remembers when this entry is picked (B-03).
    pub bus_name: String,
    pub label: String,
    /// `false` for a remembered instance that is not on the bus (B-03).
    pub available: bool,
}

/// Selector entries, one per instance, in the order of B-05. The title
/// labels an instance because it is what tells several instances of one
/// player apart (B-02). A remembered instance missing from the bus keeps
/// its entry so the user's choice is not lost when the player is closed
/// (B-03).
pub fn player_choices(players: &[Player], remembered: Option<&str>) -> Vec<Choice> {
    let present = players.iter().map(|p| Choice {
        bus_name: p.bus_name.clone(),
        label: p.track.title.clone().unwrap_or_else(|| p.identity.clone()),
        available: true,
    });
    let gone = remembered
        .filter(|name| !players.iter().any(|p| p.bus_name == *name))
        .map(|name| Choice {
            bus_name: name.to_owned(),
            label: player_key(name).map_or_else(|| name.to_owned(), |key| key.0),
            available: false,
        });
    let mut choices: Vec<Choice> = present.chain(gone).collect();
    choices.sort_by(|a, b| by_rank(&a.bus_name, &b.bus_name));
    choices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Art, PlaybackStatus, PlayerKey, PositionAnchor, Track};
    use std::time::{Duration, Instant};

    use PlaybackStatus::{Paused, Playing, Stopped};

    fn key(k: &str) -> PlayerKey {
        PlayerKey(k.into())
    }

    /// An instance of player `k`, with an instance suffix when not empty.
    fn instance(k: &str, suffix: &str, status: PlaybackStatus) -> Player {
        let bus_name = if suffix.is_empty() {
            format!("org.mpris.MediaPlayer2.{k}")
        } else {
            format!("org.mpris.MediaPlayer2.{k}.{suffix}")
        };
        Player {
            unique_name: format!(":1.{}", bus_name.len()),
            bus_name,
            key: key(k),
            identity: format!("{k} identity"),
            status,
            rate: 1.0,
            track: Track::default(),
            anchor: PositionAnchor {
                position: Duration::ZERO,
                at: Instant::now(),
            },
            art: Art::None,
        }
    }

    fn player(k: &str, status: PlaybackStatus) -> Player {
        instance(k, "", status)
    }

    /// The full bus name of `name`, as the selection returns it.
    fn bus(name: &str) -> Option<String> {
        Some(format!("org.mpris.MediaPlayer2.{name}"))
    }

    /// What a source shows for `players`, and the player it follows next.
    fn step<'a>(
        players: &'a [Player],
        remembered: Option<&str>,
        auto_select: bool,
        followed: Option<&str>,
    ) -> (Option<&'a str>, Option<String>) {
        let settings = Settings {
            player: remembered.map(str::to_owned),
            auto_select,
            ..Settings::default()
        };
        let selection = select(players, &settings, followed);
        (
            selection.player.map(|p| p.bus_name.as_str()),
            selection.followed,
        )
    }

    /// What a source that follows nothing yet shows, automatic selection on.
    fn selected<'a>(players: &'a [Player], remembered: Option<&str>) -> Option<&'a str> {
        step(players, remembered, true, None).0
    }

    /// What a source that follows `followed` shows, automatic selection on.
    fn selected_following<'a>(
        players: &'a [Player],
        remembered: Option<&str>,
        followed: &str,
    ) -> Option<&'a str> {
        step(players, remembered, true, Some(followed)).0
    }

    /// Selection with automatic selection off.
    fn selected_strictly<'a>(players: &'a [Player], remembered: Option<&str>) -> Option<&'a str> {
        step(players, remembered, false, None).0
    }

    /// What a source shows at each of `frames`, the players on the bus at
    /// successive moments, handing the followed player from one frame to
    /// the next as the OBS glue does. Every frame is also read reversed, so
    /// a result that leaks the input order shows up.
    fn shown_over(
        frames: &[Vec<Player>],
        remembered: Option<&str>,
        auto_select: bool,
    ) -> Vec<Option<String>> {
        let run = |reversed: bool| {
            let mut followed: Option<String> = None;
            let mut shown = Vec::new();
            for frame in frames {
                let mut players = frame.clone();
                if reversed {
                    players.reverse();
                }
                let (player, next) = step(&players, remembered, auto_select, followed.as_deref());
                shown.push(player.map(str::to_owned));
                followed = next;
            }
            shown
        };
        let shown = run(false);
        assert_eq!(run(true), shown, "the order of the players matters");
        shown
    }

    /// Every rotation of `players` and of its reverse, so a result that
    /// leaks the input order shows up.
    fn orders(players: &[Player]) -> Vec<Vec<Player>> {
        let reversed: Vec<Player> = players.iter().rev().cloned().collect();
        let mut all = Vec::new();
        for base in [players.to_vec(), reversed] {
            for shift in 0..base.len() {
                let mut order = base.clone();
                order.rotate_left(shift);
                all.push(order);
            }
        }
        all
    }

    // A source keeps to what it shows (B-05)

    #[test]
    fn a_player_that_starts_playing_never_takes_over_the_shown_one() {
        // Music in mpv, then a video started in Chromium, which ranks first.
        let mpv = |status| instance("mpv", "instance-a", status);
        let frames = [
            vec![mpv(Playing)],
            vec![mpv(Playing), player("chromium", Playing)],
            vec![mpv(Paused), player("chromium", Playing)],
            vec![mpv(Stopped), player("chromium", Playing)],
        ];
        for remembered in [bus("mpv.instance-a"), None] {
            assert_eq!(
                shown_over(&frames, remembered.as_deref(), true),
                vec![bus("mpv.instance-a"); 4],
                "{remembered:?}"
            );
        }
    }

    #[test]
    fn a_gone_remembered_player_leaves_the_fallback_until_it_is_relaunched() {
        let frames = [
            vec![
                instance("mpv", "instance-a", Playing),
                player("chromium", Paused),
            ],
            vec![player("chromium", Playing)],
            vec![player("chromium", Playing), player("vlc", Playing)],
            vec![
                player("chromium", Playing),
                instance("mpv", "instance-b", Stopped),
            ],
            vec![
                player("chromium", Playing),
                instance("mpv", "instance-b", Playing),
            ],
        ];
        assert_eq!(
            shown_over(&frames, bus("mpv.instance-a").as_deref(), true),
            [
                bus("mpv.instance-a"),
                None,
                None,
                bus("mpv.instance-b"),
                bus("mpv.instance-b"),
            ]
        );
    }

    #[test]
    fn without_a_remembered_player_the_first_to_play_is_kept() {
        let frames = [
            vec![player("vlc", Playing)],
            vec![player("vlc", Playing), player("audacious", Playing)],
            vec![player("vlc", Paused), player("audacious", Playing)],
            vec![player("vlc", Stopped), player("audacious", Playing)],
        ];
        assert_eq!(shown_over(&frames, None, true), vec![bus("vlc"); 4]);
    }

    #[test]
    fn a_vanished_followed_player_leaves_the_fallback_until_it_is_relaunched() {
        let frames = [
            vec![instance("vlc", "instance-a", Playing)],
            vec![player("chromium", Playing)],
            vec![
                player("chromium", Playing),
                instance("vlc", "instance-b", Paused),
            ],
            vec![
                player("chromium", Playing),
                instance("vlc", "instance-b", Playing),
            ],
        ];
        assert_eq!(
            shown_over(&frames, None, true),
            [
                bus("vlc.instance-a"),
                None,
                bus("vlc.instance-b"),
                bus("vlc.instance-b"),
            ]
        );
    }

    #[test]
    fn a_paused_or_stopped_player_is_never_latched_at_start() {
        let frames = [
            vec![player("mpv", Paused), player("vlc", Stopped)],
            vec![player("mpv", Paused), player("vlc", Stopped)],
            vec![player("mpv", Paused), player("vlc", Playing)],
            vec![player("mpv", Playing), player("vlc", Paused)],
        ];
        assert_eq!(
            shown_over(&frames, None, true),
            [None, None, bus("vlc"), bus("vlc")]
        );
    }

    #[test]
    fn a_player_standing_in_for_the_remembered_one_gives_way_on_its_return() {
        let frames = [
            vec![player("chromium", Playing), player("vlc", Paused)],
            vec![
                player("chromium", Playing),
                instance("mpv", "instance-b", Paused),
            ],
            vec![player("chromium", Playing)],
        ];
        assert_eq!(
            shown_over(&frames, bus("mpv.instance-a").as_deref(), true),
            [bus("chromium"), bus("mpv.instance-b"), None]
        );
    }

    #[test]
    fn strict_never_goes_back_to_a_relaunched_player() {
        let frames = [
            vec![instance("mpv", "instance-a", Playing)],
            vec![player("chromium", Playing)],
            vec![
                player("chromium", Playing),
                instance("mpv", "instance-b", Playing),
            ],
        ];
        assert_eq!(
            shown_over(&frames, bus("mpv.instance-a").as_deref(), false),
            [bus("mpv.instance-a"), None, None]
        );
    }

    // The followed player (B-08)

    #[test]
    fn the_shown_player_becomes_the_followed_one() {
        let players = [
            instance("mpv", "instance-a", Paused),
            player("vlc", Playing),
        ];
        for order in orders(&players) {
            let remembered = bus("mpv.instance-a");
            assert_eq!(
                step(&order, remembered.as_deref(), true, bus("vlc").as_deref()).1,
                bus("mpv.instance-a"),
                "{order:?}"
            );
            assert_eq!(step(&order, None, true, None).1, bus("vlc"), "{order:?}");
            assert_eq!(
                step(&order, remembered.as_deref(), false, None).1,
                bus("mpv.instance-a"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn while_nothing_shows_the_followed_player_is_kept() {
        let players = [player("chromium", Playing)];
        assert_eq!(
            step(&players, None, true, bus("vlc").as_deref()),
            (None, bus("vlc"))
        );
        assert_eq!(step(&[], None, true, None), (None, None));
        assert_eq!(
            step(&[player("vlc", Paused)], None, true, None),
            (None, None)
        );
    }

    #[test]
    fn a_new_remembered_player_or_switch_is_a_new_choice() {
        let nothing = Settings::default();
        let mpv = Settings {
            player: bus("mpv.instance-a"),
            ..Settings::default()
        };
        let vlc = Settings {
            player: bus("vlc"),
            ..Settings::default()
        };
        let strict = Settings {
            auto_select: false,
            ..Settings::default()
        };
        for (before, after) in [
            (&nothing, &mpv),
            (&mpv, &nothing),
            (&mpv, &vlc),
            (&nothing, &strict),
            (&strict, &nothing),
        ] {
            assert!(choice_changed(before, after), "{before:?} {after:?}");
        }
    }

    #[test]
    fn other_settings_keep_the_choice() {
        let mpv = Settings {
            player: bus("mpv.instance-a"),
            ..Settings::default()
        };
        let restyled = Settings {
            width: 900,
            show_art: false,
            scroll_speed: 80,
            fallback_text: "Rien".into(),
            ..mpv.clone()
        };
        assert!(!choice_changed(&mpv, &mpv.clone()));
        assert!(!choice_changed(&mpv, &restyled));
        assert!(!choice_changed(&Settings::default(), &Settings::default()));
    }

    // Step 1: the remembered instance

    #[test]
    fn remembered_instance_wins_whatever_its_status() {
        for status in [Stopped, Paused] {
            let players = [
                player("chromium", Playing),
                instance("mpv", "instance-a", Playing),
                instance("mpv", "instance-b", status),
                player("vlc", Playing),
            ];
            let remembered = bus("mpv.instance-b");
            for order in orders(&players) {
                assert_eq!(
                    selected(&order, remembered.as_deref()),
                    remembered.as_deref(),
                    "{status:?} {order:?}"
                );
                assert_eq!(
                    selected_following(&order, remembered.as_deref(), "org.mpris.MediaPlayer2.vlc"),
                    remembered.as_deref(),
                    "{status:?} {order:?}"
                );
            }
        }
    }

    #[test]
    fn remembered_instance_without_suffix_wins_when_stopped() {
        let players = [player("chromium", Playing), player("mpv", Stopped)];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, Some("org.mpris.MediaPlayer2.mpv")),
                Some("org.mpris.MediaPlayer2.mpv"),
                "{order:?}"
            );
        }
    }

    // Step 2: another instance of the remembered player

    #[test]
    fn gone_remembered_instance_gives_way_to_a_playing_instance_of_its_key() {
        let players = [
            player("chromium", Playing),
            instance("mpv", "instance-a", Paused),
            instance("mpv", "instance-b", Playing),
            instance("mpv", "instance-c", Stopped),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, Some("org.mpris.MediaPlayer2.mpv.instance-gone")),
                Some("org.mpris.MediaPlayer2.mpv.instance-b"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn gone_remembered_instance_gives_way_to_the_first_instance_of_its_key() {
        let players = [
            player("chromium", Playing),
            instance("mpv", "instance-c", Paused),
            instance("mpv", "instance-b", Stopped),
            instance("mpv", "instance-d", Paused),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
                Some("org.mpris.MediaPlayer2.mpv.instance-b"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn restarted_player_is_shown_under_its_new_instance_name() {
        let players = [
            player("chromium", Playing),
            instance("mpv", "instance-Qx81bZcd", Stopped),
        ];
        assert_eq!(
            selected(
                &players,
                Some("org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA")
            ),
            Some("org.mpris.MediaPlayer2.mpv.instance-Qx81bZcd")
        );
    }

    #[test]
    fn remembered_name_without_suffix_takes_any_instance_of_its_key() {
        let players = [
            player("chromium", Playing),
            instance("mpv", "instance-a", Paused),
        ];
        assert_eq!(
            selected(&players, Some("org.mpris.MediaPlayer2.mpv")),
            Some("org.mpris.MediaPlayer2.mpv.instance-a")
        );
    }

    #[test]
    fn an_instance_of_the_remembered_player_outranks_the_followed_one() {
        let players = [
            player("chromium", Playing),
            instance("mpv", "instance-b", Paused),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected_following(
                    &order,
                    Some("org.mpris.MediaPlayer2.mpv.instance-a"),
                    "org.mpris.MediaPlayer2.chromium"
                ),
                Some("org.mpris.MediaPlayer2.mpv.instance-b"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn a_key_that_merely_starts_like_the_remembered_one_is_another_player() {
        let players = [player("mpvx", Paused), player("vlc", Playing)];
        assert_eq!(
            selected(&players, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
            Some("org.mpris.MediaPlayer2.vlc")
        );
    }

    // Step 3: the followed player, or its relaunch

    #[test]
    fn the_followed_player_stays_whatever_its_status_while_others_play() {
        for status in [Playing, Paused, Stopped] {
            let players = [
                player("audacious", Playing),
                player("vlc", status),
                player("chromium", Playing),
            ];
            for remembered in [None, Some("org.mpris.MediaPlayer2.mpv.instance-gone")] {
                for order in orders(&players) {
                    assert_eq!(
                        selected_following(&order, remembered, "org.mpris.MediaPlayer2.vlc"),
                        Some("org.mpris.MediaPlayer2.vlc"),
                        "{status:?} {remembered:?} {order:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn gone_followed_instance_gives_way_to_a_playing_instance_of_its_key() {
        let players = [
            player("audacious", Playing),
            instance("mpv", "instance-a", Paused),
            instance("mpv", "instance-b", Playing),
            instance("mpv", "instance-c", Stopped),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected_following(&order, None, "org.mpris.MediaPlayer2.mpv.instance-gone"),
                Some("org.mpris.MediaPlayer2.mpv.instance-b"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn gone_followed_instance_gives_way_to_the_first_instance_of_its_key() {
        let players = [
            player("audacious", Playing),
            instance("mpv", "instance-c", Paused),
            instance("mpv", "instance-b", Stopped),
            instance("mpv", "instance-d", Paused),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected_following(&order, None, "org.mpris.MediaPlayer2.mpv.instance-a"),
                Some("org.mpris.MediaPlayer2.mpv.instance-b"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn gone_followed_player_without_relaunch_shows_nothing_though_others_play() {
        let players = [player("audacious", Playing), player("chromium", Playing)];
        for remembered in [None, Some("org.mpris.MediaPlayer2.mpv.instance-gone")] {
            for order in orders(&players) {
                assert_eq!(
                    selected_following(&order, remembered, "org.mpris.MediaPlayer2.vlc"),
                    None,
                    "{remembered:?} {order:?}"
                );
            }
        }
    }

    #[test]
    fn a_key_that_merely_starts_like_the_followed_one_is_another_player() {
        let players = [player("mpvx", Playing)];
        assert_eq!(
            selected_following(&players, None, "org.mpris.MediaPlayer2.mpv.instance-a"),
            None
        );
    }

    // Step 4: the first playing player, while nothing is followed

    #[test]
    fn without_an_instance_of_the_remembered_key_the_first_playing_wins() {
        let players = [
            player("chromium", Paused),
            player("vlc", Playing),
            player("audacious", Playing),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
                Some("org.mpris.MediaPlayer2.audacious"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn without_an_instance_of_the_remembered_key_nor_a_playing_one_nothing_shows() {
        let players = [player("vlc", Paused), player("chromium", Stopped)];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
                None,
                "{order:?}"
            );
        }
    }

    #[test]
    fn no_player_selects_nothing() {
        assert_eq!(selected(&[], None), None);
        assert_eq!(
            selected(&[], Some("org.mpris.MediaPlayer2.mpv.instance-a")),
            None
        );
        assert_eq!(
            selected_following(&[], None, "org.mpris.MediaPlayer2.mpv.instance-a"),
            None
        );
    }

    #[test]
    fn remembered_text_that_is_not_an_mpris_name_counts_as_nothing_remembered() {
        let one_playing = [
            instance("mpv", "instance-a", Paused),
            player("vlc", Playing),
        ];
        let none_playing = [
            instance("mpv", "instance-a", Paused),
            player("vlc", Stopped),
        ];
        for text in [
            "mpv",
            "",
            ":1.42",
            "org.mpris.MediaPlayer2.",
            "org.mpris.MediaPlayer2Extra.mpv",
        ] {
            assert_eq!(
                selected(&one_playing, Some(text)),
                Some("org.mpris.MediaPlayer2.vlc"),
                "{text:?}"
            );
            assert_eq!(selected(&none_playing, Some(text)), None, "{text:?}");
            assert_eq!(selected(&[], Some(text)), None, "{text:?}");
        }
    }

    #[test]
    fn without_remembered_player_the_first_playing_by_key_wins() {
        let players = [
            player("audacious", Paused),
            player("vlc", Playing),
            player("mpv", Playing),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, None),
                Some("org.mpris.MediaPlayer2.mpv"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn a_new_source_latches_no_paused_or_stopped_player() {
        let players = [
            player("vlc", Paused),
            player("audacious", Stopped),
            player("mpv", Paused),
        ];
        for order in orders(&players) {
            assert_eq!(selected(&order, None), None, "{order:?}");
        }
    }

    #[test]
    fn a_paused_player_is_not_taken_for_a_playing_one() {
        let players = [player("audacious", Paused), player("vlc", Playing)];
        assert_eq!(selected(&players, None), Some("org.mpris.MediaPlayer2.vlc"));
    }

    #[test]
    fn among_several_playing_instances_the_first_by_bus_name_wins() {
        let players = [
            instance("mpv", "instance-b", Playing),
            instance("mpv", "instance-a", Playing),
            player("vlc", Playing),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, None),
                Some("org.mpris.MediaPlayer2.mpv.instance-a"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn an_idle_instance_does_not_hide_a_playing_instance_of_the_same_key() {
        let players = [
            instance("mpv", "instance-a", Stopped),
            instance("mpv", "instance-b", Playing),
            player("vlc", Paused),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, None),
                Some("org.mpris.MediaPlayer2.mpv.instance-b"),
                "{order:?}"
            );
        }
    }

    // The meaning of "first"

    #[test]
    fn first_goes_by_key_before_bus_name() {
        // By bus name alone `mpv-x` would come first, `-` sorting before `.`.
        let players = [
            player("mpv-x", Playing),
            instance("mpv", "instance-a", Playing),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, None),
                Some("org.mpris.MediaPlayer2.mpv.instance-a"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn first_playing_ignores_case() {
        // By bytes `VLC` would come first, capitals sorting before lowercase.
        let players = [
            player("VLC", Playing),
            player("mpv", Playing),
            player("audacious", Paused),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, None),
                Some("org.mpris.MediaPlayer2.mpv"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn first_idle_instance_ignores_case() {
        let players = [
            instance("mpv", "instance-B", Paused),
            instance("mpv", "instance-a", Stopped),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected_following(&order, None, "org.mpris.MediaPlayer2.mpv.instance-gone"),
                Some("org.mpris.MediaPlayer2.mpv.instance-a"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn first_instance_ignores_case() {
        let players = [
            instance("mpv", "instance-B", Playing),
            instance("mpv", "instance-a", Playing),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, None),
                Some("org.mpris.MediaPlayer2.mpv.instance-a"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn keys_differing_only_by_case_keep_a_fixed_order() {
        let players = [
            player("mpv", Playing),
            player("MPV", Playing),
            player("Mpv", Playing),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, None),
                Some("org.mpris.MediaPlayer2.MPV"),
                "{order:?}"
            );
        }
    }

    // Automatic selection off: step 1 alone

    #[test]
    fn strict_shows_the_remembered_instance_whatever_its_status() {
        for status in [Playing, Paused, Stopped] {
            let players = [
                player("chromium", Playing),
                instance("mpv", "instance-a", Playing),
                instance("mpv", "instance-b", status),
            ];
            for order in orders(&players) {
                assert_eq!(
                    selected_strictly(&order, Some("org.mpris.MediaPlayer2.mpv.instance-b")),
                    Some("org.mpris.MediaPlayer2.mpv.instance-b"),
                    "{status:?} {order:?}"
                );
            }
        }
    }

    #[test]
    fn strict_does_not_show_another_instance_of_the_remembered_key() {
        let players = [
            instance("mpv", "instance-a", Playing),
            instance("mpv", "instance-b", Paused),
        ];
        for remembered in [
            "org.mpris.MediaPlayer2.mpv.instance-gone",
            "org.mpris.MediaPlayer2.mpv",
        ] {
            for order in orders(&players) {
                assert_eq!(
                    selected_strictly(&order, Some(remembered)),
                    None,
                    "{remembered} {order:?}"
                );
            }
        }
    }

    #[test]
    fn strict_does_not_show_other_players() {
        let players = [player("chromium", Playing), player("vlc", Paused)];
        for order in orders(&players) {
            assert_eq!(
                selected_strictly(&order, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
                None,
                "{order:?}"
            );
        }
    }

    #[test]
    fn strict_ignores_the_followed_player() {
        let players = [player("vlc", Playing), player("chromium", Playing)];
        for remembered in [None, Some("org.mpris.MediaPlayer2.mpv.instance-gone")] {
            for followed in [
                "org.mpris.MediaPlayer2.vlc",
                "org.mpris.MediaPlayer2.vlc.gone",
            ] {
                for order in orders(&players) {
                    assert_eq!(
                        step(&order, remembered, false, Some(followed)).0,
                        None,
                        "{remembered:?} {followed} {order:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn strict_without_a_remembered_instance_selects_nothing() {
        let players = [player("chromium", Playing), player("vlc", Paused)];
        assert_eq!(selected_strictly(&players, None), None);
        assert_eq!(selected_strictly(&[], None), None);
        for text in ["mpv", "", ":1.42", "org.mpris.MediaPlayer2."] {
            assert_eq!(selected_strictly(&players, Some(text)), None, "{text:?}");
        }
    }

    #[test]
    fn strict_matches_the_bus_name_with_its_case() {
        let players = [player("mpv", Playing)];
        assert_eq!(
            selected_strictly(&players, Some("org.mpris.MediaPlayer2.MPV")),
            None
        );
    }

    // player_choices

    fn titled(mut player: Player, title: &str) -> Player {
        player.track.title = Some(title.into());
        player
    }

    fn choice(bus_name: &str, label: &str, available: bool) -> Choice {
        Choice {
            bus_name: bus_name.into(),
            label: label.into(),
            available,
        }
    }

    #[test]
    fn no_player_and_nothing_remembered_gives_no_choice() {
        assert_eq!(player_choices(&[], None), []);
    }

    #[test]
    fn one_entry_per_instance_labelled_by_title_and_sorted_by_key_then_bus_name() {
        let players = [
            titled(player("vlc", Stopped), "Song V"),
            titled(instance("mpv", "instance-c", Stopped), "Song C"),
            titled(instance("mpv", "instance-a", Paused), "Song A"),
            titled(instance("mpv", "instance-b", Playing), "Song B"),
            titled(player("chromium", Playing), "Video"),
        ];
        let expected = [
            choice("org.mpris.MediaPlayer2.chromium", "Video", true),
            choice("org.mpris.MediaPlayer2.mpv.instance-a", "Song A", true),
            choice("org.mpris.MediaPlayer2.mpv.instance-b", "Song B", true),
            choice("org.mpris.MediaPlayer2.mpv.instance-c", "Song C", true),
            choice("org.mpris.MediaPlayer2.vlc", "Song V", true),
        ];
        for order in orders(&players) {
            assert_eq!(player_choices(&order, None), expected, "{order:?}");
        }
    }

    #[test]
    fn an_instance_without_a_title_is_labelled_by_its_identity() {
        let players = [
            titled(instance("mpv", "instance-a", Playing), "Song A"),
            instance("mpv", "instance-b", Stopped),
        ];
        assert_eq!(
            player_choices(&players, None),
            [
                choice("org.mpris.MediaPlayer2.mpv.instance-a", "Song A", true),
                choice(
                    "org.mpris.MediaPlayer2.mpv.instance-b",
                    "mpv identity",
                    true
                ),
            ]
        );
    }

    #[test]
    fn entries_go_by_key_before_bus_name() {
        let players = [
            player("mpv-x", Paused),
            instance("mpv", "instance-a", Paused),
        ];
        for order in orders(&players) {
            assert_eq!(
                player_choices(&order, None),
                [
                    choice(
                        "org.mpris.MediaPlayer2.mpv.instance-a",
                        "mpv identity",
                        true
                    ),
                    choice("org.mpris.MediaPlayer2.mpv-x", "mpv-x identity", true),
                ],
                "{order:?}"
            );
        }
    }

    #[test]
    fn entries_ignore_case() {
        let players = [
            player("VLC", Paused),
            instance("mpv", "instance-B", Paused),
            instance("mpv", "instance-a", Paused),
            player("AudioTube", Paused),
            player("chromium", Paused),
        ];
        let expected = [
            choice(
                "org.mpris.MediaPlayer2.AudioTube",
                "AudioTube identity",
                true,
            ),
            choice("org.mpris.MediaPlayer2.chromium", "chromium identity", true),
            choice("org.mpris.MediaPlayer2.Kodi", "Kodi", false),
            choice(
                "org.mpris.MediaPlayer2.mpv.instance-a",
                "mpv identity",
                true,
            ),
            choice(
                "org.mpris.MediaPlayer2.mpv.instance-B",
                "mpv identity",
                true,
            ),
            choice("org.mpris.MediaPlayer2.VLC", "VLC identity", true),
        ];
        for order in orders(&players) {
            assert_eq!(
                player_choices(&order, Some("org.mpris.MediaPlayer2.Kodi")),
                expected,
                "{order:?}"
            );
        }
    }

    #[test]
    fn entries_whose_keys_differ_only_by_case_keep_a_fixed_order() {
        let players = [
            player("mpv", Paused),
            player("vlc", Paused),
            player("MPV", Paused),
            player("Mpv", Paused),
        ];
        let expected = [
            choice("org.mpris.MediaPlayer2.MPV", "MPV identity", true),
            choice("org.mpris.MediaPlayer2.Mpv", "Mpv identity", true),
            choice("org.mpris.MediaPlayer2.mpv", "mpv identity", true),
            choice("org.mpris.MediaPlayer2.vlc", "vlc identity", true),
        ];
        for order in orders(&players) {
            assert_eq!(player_choices(&order, None), expected, "{order:?}");
        }
    }

    #[test]
    fn present_remembered_instance_is_listed_once_as_available() {
        let players = [
            titled(instance("mpv", "instance-a", Stopped), "Song A"),
            titled(instance("mpv", "instance-b", Stopped), "Song B"),
            titled(instance("mpv", "instance-c", Stopped), "Song C"),
        ];
        let expected = [
            choice("org.mpris.MediaPlayer2.mpv.instance-a", "Song A", true),
            choice("org.mpris.MediaPlayer2.mpv.instance-b", "Song B", true),
            choice("org.mpris.MediaPlayer2.mpv.instance-c", "Song C", true),
        ];
        for order in orders(&players) {
            assert_eq!(
                player_choices(&order, Some("org.mpris.MediaPlayer2.mpv.instance-b")),
                expected,
                "{order:?}"
            );
        }
    }

    #[test]
    fn gone_remembered_instance_stays_listed_as_unavailable_under_its_key() {
        let players = [
            player("vlc", Playing),
            titled(instance("mpv", "instance-b", Paused), "Song B"),
            player("chromium", Paused),
        ];
        let expected = [
            choice("org.mpris.MediaPlayer2.chromium", "chromium identity", true),
            choice("org.mpris.MediaPlayer2.mpv.instance-a", "mpv", false),
            choice("org.mpris.MediaPlayer2.mpv.instance-b", "Song B", true),
            choice("org.mpris.MediaPlayer2.vlc", "vlc identity", true),
        ];
        for order in orders(&players) {
            assert_eq!(
                player_choices(&order, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
                expected,
                "{order:?}"
            );
        }
    }

    #[test]
    fn gone_remembered_instance_is_listed_even_when_the_bus_is_empty() {
        assert_eq!(
            player_choices(&[], Some("org.mpris.MediaPlayer2.mpv.instance-a")),
            [choice(
                "org.mpris.MediaPlayer2.mpv.instance-a",
                "mpv",
                false
            )]
        );
    }

    #[test]
    fn remembered_text_that_is_not_an_mpris_name_is_listed_as_itself() {
        for text in ["mpv", ":1.42", "org.mpris.MediaPlayer2."] {
            assert_eq!(
                player_choices(&[], Some(text)),
                [choice(text, text, false)],
                "{text:?}"
            );
        }
    }

    #[test]
    fn remembered_text_that_is_not_an_mpris_name_comes_before_every_player() {
        let players = [player("audacious", Paused), player("vlc", Playing)];
        assert_eq!(
            player_choices(&players, Some("zzz")),
            [
                choice("zzz", "zzz", false),
                choice(
                    "org.mpris.MediaPlayer2.audacious",
                    "audacious identity",
                    true
                ),
                choice("org.mpris.MediaPlayer2.vlc", "vlc identity", true),
            ]
        );
    }
}
