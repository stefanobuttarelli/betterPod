# Changelog

## v0.35 (2026-10-28)

### Added
- **Up Next** (Now Playing → Y, or hold A): the songs that will play after the current one, in play order (shuffle included), with their covers.
  - **A** plays a song at once and goes back to Now Playing.
  - **Y** on a song: Play Next (moves it right after the current one) or Remove from Up Next.
  - **Clear Up Next**, at the bottom, empties it (after asking).
- **Queued songs are marked:** a song added with Play Next or Add to End of Queue shows the queue icon before its title in the lists and in Up Next, until it plays.
- **Queue icons** in the options: three lines with an arrow. For Play Next the first line is bold and the arrow points up; for Add to End of Queue the last line is bold and the arrow points down.

### Changed
- **Keyboard:** the Search key is a magnifying glass (with its START badge).

## v0.34 (2026-10-28)

### Added
- **The song in the header.** When a song starts playing (a new song, or play after a pause) on a screen without the Now Playing block, the header's title slides up and the song comes in from below, with its little cover, for a few seconds. Then it slides up in turn and the title comes back. A long title ends with "…", clear of the icons.
- **Y queues songs.** On a song or an album (also in Cover Flow, in front or on the back of a cover), Y opens Play Next / Add to End of Queue. A short message confirms it ("Playing Next", "Added to Queue"); with nothing playing, it plays.
- **Keyboard:** the keys with a button shortcut show it in a little square: space X, delete B, 123 Y, Search START.

### Changed
- **Option sheets are smaller:** a floating card over the dimmed screen, with what it's about as a title (the song, the album), 32 px options with the selected one in the list's blue, and Cancel on its own card.
- **Weather:** the day tiles show the date ("THU 29", "SUN 1"). The top no longer repeats the weekday.

### Fixed
- Holding A in Cover Flow opened the options of the list behind it; it now offers the album in front or the song on its back.

## v0.33.1 (2026-10-28)

### Changed
- **Weather:** the high and low next to the big temperature are plain text with arrows ("↑ 21°", "↓ 13°") instead of the red and blue pills.

## v0.33 (2026-10-28)

### Added
- **Weather** (Games → Weather): the forecast for the places you choose, as if Apple had made it for the iPod in 2006, with a nod to the Wii's Forecast Channel.
  - **Places:** there's no location, so Search for a Place opens the keyboard and lists what matches (city, region, country). Choosing one saves it. The list shows each place with an icon, its weather and temperature; holding A removes one.
  - **The forecast:** a sky that follows the weather and the time of day (deep blue when clear, grey with rain, dark and starry at night). It shows the place and its local time, the temperature now in big type, the condition, the high and low, and feels-like, humidity and wind.
  - **10 days:** glossy glass tiles (5 at a time) with the day, an icon, the high in red, the low in blue and the chance of rain. The wheel or d-pad picks a day and the top shows it, with its rain chance, sunrise and sunset.
  - **The icons:** drawn for the app, glossy and puffy, with a shine and a soft shadow: sun, moon and stars, clouds, fog, drizzle, rain, sleet, snow and thunderstorms. The big one is animated: the rays turn, the clouds drift, rain and snow fall, lightning flashes.
  - **Data:** from Open-Meteo (open-meteo.com: free, no key). The last forecast of each place is kept, so it shows at once and offline. It updates when older than 30 minutes, or with A.
  - **Units:** °C or °F (with km/h or mph).

## v0.32.3 (2026-10-28)

### Fixed
- **Internet Archive downloads no longer stop at the first error.** A long album could stop halfway ("Download stopped: archive.org answered…") because one file failed once.
  - **Retries:** when archive.org is busy (429, 5xx) or the connection drops or stalls, the file is tried again after a pause (3, 6, 12, 24, 48 s, or what archive.org asks), up to 6 times. The screen shows why and when ("archive.org answered 503 · retrying in 6 s (2/6)").
  - **Resume within a file:** a file cut off halfway continues from where it stopped (HTTP Range), instead of starting over.
  - **Skipped tracks:** a track that still fails (or is missing, 404) is skipped and the others go on. At the end: "170 of 173 tracks added · 3 failed", with **Try Again**, which downloads only what is missing (B closes).
  - **Disk errors** (card full or not writable) still stop the download at once.

### Added
- **Resume an album later.** An item page whose album is partly downloaded offers "Resume FLAC download · 113 of 173 tracks left", and only the missing tracks are downloaded. This works for albums downloaded in part by earlier versions too. A complete album shows "FLAC downloaded".

## v0.32.2 (2026-10-28)

### Added
- **Internet Archive downloads show the time left** next to the percentage ("49% · 0:07 left"), and the speed next to the size ("2 MB of 3 MB · ALAC · 258 KB/s").
  - **How it's worked out:** from the speed of the last few seconds, so it settles after a moment and follows the connection getting faster or slower.
  - **When it shows:** after the first 3 seconds of a download. If nothing arrives for 4 seconds it says "waiting for data…" instead.

## v0.32.1 (2026-10-28)

### Fixed
- **A folder that can't be read in full no longer loses all its songs.** When the card answers a folder's listing with an error ("input/output error", usually a damaged folder on the card), the songs listed before the error are scanned anyway. Scanned Folders says how many entries were read.

## v0.32 (2026-10-28)

### Added
- **Missing Covers** (Settings → Library, it replaces Find Missing Covers): the albums without a cover, one by one.
  - **Choosing an album:** MusicBrainz is searched for it at once (title and artist, then the title alone, then its main part before ":").
  - **The results:** everything MusicBrainz answers is listed, not only the sure matches. Each has its cover as a thumbnail, its artist, year and type; the ones the automatic search would take come first, marked "Best match".
  - **Choosing a result:** its cover is downloaded and given to the album, which leaves the list.
  - **Nothing right?** "Edit search" (or "None of these?" at the bottom) opens the keyboard to search by hand: plain words, matched against title, artist and the rest. Holding the button offers "Clear search" and "Search automatically again".
  - **Errors** are shown as such ("MusicBrainz didn't answer", with the reason and a retry), so a missing connection doesn't look like "no results".
  - **Automatic search:** "Search All Automatically", at the top, is still there, for all the albums at once (best matches only).

## v0.31.2 (2026-10-28)

### Added
- **Scanned Folders** (Settings → Library): every folder of the last scan.
  - **Each row:** the folder's path and how many songs it holds directly, or else "N other files", "Only subfolders" or "Empty".
  - **Skipped folders, with the reason:** hidden (name starting with a dot), system folders, links to folders (not followed, with where they point) and folders that couldn't be opened.
  - **Purpose:** for finding out why some music doesn't show up. Run **Rescan now** first, so the list is filled.

## v0.31.1 (2026-10-28)

### Fixed
- **Find Missing Covers finds soundtracks and compilations.**
  - **Artists:** all the album's artists are tried (album artist, then track artists). A compilation also accepts MusicBrainz's "Various Artists".
  - **Titles:** the main title before ":" counts too ("Hercules: An Original Walt Disney Records Soundtrack" / "Hercules"), when the artist matches or both are soundtracks. Singular and plural count as the same word ("Record" / "Records").
  - **No artist match:** the title alone is searched, still checking every result, so a soundtrack only takes the cover of a soundtrack and not of another album with the same name.
  - **End screen:** albums that could not be looked up (no connection, MusicBrainz busy) are counted apart from the ones not found.

## v0.31 (2026-10-28)

### Changed
- **New on-screen keyboard** for Search and Internet Archive: a QWERTY grid that slides up from the bottom, made for the d-pad, instead of the row of keys the click wheel scrolled through.
  - **Moving:** the d-pad moves between the keys. Left and right wrap round a row; up and down go to the nearest key of the next row; held, it keeps moving.
  - **Buttons:** A types, B deletes the character before the cursor (held, it closes the keyboard), X types a space, Y switches to 123 (digits and `- . _ ' & ! ? ( ) @ # : , / + =`), L / R move the cursor, START searches.
  - **The text:** the search row shows it with the cursor.
- **Find Missing Covers finds more, and never a wrong one.**
  - **Cleaning:** titles are cleaned of edition notes, brackets and disc numbers ("(Remastered 2009)", "[Deluxe Edition]", "CD1"), and artists of "feat. …".
  - **Search:** first as phrases, then word by word, to find other spellings.
  - **Checking:** every result is checked: the same title words (accents, punctuation, apostrophes, "&"/"and" and "The" don't matter; years and edition words are left out) and the same artist. Another album that only starts the same ("Rumours" / "Rumours Live") is not used.
  - **The end screen** lists the albums not found; the log says whether each one is missing from MusicBrainz or has no cover there.

## v0.30.2 (2026-10-27)

### Added
- **Internet Archive: more lossless formats.** Besides FLAC and MP3, an item's page now offers Download ALAC, Download WAV and Download AIFF (lossless) when the item has them, each with its size. The track list shows the first lossless format there is.

## v0.30.1 (2026-10-27)

### Changed
- **Scrolling lines move together.** When the title, artist and album all scroll (in Now Playing, in the Now Playing block, and on Cover Flow's track list card), they start together. The shorter ones wait at their start until the longest has finished, and then all start again at once, instead of drifting at different rhythms.

## v0.30 (2026-10-27)

### Added
- **Settings → Library → Find Missing Covers.** For albums without artwork, KnulliPod searches MusicBrainz and downloads the front cover from the Cover Art Archive at 1200 px.
  - **Requests:** one per second, as MusicBrainz asks, with a User-Agent that names the app.
  - **Matching:** only good matches are used.
  - **Saving:** a cover found is saved as `cover.jpg` in the album's folder and shows straight away.
  - **The page:** white, with the album being looked up, the bar, and how many covers were found and not found, plus Cancel.
- **Internet Archive:**
  - **Edit search:** reopens the keyboard on the current text, and the row shows what you are typing. Holding the centre button offers Clear search.
  - **Open by identifier:** typing an item's identifier (the part of the link after `/details/`) shows **Open item “…”**, which opens it directly. This works for items left out of archive.org's search index.
  - **Keyboard:** now has `-`, `.` and `_`.
- **Now Playing block:** after 10 minutes of pause it slides away; it comes back when the song plays again.

## v0.29 (2026-10-27)

### Changed
- **Lossless badge:** five small rounded bars, a level meter, and LOSSLESS. While the song plays the bars rise and fall softly, each at its own pace, so they move with some rhythm. Paused, they settle back to the logo's shape.
- **Internet Archive:**
  - results come 50 at a time; "More results…" at the end of the list loads the next page and shows how many there are;
  - an error on a page can be retried from the list;
  - a download that receives nothing for 30 s stops ("Connection stalled") instead of waiting forever;
  - the answer to an older search no longer replaces a newer one.

## v0.28 (2026-10-27)

### Added
- **Music → Internet Archive:** search archive.org with the wheel keyboard (✓ runs the search).
  - **Results:** title, creator, year and the item's license, as archive.org records it (Creative Commons, public domain, or "No license stated").
  - **An item:** its license, Download FLAC (lossless) and Download MP3 with their size, and the tracks with their lengths; selecting a track downloads just that one.
  - **Downloading:** a white page with the album, the track being downloaded and the blue progress bar, and Cancel / Done.
  - **Where it goes:** files go to `<music>/Internet Archive/<creator>/<title>/` with the item's cover as `cover.jpg`. The library is rescanned at the end, and the album shows up in Albums and Cover Flow.

### Fixed
- **Covers after a rescan:** albums added by a rescan (or a download) now get their cover from `cover.jpg` / `folder.jpg` straight away, instead of after the next start.

## v0.27.2 (2026-10-27)

### Changed
- **Lossless badge:** minimal. Two small crossing waves, with a gap between their tips, and LOSSLESS, without the bit depth and sample rate under it.

## v0.27.1 (2026-10-27)

### Changed
- **Lossless badge:** a small wave and LOSSLESS, for every lossless file. The bit depth and sample rate stay on the line under it.

## v0.27 (2026-10-27)

### Added
- **Lossless badge in Now Playing:** for FLAC, ALAC, WAV and AIFF files, an outlined pill under the song reads LOSSLESS, or HI-RES LOSSLESS above 48 kHz. Under it, the bit depth and sample rate ("24-bit · 96 kHz"). It follows the light and dark themes, and shows in Cover Flow's Now Playing too. The format comes from the file headers: in `.m4a` files, ALAC counts as lossless and AAC does not.

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
