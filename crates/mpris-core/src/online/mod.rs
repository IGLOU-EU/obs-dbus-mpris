//! Online covers (B-18): what to look up for a track, and how to read
//! what the services answer. Pure, so it all runs under `cargo test`
//! without a network: the requests themselves are made by
//! [`crate::network`].

pub mod cleanup;
pub mod musicbrainz;
pub mod wikipedia;
pub mod youtube;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use cleanup::Query;

/// What a search looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Album,
    Title,
}

/// The searches for `query`, in their order: the album first, then the
/// title (B-18). A lone field is also tried in the other role, since a
/// video title is often an album name, and an album field sometimes the
/// name of the track.
pub fn plans(query: &Query) -> Vec<(Plan, &str)> {
    let (title, album) = (query.title.as_str(), query.album.as_str());
    let mut plans = Vec::new();
    if !album.is_empty() {
        plans.push((Plan::Album, album));
    }
    if !title.is_empty() {
        plans.push((Plan::Title, title));
    }
    if album.is_empty() && !title.is_empty() {
        plans.push((Plan::Album, title));
    }
    if title.is_empty() && !album.is_empty() {
        plans.push((Plan::Title, album));
    }
    plans
}

/// What `application/x-www-form-urlencoded` leaves as is.
const FORM: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'_')
    .remove(b'.')
    .remove(b'-')
    .remove(b'~');

/// A query string written as Python's `urlencode` writes it, so that the
/// requests are exactly those of the study.
fn form(pairs: &[(&str, &str)]) -> String {
    let encode = |text| {
        utf8_percent_encode(text, FORM)
            .to_string()
            .replace("%20", "+")
    };
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", encode(key), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(artist: &str, title: &str, album: &str) -> Query {
        Query {
            artist: artist.into(),
            title: title.into(),
            album: album.into(),
        }
    }

    #[test]
    fn album_comes_before_title() {
        let q = query("Supertramp", "School", "Crime Of The Century");
        assert_eq!(
            plans(&q),
            [
                (Plan::Album, "Crime Of The Century"),
                (Plan::Title, "School")
            ]
        );
    }

    #[test]
    fn a_lone_field_is_tried_in_both_roles() {
        let title_only = query("Harmonium", "Vieilles courroies", "");
        assert_eq!(
            plans(&title_only),
            [
                (Plan::Title, "Vieilles courroies"),
                (Plan::Album, "Vieilles courroies")
            ]
        );
        let album_only = query("CRYSTAL KING", "", "愛をとりもどせ!!");
        assert_eq!(
            plans(&album_only),
            [
                (Plan::Album, "愛をとりもどせ!!"),
                (Plan::Title, "愛をとりもどせ!!")
            ]
        );
    }

    #[test]
    fn form_encodes_as_python_urlencode() {
        assert_eq!(
            form(&[
                ("query", "recording:(L'Arc\\-en\\-Ciel) AND x"),
                ("limit", "5")
            ]),
            "query=recording%3A%28L%27Arc%5C-en%5C-Ciel%29+AND+x&limit=5"
        );
        assert_eq!(form(&[("q", "a+b %_.~é|")]), "q=a%2Bb+%25_.~%C3%A9%7C");
    }
}
