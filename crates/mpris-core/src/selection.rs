//! Which player a source follows (B-05, B-06, B-07) and the selector entries (B-02, B-03).

use crate::model::{PlaybackStatus, Player, PlayerKey};

/// Player a source follows right now (B-05). Never stored: callers
/// re-evaluate it on every frame, which is what brings a source back to its
/// remembered player when that player reappears (B-06).
///
/// A remembered player wins whatever its status, even stopped while another
/// plays, because an explicit choice outranks the activity of other players.
pub fn select_player<'a>(
    players: &'a [Player],
    remembered: Option<&PlayerKey>,
) -> Option<&'a Player> {
    remembered
        .and_then(|key| instance_of(players, key))
        .or_else(|| first(players.iter().filter(|p| is_playing(p))))
        .or_else(|| first(players.iter()))
}

/// The instance that stands for `key` when several share it (B-04).
fn instance_of<'a>(players: &'a [Player], key: &PlayerKey) -> Option<&'a Player> {
    first(players.iter().filter(|p| p.key == *key))
}

fn first<'a>(players: impl Iterator<Item = &'a Player>) -> Option<&'a Player> {
    players.min_by(|a, b| rank(a).cmp(&rank(b)))
}

/// Keys come first so the result stays the same from one session to the
/// next (B-05), the playing instance leads within a key (B-04), and the bus
/// name settles the rest so the order players appeared in never matters.
fn rank(player: &Player) -> (&PlayerKey, bool, &str) {
    (&player.key, !is_playing(player), &player.bus_name)
}

fn is_playing(player: &Player) -> bool {
    player.status == PlaybackStatus::Playing
}

/// One entry of the player selector (B-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub key: PlayerKey,
    pub label: String,
    /// `false` for a remembered player that is not on the bus (B-03).
    pub available: bool,
}

/// Selector entries, one per key, in the order of B-05. A remembered player
/// missing from the bus keeps its entry so the user's choice is not lost
/// when the player is closed (B-03).
pub fn player_choices(players: &[Player], remembered: Option<&PlayerKey>) -> Vec<Choice> {
    let mut keys: Vec<&PlayerKey> = players.iter().map(|p| &p.key).chain(remembered).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .map(|key| match instance_of(players, key) {
            Some(player) => Choice {
                key: key.clone(),
                label: label(player),
                available: true,
            },
            None => Choice {
                key: key.clone(),
                label: key.0.clone(),
                available: false,
            },
        })
        .collect()
}

/// A blank `Identity` would leave an entry the user cannot read.
fn label(player: &Player) -> String {
    if player.identity.trim().is_empty() {
        player.key.0.clone()
    } else {
        player.identity.clone()
    }
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

    fn selected<'a>(players: &'a [Player], remembered: Option<&str>) -> Option<&'a str> {
        select_player(players, remembered.map(key).as_ref()).map(|p| p.bus_name.as_str())
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

    // select_player

    #[test]
    fn no_player_selects_nothing() {
        assert_eq!(selected(&[], None), None);
    }

    #[test]
    fn no_player_selects_nothing_even_with_a_remembered_one() {
        assert_eq!(selected(&[], Some("mpv")), None);
    }

    #[test]
    fn remembered_player_wins_over_a_playing_one_even_when_stopped() {
        let players = [player("chromium", Playing), player("mpv", Stopped)];
        assert_eq!(
            selected(&players, Some("mpv")),
            Some("org.mpris.MediaPlayer2.mpv")
        );
    }

    #[test]
    fn remembered_player_wins_when_paused() {
        let players = [player("chromium", Playing), player("mpv", Paused)];
        assert_eq!(
            selected(&players, Some("mpv")),
            Some("org.mpris.MediaPlayer2.mpv")
        );
    }

    #[test]
    fn remembered_player_is_found_under_a_new_instance_name() {
        let players = [
            player("chromium", Playing),
            instance("mpv", "instance-mWyyJbmA", Paused),
        ];
        assert_eq!(
            selected(&players, Some("mpv")),
            Some("org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA")
        );
    }

    #[test]
    fn absent_remembered_player_falls_back_to_the_playing_one() {
        let players = [player("chromium", Paused), player("vlc", Playing)];
        assert_eq!(
            selected(&players, Some("mpv")),
            Some("org.mpris.MediaPlayer2.vlc")
        );
    }

    #[test]
    fn absent_remembered_player_falls_back_to_the_first_present() {
        let players = [player("vlc", Paused), player("chromium", Stopped)];
        assert_eq!(
            selected(&players, Some("mpv")),
            Some("org.mpris.MediaPlayer2.chromium")
        );
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
    fn among_instances_of_the_remembered_key_the_playing_one_wins() {
        let players = [
            instance("mpv", "instance-a", Paused),
            instance("mpv", "instance-b", Playing),
            instance("mpv", "instance-c", Stopped),
        ];
        for order in orders(&players) {
            assert_eq!(
                selected(&order, Some("mpv")),
                Some("org.mpris.MediaPlayer2.mpv.instance-b"),
                "{order:?}"
            );
        }
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
                selected(&order, Some("mpv")),
                Some("org.mpris.MediaPlayer2.mpv.instance-a"),
                "{order:?}"
            );
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

    // player_choices

    fn choice(k: &str, label: &str, available: bool) -> Choice {
        Choice {
            key: key(k),
            label: label.into(),
            available,
        }
    }

    fn choices(players: &[Player], remembered: Option<&str>) -> Vec<Choice> {
        player_choices(players, remembered.map(key).as_ref())
    }

    #[test]
    fn no_player_and_nothing_remembered_gives_no_choice() {
        assert_eq!(choices(&[], None), []);
    }

    #[test]
    fn one_choice_per_player_sorted_by_key_and_labelled_by_identity() {
        let players = [
            player("vlc", Stopped),
            player("chromium", Playing),
            player("mpv", Paused),
        ];
        let expected = [
            choice("chromium", "chromium identity", true),
            choice("mpv", "mpv identity", true),
            choice("vlc", "vlc identity", true),
        ];
        for order in orders(&players) {
            assert_eq!(choices(&order, None), expected, "{order:?}");
        }
    }

    #[test]
    fn instances_of_one_key_are_merged_under_the_selected_instance_label() {
        let mut idle = instance("mpv", "instance-a", Paused);
        idle.identity = "mpv (idle)".into();
        let mut playing = instance("mpv", "instance-b", Playing);
        playing.identity = "mpv (playing)".into();
        let players = [idle, playing, player("vlc", Paused)];
        let expected = [
            choice("mpv", "mpv (playing)", true),
            choice("vlc", "vlc identity", true),
        ];
        for order in orders(&players) {
            assert_eq!(choices(&order, None), expected, "{order:?}");
        }
    }

    #[test]
    fn empty_identity_falls_back_to_the_key() {
        let mut nameless = player("mpv", Playing);
        nameless.identity = String::new();
        let mut blank = player("vlc", Paused);
        blank.identity = " \t".into();
        assert_eq!(
            choices(&[nameless, blank], None),
            [choice("mpv", "mpv", true), choice("vlc", "vlc", true)]
        );
    }

    #[test]
    fn absent_remembered_player_stays_listed_as_unavailable() {
        let players = [player("vlc", Playing), player("chromium", Paused)];
        assert_eq!(
            choices(&players, Some("mpv")),
            [
                choice("chromium", "chromium identity", true),
                choice("mpv", "mpv", false),
                choice("vlc", "vlc identity", true),
            ]
        );
    }

    #[test]
    fn absent_remembered_player_is_listed_even_when_the_bus_is_empty() {
        assert_eq!(choices(&[], Some("mpv")), [choice("mpv", "mpv", false)]);
    }

    #[test]
    fn present_remembered_player_is_listed_once_as_available() {
        let players = [
            instance("mpv", "instance-a", Stopped),
            instance("mpv", "instance-b", Stopped),
        ];
        assert_eq!(
            choices(&players, Some("mpv")),
            [choice("mpv", "mpv identity", true)]
        );
    }
}
