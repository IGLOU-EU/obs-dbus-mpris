//! Which player a source shows (B-05 to B-08) and the selector entries
//! (B-02, B-03).

use std::cmp::Ordering;
use std::time::{Duration, Instant};

use crate::model::{PlaybackStatus, Player, Settings};
use crate::player::{key_str, player_key};

/// How long after its start a source keeps choosing its player afresh
/// (B-05). Fixed (B-08).
pub const GRACE: Duration = Duration::from_secs(5 * 60);

/// Whether a source may still change its player of itself (B-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The first minutes of a source: every frame runs the whole rule
    /// again, which leaves time to launch the usual player, and shows it
    /// in place of whatever stood in for it.
    Grace,
    /// After the grace period, or once a player is chosen by hand: the
    /// source keeps to its followed player, so that nothing changes by
    /// itself in the middle of a live show.
    Settled,
}

/// The phase of a source started at `started`, at `now` (B-05). Derived on
/// every frame rather than stored, so it never needs updating. A choice by
/// hand settles the source for good, since it says which player the user
/// means.
pub fn phase(started: Instant, chosen_by_hand: bool, now: Instant) -> Phase {
    if chosen_by_hand || now.saturating_duration_since(started) >= GRACE {
        Phase::Settled
    } else {
        Phase::Grace
    }
}

/// What a source shows on one frame, and the player it follows from then on.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection<'a> {
    /// `None` for the fallback message (B-23).
    pub player: Option<&'a Player>,
    /// Bus name of the followed player (B-08), handed back on the next frame.
    pub followed: Option<String>,
}

/// What a source shows on this frame (B-05), given `followed`, what the
/// previous frame returned, and `phase`. The shown player is never stored:
/// callers re-evaluate it on every frame, which is what lets a source find
/// its player again once it comes back under a new instance name (B-06).
///
/// In the grace period the rule runs whole on every frame, so the usual
/// player takes over as soon as it is launched. Settled, the source keeps
/// to its followed player, or the relaunch of it, and to no other: in the
/// middle of a live show, another player starting to play, or even the
/// remembered one coming back, may be a mistake, while the shown player was
/// right. A settled source that has shown nothing yet still runs the rule,
/// so that it does not stay empty for good. The remembered instance wins
/// whatever its status, because an explicit choice outranks the activity of
/// other players. With automatic selection off it is the only candidate, so
/// a source dedicated to one player never shows another.
pub fn select<'a>(
    players: &'a [Player],
    settings: &Settings,
    followed: Option<&str>,
    phase: Phase,
) -> Selection<'a> {
    let player = shown(players, settings, followed, phase);
    // Kept while nothing shows, so that a settled source waits for the
    // relaunch of its player rather than taking the first other one.
    let followed = player.map(|p| p.bus_name.as_str()).or(followed);
    Selection {
        player,
        followed: followed.map(str::to_owned),
    }
}

/// How a source chooses its player from one frame to the next (B-08).
/// Held in memory only, never in the settings, so that each start of OBS
/// gives the remembered player a new grace period.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChoiceState {
    /// Bus name of the followed player, handed to [`select`] and taken back
    /// from it.
    pub followed: Option<String>,
    /// Whether a player was chosen by hand, which ends the grace period for
    /// good (see [`phase`]).
    pub chosen_by_hand: bool,
}

impl ChoiceState {
    /// Right after `settings` were chosen by hand (B-05). The chosen
    /// instance is followed at once, present or not, so that the source
    /// keeps to it from the first frame. "No choice" follows nothing, which
    /// leaves the next player to appear to the rule.
    pub fn chosen_by_hand(settings: &Settings) -> ChoiceState {
        ChoiceState {
            followed: settings.player.clone(),
            chosen_by_hand: true,
        }
    }
}

/// The choice of player a source had before its properties first changed
/// it, and its choice state then, which Cancel brings back (B-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceUndo {
    player: Option<String>,
    auto_select: bool,
    state: ChoiceState,
}

/// The choice state once the settings of a source go from `before` to
/// `after`, and the record that undoes it (B-05).
///
/// OBS applies each edit of the properties at once, then sets the former
/// settings again on Cancel. A choice that comes back to the recorded one
/// is taken for that Cancel: the state from before comes back, grace
/// period included, since cancelling is no choice by hand. Any other change
/// of the choice is a choice by hand. The record keeps the state from
/// before the first change, so that Cancel after several edits still finds
/// it.
pub fn rechoose(
    state: ChoiceState,
    undo: Option<ChoiceUndo>,
    before: &Settings,
    after: &Settings,
) -> (ChoiceState, Option<ChoiceUndo>) {
    if !choice_changed(before, after) {
        return (state, undo);
    }
    match undo {
        Some(undo) if (undo.player.as_deref(), undo.auto_select) == choice(after) => {
            (undo.state, None)
        }
        undo => {
            let undo = undo.unwrap_or_else(|| ChoiceUndo {
                player: before.player.clone(),
                auto_select: before.auto_select,
                state,
            });
            (ChoiceState::chosen_by_hand(after), Some(undo))
        }
    }
}

fn choice_changed(before: &Settings, after: &Settings) -> bool {
    choice(before) != choice(after)
}

/// The settings that choose the player (B-05), the others only change how
/// it is drawn.
fn choice(settings: &Settings) -> (Option<&str>, bool) {
    (settings.player.as_deref(), settings.auto_select)
}

/// The steps of B-05, in their order.
fn shown<'a>(
    players: &'a [Player],
    settings: &Settings,
    followed: Option<&str>,
    phase: Phase,
) -> Option<&'a Player> {
    let remembered = settings.player.as_deref();
    if !settings.auto_select {
        return remembered.and_then(|name| exact(players, name));
    }
    match (phase, followed) {
        (Phase::Settled, Some(followed)) => itself_or_relaunch(players, followed),
        _ => remembered
            .and_then(|name| itself_or_relaunch(players, name))
            .or_else(|| preferred(players.iter().filter(|p| !is_stopped(p)))),
    }
}

fn exact<'a>(players: &'a [Player], bus_name: &str) -> Option<&'a Player> {
    players.iter().find(|p| p.bus_name == bus_name)
}

/// The instance named `bus_name`, or once it is gone, the instance that
/// stands for it: one of the same key (B-06).
fn itself_or_relaunch<'a>(players: &'a [Player], bus_name: &str) -> Option<&'a Player> {
    exact(players, bus_name).or_else(|| {
        let key = key_str(bus_name)?;
        preferred(players.iter().filter(|p| p.key.0 == key))
    })
}

/// The first playing player, else the first one. Playing tells best which
/// player the user means, but a paused one still beats the fallback
/// message.
fn preferred<'a>(players: impl Iterator<Item = &'a Player> + Clone) -> Option<&'a Player> {
    first(players.clone().filter(|p| is_playing(p))).or_else(|| first(players))
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

/// A stopped player only shows the fallback message (B-22), so another
/// player taken while stopped would be followed with nothing to show, and
/// would keep a player that starts later from being taken.
fn is_stopped(player: &Player) -> bool {
    player.status == PlaybackStatus::Stopped
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

    use Phase::{Grace, Settled};
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
            online: Vec::new(),
        }
    }

    fn player(k: &str, status: PlaybackStatus) -> Player {
        instance(k, "", status)
    }

    /// The full bus name of `name`, as the selection returns it.
    fn bus(name: &str) -> Option<String> {
        Some(format!("org.mpris.MediaPlayer2.{name}"))
    }

    fn minutes(n: u64) -> Duration {
        Duration::from_secs(n * 60)
    }

    fn settings(remembered: Option<&str>, auto_select: bool) -> Settings {
        Settings {
            player: remembered.map(str::to_owned),
            auto_select,
            ..Settings::default()
        }
    }

    /// What a source shows for `players` in `phase`, and the player it
    /// follows next.
    fn step<'a>(
        players: &'a [Player],
        remembered: Option<&str>,
        auto_select: bool,
        followed: Option<&str>,
        phase: Phase,
    ) -> (Option<&'a str>, Option<String>) {
        let selection = select(players, &settings(remembered, auto_select), followed, phase);
        (
            selection.player.map(|p| p.bus_name.as_str()),
            selection.followed,
        )
    }

    /// What a source in its grace period shows, automatic selection on.
    fn in_grace<'a>(players: &'a [Player], remembered: Option<&str>) -> Option<&'a str> {
        step(players, remembered, true, None, Grace).0
    }

    /// What a settled source that follows `followed` shows, automatic
    /// selection on.
    fn settled<'a>(
        players: &'a [Player],
        remembered: Option<&str>,
        followed: Option<&str>,
    ) -> Option<&'a str> {
        step(players, remembered, true, followed, Settled).0
    }

    /// What a source shows with automatic selection off, checked to be the
    /// same in both phases whatever it follows.
    fn strictly<'a>(players: &'a [Player], remembered: Option<&str>) -> Option<&'a str> {
        let shown = step(players, remembered, false, None, Grace).0;
        for phase in [Grace, Settled] {
            for followed in [
                None,
                Some("org.mpris.MediaPlayer2.vlc"),
                Some("org.mpris.MediaPlayer2.vlc.gone"),
            ] {
                assert_eq!(
                    step(players, remembered, false, followed, phase).0,
                    shown,
                    "{phase:?} {followed:?}"
                );
            }
        }
        shown
    }

    /// Every rotation of `players` and of its reverse, so a result that
    /// leaks the input order shows up.
    fn orders(players: &[Player]) -> Vec<Vec<Player>> {
        let reversed: Vec<Player> = players.iter().rev().cloned().collect();
        let mut all = Vec::new();
        for base in [players.to_vec(), reversed] {
            for shift in 0..base.len().max(1) {
                let mut order = base.clone();
                order.rotate_left(shift);
                all.push(order);
            }
        }
        all
    }

    /// One moment in the life of a source.
    enum Moment {
        /// The players on the bus, this long after the source started.
        Frame(Duration, Vec<Player>),
        /// The player and the automatic selection set in the properties, as
        /// `update` receives them.
        Choose(Option<String>, bool),
    }

    use Moment::{Choose, Frame};

    /// A source as the OBS glue drives it, its start instant, its choice
    /// state and what undoes it held beside its settings (B-08).
    struct Source {
        settings: Settings,
        started: Instant,
        choice: ChoiceState,
        undo: Option<ChoiceUndo>,
    }

    impl Source {
        fn choose(&mut self, settings: Settings) {
            let state = std::mem::take(&mut self.choice);
            (self.choice, self.undo) = rechoose(state, self.undo.take(), &self.settings, &settings);
            self.settings = settings;
        }

        fn frame(&mut self, players: &[Player], now: Instant) -> Option<String> {
            let phase = phase(self.started, self.choice.chosen_by_hand, now);
            let followed = self.choice.followed.as_deref();
            let selection = select(players, &self.settings, followed, phase);
            self.choice.followed = selection.followed;
            selection.player.map(|p| p.bus_name.clone())
        }
    }

    /// What a source started with `remembered` and `auto_select` shows at
    /// each frame of `moments`. Its whole life is lived again with the
    /// players of every frame in each rotation and its reverse, so a result
    /// that leaks the input order shows up.
    fn shown_over(
        moments: &[Moment],
        remembered: Option<&str>,
        auto_select: bool,
    ) -> Vec<Option<String>> {
        let live = |reversed: bool, shift: usize| {
            let started = Instant::now();
            let mut source = Source {
                settings: settings(remembered, auto_select),
                started,
                choice: ChoiceState::default(),
                undo: None,
            };
            let mut shown = Vec::new();
            for moment in moments {
                match moment {
                    Frame(after, players) => {
                        let mut order = players.clone();
                        if reversed {
                            order.reverse();
                        }
                        let len = order.len().max(1);
                        order.rotate_left(shift % len);
                        shown.push(source.frame(&order, started + *after));
                    }
                    Choose(player, auto) => source.choose(settings(player.as_deref(), *auto)),
                }
            }
            shown
        };
        let shown = live(false, 0);
        let most = moments
            .iter()
            .map(|moment| match moment {
                Frame(_, players) => players.len(),
                Choose(..) => 0,
            })
            .max()
            .unwrap_or(0);
        for reversed in [false, true] {
            for shift in 0..most.max(1) {
                assert_eq!(
                    live(reversed, shift),
                    shown,
                    "reversed {reversed}, shifted {shift}"
                );
            }
        }
        shown
    }

    // The grace period (B-05, B-08)

    #[test]
    fn the_grace_period_lasts_five_minutes() {
        assert_eq!(GRACE, Duration::from_secs(300));
    }

    #[test]
    fn a_source_is_in_grace_until_five_minutes_after_its_start() {
        let started = Instant::now();
        for after in [Duration::ZERO, minutes(1), GRACE - Duration::from_nanos(1)] {
            assert_eq!(phase(started, false, started + after), Grace, "{after:?}");
        }
        for after in [GRACE, GRACE + Duration::from_nanos(1), minutes(24 * 60)] {
            assert_eq!(phase(started, false, started + after), Settled, "{after:?}");
        }
    }

    #[test]
    fn a_choice_by_hand_settles_the_source_at_once() {
        let started = Instant::now();
        for after in [Duration::ZERO, minutes(1), GRACE, minutes(24 * 60)] {
            assert_eq!(phase(started, true, started + after), Settled, "{after:?}");
        }
    }

    #[test]
    fn a_clock_read_before_the_start_counts_as_the_start() {
        let now = Instant::now();
        assert_eq!(phase(now + minutes(1), false, now), Grace);
        assert_eq!(phase(now + minutes(1), true, now), Settled);
    }

    #[test]
    fn a_player_chosen_by_hand_is_followed_at_once() {
        for remembered in [bus("mpv.instance-a"), bus("vlc")] {
            for auto_select in [true, false] {
                assert_eq!(
                    ChoiceState::chosen_by_hand(&settings(remembered.as_deref(), auto_select)),
                    ChoiceState {
                        followed: remembered.clone(),
                        chosen_by_hand: true,
                    },
                    "{auto_select}"
                );
            }
        }
    }

    #[test]
    fn choosing_no_player_by_hand_follows_nothing() {
        for auto_select in [true, false] {
            assert_eq!(
                ChoiceState::chosen_by_hand(&settings(None, auto_select)),
                ChoiceState {
                    followed: None,
                    chosen_by_hand: true,
                },
                "{auto_select}"
            );
        }
    }

    // Cancel in the properties (B-05)

    /// How a source chooses in grace once it has shown Chromium.
    fn showing_chromium() -> ChoiceState {
        ChoiceState {
            followed: bus("chromium"),
            chosen_by_hand: false,
        }
    }

    #[test]
    fn other_settings_leave_the_choice_state_alone() {
        let mpv = settings(bus("mpv.instance-a").as_deref(), true);
        let restyled = Settings {
            width: 900,
            fallback_text: "Rien".into(),
            ..mpv.clone()
        };
        let undo = ChoiceUndo {
            player: bus("vlc"),
            auto_select: true,
            state: ChoiceState::default(),
        };
        for undo in [None, Some(undo)] {
            assert_eq!(
                rechoose(showing_chromium(), undo.clone(), &mpv, &restyled),
                (showing_chromium(), undo),
            );
        }
    }

    #[test]
    fn a_new_choice_is_made_by_hand_and_recorded_for_cancel() {
        let mpv = settings(bus("mpv.instance-a").as_deref(), true);
        for after in [
            settings(bus("vlc").as_deref(), true),
            settings(None, true),
            settings(bus("mpv.instance-a").as_deref(), false),
        ] {
            assert_eq!(
                rechoose(showing_chromium(), None, &mpv, &after),
                (
                    ChoiceState::chosen_by_hand(&after),
                    Some(ChoiceUndo {
                        player: bus("mpv.instance-a"),
                        auto_select: true,
                        state: showing_chromium(),
                    })
                ),
                "{after:?}"
            );
        }
    }

    #[test]
    fn coming_back_to_the_recorded_choice_restores_the_state_before_it() {
        let mpv = settings(bus("mpv.instance-a").as_deref(), true);
        for edited in [
            settings(bus("vlc").as_deref(), true),
            settings(None, true),
            settings(bus("mpv.instance-a").as_deref(), false),
        ] {
            let (state, undo) = rechoose(showing_chromium(), None, &mpv, &edited);
            assert_eq!(
                rechoose(state, undo, &edited, &mpv),
                (showing_chromium(), None),
                "{edited:?}"
            );
        }
    }

    #[test]
    fn further_edits_keep_the_state_before_the_first_one() {
        let mpv = settings(bus("mpv.instance-a").as_deref(), true);
        let vlc = settings(bus("vlc").as_deref(), true);
        let strict = settings(bus("audacious").as_deref(), false);
        let (state, undo) = rechoose(showing_chromium(), None, &mpv, &vlc);
        let (state, undo) = rechoose(state, undo, &vlc, &strict);
        assert_eq!(
            (&state, &undo),
            (
                &ChoiceState::chosen_by_hand(&strict),
                &Some(ChoiceUndo {
                    player: bus("mpv.instance-a"),
                    auto_select: true,
                    state: showing_chromium(),
                })
            )
        );
        assert_eq!(
            rechoose(state, undo, &strict, &mpv),
            (showing_chromium(), None)
        );
    }

    // Scenario 13 (besoins.md, section 7)

    #[test]
    fn scenario_13_the_usual_player_launched_in_grace_takes_over_then_is_followed() {
        let chrome = || instance("chromium", "instance1234", Playing);
        let video = || player("vlc", Playing);
        let mpv = |suffix, status| instance("mpv", suffix, status);
        let moments = [
            Frame(Duration::ZERO, vec![chrome()]),
            Frame(minutes(2), vec![chrome(), mpv("instance-b", Playing)]),
            // Past the grace period, a video started in another player.
            Frame(
                minutes(6),
                vec![chrome(), mpv("instance-b", Playing), video()],
            ),
            Frame(
                minutes(7),
                vec![chrome(), mpv("instance-b", Paused), video()],
            ),
            // The followed player closed, then relaunched.
            Frame(minutes(8), vec![chrome(), video()]),
            Frame(
                minutes(9),
                vec![chrome(), video(), mpv("instance-c", Stopped)],
            ),
            Frame(
                minutes(10),
                vec![chrome(), video(), mpv("instance-c", Playing)],
            ),
            // Closed for good.
            Frame(minutes(11), vec![chrome(), video()]),
            Frame(minutes(60), vec![chrome(), video()]),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [
                bus("chromium.instance1234"),
                bus("mpv.instance-b"),
                bus("mpv.instance-b"),
                bus("mpv.instance-b"),
                None,
                bus("mpv.instance-c"),
                bus("mpv.instance-c"),
                None,
                None,
            ]
        );
    }

    #[test]
    fn scenario_13_the_usual_player_launched_after_grace_changes_nothing() {
        let chrome = |suffix, status| instance("chromium", suffix, status);
        let usual = || instance("mpv", "instance-a", Playing);
        let moments = [
            Frame(Duration::ZERO, vec![chrome("instance1234", Playing)]),
            Frame(minutes(6), vec![chrome("instance1234", Playing), usual()]),
            Frame(minutes(7), vec![chrome("instance1234", Paused), usual()]),
            // Chrome closed: not even the remembered instance stands in.
            Frame(minutes(8), vec![usual(), player("vlc", Playing)]),
            // Chrome relaunched.
            Frame(
                minutes(9),
                vec![
                    usual(),
                    player("vlc", Playing),
                    chrome("instance5678", Paused),
                ],
            ),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [
                bus("chromium.instance1234"),
                bus("chromium.instance1234"),
                bus("chromium.instance1234"),
                None,
                bus("chromium.instance5678"),
            ]
        );
    }

    #[test]
    fn scenario_13_a_player_chosen_by_hand_in_grace_is_final() {
        let chrome = || instance("chromium", "instance1234", Playing);
        let usual = || instance("mpv", "instance-a", Playing);
        let moments = [
            Frame(Duration::ZERO, vec![chrome(), player("vlc", Paused)]),
            Choose(bus("vlc"), true),
            Frame(minutes(1), vec![chrome(), player("vlc", Paused)]),
            // The usual player, launched well within the grace period.
            Frame(minutes(2), vec![chrome(), player("vlc", Paused), usual()]),
            // The chosen player closed, then relaunched.
            Frame(minutes(3), vec![chrome(), usual()]),
            Frame(minutes(4), vec![chrome(), usual(), player("vlc", Stopped)]),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [
                bus("chromium.instance1234"),
                bus("vlc"),
                bus("vlc"),
                None,
                bus("vlc"),
            ]
        );
    }

    // The rest of the rule over time

    #[test]
    fn in_grace_the_rule_runs_afresh_on_every_frame() {
        let mpv = |suffix, status| instance("mpv", suffix, status);
        let moments = [
            // Nothing plays: a paused player beats the fallback message.
            Frame(
                Duration::ZERO,
                vec![player("vlc", Paused), player("chromium", Paused)],
            ),
            // A player starting to play takes over (step 3).
            Frame(
                minutes(1),
                vec![player("vlc", Playing), player("chromium", Paused)],
            ),
            // An instance of the usual player takes over (step 2).
            Frame(
                minutes(2),
                vec![player("vlc", Playing), mpv("instance-b", Paused)],
            ),
            // The remembered instance itself takes over (step 1).
            Frame(
                minutes(3),
                vec![
                    player("vlc", Playing),
                    mpv("instance-b", Playing),
                    mpv("instance-a", Stopped),
                ],
            ),
            // Both gone: back to step 3, then step 4.
            Frame(
                minutes(4),
                vec![player("vlc", Paused), player("audacious", Paused)],
            ),
            Frame(minutes(4) + Duration::from_secs(30), vec![]),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [
                bus("chromium"),
                bus("vlc"),
                bus("mpv.instance-b"),
                bus("mpv.instance-a"),
                bus("audacious"),
                None,
            ]
        );
    }

    #[test]
    fn the_grace_period_ends_at_exactly_five_minutes() {
        let chrome = || player("chromium", Playing);
        let usual = || instance("mpv", "instance-a", Paused);
        for (launched, expected) in [
            (GRACE - Duration::from_millis(1), bus("mpv.instance-a")),
            (GRACE, bus("chromium")),
        ] {
            let moments = [
                Frame(Duration::ZERO, vec![chrome()]),
                Frame(launched, vec![chrome(), usual()]),
                Frame(launched + minutes(1), vec![chrome(), usual()]),
            ];
            assert_eq!(
                shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
                [bus("chromium"), expected.clone(), expected],
                "{launched:?}"
            );
        }
    }

    #[test]
    fn a_player_shown_in_grace_then_gone_stays_the_followed_one() {
        let usual = || instance("mpv", "instance-a", Playing);
        let moments = [
            Frame(Duration::ZERO, vec![player("chromium", Playing)]),
            Frame(minutes(2), vec![]),
            Frame(minutes(6), vec![usual(), player("vlc", Playing)]),
            Frame(minutes(7), vec![usual(), player("chromium", Paused)]),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [bus("chromium"), None, None, bus("chromium")]
        );
    }

    #[test]
    fn after_grace_with_nothing_shown_yet_the_first_player_to_appear_is_kept() {
        let usual = || instance("mpv", "instance-a", Playing);
        let moments = [
            Frame(Duration::ZERO, vec![]),
            Frame(minutes(4), vec![]),
            Frame(minutes(6), vec![player("vlc", Paused)]),
            Frame(
                minutes(7),
                vec![player("vlc", Paused), usual(), player("audacious", Playing)],
            ),
            Frame(minutes(8), vec![usual(), player("audacious", Playing)]),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [None, None, bus("vlc"), bus("vlc"), None]
        );
    }

    #[test]
    fn after_grace_with_nothing_shown_yet_steps_one_to_three_rank_the_first_players() {
        let cases = [
            (
                vec![
                    player("audacious", Playing),
                    instance("mpv", "instance-b", Paused),
                ],
                bus("mpv.instance-b"),
            ),
            (
                vec![
                    instance("mpv", "instance-b", Playing),
                    instance("mpv", "instance-a", Stopped),
                ],
                bus("mpv.instance-a"),
            ),
            (
                vec![player("vlc", Paused), player("audacious", Playing)],
                bus("audacious"),
            ),
            (
                vec![player("vlc", Paused), player("audacious", Stopped)],
                bus("vlc"),
            ),
        ];
        for (appearing, expected) in cases {
            let moments = [
                Frame(Duration::ZERO, vec![]),
                Frame(minutes(6), appearing.clone()),
                Frame(minutes(7), appearing.clone()),
            ];
            assert_eq!(
                shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
                [None, expected.clone(), expected],
                "{appearing:?}"
            );
        }
    }

    #[test]
    fn choosing_no_player_by_hand_keeps_the_first_player_to_appear() {
        let moments = [
            Frame(Duration::ZERO, vec![]),
            Choose(None, true),
            Frame(minutes(1), vec![]),
            Frame(minutes(2), vec![player("vlc", Paused)]),
            Frame(
                minutes(3),
                vec![player("vlc", Paused), player("audacious", Playing)],
            ),
            Frame(minutes(4), vec![player("audacious", Playing)]),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [None, None, bus("vlc"), bus("vlc"), None]
        );
    }

    #[test]
    fn choosing_no_player_by_hand_lets_go_of_the_shown_one() {
        let moments = [
            Frame(Duration::ZERO, vec![player("chromium", Playing)]),
            Choose(None, true),
            Frame(
                minutes(1),
                vec![player("chromium", Paused), player("vlc", Playing)],
            ),
            Frame(
                minutes(2),
                vec![player("chromium", Playing), player("vlc", Paused)],
            ),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [bus("chromium"), bus("vlc"), bus("vlc")]
        );
    }

    #[test]
    fn a_player_chosen_by_hand_while_closed_waits_for_its_launch() {
        let moments = [
            Frame(Duration::ZERO, vec![player("chromium", Playing)]),
            Choose(bus("vlc"), true),
            Frame(minutes(1), vec![player("chromium", Playing)]),
            // Another entry, then back to VLC, listed as unavailable.
            Choose(bus("chromium"), true),
            Choose(bus("vlc"), true),
            Frame(minutes(2), vec![player("chromium", Playing)]),
            Frame(
                minutes(3),
                vec![player("chromium", Playing), player("vlc", Paused)],
            ),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [bus("chromium"), None, None, bus("vlc")]
        );
    }

    #[test]
    fn checking_automatic_selection_by_hand_opens_no_grace_period() {
        let moments = [
            Frame(Duration::ZERO, vec![player("chromium", Playing)]),
            Choose(None, true),
            Frame(minutes(1), vec![player("chromium", Playing)]),
            Frame(
                minutes(2),
                vec![player("chromium", Paused), player("audacious", Playing)],
            ),
        ];
        assert_eq!(
            shown_over(&moments, None, false),
            [None, bus("chromium"), bus("chromium")]
        );
    }

    #[test]
    fn cancel_in_grace_keeps_the_grace_period() {
        let moments = [
            Frame(Duration::ZERO, vec![player("chromium", Playing)]),
            Choose(bus("vlc"), true),
            Frame(
                minutes(1),
                vec![player("chromium", Playing), player("vlc", Paused)],
            ),
            Choose(bus("mpv.instance-a"), true),
            Frame(
                minutes(2),
                vec![
                    player("chromium", Playing),
                    instance("mpv", "instance-b", Paused),
                ],
            ),
            Frame(minutes(3), vec![player("chromium", Playing)]),
            Frame(
                minutes(6),
                vec![
                    player("chromium", Playing),
                    instance("mpv", "instance-c", Playing),
                ],
            ),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [
                bus("chromium"),
                bus("vlc"),
                bus("mpv.instance-b"),
                bus("chromium"),
                bus("chromium"),
            ]
        );
    }

    #[test]
    fn cancel_after_grace_keeps_the_followed_player() {
        let usual = || instance("mpv", "instance-a", Playing);
        let moments = [
            Frame(Duration::ZERO, vec![player("chromium", Playing)]),
            Frame(minutes(6), vec![player("chromium", Playing), usual()]),
            Choose(bus("vlc"), true),
            Frame(
                minutes(7),
                vec![player("chromium", Playing), usual(), player("vlc", Paused)],
            ),
            Choose(bus("mpv.instance-a"), true),
            Frame(
                minutes(8),
                vec![player("chromium", Paused), usual(), player("vlc", Paused)],
            ),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [
                bus("chromium"),
                bus("chromium"),
                bus("vlc"),
                bus("chromium")
            ]
        );
    }

    #[test]
    fn cancel_after_several_edits_restores_the_state_before_the_first() {
        let usual = || instance("mpv", "instance-a", Playing);
        let moments = [
            Frame(Duration::ZERO, vec![player("chromium", Playing)]),
            Frame(minutes(6), vec![player("chromium", Playing), usual()]),
            Choose(bus("vlc"), true),
            Frame(
                minutes(7),
                vec![player("chromium", Playing), usual(), player("vlc", Paused)],
            ),
            Choose(bus("audacious"), false),
            Frame(
                minutes(8),
                vec![player("chromium", Playing), usual(), player("vlc", Paused)],
            ),
            Choose(None, true),
            Choose(bus("mpv.instance-a"), true),
            Frame(
                minutes(9),
                vec![player("chromium", Paused), usual(), player("vlc", Paused)],
            ),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [
                bus("chromium"),
                bus("chromium"),
                bus("vlc"),
                None,
                bus("chromium"),
            ]
        );
    }

    #[test]
    fn an_edit_kept_stays_a_choice_by_hand() {
        let moments = [
            Frame(Duration::ZERO, vec![player("chromium", Playing)]),
            Choose(bus("vlc"), true),
            Frame(
                minutes(1),
                vec![player("chromium", Playing), player("vlc", Paused)],
            ),
            Frame(
                minutes(2),
                vec![
                    player("chromium", Playing),
                    instance("mpv", "instance-a", Playing),
                ],
            ),
            Choose(bus("chromium"), true),
            Frame(minutes(3), vec![player("chromium", Paused)]),
            Frame(
                minutes(4),
                vec![player("audacious", Playing), player("vlc", Playing)],
            ),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [bus("chromium"), bus("vlc"), None, bus("chromium"), None]
        );
    }

    #[test]
    fn the_defaults_button_then_ok_is_a_choice_of_no_player_by_hand() {
        let moments = [
            Frame(Duration::ZERO, vec![player("chromium", Playing)]),
            // Defaults sets every setting back to its default, then OK sets
            // the same again.
            Choose(None, true),
            Choose(None, true),
            Frame(
                minutes(1),
                vec![player("chromium", Paused), player("vlc", Playing)],
            ),
            Frame(
                minutes(2),
                vec![
                    player("chromium", Playing),
                    player("vlc", Paused),
                    instance("mpv", "instance-a", Playing),
                ],
            ),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [bus("chromium"), bus("vlc"), bus("vlc")]
        );
    }

    #[test]
    fn the_defaults_button_then_cancel_changes_nothing() {
        let moments = [
            Frame(Duration::ZERO, vec![player("chromium", Playing)]),
            Choose(None, true),
            Choose(bus("mpv.instance-a"), true),
            Frame(
                minutes(1),
                vec![player("chromium", Paused), player("vlc", Playing)],
            ),
            Frame(
                minutes(2),
                vec![
                    player("vlc", Playing),
                    instance("mpv", "instance-b", Paused),
                ],
            ),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [bus("chromium"), bus("vlc"), bus("mpv.instance-b")]
        );
    }

    #[test]
    fn setting_the_same_choice_again_keeps_the_grace_period() {
        let moments = [
            Frame(Duration::ZERO, vec![player("chromium", Playing)]),
            Choose(bus("mpv.instance-a"), true),
            Frame(
                minutes(1),
                vec![
                    player("chromium", Playing),
                    instance("mpv", "instance-b", Paused),
                ],
            ),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [bus("chromium"), bus("mpv.instance-b")]
        );
    }

    #[test]
    fn strict_never_goes_back_to_a_relaunched_player() {
        let moments = [
            Frame(Duration::ZERO, vec![instance("mpv", "instance-a", Playing)]),
            Frame(minutes(1), vec![player("chromium", Playing)]),
            Frame(
                minutes(2),
                vec![
                    player("chromium", Playing),
                    instance("mpv", "instance-b", Playing),
                ],
            ),
            Frame(
                minutes(6),
                vec![
                    player("chromium", Playing),
                    instance("mpv", "instance-b", Playing),
                ],
            ),
            Frame(minutes(7), vec![instance("mpv", "instance-a", Paused)]),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), false),
            [
                bus("mpv.instance-a"),
                None,
                None,
                None,
                bus("mpv.instance-a")
            ]
        );
    }

    #[test]
    fn a_stopped_player_through_the_grace_period_leaves_the_next_one_free() {
        let moments = [
            Frame(Duration::ZERO, vec![player("vlc", Stopped)]),
            Frame(minutes(4), vec![player("vlc", Stopped)]),
            Frame(
                minutes(6),
                vec![player("vlc", Stopped), player("chromium", Playing)],
            ),
            Frame(
                minutes(7),
                vec![player("vlc", Playing), player("chromium", Paused)],
            ),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [None, None, bus("chromium"), bus("chromium")]
        );
    }

    #[test]
    fn a_stopped_player_that_starts_after_the_grace_period_is_kept() {
        let moments = [
            Frame(Duration::ZERO, vec![player("vlc", Stopped)]),
            Frame(minutes(6), vec![player("vlc", Playing)]),
            Frame(
                minutes(7),
                vec![player("vlc", Paused), player("chromium", Playing)],
            ),
            Frame(
                minutes(8),
                vec![player("vlc", Stopped), player("chromium", Playing)],
            ),
        ];
        assert_eq!(
            shown_over(&moments, bus("mpv.instance-a").as_deref(), true),
            [None, bus("vlc"), bus("vlc"), bus("vlc")]
        );
    }

    // The followed player (B-08)

    #[test]
    fn the_shown_player_becomes_the_followed_one() {
        let players = [
            instance("mpv", "instance-a", Paused),
            player("vlc", Playing),
        ];
        let remembered = bus("mpv.instance-a");
        for order in orders(&players) {
            for phase in [Grace, Settled] {
                for (remembered, auto_select, followed, expected) in [
                    (remembered.as_deref(), true, None, &remembered),
                    (None, true, None, &bus("vlc")),
                    (remembered.as_deref(), false, None, &remembered),
                    (None, true, bus("vlc"), &bus("vlc")),
                ] {
                    assert_eq!(
                        &step(&order, remembered, auto_select, followed.as_deref(), phase).1,
                        expected,
                        "{phase:?} {remembered:?} {auto_select} {followed:?} {order:?}"
                    );
                }
            }
            assert_eq!(
                step(
                    &order,
                    remembered.as_deref(),
                    true,
                    bus("vlc").as_deref(),
                    Grace
                )
                .1,
                remembered,
                "{order:?}"
            );
        }
    }

    #[test]
    fn while_nothing_shows_the_followed_player_is_kept() {
        for phase in [Grace, Settled] {
            assert_eq!(
                step(&[], None, true, bus("vlc").as_deref(), phase),
                (None, bus("vlc")),
                "{phase:?}"
            );
            assert_eq!(
                step(&[], None, true, None, phase),
                (None, None),
                "{phase:?}"
            );
        }
        let others = [player("chromium", Playing)];
        assert_eq!(
            step(&others, None, true, bus("vlc").as_deref(), Settled),
            (None, bus("vlc"))
        );
        assert_eq!(
            step(
                &others,
                bus("mpv.instance-a").as_deref(),
                false,
                bus("vlc").as_deref(),
                Grace
            ),
            (None, bus("vlc"))
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
    fn in_grace_the_remembered_instance_wins_whatever_its_status() {
        for status in [Stopped, Paused] {
            let players = [
                player("chromium", Playing),
                instance("mpv", "instance-a", Playing),
                instance("mpv", "instance-b", status),
                player("vlc", Playing),
            ];
            let remembered = bus("mpv.instance-b");
            for order in orders(&players) {
                for followed in [None, bus("vlc"), bus("mpv.instance-a")] {
                    assert_eq!(
                        step(
                            &order,
                            remembered.as_deref(),
                            true,
                            followed.as_deref(),
                            Grace
                        )
                        .0,
                        remembered.as_deref(),
                        "{status:?} {followed:?} {order:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn remembered_instance_without_suffix_wins_when_stopped() {
        let players = [player("chromium", Playing), player("mpv", Stopped)];
        for order in orders(&players) {
            assert_eq!(
                in_grace(&order, Some("org.mpris.MediaPlayer2.mpv")),
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
                in_grace(&order, Some("org.mpris.MediaPlayer2.mpv.instance-gone")),
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
                in_grace(&order, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
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
            in_grace(
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
            in_grace(&players, Some("org.mpris.MediaPlayer2.mpv")),
            Some("org.mpris.MediaPlayer2.mpv.instance-a")
        );
    }

    #[test]
    fn in_grace_an_instance_of_the_remembered_player_outranks_the_followed_one() {
        let players = [
            player("chromium", Playing),
            instance("mpv", "instance-b", Paused),
        ];
        for order in orders(&players) {
            assert_eq!(
                step(
                    &order,
                    Some("org.mpris.MediaPlayer2.mpv.instance-a"),
                    true,
                    Some("org.mpris.MediaPlayer2.chromium"),
                    Grace
                )
                .0,
                Some("org.mpris.MediaPlayer2.mpv.instance-b"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn a_key_that_merely_starts_like_the_remembered_one_is_another_player() {
        let players = [player("mpvx", Paused), player("vlc", Playing)];
        assert_eq!(
            in_grace(&players, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
            Some("org.mpris.MediaPlayer2.vlc")
        );
    }

    // Step 3: another player, playing preferably

    #[test]
    fn without_an_instance_of_the_remembered_key_the_first_playing_wins() {
        let players = [
            player("chromium", Paused),
            player("vlc", Playing),
            player("audacious", Playing),
        ];
        for order in orders(&players) {
            assert_eq!(
                in_grace(&order, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
                Some("org.mpris.MediaPlayer2.audacious"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn without_a_playing_player_the_first_paused_shows() {
        let players = [player("vlc", Paused), player("chromium", Stopped)];
        for order in orders(&players) {
            assert_eq!(
                in_grace(&order, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
                Some("org.mpris.MediaPlayer2.vlc"),
                "{order:?}"
            );
        }
        let players = [
            player("vlc", Paused),
            player("audacious", Stopped),
            player("mpv", Paused),
        ];
        for order in orders(&players) {
            assert_eq!(
                in_grace(&order, None),
                Some("org.mpris.MediaPlayer2.mpv"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn a_stopped_player_is_neither_shown_nor_followed_by_step_three() {
        let players = [player("vlc", Stopped), player("audacious", Stopped)];
        for phase in [Grace, Settled] {
            for remembered in [None, Some("org.mpris.MediaPlayer2.mpv.instance-a")] {
                for order in orders(&players) {
                    assert_eq!(
                        step(&order, remembered, true, None, phase),
                        (None, None),
                        "{phase:?} {remembered:?} {order:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn in_grace_a_playing_player_takes_over_the_followed_one() {
        let players = [player("vlc", Paused), player("chromium", Playing)];
        for order in orders(&players) {
            assert_eq!(
                step(&order, None, true, bus("vlc").as_deref(), Grace).0,
                Some("org.mpris.MediaPlayer2.chromium"),
                "{order:?}"
            );
        }
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
                in_grace(&one_playing, Some(text)),
                Some("org.mpris.MediaPlayer2.vlc"),
                "{text:?}"
            );
            assert_eq!(
                in_grace(&none_playing, Some(text)),
                Some("org.mpris.MediaPlayer2.mpv.instance-a"),
                "{text:?}"
            );
            assert_eq!(in_grace(&[], Some(text)), None, "{text:?}");
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
                in_grace(&order, None),
                Some("org.mpris.MediaPlayer2.mpv"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn a_paused_player_is_not_taken_for_a_playing_one() {
        let players = [player("audacious", Paused), player("vlc", Playing)];
        assert_eq!(in_grace(&players, None), Some("org.mpris.MediaPlayer2.vlc"));
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
                in_grace(&order, None),
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
                in_grace(&order, None),
                Some("org.mpris.MediaPlayer2.mpv.instance-b"),
                "{order:?}"
            );
        }
    }

    // Step 4: no player at all

    #[test]
    fn no_player_selects_nothing() {
        for phase in [Grace, Settled] {
            for remembered in [None, Some("org.mpris.MediaPlayer2.mpv.instance-a")] {
                for followed in [None, Some("org.mpris.MediaPlayer2.mpv.instance-a")] {
                    assert_eq!(
                        step(&[], remembered, true, followed, phase).0,
                        None,
                        "{phase:?} {remembered:?} {followed:?}"
                    );
                }
            }
        }
    }

    // Settled: the followed player, or its relaunch

    #[test]
    fn settled_the_followed_player_stays_whatever_its_status_while_others_play() {
        for status in [Playing, Paused, Stopped] {
            let players = [
                player("audacious", Playing),
                player("vlc", status),
                player("chromium", Playing),
                instance("mpv", "instance-a", Playing),
            ];
            for remembered in [
                None,
                Some("org.mpris.MediaPlayer2.mpv.instance-gone"),
                Some("org.mpris.MediaPlayer2.mpv.instance-a"),
            ] {
                for order in orders(&players) {
                    assert_eq!(
                        settled(&order, remembered, Some("org.mpris.MediaPlayer2.vlc")),
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
                settled(
                    &order,
                    None,
                    Some("org.mpris.MediaPlayer2.mpv.instance-gone")
                ),
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
                settled(&order, None, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
                Some("org.mpris.MediaPlayer2.mpv.instance-b"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn settled_a_gone_followed_player_without_relaunch_shows_nothing_though_others_play() {
        let players = [
            player("audacious", Playing),
            player("chromium", Playing),
            instance("mpv", "instance-a", Playing),
        ];
        for remembered in [
            None,
            Some("org.mpris.MediaPlayer2.mpv.instance-gone"),
            Some("org.mpris.MediaPlayer2.mpv.instance-a"),
        ] {
            for order in orders(&players) {
                assert_eq!(
                    settled(&order, remembered, Some("org.mpris.MediaPlayer2.vlc")),
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
            settled(
                &players,
                None,
                Some("org.mpris.MediaPlayer2.mpv.instance-a")
            ),
            None
        );
    }

    #[test]
    fn settled_with_nothing_followed_yet_the_steps_pick_the_player() {
        let remembered = Some("org.mpris.MediaPlayer2.mpv.instance-a");
        let cases = [
            (
                vec![
                    player("chromium", Playing),
                    instance("mpv", "instance-a", Stopped),
                ],
                Some("org.mpris.MediaPlayer2.mpv.instance-a"),
            ),
            (
                vec![
                    player("chromium", Playing),
                    instance("mpv", "instance-b", Paused),
                ],
                Some("org.mpris.MediaPlayer2.mpv.instance-b"),
            ),
            (
                vec![player("vlc", Paused), player("chromium", Playing)],
                Some("org.mpris.MediaPlayer2.chromium"),
            ),
            (
                vec![player("vlc", Paused), player("chromium", Stopped)],
                Some("org.mpris.MediaPlayer2.vlc"),
            ),
            (vec![player("chromium", Stopped)], None),
            (vec![], None),
        ];
        for (players, expected) in &cases {
            for order in orders(players) {
                assert_eq!(settled(&order, remembered, None), *expected, "{order:?}");
            }
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
                in_grace(&order, None),
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
                in_grace(&order, None),
                Some("org.mpris.MediaPlayer2.mpv"),
                "{order:?}"
            );
        }
    }

    #[test]
    fn first_paused_ignores_case() {
        let players = [player("VLC", Paused), player("mpv", Paused)];
        for order in orders(&players) {
            assert_eq!(
                in_grace(&order, None),
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
                settled(
                    &order,
                    None,
                    Some("org.mpris.MediaPlayer2.mpv.instance-gone")
                ),
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
                in_grace(&order, None),
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
                in_grace(&order, None),
                Some("org.mpris.MediaPlayer2.MPV"),
                "{order:?}"
            );
        }
    }

    // Automatic selection off: the exact remembered instance alone

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
                    strictly(&order, Some("org.mpris.MediaPlayer2.mpv.instance-b")),
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
                    strictly(&order, Some(remembered)),
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
                strictly(&order, Some("org.mpris.MediaPlayer2.mpv.instance-a")),
                None,
                "{order:?}"
            );
        }
    }

    #[test]
    fn strict_ignores_the_followed_player() {
        let players = [player("vlc", Playing), player("chromium", Playing)];
        for remembered in [None, Some("org.mpris.MediaPlayer2.mpv.instance-gone")] {
            for order in orders(&players) {
                assert_eq!(
                    strictly(&order, remembered),
                    None,
                    "{remembered:?} {order:?}"
                );
            }
        }
    }

    #[test]
    fn strict_without_a_remembered_instance_selects_nothing() {
        let players = [player("chromium", Playing), player("vlc", Paused)];
        assert_eq!(strictly(&players, None), None);
        assert_eq!(strictly(&[], None), None);
        for text in ["mpv", "", ":1.42", "org.mpris.MediaPlayer2."] {
            assert_eq!(strictly(&players, Some(text)), None, "{text:?}");
        }
    }

    #[test]
    fn strict_matches_the_bus_name_with_its_case() {
        let players = [player("mpv", Playing)];
        assert_eq!(strictly(&players, Some("org.mpris.MediaPlayer2.MPV")), None);
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
