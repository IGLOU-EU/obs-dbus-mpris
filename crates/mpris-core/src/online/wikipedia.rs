//! Wikipedia in English (D-09): the image of the article of an album or a
//! song, for known records only. The search is the same for an album and
//! a title, the article description tells which artist made it.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use super::cleanup::{artist_matches, text_matches};
use super::{Plan, form};

const API: &str = "https://en.wikipedia.org/w/api.php";
/// Articles found, and images tried among them.
const ARTICLES_TRIED: usize = 2;

/// "1974 studio album by Supertramp": the artist of a record.
static BY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bby\s+(.+)$").expect("constant"));
static RECORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(album|single|song|EP|soundtrack|extended play)\b").expect("constant")
});
static ALBUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\balbum\b").expect("constant"));

/// The search for `text` by `artist`. Images of any license, since a
/// cover is almost never free and its article shows it as fair use: the
/// free ones gave nothing in the study.
pub fn search_url(_plan: Plan, artist: &str, text: &str) -> String {
    let search = format!("{text} {artist}");
    let params = form(&[
        ("action", "query"),
        ("format", "json"),
        ("formatversion", "2"),
        ("generator", "search"),
        ("gsrsearch", search.trim()),
        ("gsrlimit", "5"),
        ("prop", "pageimages|description"),
        ("piprop", "thumbnail"),
        ("pithumbsize", "500"),
        ("pilicense", "any"),
    ]);
    format!("{API}?{params}")
}

/// The images to try among the articles a search of `text` by `artist`
/// found, best first: the articles on a record by the artist whose title
/// matches, in the order of the search, an album before a single of the
/// same name for an album. Empty when the answer cannot be read.
pub fn covers(plan: Plan, artist: &str, text: &str, body: &[u8]) -> Vec<String> {
    let Ok(answer) = serde_json::from_slice::<Answer>(body) else {
        return Vec::new();
    };
    let mut pages = answer.query.pages;
    pages.sort_by_key(|page| page.index);
    let mut matching: Vec<&Page> = pages
        .iter()
        .filter(|page| {
            let by = record_artist(&page.description);
            artist_matches(artist, by) && text_matches(text, &page.title)
        })
        .collect();
    if plan == Plan::Album {
        matching.sort_by_key(|page| !ALBUM.is_match(&page.description));
    }
    matching
        .into_iter()
        .take(ARTICLES_TRIED)
        .map(|page| page.thumbnail.source.clone())
        .filter(|source| on_wikimedia(source))
        .collect()
}

/// The artist an article description gives for a record, none for an
/// article on anything else.
fn record_artist(description: &str) -> Option<&str> {
    let by = BY.captures(description)?.get(1)?;
    RECORD.is_match(description).then_some(by.as_str())
}

/// Over HTTPS on a host of Wikimedia, so that an answer can never make the
/// plugin request anything but an image of the article.
fn on_wikimedia(url: &str) -> bool {
    url.strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|host| host.ends_with(".wikimedia.org"))
}

/// The fields read from an answer, missing ones read as empty.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Answer {
    query: Pages,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Pages {
    pages: Vec<Page>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct Page {
    /// Rank in the search, which the answer does not keep in its order.
    index: u32,
    title: String,
    description: String,
    thumbnail: Thumbnail,
}

impl Default for Page {
    /// Unranked pages come last.
    fn default() -> Page {
        Page {
            index: u32::MAX,
            title: String::new(),
            description: String::new(),
            thumbnail: Thumbnail::default(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Thumbnail {
    source: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const UPLOAD: &str = "https://thumb.wikimedia.org/wikipedia/";

    fn fixture(name: &str) -> &'static [u8] {
        match name {
            "crime" => include_bytes!("../../tests/fixtures/wikipedia-crime-of-the-century.json"),
            "back-in-black" => include_bytes!("../../tests/fixtures/wikipedia-back-in-black.json"),
            "courroies" => include_bytes!("../../tests/fixtures/wikipedia-vieilles-courroies.json"),
            "bebop" => include_bytes!("../../tests/fixtures/wikipedia-cowboy-bebop-sunday.json"),
            other => panic!("no fixture {other}"),
        }
    }

    fn pages(pages: &[(&str, &str, &str)]) -> Vec<u8> {
        let pages: Vec<String> = pages
            .iter()
            .enumerate()
            .map(|(index, (title, description, thumbnail))| {
                format!(
                    r#"{{"title": "{title}", "index": {}, "description": "{description}", "thumbnail": {{"source": "{thumbnail}"}}}}"#,
                    index + 1
                )
            })
            .collect();
        format!(r#"{{"query": {{"pages": [{}]}}}}"#, pages.join(",")).into_bytes()
    }

    #[test]
    fn search_asks_for_page_images_of_any_license() {
        assert_eq!(
            search_url(Plan::Album, "AC/DC", "Back In Black"),
            "https://en.wikipedia.org/w/api.php?action=query&format=json&formatversion=2\
             &generator=search&gsrsearch=Back+In+Black+AC%2FDC&gsrlimit=5\
             &prop=pageimages%7Cdescription&piprop=thumbnail&pithumbsize=500&pilicense=any"
        );
        assert_eq!(
            search_url(Plan::Title, "AC/DC", "Back In Black"),
            search_url(Plan::Album, "AC/DC", "Back In Black")
        );
    }

    #[test]
    fn the_article_of_the_album_gives_its_cover() {
        assert_eq!(
            covers(
                Plan::Album,
                "Supertramp",
                "Crime Of The Century",
                fixture("crime")
            ),
            [format!(
                "{UPLOAD}en/thumb/1/15/Supertramp_-_Crime_of_the_Century.jpg/250px-Supertramp_-_Crime_of_the_Century.jpg?utm_source=en.wikipedia.org&utm_campaign=api&utm_content=thumbnail"
            )]
        );
    }

    /// The article of the single comes first in the answer, its image a
    /// label disc.
    #[test]
    fn an_album_search_puts_the_album_before_a_single_of_the_same_name() {
        let album = format!(
            "{UPLOAD}commons/thumb/a/ae/AC-DC_-_Back_In_Black_%28original_1980_album_cover_with_embossed_lettering%29.jpg/250px-AC-DC_-_Back_In_Black_%28original_1980_album_cover_with_embossed_lettering%29.jpg?utm_source=en.wikipedia.org&utm_campaign=api&utm_content=thumbnail"
        );
        let single = format!(
            "{UPLOAD}commons/thumb/d/d2/Back_in_black_by_acdc_US_7-inch_single_side-A.png/250px-Back_in_black_by_acdc_US_7-inch_single_side-A.png?utm_source=en.wikipedia.org&utm_campaign=api&utm_content=thumbnail"
        );
        let answer = fixture("back-in-black");
        assert_eq!(
            covers(Plan::Album, "AC/DC", "Back In Black", answer),
            [album.clone(), single.clone()]
        );
        assert_eq!(
            covers(Plan::Title, "AC/DC", "Back In Black", answer),
            [single, album]
        );
    }

    #[test]
    fn articles_of_other_titles_give_nothing() {
        let courroies = fixture("courroies");
        assert!(covers(Plan::Title, "Harmonium", "Vieilles courroies", courroies).is_empty());
        let bebop = "Cowboy Bebop (Original Soundtrack) by Seatbelts";
        assert!(covers(Plan::Title, "SUNDAY", bebop, fixture("bebop")).is_empty());
    }

    #[test]
    fn only_a_record_by_the_artist_counts() {
        let image = format!("{UPLOAD}en/a.jpg");
        let answer = pages(&[
            ("School", "2004 film by Supertramp", &image),
            ("School", "", &image),
            ("School", "Song By Supertramp", &image),
            ("School", "1983 single by Supertramp", &image),
        ]);
        assert_eq!(
            covers(Plan::Title, "Supertramp", "School", &answer),
            [image.clone()]
        );
        let other = pages(&[("School", "1983 single by Another Band", &image)]);
        assert!(covers(Plan::Title, "Supertramp", "School", &other).is_empty());
    }

    #[test]
    fn two_articles_at_most_and_only_those_with_an_image() {
        let (a, b) = (format!("{UPLOAD}en/a.jpg"), format!("{UPLOAD}en/b.jpg"));
        let answer = pages(&[
            ("School (song)", "single by Supertramp", ""),
            ("School", "song by Supertramp", &a),
            ("School (live)", "song by Supertramp", &b),
        ]);
        assert_eq!(covers(Plan::Title, "Supertramp", "School", &answer), [a]);
    }

    #[test]
    fn an_image_off_wikimedia_is_never_requested() {
        let answer = pages(&[
            (
                "School",
                "single by Supertramp",
                "https://example.org/a.jpg",
            ),
            (
                "School",
                "single by Supertramp",
                "http://upload.wikimedia.org/a.jpg",
            ),
            (
                "School",
                "single by Supertramp",
                "https://wikimedia.org.evil/a.jpg",
            ),
        ]);
        assert!(covers(Plan::Title, "Supertramp", "School", &answer).is_empty());
    }

    #[test]
    fn malformed_answers_give_nothing() {
        for body in [
            &b""[..],
            b"<html>",
            b"{\"query\": {\"pages\": {}}}",
            b"{\"error\": {\"code\": \"maxlag\"}}",
        ] {
            assert!(
                covers(Plan::Album, "Supertramp", "School", body).is_empty(),
                "{body:?}"
            );
        }
    }
}
