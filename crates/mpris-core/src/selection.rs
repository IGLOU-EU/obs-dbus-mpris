//! Which player a source follows (B-05, B-06, B-07) and the selector entries (B-02, B-03).

use std::cmp::Ordering;

use crate::model::{PlaybackStatus, Player, PlayerKey, Settings};
use crate::player::{key_str, player_key};

/// Player a source follows right now (B-05), given the remembered instance
/// and the automatic selection switch of its settings. Never stored: callers
/// re-evaluate it on every frame, which is what lets a source follow its
/// player again once it comes back under a new instance name (B-06).
///
/// The remembered instance wins whatever its status, even stopped while
/// another plays, because an explicit choice outranks the activity of other
/// players. With automatic selection off it is the only candidate, so a
/// source dedicated to one player never shows another (B-05, B-07).
pub fn select_player<'a>(players: &'a [Player], settings: &Settings) -> Option<&'a Player> {
    let remembered = settings.player.as_deref();
    let exact = remembered.and_then(|name| players.iter().find(|p| p.bus_name == name));
    if !settings.auto_select {
        return exact;
    }
    exact
        .or_else(|| instance_of(players, &player_key(remembered?)?))
        .or_else(|| preferred(players.iter()))
}

/// The instance that stands for `key` once the remembered one is gone (B-06).
fn instance_of<'a>(players: &'a [Player], key: &PlayerKey) -> Option<&'a Player> {
    preferred(players.iter().filter(|p| p.key == *key))
}

/// The first playing player, or the first player when none plays (B-05).
fn preferred<'a>(players: impl Iterator<Item = &'a Player>) -> Option<&'a Player> {
    players.min_by(|a, b| {
        (!is_playing(a))
            .cmp(&!is_playing(b))
            .then_with(|| by_rank(&a.bus_name, &b.bus_name))
    })
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
    use crate::model::{Art, PlaybackStatus, PositionAnchor, Track};
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

    fn selected_with<'a>(
        players: &'a [Player],
        remembered: Option<&str>,
        auto_select: bool,
    ) -> Option<&'a str> {
        let settings = Settings {
            player: remembered.map(str::to_owned),
            auto_select,
            ..Settings::default()
        };
        select_player(players, &settings).map(|p| p.bus_name.as_str())
    }

    fn selected<'a>(players: &'a [Player], remembered: Option<&str>) -> Option<&'a str> {
        selected_with(players, remembered, true)
    }

    /// Selection with automatic selection off.
    fn selected_strictly<'a>(players: &'a [Player], remembered: Option<&str>) -> Option<&'a str> {
        selected_with(players, remembered, false)
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

    // select_player, step 1: the remembered instance

    #[test]
    fn remembered_instance_wins_whatever_its_status() {
        for status in [Stopped, Paused] {
            let players = [
                player("chromium", Playing),
                instance("mpv", "instance-a", Playing),
                instance("mpv", "instance-b", status),
                player("vlc", Playing),
            ];
            for order in orders(&players) {
                assert_eq!(
                    selected(&order, Some("org.mpris.MediaPlayer2.mpv.instance-b")),
                    Some("org.mpris.MediaPlayer2.mpv.instance-b"),
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

    // select_player, step 2: another instance of the remembered key

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
    fn restarted_player_is_followed_under_its_new_instance_name() {
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
    fn remembered_name_without_suffix_follows_any_instance_of_its_key() {
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
    fn a_key_that_merely_starts_like_the_remembered_one_is_another_player() {
        let players = [player("mpvx", Paused), player("vlc", Playing)];
        assert_eq!(
            selected(&players, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
            Some("org.mpris.MediaPlayer2.vlc")
        );
    }

    // select_player, steps 3 to 5: no instance of the remembered key

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
    fn without_an_instance_of_the_remembered_key_nor_a_playing_one_the_first_wins() {
        let players = [player("vlc", Paused), player("chromium", Stopped)];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
                Some("org.mpris.MediaPlayer2.chromium"),
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
            assert_eq!(
                selected(&none_playing, Some(text)),
                Some("org.mpris.MediaPlayer2.mpv.instance-a"),
                "{text:?}"
            );
            assert_eq!(selected(&[], Some(text)), None, "{text:?}");
        }
    }

    // select_player, nothing remembered (B-07) and the meaning of "first"

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
    fn without_any_playing_the_first_by_key_wins_whatever_its_status() {
        let players = [
            player("vlc", Paused),
            player("audacious", Stopped),
            player("mpv", Paused),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, None),
                Some("org.mpris.MediaPlayer2.audacious"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn a_paused_player_is_not_taken_for_a_playing_one() {
        let players = [player("audacious", Paused), player("vlc", Playing)];
        assert_eq!(selected(&players, None), Some("org.mpris.MediaPlayer2.vlc"));
    }

    #[test]
    fn among_idle_instances_the_first_by_bus_name_wins() {
        let players = [
            instance("mpv", "instance-c", Paused),
            instance("mpv", "instance-a", Stopped),
            instance("mpv", "instance-b", Paused),
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

    #[test]
    fn first_goes_by_key_before_bus_name() {
        // By bus name alone `mpv-x` would come first, `-` sorting before `.`.
        let players = [
            player("mpv-x", Paused),
            instance("mpv", "instance-a", Paused),
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
    fn first_idle_player_ignores_case() {
        let players = [player("Strawberry", Paused), player("audacious", Stopped)];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, None),
                Some("org.mpris.MediaPlayer2.audacious"),
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

    // select_player, automatic selection off: step 1 alone

    #[test]
    fn strict_follows_the_remembered_instance_whatever_its_status() {
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
    fn strict_does_not_follow_another_instance_of_the_remembered_key() {
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
    fn strict_does_not_follow_other_players() {
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
