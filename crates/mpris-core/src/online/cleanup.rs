//! Clean-up of player metadata into one search query, and the minimum
//! match score a candidate has to pass (B-18).
//!
//! Ported from the contract the study behind D-09 set and tested
//! (`scripts/nettoyage.py`): a wrong cover is worse than none, so a track
//! without an artist is not searched, and every candidate is checked
//! against the query. The study's "X by Y" variant is left out, since its
//! only effect there was a wrong cover.

use std::iter;
use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Titles and albums.
const TEXT_THRESHOLD: f64 = 0.85;
/// Stricter, since two real artists often differ by one letter.
const ARTIST_THRESHOLD: f64 = 0.9;

/// The dashes players put between an artist and a title, or before a
/// version: hyphen, en dash, em dash.
const SEPARATOR: &str = r"\s+[-\x{2013}\x{2014}]\s+";

/// Markers that say which version or which video a track is, never what
/// it is called, removed from the end of a text.
static SUFFIXES: LazyLock<[Regex; 4]> = LazyLock::new(|| {
    [
        // (Remastered), [Official Video], (MOVIE ver.), (2005), 【MV】...
        pattern(r"\s*[(\[【（][^()\[\]【】（）]*[)\]】）]\s*$"),
        // - 1998, - Remastered 2015, - Live, - Radio Edit...
        pattern(&format!(
            r"(?i){SEPARATOR}(\d{{4}}|(\d{{4}}\s+)?remaster(ed)?(\s+\d{{4}})?|live\b.*|radio edit|single version|mono|stereo)\s*$"
        )),
        pattern(
            r"(?i)\s+(official\s+(music\s+|lyric\s+)?video|official\s+audio|lyric\s+video|music\s+video|official\s+visualizer)\s*$",
        ),
        // Video titles that append their channel or source.
        pattern(r"\s+(//|\|)\s+.*$"),
    ]
});

/// `Artist - Title`, the form of many video titles.
static ARTIST_TITLE: LazyLock<Regex> =
    LazyLock::new(|| pattern(&format!(r"^(.+?){SEPARATOR}(.+)$")));

/// `S U N D A Y`: a name spelled with spaced letters.
static SPACED: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(?:\w\s)+\w$"));

/// What joins the members of a credit shared by several artists.
static JOINERS: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"(?i)\s+(?:feat\.?|ft\.?|featuring|with|x|and|&)\s+|\s*,\s*|\s*;\s*")
});

static LEADING_THE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^the\s+"));

/// The patterns above are constants, so a typo fails every test.
fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("constant pattern")
}

/// What a service is asked for: one artist, and a title or an album.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Query {
    pub artist: String,
    pub title: String,
    pub album: String,
}

/// `text` without its version and video markers, removed one after the
/// other until none is left. Never empty unless `text` is blank: a text
/// that is only a marker stays whole.
pub fn strip_suffixes(text: &str) -> String {
    let mut text = text.trim().to_owned();
    let mut changed = true;
    while changed {
        changed = false;
        for suffix in SUFFIXES.iter() {
            let shorter = suffix.replace_all(&text, "").trim().to_owned();
            if !shorter.is_empty() && shorter != text {
                text = shorter;
                changed = true;
            }
        }
    }
    text
}

fn collapse(text: String) -> String {
    if SPACED.is_match(&text) {
        text.replace(' ', "")
    } else {
        text
    }
}

/// The query for a track, `None` when it names no artist, or only an
/// artist: a search without one is what gave wrong covers in the study.
/// Without an artist, a title of the form `Artist - Title` gives both.
pub fn query(title: &str, artist: &str, album: &str) -> Option<Query> {
    let (title, album) = (strip_suffixes(title), strip_suffixes(album));
    let artist = collapse(strip_suffixes(artist));
    let query = if artist.is_empty() {
        let parts = ARTIST_TITLE.captures(&title)?;
        Query {
            artist: strip_suffixes(&parts[1]),
            title: strip_suffixes(&parts[2]),
            album,
        }
    } else {
        Query {
            artist,
            title,
            album,
        }
    };
    let named = !query.title.is_empty() || !query.album.is_empty();
    (!query.artist.is_empty() && named).then_some(query)
}

/// Letters and digits only, without case or accents, full-width forms
/// folded, so that punctuation and typography never tell two names apart.
pub fn normalize(text: &str) -> String {
    let lower: String = text.nfkc().flat_map(char::to_lowercase).collect();
    // The rest of Python's `casefold`, which the thresholds were set with,
    // that matters for names.
    let folded = lower.replace('ς', "σ").replace('ß', "ss");
    folded
        .nfd()
        .filter(|c| !is_combining_mark(*c) && c.is_alphanumeric())
        .collect()
}

/// Similarity of two texts in `[0, 1]`, as Python's
/// `difflib.SequenceMatcher.ratio` gives it, which set the thresholds:
/// twice the characters of the matching blocks over both lengths. difflib
/// also treats frequent characters as junk, but only in texts of 200
/// characters or more, far longer than any name compared here. Empty
/// texts never match.
pub fn ratio(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    2.0 * matched(&a, &b) as f64 / (a.len() + b.len()) as f64
}

/// Characters in the blocks difflib matches: the longest common block,
/// then the same on each side of it.
fn matched(a: &[char], b: &[char]) -> usize {
    let (i, j, size) = longest_block(a, b);
    if size == 0 {
        return 0;
    }
    size + matched(&a[..i], &b[..j]) + matched(&a[i + size..], &b[j + size..])
}

/// `(start in a, start in b, size)` of the longest common block, the
/// earliest in `a`, then in `b`, among blocks of the same size.
fn longest_block(a: &[char], b: &[char]) -> (usize, usize, usize) {
    let mut best = (0, 0, 0);
    // Length of the common run ending just before each position of `b`,
    // for the previous character of `a`.
    let mut previous = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![0; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            if ca == cb {
                let size = previous[j] + 1;
                current[j + 1] = size;
                if size > best.2 {
                    best = (i + 1 - size, j + 1 - size, size);
                }
            }
        }
        previous = current;
    }
    best
}

/// Whether the artist asked for is one of `names`, the names a candidate
/// goes by (credit, sort name, aliases). Credits shared by several artists
/// match member by member, on either side.
pub fn artist_matches<'a>(query: &str, names: impl IntoIterator<Item = &'a str>) -> bool {
    if artist_key(query).is_empty() {
        return false;
    }
    let wanted: Vec<String> = members(query)
        .map(artist_key)
        .filter(|key| !key.is_empty())
        .collect();
    names
        .into_iter()
        .filter(|name| !name.is_empty())
        .flat_map(members)
        .map(artist_key)
        .any(|member| {
            wanted
                .iter()
                .any(|key| ratio(key, &member) >= ARTIST_THRESHOLD)
        })
}

/// `name` itself, then each member of a shared credit.
fn members(name: &str) -> impl Iterator<Item = &str> {
    iter::once(name).chain(JOINERS.split(name).filter(|part| !part.is_empty()))
}

/// A leading "The" is often left out of a band's name.
fn artist_key(name: &str) -> String {
    normalize(&LEADING_THE.replace(&strip_suffixes(name), ""))
}

/// Whether a candidate's title or album is the one asked for, version
/// markers aside.
pub fn text_matches(query: &str, candidate: &str) -> bool {
    let query = normalize(&strip_suffixes(query));
    let candidate = normalize(&strip_suffixes(candidate));
    ratio(&query, &candidate) >= TEXT_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query_of(title: &str, artist: &str, album: &str) -> Option<(String, String, String)> {
        query(title, artist, album).map(|q| (q.artist, q.title, q.album))
    }

    fn owned(artist: &str, title: &str, album: &str) -> Option<(String, String, String)> {
        Some((artist.to_owned(), title.to_owned(), album.to_owned()))
    }

    // strip_suffixes

    #[test]
    fn version_markers_are_removed() {
        let cases = [
            ("Crime Of The Century (Remastered)", "Crime Of The Century"),
            ("愛をとりもどせ!! (MOVIE ver.)", "愛をとりもどせ!!"),
            ("Hey Jude - Remastered 2015", "Hey Jude"),
            ("One More Time (Official Video)", "One More Time"),
            ("Song [Official Music Video]", "Song"),
            ("Album - 1998", "Album"),
            ("Song \u{2013} Live at Wembley", "Song"),
            ("Song 【MV】", "Song"),
            ("Song Official Audio", "Song"),
        ];
        for (text, expected) in cases {
            assert_eq!(strip_suffixes(text), expected, "{text}");
        }
    }

    #[test]
    fn markers_pile_up_and_all_go() {
        assert_eq!(
            strip_suffixes("Song - Remastered 2015 (Official Video) [HD]"),
            "Song"
        );
    }

    #[test]
    fn everything_after_a_double_slash_or_a_pipe_goes() {
        assert_eq!(
            strip_suffixes("Kokoroko - ABUSEY JUNCTION // WE OUT HERE"),
            "Kokoroko - ABUSEY JUNCTION"
        );
        assert_eq!(strip_suffixes("Song | Channel name"), "Song");
    }

    #[test]
    fn leading_parentheses_and_inner_hyphens_stay() {
        for text in ["(Don't Fear) The Reaper", "Anti-Hero", "Song 2", "AC/DC"] {
            assert_eq!(strip_suffixes(text), text);
        }
    }

    #[test]
    fn a_text_made_only_of_a_marker_stays_whole() {
        assert_eq!(strip_suffixes("(Untitled)"), "(Untitled)");
        assert_eq!(strip_suffixes("  (Untitled)  "), "(Untitled)");
        assert_eq!(strip_suffixes(""), "");
    }

    // query

    #[test]
    fn a_plain_track_gives_its_cleaned_fields() {
        assert_eq!(
            query_of("School", "Supertramp", "Crime Of The Century (Remastered)"),
            owned("Supertramp", "School", "Crime Of The Century")
        );
    }

    #[test]
    fn without_artist_an_artist_title_form_is_split() {
        assert_eq!(
            query_of("Kokoroko - ABUSEY JUNCTION // WE OUT HERE", "", ""),
            owned("Kokoroko", "ABUSEY JUNCTION", "")
        );
        assert_eq!(
            query_of(
                "Stargate Atlantis - S1:E17 - Derniers messages (2005)",
                "",
                ""
            ),
            owned("Stargate Atlantis", "S1:E17 - Derniers messages", "")
        );
    }

    #[test]
    fn with_an_artist_the_title_is_never_split() {
        assert_eq!(
            query_of("Daft Punk - One More Time", "Daft Punk", ""),
            owned("Daft Punk", "Daft Punk - One More Time", "")
        );
    }

    #[test]
    fn without_artist_nor_separator_nothing_is_searched() {
        assert_eq!(query_of("Nothing Else Matters", "", ""), None);
        assert_eq!(query_of("", "", "Some Album"), None);
    }

    #[test]
    fn an_artist_alone_is_not_searched() {
        assert_eq!(query_of("", "Supertramp", ""), None);
        assert_eq!(query_of("(Official Video)", "", ""), None);
    }

    #[test]
    fn an_album_alone_with_its_artist_is_searched() {
        assert_eq!(
            query_of("", "CRYSTAL KING", "愛をとりもどせ!! (MOVIE ver.)"),
            owned("CRYSTAL KING", "", "愛をとりもどせ!!")
        );
    }

    /// The study's "X by Y" variant found the Netflix series of 2021 for
    /// the Cowboy Bebop soundtrack of 1998: a channel name stays the artist.
    #[test]
    fn by_in_a_title_never_makes_another_artist() {
        assert_eq!(
            query_of(
                "Cowboy Bebop (Original Soundtrack) by Seatbelts - 1998",
                "S U N D A Y",
                ""
            ),
            owned(
                "SUNDAY",
                "Cowboy Bebop (Original Soundtrack) by Seatbelts",
                ""
            )
        );
        assert_eq!(
            query_of("Stand by Me", "Ben E. King", ""),
            owned("Ben E. King", "Stand by Me", "")
        );
    }

    #[test]
    fn spaced_letters_collapse_only_when_all_single() {
        let artist = |name: &str| query(name, name, "").map(|q| q.artist);
        assert_eq!(artist("S U N D A Y").as_deref(), Some("SUNDAY"));
        assert_eq!(
            artist("A Tribe Called Quest").as_deref(),
            Some("A Tribe Called Quest")
        );
    }

    // normalize and ratio

    #[test]
    fn normalizing_keeps_letters_and_digits_without_case_or_accents() {
        assert_eq!(normalize("Blue Öyster Cult"), "blueoystercult");
        assert_eq!(normalize("L’Arc〜en〜Ciel"), "larcenciel");
        assert_eq!(normalize("ＡＢＣ　１２３"), "abc123");
        assert_eq!(normalize("AC/DC"), "acdc");
        assert_eq!(normalize("が"), "か");
        assert_eq!(normalize("!!"), "");
    }

    /// As Python's `casefold`, which the thresholds were set with.
    #[test]
    fn normalizing_folds_case_beyond_lower_case() {
        assert_eq!(normalize("Weiß"), normalize("WEISS"));
        assert_eq!(normalize("μπαλάντες"), normalize("ΜΠΑΛΑΝΤΕΣ"));
    }

    #[test]
    fn ratio_is_the_one_of_difflib() {
        let cases = [
            ("abcd", "abcd", 1.0),
            ("abcd", "bcde", 0.75),
            ("abxcd", "abcd", 8.0 / 9.0),
            ("mariyatakeuchi", "memiyatakeuchi", 24.0 / 28.0),
            ("school", "oldschoolhits", 12.0 / 19.0),
            ("acbd", "abcd", 0.75),
            ("abcd", "wxyz", 0.0),
        ];
        for (a, b, expected) in cases {
            assert!((ratio(a, b) - expected).abs() < 1e-12, "{a} {b}");
        }
    }

    /// difflib keeps the longest block that starts earliest in the first
    /// text, and the total depends on that choice.
    #[test]
    fn ratio_breaks_ties_as_difflib_does() {
        assert_eq!(ratio("tide", "diet"), 0.25);
        assert_eq!(ratio("aaab", "abaa"), 0.5);
    }

    #[test]
    fn empty_texts_never_match() {
        assert_eq!(ratio("", "abc"), 0.0);
        assert_eq!(ratio("abc", ""), 0.0);
        assert_eq!(ratio("", ""), 0.0);
    }

    // artist_matches and text_matches

    #[test]
    fn artists_tolerate_punctuation_and_joiners() {
        let cases = [
            ("Black Hill, Silent Island", "Black Hill & Silent Island"),
            ("L'Arc-en-Ciel", "L’Arc〜en〜Ciel"),
            ("AC/DC", "AC/DC"),
            ("Metric", "Metric feat. Brie Larson"),
            ("Black Hill, Silent Island", "Black Hill"),
            ("The Beatles", "Beatles"),
        ];
        for (query, name) in cases {
            assert!(artist_matches(query, [name]), "{query} vs {name}");
        }
    }

    #[test]
    fn artists_refuse_another_artist() {
        let cases = [
            ("Black Sheep", "Metric feat. Brie Larson"),
            ("SUNDAY", "Sunday Service Choir"),
            ("", "AC/DC"),
            ("Me", "Mel"),
            ("Mariya Takeuchi", "Memiya Takeuchi"),
            ("CRYSTAL KING", "King Kong"),
        ];
        for (query, name) in cases {
            assert!(!artist_matches(query, [name]), "{query} vs {name}");
        }
    }

    #[test]
    fn any_name_of_the_candidate_may_match() {
        assert!(artist_matches(
            "Seatbelts",
            ["シートベルツ", "", "Seatbelts"]
        ));
        assert!(!artist_matches("Seatbelts", ["シートベルツ", ""]));
        assert!(!artist_matches("Seatbelts", []));
    }

    #[test]
    fn texts_ignore_the_version_suffix_of_the_candidate() {
        let cases = [
            ("Back In Black", "Back in Black (2003 remaster)"),
            ("Hey Jude", "Hey Jude - Remastered 2015"),
            ("Driver's High", "Driver’s High"),
            ("Crime Of The Century", "Crime of the Century (album)"),
        ];
        for (query, candidate) in cases {
            assert!(text_matches(query, candidate), "{query} vs {candidate}");
        }
    }

    #[test]
    fn texts_refuse_a_partial_or_other_title() {
        let cases = [
            ("School", "Old School Hits"),
            ("Part 04 - Our Mutual Friend Audiobook", "Our Mutual Friend"),
            ("", "Anything"),
            ("Anything", ""),
            ("Harmonium", "Vieilles courroies"),
        ];
        for (query, candidate) in cases {
            assert!(!text_matches(query, candidate), "{query} vs {candidate}");
        }
    }
}
