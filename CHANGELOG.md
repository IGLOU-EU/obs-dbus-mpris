# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - Unreleased

First release: an OBS Studio 32 source that shows what an MPRIS media player is playing, read from the D-Bus session bus.

### Added

- A "Now Playing (MPRIS)" source that discovers players on the session bus as they appear and disappear, without restarting OBS.
- A player list in the source properties, one entry per running instance, labeled by what it plays. The chosen instance is saved with the scene collection, and stays listed as unavailable while it is gone.
- Automatic selection during a five minute grace period after the source starts: the saved instance first, then an instance of the same player, then another playing or paused player. After the grace period, or after a choice made by hand, the source keeps to its player and follows it when it restarts with a new instance name. Cancel in the properties restores the selection as it was.
- Displayed elements, each one optional: cover, title, artist, album, progress bar, position, and duration or remaining time.
- Covers from `data:` and `file://` URLs, in JPEG, PNG and WebP, with a default picture when the cover is missing or unreadable.
- Optional online covers from one service at a time: MusicBrainz with the Cover Art Archive, Wikipedia, or YouTube thumbnails. Off by default: the plugin sends no network request until a source picks a service, looks a track up only when it has no local cover, and caches results and failures for the session.
- Position extrapolated from the last known value and the playback rate, corrected on seeks, track and state changes, and by periodic reads.
- Paused state with a pause icon at the end of the title line, and a translatable fallback message when nothing plays.
- Layout settings: width in pixels with a height derived from font sizes, cover on the left or right, round cover, rounded corners, gap between cover and panel, progress bar thickness, three font sizes, and four colors with alpha.
- Text rendered with cosmic-text, with fallback to any installed font for characters the default font lacks, such as Japanese.
- Text too long for its line scrolls with an adjustable speed and pause, title and subtitle in sync. A speed of 0 truncates it with an ellipsis instead.
- Grouped properties with help texts on the scroll speed and the online cover.
- English and French translations.
- A native build for the user plugin folder of OBS, and a Flatpak extension, `com.obsproject.Studio.Plugin.DbusMpris`, for the stable branch of OBS Flatpak.

[1.0.0]: https://gitlab.com/iglou.eu/obs-dbus-mpris/-/releases/v1.0.0
