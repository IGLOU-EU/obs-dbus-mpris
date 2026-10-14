//! Online covers (B-18): what to look up for a track, and how to read
//! what the services answer. Pure, so it all runs under `cargo test`
//! without a network: the requests themselves are made by
//! [`crate::network`].

pub mod cleanup;
pub mod musicbrainz;
pub mod wikipedia;
pub mod youtube;

use std::collections::HashMap;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::art::ArtError;
use crate::content::artist;
use crate::model::{Art, ArtImage, PlaybackStatus, Player, Service, Settings, Track};
use cleanup::Query;

/// One lookup: a service and what it is asked, which is also the key of
/// its result in the session cache (B-18).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Request {
    MusicBrainz(Query),
    Wikipedia(Query),
    /// The id of a video.
    YouTube(String),
}

/// What `service` is asked for `track`, `None` when the track gives it
/// nothing to look up: no artist to search, no video page.
pub fn request(service: Service, track: &Track) -> Option<Request> {
    let search = || {
        let artist = artist(track);
        cleanup::query(
            track.title.as_deref().unwrap_or_default(),
            artist.as_deref().unwrap_or_default(),
            track.album.as_deref().unwrap_or_default(),
        )
    };
    match service {
        Service::MusicBrainz => search().map(Request::MusicBrainz),
        Service::Wikipedia => search().map(Request::Wikipedia),
        Service::YouTube => youtube::video_id(track.url.as_deref()?).map(Request::YouTube),
    }
}

/// The bytes of the cover `request` finds, `None` when there is none.
/// `fetch` gets one URL and answers the body of a successful response
/// only. A URL is fetched once per lookup, since Wikipedia searches an
/// album and a title alike.
pub fn look_up(
    request: &Request,
    mut fetch: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    let mut fetched: HashMap<String, Option<Vec<u8>>> = HashMap::new();
    let mut get = |url: String| {
        fetched
            .entry(url)
            .or_insert_with_key(|url| fetch(url))
            .clone()
    };
    match request {
        Request::MusicBrainz(query) => search(
            query,
            &mut get,
            musicbrainz::search_url,
            musicbrainz::covers,
        ),
        Request::Wikipedia(query) => {
            search(query, &mut get, wikipedia::search_url, wikipedia::covers)
        }
        Request::YouTube(id) => get(youtube::thumbnail_url(id)),
    }
}

/// Each search of `query` in turn, and each cover it finds, until one
/// answers.
fn search(
    query: &Query,
    get: &mut impl FnMut(String) -> Option<Vec<u8>>,
    search_url: fn(Plan, &str, &str) -> String,
    covers: fn(Plan, &str, &str, &[u8]) -> Vec<String>,
) -> Option<Vec<u8>> {
    for (plan, text) in plans(query) {
        let Some(answer) = get(search_url(plan, &query.artist, text)) else {
            continue;
        };
        for cover in covers(plan, &query.artist, text, &answer) {
            if let Some(image) = get(cover) {
                return Some(image);
            }
        }
    }
    None
}

/// The cover in `bytes`, decoded as a local one is (D-03). A YouTube
/// thumbnail keeps only the square of its picture.
pub fn decode(request: &Request, bytes: &[u8]) -> Result<ArtImage, ArtError> {
    let image = crate::art::decode(bytes)?;
    Ok(match request {
        Request::YouTube(_) => youtube::crop(image),
        Request::MusicBrainz(_) | Request::Wikipedia(_) => image,
    })
}

/// What a source asks the core for: a cover from `service` for the player
/// it shows, should that player have none of its own (B-18).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Wish {
    pub bus_name: String,
    pub service: Service,
}

/// The wish of a source that shows `player` with `settings`: none while it
/// shows no track, or no thumbnail, or picked no service, so that nothing
/// is sent that nobody would see (B-48).
pub fn wish(player: Option<&Player>, settings: &Settings) -> Option<Wish> {
    let player = player.filter(|player| player.status != PlaybackStatus::Stopped)?;
    let service = settings.art_online.filter(|_| settings.show_art)?;
    Some(Wish {
        bus_name: player.bus_name.clone(),
        service,
    })
}

/// Whether a player's own cover leaves room for an online one: none, or
/// one that could not be read (B-18). A pending one may still come.
pub fn needs_lookup(art: &Art) -> bool {
    matches!(art, Art::None | Art::Failed)
}

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
    use std::io::Cursor;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::model::{PlayerKey, PositionAnchor};

    fn track(title: &str, artists: &[&str], album: &str) -> Track {
        Track {
            title: (!title.is_empty()).then(|| title.into()),
            artists: artists.iter().map(|a| a.to_string()).collect(),
            album: (!album.is_empty()).then(|| album.into()),
            ..Track::default()
        }
    }

    fn school() -> Track {
        track(
            "School",
            &["Supertramp"],
            "Crime Of The Century (Remastered)",
        )
    }

    fn on_page(url: &str) -> Track {
        Track {
            url: Some(url.into()),
            ..track("Me at the zoo", &[], "")
        }
    }

    // request

    #[test]
    fn searches_ask_the_cleaned_metadata() {
        let expected = Query {
            artist: "Supertramp".into(),
            title: "School".into(),
            album: "Crime Of The Century".into(),
        };
        assert_eq!(
            request(Service::MusicBrainz, &school()),
            Some(Request::MusicBrainz(expected.clone()))
        );
        assert_eq!(
            request(Service::Wikipedia, &school()),
            Some(Request::Wikipedia(expected))
        );
    }

    #[test]
    fn several_artists_are_joined_and_album_artists_stand_in() {
        let duet = track("Song", &["A", "B"], "");
        let ask = |track: &Track| match request(Service::MusicBrainz, track) {
            Some(Request::MusicBrainz(query)) => query.artist,
            other => panic!("{other:?}"),
        };
        assert_eq!(ask(&duet), "A, B");
        let band = Track {
            album_artists: vec!["Band".into()],
            ..track("Song", &[], "")
        };
        assert_eq!(ask(&band), "Band");
    }

    #[test]
    fn a_track_without_artist_is_not_searched() {
        for service in [Service::MusicBrainz, Service::Wikipedia] {
            assert_eq!(request(service, &track("Song", &[], "Album")), None);
            assert_eq!(request(service, &Track::default()), None);
        }
    }

    #[test]
    fn youtube_asks_the_video_of_the_page() {
        let page = on_page("https://www.youtube.com/watch?v=jNQXAC9IVRw");
        assert_eq!(
            request(Service::YouTube, &page),
            Some(Request::YouTube("jNQXAC9IVRw".into()))
        );
        for other in [on_page("https://example.org/"), school()] {
            assert_eq!(request(Service::YouTube, &other), None);
        }
    }

    // look_up, on the answers the study saved

    /// Answers by URL, and every URL asked, in order.
    struct Web {
        pages: HashMap<String, Vec<u8>>,
        asked: Vec<String>,
    }

    impl Web {
        fn new(pages: &[(String, &[u8])]) -> Web {
            Web {
                pages: pages.iter().map(|(u, b)| (u.clone(), b.to_vec())).collect(),
                asked: Vec::new(),
            }
        }

        fn look_up(&mut self, request: &Request) -> Option<Vec<u8>> {
            look_up(request, |url| {
                self.asked.push(url.to_owned());
                self.pages.get(url).cloned()
            })
        }
    }

    fn mb(plan: Plan, artist: &str, text: &str) -> String {
        musicbrainz::search_url(plan, artist, text)
    }

    fn caa(group: &str) -> String {
        format!("https://coverartarchive.org/release-group/{group}/front-500")
    }

    const CRIME: &str = "c3749af5-0d0c-3ed1-b1c0-8b4a8f6c3436";
    const CRIME_LIVE: &str = "89694743-3729-4cc6-b392-d399623f96bd";

    fn ask(service: Service, track: &Track) -> Request {
        request(service, track).expect("a request")
    }

    #[test]
    fn musicbrainz_finds_the_album_in_two_requests() {
        let search = mb(Plan::Album, "Supertramp", "Crime Of The Century");
        let mut web = Web::new(&[
            (
                search.clone(),
                include_bytes!(
                    "../../tests/fixtures/musicbrainz-release-group-crime-of-the-century.json"
                ),
            ),
            (caa(CRIME), b"cover"),
        ]);
        assert_eq!(
            web.look_up(&ask(Service::MusicBrainz, &school())),
            Some(b"cover".to_vec())
        );
        assert_eq!(web.asked, [search, caa(CRIME)]);
    }

    #[test]
    fn a_release_group_without_cover_passes_to_the_next() {
        let search = mb(Plan::Album, "Supertramp", "Crime Of The Century");
        let mut web = Web::new(&[
            (
                search.clone(),
                include_bytes!(
                    "../../tests/fixtures/musicbrainz-release-group-crime-of-the-century.json"
                ),
            ),
            (caa(CRIME_LIVE), b"live"),
        ]);
        assert_eq!(
            web.look_up(&ask(Service::MusicBrainz, &school())),
            Some(b"live".to_vec())
        );
        assert_eq!(web.asked, [search, caa(CRIME), caa(CRIME_LIVE)]);
    }

    #[test]
    fn a_failed_search_passes_to_the_next_plan() {
        let album = mb(Plan::Album, "Supertramp", "Crime Of The Century");
        let title = mb(Plan::Title, "Supertramp", "School");
        let mut web = Web::new(&[]);
        assert_eq!(web.look_up(&ask(Service::MusicBrainz, &school())), None);
        assert_eq!(web.asked, [album, title]);
    }

    /// The study's "X by Y" variant found the 2021 Netflix soundtrack here.
    #[test]
    fn musicbrainz_finds_no_cover_for_a_channel_as_artist() {
        let bebop = track(
            "Cowboy Bebop (Original Soundtrack) by Seatbelts - 1998",
            &["S U N D A Y"],
            "",
        );
        let text = "Cowboy Bebop (Original Soundtrack) by Seatbelts";
        let searches = [
            mb(Plan::Title, "SUNDAY", text),
            mb(Plan::Album, "SUNDAY", text),
        ];
        let mut web = Web::new(&[
            (
                searches[0].clone(),
                include_bytes!(
                    "../../tests/fixtures/musicbrainz-recording-cowboy-bebop-sunday.json"
                ),
            ),
            (
                searches[1].clone(),
                include_bytes!(
                    "../../tests/fixtures/musicbrainz-release-group-cowboy-bebop-sunday.json"
                ),
            ),
        ]);
        assert_eq!(web.look_up(&ask(Service::MusicBrainz, &bebop)), None);
        assert_eq!(web.asked, searches);
    }

    #[test]
    fn musicbrainz_finds_no_cover_for_a_series_episode() {
        let episode = track(
            "Stargate Atlantis - S1:E17 - Derniers messages (2005)",
            &[],
            "",
        );
        let (artist, text) = ("Stargate Atlantis", "S1:E17 - Derniers messages");
        let searches = [mb(Plan::Title, artist, text), mb(Plan::Album, artist, text)];
        let mut web = Web::new(&[
            (
                searches[0].clone(),
                include_bytes!("../../tests/fixtures/musicbrainz-recording-stargate.json"),
            ),
            (
                searches[1].clone(),
                include_bytes!("../../tests/fixtures/musicbrainz-release-group-stargate.json"),
            ),
        ]);
        assert_eq!(web.look_up(&ask(Service::MusicBrainz, &episode)), None);
        assert_eq!(web.asked, searches);
    }

    #[test]
    fn wikipedia_finds_the_album_article_image() {
        let back = track("Back In Black", &["AC/DC"], "Back In Black");
        let search = wikipedia::search_url(Plan::Album, "AC/DC", "Back In Black");
        let album = "https://thumb.wikimedia.org/wikipedia/commons/thumb/a/ae/AC-DC_-_Back_In_Black_%28original_1980_album_cover_with_embossed_lettering%29.jpg/250px-AC-DC_-_Back_In_Black_%28original_1980_album_cover_with_embossed_lettering%29.jpg?utm_source=en.wikipedia.org&utm_campaign=api&utm_content=thumbnail";
        let mut web = Web::new(&[
            (
                search.clone(),
                include_bytes!("../../tests/fixtures/wikipedia-back-in-black.json"),
            ),
            (album.to_owned(), b"album"),
        ]);
        assert_eq!(
            web.look_up(&ask(Service::Wikipedia, &back)),
            Some(b"album".to_vec())
        );
        assert_eq!(web.asked, [search, album.to_owned()]);
    }

    /// Wikipedia searches an album and a title the same way.
    #[test]
    fn a_url_is_asked_once_per_lookup() {
        let courroies = track("Vieilles courroies (Remastered)", &["Harmonium"], "");
        let search = wikipedia::search_url(Plan::Title, "Harmonium", "Vieilles courroies");
        let mut web = Web::new(&[(
            search.clone(),
            include_bytes!("../../tests/fixtures/wikipedia-vieilles-courroies.json"),
        )]);
        assert_eq!(web.look_up(&ask(Service::Wikipedia, &courroies)), None);
        assert_eq!(web.asked, [search]);
    }

    #[test]
    fn youtube_asks_only_the_thumbnail() {
        let thumbnail = youtube::thumbnail_url("jNQXAC9IVRw");
        let request = Request::YouTube("jNQXAC9IVRw".into());
        let mut web = Web::new(&[(thumbnail.clone(), b"thumbnail")]);
        assert_eq!(web.look_up(&request), Some(b"thumbnail".to_vec()));
        assert_eq!(web.asked, [thumbnail.clone()]);
        let mut missing = Web::new(&[]);
        assert_eq!(missing.look_up(&request), None);
        assert_eq!(missing.asked, [thumbnail]);
    }

    // decode

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbImage::new(width, height)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("encode");
        bytes
    }

    #[test]
    fn a_youtube_thumbnail_loses_its_black_bars() {
        let image =
            decode(&Request::YouTube("jNQXAC9IVRw".into()), &png(480, 360)).expect("decoded");
        assert_eq!((image.width, image.height), (270, 270));
    }

    #[test]
    fn a_cover_is_decoded_as_it_comes() {
        let request = ask(Service::MusicBrainz, &school());
        let image = decode(&request, &png(500, 498)).expect("decoded");
        assert_eq!((image.width, image.height), (500, 498));
    }

    #[test]
    fn bytes_that_are_no_image_are_refused() {
        for request in [
            ask(Service::MusicBrainz, &school()),
            Request::YouTube("jNQXAC9IVRw".into()),
        ] {
            assert!(decode(&request, b"<html>404</html>").is_err());
            assert!(decode(&request, b"").is_err());
        }
    }

    // wish and needs_lookup

    fn player(status: PlaybackStatus) -> Player {
        Player {
            unique_name: ":1.1".into(),
            bus_name: "org.mpris.MediaPlayer2.firefox".into(),
            key: PlayerKey("firefox".into()),
            identity: "Firefox".into(),
            status,
            rate: 1.0,
            track: school(),
            anchor: PositionAnchor {
                position: Duration::ZERO,
                at: Instant::now(),
            },
            art: Art::None,
            online: Vec::new(),
        }
    }

    fn online(service: Option<Service>) -> Settings {
        Settings {
            art_online: service,
            ..Settings::default()
        }
    }

    #[test]
    fn a_source_showing_a_track_wishes_its_service() {
        for status in [PlaybackStatus::Playing, PlaybackStatus::Paused] {
            assert_eq!(
                wish(Some(&player(status)), &online(Some(Service::Wikipedia))),
                Some(Wish {
                    bus_name: "org.mpris.MediaPlayer2.firefox".into(),
                    service: Service::Wikipedia,
                })
            );
        }
    }

    /// No request may leave while no source chose a service (B-48), nor
    /// for a cover nobody would see.
    #[test]
    fn no_wish_without_service_track_or_thumbnail() {
        let playing = player(PlaybackStatus::Playing);
        assert_eq!(wish(Some(&playing), &online(None)), None);
        assert_eq!(wish(None, &online(Some(Service::MusicBrainz))), None);
        let stopped = player(PlaybackStatus::Stopped);
        assert_eq!(
            wish(Some(&stopped), &online(Some(Service::MusicBrainz))),
            None
        );
        let hidden = Settings {
            show_art: false,
            ..online(Some(Service::MusicBrainz))
        };
        assert_eq!(wish(Some(&playing), &hidden), None);
    }

    #[test]
    fn only_a_missing_or_unreadable_cover_is_looked_up() {
        let image = Arc::new(ArtImage {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        });
        assert!(needs_lookup(&Art::None));
        assert!(needs_lookup(&Art::Failed));
        assert!(!needs_lookup(&Art::Pending));
        assert!(!needs_lookup(&Art::Ready(image)));
    }

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
