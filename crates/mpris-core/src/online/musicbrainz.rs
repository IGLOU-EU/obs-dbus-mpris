//! MusicBrainz and the Cover Art Archive (D-09): the searches, and the
//! covers worth asking for in what they answer, best first.

use serde::Deserialize;

use super::cleanup::{artist_matches, text_matches};
use super::{Plan, form};

const SEARCH: &str = "https://musicbrainz.org/ws/2/";
/// 500 pixels: as fast as 250 to get, and enough for a thumbnail at its
/// height on a dense screen, where 1200 weighs five times more for
/// nothing (study, section 8).
const COVER: &str = "https://coverartarchive.org/release-group/";

/// Albums found, and covers tried among them.
const ALBUMS_TRIED: usize = 2;
/// Release groups tried among those of the recordings found.
const RELEASE_GROUPS_TRIED: usize = 3;
/// A wide page for recordings, since live versions often fill the first
/// results.
const RECORDINGS_PAGE: &str = "25";
const ALBUMS_PAGE: &str = "5";

/// The search for `text` by `artist`.
pub fn search_url(plan: Plan, artist: &str, text: &str) -> String {
    let (entity, field, limit) = match plan {
        Plan::Album => ("release-group", "releasegroup", ALBUMS_PAGE),
        Plan::Title => ("recording", "recording", RECORDINGS_PAGE),
    };
    let query = format!("{field}:({}) AND artist:({})", lucene(text), lucene(artist));
    let params = form(&[("query", &query), ("fmt", "json"), ("limit", limit)]);
    format!("{SEARCH}{entity}?{params}")
}

/// `text` as literal terms of a Lucene query, where these characters are
/// operators.
fn lucene(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if r#"+-&|!(){}[]^"~*?:\/"#.contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// The covers to try for what a search of `text` by `artist` answered,
/// best first, empty when nothing passes the minimum score or the answer
/// cannot be read.
pub fn covers(plan: Plan, artist: &str, text: &str, body: &[u8]) -> Vec<String> {
    let Ok(answer) = serde_json::from_slice::<Answer>(body) else {
        return Vec::new();
    };
    let release_groups = match plan {
        Plan::Album => albums(&answer.release_groups, artist, text),
        Plan::Title => releases(&answer.recordings, artist, text),
    };
    release_groups
        .into_iter()
        .filter(|id| is_mbid(id))
        .map(|id| format!("{COVER}{id}/front-500"))
        .collect()
}

/// An actual album before a single of the same name, whose cover is often
/// a label disc, then the original release before its reissues.
fn albums<'a>(found: &'a [ReleaseGroup], artist: &str, text: &str) -> Vec<&'a str> {
    let mut matching: Vec<&ReleaseGroup> = found
        .iter()
        .filter(|group| credited(artist, &group.artist_credit) && text_matches(text, &group.title))
        .collect();
    matching.sort_by_key(|group| (type_rank(group), oldest_first(&group.first_release_date)));
    matching
        .into_iter()
        .take(ALBUMS_TRIED)
        .map(|group| group.id.as_str())
        .collect()
}

fn type_rank(group: &ReleaseGroup) -> u8 {
    match group.primary_type.as_deref() {
        Some("Album") => 0,
        Some("EP") => 1,
        Some("Single") => 2,
        _ => 3,
    }
}

/// The release groups of every matching recording, ranked together: the
/// artist's own plain releases first, then the oldest. A release credited
/// to someone else, a compilation of various artists or a DJ mix, holds
/// the song under another cover. MusicBrainz leaves out the credit of a
/// release that has the recording's.
fn releases<'a>(found: &'a [Recording], artist: &str, text: &str) -> Vec<&'a str> {
    let mut releases: Vec<&Release> = found
        .iter()
        .filter(|recording| {
            credited(artist, &recording.artist_credit) && text_matches(text, &recording.title)
        })
        .flat_map(|recording| &recording.releases)
        .filter(|release| {
            release.artist_credit.is_empty() || credited(artist, &release.artist_credit)
        })
        .collect();
    releases.sort_by_key(|release| (!plain(release), oldest_first(&release.date)));
    let mut groups: Vec<&str> = Vec::new();
    for release in releases {
        let id = release.release_group.id.as_str();
        if !groups.contains(&id) {
            groups.push(id);
        }
    }
    groups.truncate(RELEASE_GROUPS_TRIED);
    groups
}

/// An official album, single or EP of the artist alone, neither live nor
/// a compilation.
fn plain(release: &Release) -> bool {
    let group = &release.release_group;
    let various = release
        .artist_credit
        .iter()
        .any(|credit| credit.name.contains("Various"));
    release.status.as_deref() == Some("Official")
        && matches!(
            group.primary_type.as_deref(),
            Some("Album" | "Single" | "EP")
        )
        && group.secondary_types.is_empty()
        && !various
}

/// Dates sort as text (`1974-09` before `1974-09-13`), and an unknown one
/// after every known one.
fn oldest_first(date: &Option<String>) -> &str {
    date.as_deref()
        .filter(|date| !date.is_empty())
        .unwrap_or("9999")
}

/// Whether `artist` is one of the names of `credits`: the whole credit as
/// written, each artist's name and sort name, and their aliases.
fn credited(artist: &str, credits: &[Credit]) -> bool {
    let whole: String = credits
        .iter()
        .flat_map(|credit| [credit.name.as_str(), credit.joinphrase.as_str()])
        .collect();
    let names = credits.iter().flat_map(|credit| {
        let artist = &credit.artist;
        let aliases = artist
            .aliases
            .iter()
            .flat_map(|alias| [alias.name.as_str(), alias.sort_name.as_str()]);
        [
            credit.name.as_str(),
            artist.name.as_str(),
            artist.sort_name.as_str(),
        ]
        .into_iter()
        .chain(aliases)
    });
    artist_matches(artist, std::iter::once(whole.as_str()).chain(names))
}

/// A MusicBrainz identifier, so that an answer can never make the plugin
/// request anything but a cover.
fn is_mbid(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// The fields read from an answer, missing ones read as empty.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct Answer {
    release_groups: Vec<ReleaseGroup>,
    recordings: Vec<Recording>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct ReleaseGroup {
    id: String,
    title: String,
    primary_type: Option<String>,
    secondary_types: Vec<String>,
    first_release_date: Option<String>,
    artist_credit: Vec<Credit>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct Recording {
    title: String,
    artist_credit: Vec<Credit>,
    releases: Vec<Release>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct Release {
    status: Option<String>,
    date: Option<String>,
    artist_credit: Vec<Credit>,
    release_group: ReleaseGroup,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Credit {
    name: String,
    joinphrase: String,
    artist: Artist,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct Artist {
    name: String,
    sort_name: String,
    aliases: Vec<Alias>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct Alias {
    name: String,
    sort_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: &str = "https://musicbrainz.org/ws/2/";

    fn cover(release_group: &str) -> String {
        format!("https://coverartarchive.org/release-group/{release_group}/front-500")
    }

    fn fixture(name: &str) -> &'static [u8] {
        match name {
            "crime" => include_bytes!(
                "../../tests/fixtures/musicbrainz-release-group-crime-of-the-century.json"
            ),
            "back-in-black" => {
                include_bytes!("../../tests/fixtures/musicbrainz-release-group-back-in-black.json")
            }
            "courroies" => {
                include_bytes!("../../tests/fixtures/musicbrainz-recording-vieilles-courroies.json")
            }
            "bebop-recording" => {
                include_bytes!(
                    "../../tests/fixtures/musicbrainz-recording-cowboy-bebop-sunday.json"
                )
            }
            "bebop-release-group" => include_bytes!(
                "../../tests/fixtures/musicbrainz-release-group-cowboy-bebop-sunday.json"
            ),
            "stargate-recording" => {
                include_bytes!("../../tests/fixtures/musicbrainz-recording-stargate.json")
            }
            "stargate-release-group" => {
                include_bytes!("../../tests/fixtures/musicbrainz-release-group-stargate.json")
            }
            other => panic!("no fixture {other}"),
        }
    }

    // search_url

    /// The very requests of the study, whose answers are the fixtures.
    #[test]
    fn searches_are_those_of_the_study() {
        assert_eq!(
            search_url(Plan::Album, "Supertramp", "Crime Of The Century"),
            format!(
                "{MB}release-group?query=releasegroup%3A%28Crime+Of+The+Century%29+AND+artist%3A%28Supertramp%29&fmt=json&limit=5"
            )
        );
        assert_eq!(
            search_url(Plan::Title, "Harmonium", "Vieilles courroies"),
            format!(
                "{MB}recording?query=recording%3A%28Vieilles+courroies%29+AND+artist%3A%28Harmonium%29&fmt=json&limit=25"
            )
        );
    }

    #[test]
    fn lucene_syntax_in_names_is_escaped() {
        assert_eq!(
            search_url(Plan::Album, "AC/DC", "Back In Black"),
            format!(
                "{MB}release-group?query=releasegroup%3A%28Back+In+Black%29+AND+artist%3A%28AC%5C%2FDC%29&fmt=json&limit=5"
            )
        );
        assert_eq!(
            search_url(
                Plan::Title,
                "Stargate Atlantis",
                "S1:E17 - Derniers messages"
            ),
            format!(
                "{MB}recording?query=recording%3A%28S1%5C%3AE17+%5C-+Derniers+messages%29+AND+artist%3A%28Stargate+Atlantis%29&fmt=json&limit=25"
            )
        );
        let all = r#"+-&|!(){}[]^"~*?:\/"#;
        let url = search_url(Plan::Title, "a", all);
        let escaped: String = all.chars().flat_map(|c| ['\\', c]).collect();
        assert!(
            url.contains(&super::super::form(&[("q", &escaped)])[2..]),
            "{url}"
        );
    }

    // covers: real answers saved by the study

    #[test]
    fn an_album_search_gives_the_album_then_the_next_match() {
        assert_eq!(
            covers(
                Plan::Album,
                "Supertramp",
                "Crime Of The Century",
                fixture("crime")
            ),
            [
                cover("c3749af5-0d0c-3ed1-b1c0-8b4a8f6c3436"),
                cover("89694743-3729-4cc6-b392-d399623f96bd")
            ]
        );
    }

    /// The single of the same name comes first in the answer, with a label
    /// disc for a cover.
    #[test]
    fn an_album_comes_before_a_single_of_the_same_name() {
        assert_eq!(
            covers(
                Plan::Album,
                "AC/DC",
                "Back In Black",
                fixture("back-in-black")
            ),
            [
                cover("d3bc1a64-7561-3787-b680-0003aa50f8f1"),
                cover("79c5075c-630c-498d-b7c6-d83b4a982490")
            ]
        );
    }

    #[test]
    fn a_title_search_gives_the_release_groups_of_its_releases() {
        assert_eq!(
            covers(
                Plan::Title,
                "Harmonium",
                "Vieilles courroies",
                fixture("courroies")
            ),
            [
                cover("dac6bc96-0d9f-315a-974f-e5ab2d685d90"),
                cover("8c304a5d-5e3d-4ecd-8343-a3a3b4a7e9d4")
            ]
        );
    }

    /// The soundtrack of 1998 is credited シートベルツ, so a channel name as
    /// artist finds nothing, which beats a wrong cover.
    #[test]
    fn candidates_below_the_score_give_nothing() {
        let bebop = "Cowboy Bebop (Original Soundtrack) by Seatbelts";
        assert!(covers(Plan::Title, "SUNDAY", bebop, fixture("bebop-recording")).is_empty());
        assert!(covers(Plan::Album, "SUNDAY", bebop, fixture("bebop-release-group")).is_empty());
        let episode = "S1:E17 - Derniers messages";
        let artist = "Stargate Atlantis";
        assert!(covers(Plan::Title, artist, episode, fixture("stargate-recording")).is_empty());
        assert!(
            covers(
                Plan::Album,
                artist,
                episode,
                fixture("stargate-release-group")
            )
            .is_empty()
        );
    }

    #[test]
    fn the_wrong_kind_of_answer_gives_nothing() {
        assert!(
            covers(
                Plan::Album,
                "Harmonium",
                "Vieilles courroies",
                fixture("courroies")
            )
            .is_empty()
        );
    }

    // covers: ranking rules, on answers made for each

    fn credit(name: &str) -> String {
        format!(r#"[{{"name": "{name}", "artist": {{"name": "{name}", "sort-name": "{name}"}}}}]"#)
    }

    fn release(
        group: &str,
        status: &str,
        date: &str,
        primary: &str,
        secondary: &str,
        by: &str,
    ) -> String {
        format!(
            r#"{{"status": "{status}", "date": "{date}", "artist-credit": {}, "release-group": {{"id": "{group}", "primary-type": "{primary}", "secondary-types": [{secondary}]}}}}"#,
            if by.is_empty() {
                "[]".to_owned()
            } else {
                credit(by)
            }
        )
    }

    fn recordings(title: &str, by: &str, releases: &[String]) -> Vec<u8> {
        format!(
            r#"{{"recordings": [{{"title": "{title}", "artist-credit": {}, "releases": [{}]}}]}}"#,
            credit(by),
            releases.join(",")
        )
        .into_bytes()
    }

    const A: &str = "aaaaaaaa-0000-0000-0000-000000000001";
    const B: &str = "bbbbbbbb-0000-0000-0000-000000000002";
    const C: &str = "cccccccc-0000-0000-0000-000000000003";
    const D: &str = "dddddddd-0000-0000-0000-000000000004";

    #[test]
    fn plain_official_releases_come_first_then_the_oldest() {
        let body = recordings(
            "Song",
            "Band",
            &[
                release(A, "Official", "1990", "Album", r#""Compilation""#, ""),
                release(B, "Bootleg", "1970", "Album", "", ""),
                release(C, "Official", "1985", "Single", "", ""),
                release(D, "Official", "1980", "Album", "", ""),
            ],
        );
        assert_eq!(
            covers(Plan::Title, "Band", "Song", &body),
            [cover(D), cover(C), cover(B)]
        );
    }

    /// A compilation of various artists or a DJ mix holds the song, but
    /// its cover is not the artist's.
    #[test]
    fn releases_of_another_artist_are_left_out_and_shared_with_various_ones_come_last() {
        let body = recordings(
            "Song",
            "Band",
            &[
                release(A, "Official", "1970", "Album", "", "DJ Someone"),
                release(B, "Official", "1971", "Album", "", "Band & Various Artists"),
                release(C, "Official", "1990", "Album", "", "Band"),
                release(D, "Official", "1972", "Album", "", "Various Artists"),
            ],
        );
        assert_eq!(
            covers(Plan::Title, "Band", "Song", &body),
            [cover(C), cover(B)]
        );
    }

    #[test]
    fn a_release_group_counts_once_and_three_at_most() {
        let body = recordings(
            "Song",
            "Band",
            &[
                release(A, "Official", "1970", "Album", "", ""),
                release(A, "Official", "1971", "Album", "", ""),
                release(B, "Official", "1972", "Album", "", ""),
                release(C, "Official", "1973", "Album", "", ""),
                release(D, "Official", "1974", "Album", "", ""),
            ],
        );
        assert_eq!(
            covers(Plan::Title, "Band", "Song", &body),
            [cover(A), cover(B), cover(C)]
        );
    }

    #[test]
    fn a_release_group_id_that_is_not_an_mbid_is_never_requested() {
        let body = recordings(
            "Song",
            "Band",
            &[
                release("../../evil", "Official", "1970", "Album", "", ""),
                release("", "Official", "1971", "Album", "", ""),
                release(A, "Official", "1972", "Album", "", ""),
            ],
        );
        assert_eq!(covers(Plan::Title, "Band", "Song", &body), [cover(A)]);
    }

    #[test]
    fn malformed_answers_give_nothing() {
        for body in [
            &b""[..],
            b"not json",
            b"{\"recordings\": [{\"title\": 3}]}",
            b"{\"error\": \"Invalid query\"}",
            b"[]",
            b"\xff\xfe",
        ] {
            assert!(
                covers(Plan::Title, "Band", "Song", body).is_empty(),
                "{body:?}"
            );
            assert!(
                covers(Plan::Album, "Band", "Song", body).is_empty(),
                "{body:?}"
            );
        }
    }
}
