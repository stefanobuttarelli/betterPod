# Changelog

## v0.26 (2026-10-27)

### Added
- **Automatic update check, once a day.** A quiet check runs 30 s after start-up when a day has passed since the last one; with no connection it tries again an hour later. Nothing is downloaded without asking. When a new version is found:
  - the main menu shows **Update Available** under Now Playing, a shortcut to Software Update;
  - Software Update shows what's new and the Download and Install button.
- Software Update shows when the last check ran ("KnulliPod is up to date · checked at 14:32").
- `MIYOOPOD_AUTO_UPDATE_CHECK=0` turns the automatic check off.

## v0.25.4 (2026-10-27)

### Fixed
- **Volume overlay outside Now Playing:** KnulliPod now also reads the console's volume keys from the input devices (read-only; Knulli still changes the volume). A press shows the overlay at once, even at 0% or 100%, and the level follows as soon as the system has changed it. Before, the overlay depended only on PipeWire reporting the change.
- The log notes which input devices are watched, and the first volume change made outside the app.

## v0.25.3 (2026-10-27)

### Fixed
- **Software Update finds a new version right after it is published.** The version information and the download now come through GitHub's API. Before, they came through raw.githubusercontent.com, which can keep serving the previous version for up to 5 minutes after a release. raw.githubusercontent.com stays as the fallback.

## v0.25.2 (2026-10-27)

### Changed
- **Music preview:** the covers beside the menu now cycle in a random order, reshuffled each time the preview starts, instead of alphabetically.

## v0.25.1 (2026-10-27)

### Fixed
- **Software Update:** the button labels are centred vertically on the button.

## v0.25 (2026-10-27)

### Changed
- **Software Update** is now a full-screen page:
  - the version and state at the top;
  - a blue glossy download bar in the middle that fills during the download;
  - buttons at the bottom: Check for Updates, then Download and Install (or Not Now).

  When a new version is found it shows what's new. After the download it installs and restarts by itself.

## v0.24 (2026-10-27)

### Changed
- **Now Playing block:** the song title is shown without quotes.

## v0.23 (2026-10-27)

### Added
- **Now Playing block in every menu:** Music, Artists, Albums, Songs, Playlists, Games and Settings keep it at the bottom while you browse. It stays in place while the menus slide, and hides on the settings detail screens.
  - **Paused:** the cover shrinks and dims, shows a pause sign, and the lines stop scrolling.
  - **Next / previous song:** the old song slides out and the new one slides in, in the direction of the change.
- **Scrolling lines:** long titles, artists and albums scroll in Now Playing and in the block while the song plays.
- **Japanese titles:** characters Inter lacks (kana, kanji) are drawn with Noto Sans JP.

### Changed
- **Now Playing:** paused, the cover shrinks to 85% as well as straightening up. A song change slides the lines, and cross-fades the cover when the album changes.
- **Cover Flow track list:**
  - every other row is light grey;
  - the rows line up with the album title above them;
  - the scrollbar shows only while the list scrolls.
- **Cover Flow animations:**
  - the track list stays where it is while it fades;
  - a turned cover shows its plain back instead of mirrored artwork;
  - the cover keeps turning the same way into Now Playing;
  - the album title under the flow no longer flashes.
- **Artwork:** covers already shown once appear immediately instead of reloading. The cache keeps the most recently used covers (by memory, not by count), and list thumbnails are prepared in the background.
- **Battery icon:** reads full down to 85%; at 80% it shows a small gap.

### Fixed
- Black bars under the progress bar during Cover Flow's Now Playing transitions (a premultiplied-alpha overflow).

## v0.22 (2026-10-27)

### Added
- **Settings → Software Update:** checks for a new version over Wi-Fi and installs it in place, then restarts. The download is checked against the size and SHA-256 in `release/latest.json`; the database is never touched, and the previous binary is kept as `MiyooPod.prev`.
- **Now Playing in the main menu:** while music plays, a block at the bottom of the main menu shows the cover, song, artist and album, on a dark woven panel, like the iPod nano.
- **Track lengths before playing:** durations are read from the file headers (FLAC, MP3, AAC/ALAC, WAV, AIFF, Ogg) during the scan. Libraries scanned by older versions fill them in when an album is opened.

### Changed
- **Cover Flow:**
  - the artist under the album title is smaller and grey;
  - the flipped cover's track list reaches the bottom of the screen;
  - each track shows its length at the right;
  - the playing track has a play mark before its title, instead of the speaker at the right;
  - a scrollbar shows the position in long albums;
  - long titles scroll when selected.
- **Status bar:** the pause icon's two bars are now the same width.

## v0.21 (2026-10-27)

### Added
- **Settings → Database:** choose the folder that holds the library, artwork, settings, scan statistics and game saves. The choices are:
  - the port folder;
  - the music folder (`/userdata/music/.knullipod`);
  - a `knullipod` folder on any card or drive under `/media`;
  - any folder picked with the built-in browser, which can also create a `knullipod` folder.
- **Moving and sharing the database:**
  - "Move database here" moves the current database to the chosen folder;
  - when that folder already has a database (for example a card shared between consoles), you can use it as it is or replace it with the current one.
- **Missing folder:** if the chosen folder is missing at start-up (card removed), KnulliPod falls back to the port folder and says so on the Database screen.

### Fixed
- Data paths are computed on use, so moving the database never races with background saves.
- A move also copies the files written while it is running, such as new artwork and the playback position.
- Browsing for a folder no longer creates empty folders before the move is confirmed.
- The Library screen measures only the database files, not the whole folder that holds them.

## v0.20 (2026-10-26)

Sources rebuilt from the v0.20 release binary (see [docs/recovery](docs/recovery)). Compared with upstream MiyooPod:

- **Interface:** the iPod.js interface. It adds split-screen menus with previews, Cover Flow, the 3D Now Playing screen, search with the wheel keyboard, action sheets, Brick and Solitaire, and light, dark and automatic themes.
- **Audio:**
  - playback at each track's own sample rate, mixed in 32-bit float;
  - 24-bit FLAC and ALAC;
  - AAC/HE-AAC `.m4a` through a minimal FFmpeg AAC decoder.
- **Library:**
  - albums grouped by folder, with CD/Disc subfolders counted as one album;
  - artist, album and track number taken from folder and file names for untagged files;
  - folder artwork (`cover.jpg`, `folder.jpg`…), plus WebP covers;
  - scan statistics in Settings → Library.
- **Knulli integration:**
  - system volume and backlight;
  - wired and Bluetooth media keys, with pause on unplug or disconnect;
  - lock key, vsync and partial screen updates;
  - no OTA updates from the upstream project.
