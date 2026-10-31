<h1 align="center">betterPod</h1>

<p align="center">
  <b>An iPod-style music player for retro handhelds: Anbernic handhelds running Knulli, and the Miyoo Mini running Onion OS.</b><br>
  The iPod classic interface, rebuilt natively from <a href="https://github.com/tvillarete/ipod-classic-js">iPod.js</a>.<br>
  Split-screen menus, Cover Flow, a 3D Now Playing screen, wheel-keyboard search, games and apps.<br>
  Your own music, from your own card (on Knulli lossless and hi-res, played at its native sample rate).
</p>

<p align="center">
  <a href="https://github.com/stefanobuttarelli/betterPod/releases/latest"><img src="https://img.shields.io/badge/Download-for%20Anbernic%20%26%20Miyoo-2f7fd0?style=for-the-badge" alt="Download for Anbernic and Miyoo" height="36"></a>
  &nbsp;
  <a href="https://betterpod.xyz/mac/"><img src="https://img.shields.io/badge/Download-Share%20for%20Mac-1d1f23?style=for-the-badge" alt="Download betterPod Share for Mac" height="36"></a>
</p>

<p align="center">
  <a href="#installation"><b>Download</b></a> ·
  <a href="#betterpod-share-for-mac">Mac app</a> ·
  <a href="#supported-systems">Systems</a> ·
  <a href="#installation">Install</a> ·
  <a href="#controls">Controls</a> ·
  <a href="#building-from-source">Build</a> ·
  <a href="CHANGELOG.md">Changelog</a>
</p>

<p align="center">
  <img src="docs/images/hero.png" alt="betterPod: main menu, Cover Flow and Now Playing" width="100%">
</p>

---

## Supported systems

| System | Consoles | Inside | Where it starts |
|---|---|---|---|
| [Knulli](https://knulli.org) | Anbernic handhelds with the Allwinner H700 | 4 × Cortex-A53, 1 GB, aarch64 | Ports |
| [Onion OS](https://onionui.github.io) | Miyoo Mini, Mini Plus, Mini v4, Mini Flip | 2 × Cortex-A7, 128 MB, armv7 | Apps |

The interface is the same on both. What differs is listed in [Features by system](#features-by-system).

## Features

- **The iPod classic interface, faithfully.** betterPod redraws iPod.js natively at 640×480:
  - split-screen menus with live previews and a Ken Burns pan over the album art;
  - the list styles and the slide transitions.

  Sizes, colours, icons and animation timings come from the original code.
- **Cover Flow.** Flick through your albums in 3D with reflections. Flip a cover to see its tracks, with their lengths and a scrollbar, and play one straight from the back.
- **Now Playing while you browse.** While music plays, the menus show the cover, song, artist and album at the bottom, like on an iPod nano. Long lines scroll, a pause dims the cover, and changing song slides to the next one.
- **Now Playing.** The tilted 3D cover, a scrubber for seeking and a volume bar. Small icons show shuffle and repeat. Lossless files get a LOSSLESS badge with a small animated level meter.<br><img src="docs/images/lossless.gif" width="240" alt="Lossless badge">
- **Japanese titles too.** Latin, Greek and Cyrillic come from Inter, kana and kanji from Noto Sans JP.
- **Search** with the iPod's wheel keyboard, across artists, albums, songs and playlists.
- **Playlists:** Music → Playlists → New Playlist… (a name with the keyboard), and **Add to Playlist…** in a song's or an album's options (Y, or holding A), to one there is or a new one. In a playlist, a song's options remove it; a playlist's options delete it. They are plain M3U files in `<music>/Playlists`, so a computer's player reads them too, and any .m3u in the music folder shows up.
- **YouTube (optional):** turn it on in Settings → YouTube and search in Music → Search → YouTube. The search bar's gear chooses Videos or Playlists & Albums. On a video's or playlist's page, **Listen** plays it right there (A plays and pauses), without keeping it. A video with chapters becomes an album with a track per chapter, a playlist an album with a track per video. Tracks are tagged M4A with a square cover made from the video's picture. The tools (yt-dlp, QuickJS) are downloaded from their official GitHub releases when you turn it on, checked (SHA-256 where published, CPU, a test run) and not shipped. YouTube's terms don't allow downloading: use it for your own or freely licensed videos.
- **Share:** send an album or a song to a friend's betterPod with an 8-digit code (an album's or a song's options → Share; on the other one Music → Receive). It works over the internet or the same Wi-Fi, encrypted from one console to the other, through [magic-wormhole](https://magic-wormhole.readthedocs.io)'s public servers; the songs land in `Shared/` in the music folder. The cover stands on a pedestal while a laser sends it, and is built line by line on the other. The first time, betterPod asks for its name (the one friends see; Settings → Sharing changes it).
- **betterPod Share for Mac:** a small Mac app that sends songs and albums to a betterPod the same way: drop them on its window, give your friend the 8-digit code. Songs of several albums go with one code and arrive as their albums. [Download it](https://betterpod.xyz/mac/) (see [betterPod Share for Mac](#betterpod-share-for-mac)).
- **Apps:** Brick and Solitaire, the classic iPod games (Solitaire saves your game); **2048** (after Gabriele Cirulli's game, MIT; your game is kept); **Five Letters**, guess a five-letter word in six tries with an on-screen keyboard (words from SCOWL and ENABLE); **Guess the Song**, a quiz on your own music (a clip plays, pick its title among four, ten rounds); **Puzzles**, twelve from [Simon Tatham's Portable Puzzle Collection](https://www.chiark.greenend.org.uk/~sgtatham/puzzles/) (MIT): Mines, Sudoku, Net, Flood, Same Game, Fifteen, Light Up, Unruly, Towers, Keen, Pattern and Tents, played with the arrows and kept when you leave; and **Weather**: search for a place and see 10 days of forecast, with glossy animated icons in the spirit of the mid-2000s (data by [Open-Meteo](https://open-meteo.com), free, no account).
- **Audio quality:**
  - every track plays at its own sample rate (44.1, 48, 96, 192 kHz…), with 32-bit float mixing;
  - 24-bit FLAC and ALAC reach the system mixer untouched;
  - formats: FLAC, ALAC, AAC/HE-AAC (`.m4a`), WAV, AIFF, MP3 and OGG.
- **A library that handles real-world folders:**
  - albums stay whole across mixed artists, and CD1/CD2 subfolders count as one album;
  - untagged files get artist, album and track number from their folders and file names;
  - cover art comes from the file (JPEG, PNG, WebP) or from `cover.jpg` / `folder.jpg` next to it.
- **Headphones:**
  - wired and Bluetooth media buttons work, even in Hold;
  - unplugging pauses the music, like on an iPod;
  - a Bluetooth icon shows in the status bar.
- **Hold switch.** SELECT locks the controls and dims the screen while the music keeps playing.
- **Light, dark or automatic theme**, with dark from 20:00 to 7:00.
- **Missing covers found online.** Settings → Library → Missing Covers lists the albums without one: choose one to see what MusicBrainz, iTunes, Deezer and Wikipedia find (with thumbnails and the source) and pick, or search by hand; Search All Automatically fetches them from the Cover Art Archive (MusicBrainz), or else from iTunes, Deezer or Wikipedia, with the same checks. For a soundtrack ripped from a game, which has no album cover of its own, Wikipedia gives the game's box.
- **Internet Archive.** Search archive.org, see each item's license, listen to it, and download albums (FLAC, ALAC, WAV, AIFF or MP3) straight into your library.
- **Suno.** Type a Suno profile's name, or a song's code from its share link, to listen to the songs and download them (MP3, with their covers and tags).
- **Genius,** in the main menu when nothing is playing: 50 songs of your library at random start at once. (Random, despite the name.)
- **Album Info.** Hold A (or press Y) on an album, in the lists or in Cover Flow, and choose Edit Album Info; Settings → Library → **Missing Info** lists the albums without an artist or track numbers. **Find Info Online** asks MusicBrainz, iTunes, Deezer and Wikipedia; choosing a result fills in album, album artist, year and genre and pairs its songs with yours (titles, artists, numbers). For a game rip, Wikipedia gives the composer or the game's developer. Every field can also be typed. **Save Changes** writes the tags into the MP3, FLAC and Ogg Vorbis files and scans the library again. **Cover** searches for another cover for any album (the one you choose stays, through rescans). **Add to Another Album** (on the album, or on one song in Songs) merges it into an album the library sees as another one (another folder, another spelling, a bonus disc tagged apart): the files stay as they are, any format, and the database remembers where the songs belong; when their numbers are taken they come after it as the next disc. **Separate Added Songs** sends them back.
- **Podcasts,** in the main menu under Music, as on the iPod. Search Apple's podcast directory, see a podcast's newest episodes and download them into `<music>/Podcasts/<podcast>/`, tagged, with the podcast's artwork, so each podcast is an album. A downloaded episode plays when chosen; Y on an episode or a podcast deletes what was downloaded (asking first). The podcasts you download from stay listed under the search field. Only the files betterPod downloaded count as downloaded and get deleted. The search goes to Apple; feeds and episodes come from each podcast's own servers, which see the console's IP address, like any podcast app.
- **Bluetooth headphones** connect from Settings → Bluetooth (paired ones with A, new ones with Find New Devices), without going back to Knulli.
- **Now Playing:** START turns the cover over to the album's songs (like Cover Flow); A, A again shows Shuffle and Repeat buttons.
- **L2 / R2 set the brightness** anywhere, shown in the header.
- **Screensaver — Visualizer:** bars, waves, tunnel, stars and a spectrogram that move with the music (Settings → Screensaver → Style; LEFT / RIGHT change the effect, A the colours, START a big clock, UP / DOWN the song).
- **Screensaver:** a mosaic of album covers that turn over, the album playing big; pick one with the arrows and A to see its songs (Settings → Screensaver, or SELECT + Y).
- **Move the music to another card** (Settings → Library → Music Folder): copied, checked file by file, then the originals deleted; a card taken out only pauses it.
- **Updates over Wi-Fi.** betterPod checks this repository once a day and shows **Update Available** in the main menu; Software Update installs the new version in place.
- **betterPod, or iPod.** The main menu says betterPod; Settings → **Turn it Apple** makes it say iPod.
- **Settings in two columns.** The right side shows the menu of the row the cursor is on (Shuffle's choices, the Screensaver's options…), under its own header. A or RIGHT move into it, B or LEFT come back; pages of their own (About, Bluetooth, Software Update) show a summary there.
- **Your data where you want it.** Settings → Database moves the library, artwork and settings to another folder, such as an SD card shared between consoles.

<p align="center">
  <img src="docs/images/coverflow.gif" alt="Cover Flow animation" width="480">
</p>

## Features by system

The Miyoo has two slower cores and 128 MB of memory, so what would take the processor away from the music is switched off or toned down there.

| | Knulli (Anbernic) | Onion OS (Miyoo) |
|---|:-:|:-:|
| Interface, Cover Flow, Now Playing, Up Next, Search | ✓ | ✓ |
| Apps: Brick, Solitaire, 2048, Five Letters, Guess the Song, Puzzles, Weather | ✓ | ✓ |
| Share (send an album or a song to another betterPod) | ✓ | ✓ with Wi-Fi; with Light effects its screen is drawn 10 times a second; AAC / ALAC albums are refused, on both screens |
| Screensaver: Cover Flip | ✓ | ✓ (48 covers kept ready instead of 240) |
| Screensaver: Visualizer | ✓ | — (it redraws the whole screen 30 times a second) |
| Formats | FLAC, ALAC, AAC, WAV, AIFF, MP3, Ogg | FLAC, WAV, AIFF, MP3, Ogg (no AAC / ALAC decoder) |
| Output | each track's own rate, 24-bit | 44.1 kHz, 16-bit |
| LOSSLESS badge | ✓ | — |
| Internet Archive, Suno, Podcasts, Weather, covers and Album Info online, Software Update | ✓ | ✓ on the Mini Plus, v4 and Flip (Wi-Fi); Internet Archive without ALAC |
| YouTube (optional) | ✓ | — (it needs more memory than the Miyoo has) |
| Bluetooth headphones | ✓ | — (no Bluetooth) |
| Headphone buttons, pause on unplug | ✓ | — |
| Music Folder (move the music to another card) | ✓ | — (one card slot) |
| Settings → Effects (Light / Full) | ✓ (Full by default) | ✓ (Light by default) |
| Volume | the console's (PipeWire) | the volume keys (Mini Plus), MI_AO's hardware volume |
| Brightness | L2 / R2 | L2 / R2, or MENU + volume keys, in Onion's steps |
| Screen off and sleep | MENU tap; Knulli's own sleep | POWER tap; Onion's sleep timer: with nothing playing, the screen goes off and then the Miyoo powers off |

## Screenshots

| | | |
|:-:|:-:|:-:|
| <img src="docs/images/albums.png" width="260" alt="Albums"><br>Albums | <img src="docs/images/coverflow-back.png" width="260" alt="Cover Flow, flipped"><br>Cover Flow track list | <img src="docs/images/np-volume.png" width="260" alt="Now Playing volume"><br>Now Playing, volume |
| <img src="docs/images/search.png" width="260" alt="Search"><br>Search | <img src="docs/images/brick.png" width="260" alt="Brick"><br>Brick | <img src="docs/images/solitaire.png" width="260" alt="Solitaire"><br>Solitaire |
| <img src="docs/images/weather.png" width="260" alt="Weather"><br>Weather | | |
| <img src="docs/images/albums-dark.png" width="260" alt="Albums, dark theme"><br>Dark theme | <img src="docs/images/coverflow-dark.png" width="260" alt="Cover Flow, dark theme"><br>Cover Flow, dark | <img src="docs/images/settings.png" width="260" alt="Settings in two columns"><br>Settings, two columns |

<sub>Screenshots are real frames from the app. The albums, artists and covers are made up for the demo.</sub>

## Requirements

- **Knulli:** an Anbernic handheld with the Allwinner H700 (aarch64) running [Knulli](https://knulli.org), with PortMaster-style ports enabled: betterPod is launched from **Ports**.
- **Onion OS:** a Miyoo Mini, Mini Plus, Mini v4 or Mini Flip running [Onion](https://github.com/OnionUI/Onion): betterPod is launched from **Apps**. The online features need Wi-Fi (Mini Plus, v4, Flip).
- Your music on the console (or a card), in the formats of your system (see above).

## Installation

### Knulli (Anbernic)

1. Download the newest `betterpod-v…-knulli.zip` from the [release folder](https://github.com/stefanobuttarelli/betterPod/tree/main/release) and unzip it.
2. Copy `betterPod.sh` and the `betterpod` folder to `/userdata/roms/ports/`.
3. Put your music in `/userdata/music`, the `music` share on the network.
   - `Artist/Album/` folders with tagged files work best.
   - DRM-protected `.m4a` files, such as old iTunes purchases, can't be played.
4. In EmulationStation, update the game lists and start **betterPod** from Ports.

The first start scans the music folder. Later starts load the saved library instantly. Settings → Library → **Rescan now** picks up new music.

#### Coming from KnulliPod

betterPod was called KnulliPod. An installation of KnulliPod gets betterPod through Software Update as usual:

1. the update installs betterPod and restarts it;
2. the next time it is started from Ports, it moves itself to its new name, once: `ports/knullipod` becomes `ports/betterpod` and `KnulliPod.sh` becomes `betterPod.sh`. The database goes with it, and a database folder chosen in Settings → Database (`.knullipod` in the music folder, `knullipod` on a card) takes the new name too;
3. in EmulationStation, update the game lists once: Ports then shows **betterPod**.

Installing by hand over KnulliPod instead: copy `betterPod.sh` and `betterpod` as above, move `knullipod/conf` to `betterpod/conf` (your library and settings), then delete `KnulliPod.sh` and the `knullipod` folder.

### Onion OS (Miyoo)

1. Download the newest `betterpod-v…-onion.zip` from the [release folder](https://github.com/stefanobuttarelli/betterPod/tree/main/release) and unzip it **at the root of the SD card**. It holds `App/betterpod`, which ends up next to Onion's other apps.
2. Put your music in `Media/Music` on the card. `Artist/Album/` folders with tagged files work best.
3. Start **betterPod** from Apps.

The first start scans the music folder; later starts load the saved library at once.

#### Coming from MacriPod

The Miyoo version was first released as MacriPod. Copy `App/MacriPod/conf` to `App/betterpod/conf` to keep your library, settings and podcasts, then delete `App/MacriPod`.

### betterPod Share for Mac

Send songs and albums from a Mac to a betterPod, as one betterPod shares with another: drop them on the window, and type the 8-digit code on the betterPod (Music → Receive).

**[Download betterPod Share for Mac](https://betterpod.xyz/mac/)**: macOS 13 or later, Apple silicon or Intel. It isn't signed by Apple yet, so the first time macOS stops it: open it, then System Settings → Privacy & Security → Open Anyway (on macOS 13 and 14: right-click it → Open). Keep it in Applications: it updates itself.

## Controls

| Button | On the iPod | Action |
|---|---|---|
| D-pad or either stick | Click wheel | Down/right = forward, up/left = back; hold to keep scrolling |
| A, or a stick click | Centre button | Select; hold 0.5 s for options (Play Next / Add to End of Queue) |
| B | MENU | Back; hold 0.5 s to return to the main menu |
| START or X | Play/Pause | Play/Pause |
| L / R | ⏮ / ⏭ | Previous / next track |
| Y | | Queue the selected song or album: Play Next or Add to End of Queue (in Cover Flow too) |
| SELECT | Hold switch | Lock/unlock the controls and dim the screen; the music keeps playing |
| MENU (function key) | | Tap: screen off/on. Hold 5 s: quit |
| SELECT + START | | Quit |

On the Miyoo:

- **MENU** is back, like B; **POWER** turns the screen off with the music playing (hold 5 s: quit).
- The **volume keys** of the Mini Plus set the volume, also with the screen off; with MENU held they set the brightness. L2 / R2 also set it (the Mini has no volume keys).
- There are no sticks: the d-pad is the wheel.

- **Now Playing:**
  - Y (or holding A) opens **Up Next** (also in the Music menu), the songs that will play next: A plays one at once; Y on a song offers Play Next and Remove from Up Next; Clear Up Next empties it; **Clear Now Playing**, at its end, stops the music and empties the queue (also: hold A on Now Playing in the menus);
  - up/right raise the volume and down/left lower it (in Cover Flow's Now Playing too);
  - the centre button switches to the scrubber, and then the same keys seek.
- **Search keyboard:** a QWERTY grid. The keys with a shortcut show its button: X space, B delete, Y 123, START search (the magnifying glass).
  - the d-pad moves between the keys;
  - A types the selected key, B deletes, holding B closes the keyboard;
  - X types a space, Y switches to 123 (digits and symbols);
  - L / R move the cursor in the text, START searches.
- **Hold:**
  - up/down set the brightness, and betterPod remembers it;
  - after 5 seconds the Now Playing screen comes up, with the clock in the status bar.
- **Games:** START opens the game's menu, with its options (New Game…) and Quit.
- **Solitaire:**
  - the wheel moves the hand, and A picks up or drops cards;
  - holding A undoes a move, and B (or START) opens the pause menu.
- **Brick:** left/right move the paddle, and A launches the ball; START pauses it, with its menu.
- **Five Letters:** A types the key under the cursor, as on a keyboard; with the row full, A sends it, and Y sends it too. B deletes a letter. The letters found in place (green) are already in the next row. START → New Word gives another word at any time (the one left counts as missed), START → History lists the words played, found or not.
- **Weather:**
  - in the list, Search for a Place opens the keyboard; choosing a result saves the place;
  - in a forecast, the wheel or d-pad picks the day, A updates the forecast and B goes back;
  - holding A on a place removes it; Units switches between °C and °F.

## Settings

| Setting | What it does |
|---|---|
| Shuffle, Repeat | Submenus with the options; the active one is ticked |
| Theme | Light (as iPod.js), dark, or automatic (dark from 20:00 to 7:00) |
| Effects | Full, or Light: the covers beside the menus and the Weather icons stand still and option sheets appear at once, so a screen nobody touches isn't redrawn (the default on the Miyoo) |
| About, Software Update | betterPod's version and credits; checking for a new version over Wi-Fi and installing it |
| Library | Music folder, songs/albums/artists, formats found and unsupported files, disk space, last scan, **Rescan now**, **Missing Covers** (each album's search results with thumbnails, search by hand, or all automatically), **Browse Files** (see and delete files and folders), **Scanned Folders** (every folder of the last scan with its song count, and the skipped ones with why) |
| Database | Folder for the library, artwork, settings, statistics and game saves |
| Screensaver | Start Now, its style, dimming the screen, and after how long it starts |
| Quit betterPod | Quit Now: the music stops and the system's menu comes back |

A setting with a few choices shows them in the right column; one with more opens a page, summed up in the right column.

### Database folder

By default everything is kept in `betterpod/conf`. Settings → Database offers:

- **Port folder:** the default, `betterpod/conf`.
- **Music folder:** `/userdata/music/.betterpod`, which is hidden so the library scan skips it.
- **A `betterpod` folder** on each card or drive mounted under `/media`.
- **Choose a folder…:** browse to any folder, or create a new `betterpod` folder.

What happens depends on the folder:

- **No database there:** betterPod moves the current one ("Move database here").
- **A database already there**, for example from another console:
  - **Use database there** loads it as it is;
  - **Replace with current** overwrites it with this console's database.

The choice is stored in `betterpod/conf/datadir.txt`. If the folder is missing at start-up, for example because the card was removed, betterPod falls back to the port folder and says so on the Database screen.

### Internet Archive

<img src="docs/images/internet-archive.png" width="320" align="right" alt="Internet Archive search">

Music → Internet Archive searches [archive.org](https://archive.org) over Wi-Fi.

- **Results:** each one shows its title, creator, year and license, as archive.org records it (Creative Commons, public domain, or "No license stated"). They come 50 at a time; **More results…** loads the next page.
- **An item:** **Listen** first: A plays it right there on its row (A again pauses), song after song; A on a track plays that one, Y downloads just it. Nothing is kept (the MP3s are used when there are some). Then a download button for each format it has (FLAC, ALAC, WAV, AIFF, MP3) with its size, and the tracks; selecting a track downloads just that one.
- **Where it goes:** downloads go to `<music>/Internet Archive/<creator>/<title>/` with the item's cover. The library is rescanned when they finish.

To open an item you know, type its identifier (the part of the link after `/details/`) and choose **Open item**. This also works for items that archive.org's search doesn't list. The search is not filtered: check the license before you download, and download only what you have the right to.

### Suno

Music → Search → Suno gets public songs from [Suno](https://suno.com) over Wi-Fi. Suno has no public API: betterPod uses the one its web player uses, which needs no account for public songs but may change without notice.

- **Profile:** type the name after the @ (suno.com/@**name**). Its public songs come 20 at a time, newest first; **More Songs…** loads the next ones.
- **Song code:** type what a share link ends with (suno.com/s/**code**), or a song's id. A playlist's link works too.
- **Listen** plays the songs right there, as on Internet Archive; **Download All** or **Download** (Y on a song: just that one) gets the MP3s.
- **Where it goes:** each song is a single, in `<music>/Suno/<artist>/<title>/` with its cover, tagged (its first style as the genre). A song already downloaded says so.

Download your own songs, or songs you have the right to keep: check Suno's terms.

### Software Update

<img src="docs/images/update.png" width="320" align="right" alt="Software Update">

Settings → Software Update opens a full-screen page with a download bar. **Check for Updates** reads [`release/latest-knulli.json`](release/latest-knulli.json) (on the Miyoo, `release/latest-onion.json`) from this repository over Wi-Fi. When a newer version is listed, the page shows what's new, and **Download and Install** does the following:

1. downloads the release zip and checks its size and SHA-256;
2. replaces the app's files one by one, keeping the previous binary as `betterpod/betterpod.prev`;
3. restarts betterPod.

betterPod also checks by itself once a day (30 s after start-up, when a day has passed since the last check). When a new version is out, the main menu shows **Update Available**, a shortcut to this page. Nothing is downloaded without asking.

The database (`betterpod/conf`, or the folder chosen in Settings → Database) is never touched. You can also update by hand at any time: copy the new zip over the old installation.

## Options

Advanced switches are environment variables. Uncomment them in `betterPod.sh` (on the Miyoo, in `App/betterpod/launch.sh`, which lists its own):

| Variable | Effect |
|---|---|
| `MIYOOPOD_SWAP_AB=0` | Swap A and B, if they are reversed on your device |
| `MIYOOPOD_STICK_WHEEL=1` | Turn the stick like the click wheel instead of using it as arrows (faster spin, faster scroll) |
| `MIYOOPOD_WHEEL_STEP=15` | Degrees of stick rotation per wheel step (5–90) |
| `MIYOOPOD_HOLD_BRIGHTNESS=10` | Brightness in Hold, in % (default: a fifth of the current one) |
| `MIYOOPOD_HOLD_ADJUST=0` | Up/down don't change the brightness in Hold |
| `MIYOOPOD_SOFT_VOLUME=1` | Use the app's own volume instead of the console's |
| `MIYOOPOD_MEDIA_KEYS=0` | Ignore headphone buttons |
| `MIYOOPOD_BT_ICON=0` | No Bluetooth icon and no pause on disconnect |
| `MIYOOPOD_VOLUME_KEYS=0` | Don't read the volume keys (the overlay then follows the system volume only) |
| `MIYOOPOD_AUTO_UPDATE_CHECK=0` | No automatic daily check for new versions |
| `MIYOOPOD_PERF=1` | Log frame timings every second (for reporting stutter) |
| `MIYOOPOD_VSYNC=0` | Disable vertical sync |
| `MIYOOPOD_CLASSIC_UI=1` | Use the original MiyooPod interface |

The log is written to `betterpod/miyoopod.log` (on the Miyoo, `App/betterpod/miyoopod.log`).

## Building from source

You need:

- Go 1.24;
- [zig](https://ziglang.org), installed with `pip install ziglang`, used as the aarch64 C compiler with glibc 2.28;
- the SDL2 headers and `libSDL2-2.0.so` for aarch64, in a folder with `include/SDL2` and `lib/`.

`SDL2_mixer` (with the ALAC, AAC and 24-bit patches) is linked against the copy shipped in `port/betterpod/libs`.

```sh
SDL2_SYSROOT=/path/to/sysroot tools/build.sh
# -> dist/betterpod-knulli.zip   (betterPod.sh + betterpod/, binary: betterpod/betterpod)
#    dist/betterpod-knulli-bridge.zip   (for installations still called betterPod)
```

The binary is built from the file list (`src/*.go`), not as a package, because `main.go` includes the C files (`main.c`, `audio.c`).

### Onion OS (Miyoo)

Everything runs in Docker (Go 1.24, zig cc for 32-bit ARM with glibc 2.28, the SDL2 headers); the binary links against the Miyoo's SDL2 in `onion/betterpod/libs`.

```sh
tools/onion/build.sh
# -> dist/sdcard/App/betterpod/          (copy App/ to the SD card)
#    dist/betterpod-v<version>-onion.zip  (unzip at the root of the card)
tools/onion/unittest.sh    # update zip, curves, sleep timer, C pixel loops, Album Info, Podcasts, Suno
tools/onion/smoketest.sh   # the ARM binary emulated, a frame per step
tools/onion/sleeptest.sh   # the sleep timer, with a 3-second "minute"
tools/onion/perf.sh        # drawing time and load per scenario
tools/onion/release.sh "What's new"   # release/betterpod-v<version>-onion.zip and latest-onion.json
```

### Source layout

| Path | Contents |
|---|---|
| `src/ij_*.go` | The iPod.js interface: views, Cover Flow, Now Playing, search, sheets, games, themes |
| `src/main.c`, `src/audio.c` | SDL2 input (gamepad, stick wheel, media keys), display and vsync; SDL_mixer playback with sample-rate switching |
| `src/library.go`, `src/ij_libinfo.go` | Library scan, album grouping, artwork, scan statistics |
| `src/datadir.go`, `src/ij_datadir.go` | Database folder and Settings → Database |
| `src/ij_archive.go` | Music → Internet Archive (search, item, download) |
| `src/ij_suno.go` | Music → Search → Suno (a profile's songs, a song or playlist by its code, download) |
| `src/ij_genius.go` | Genius: 50 songs at random, from the main menu |
| `src/ij_playlists.go` | Playlists: New Playlist, Add to Playlist, removing and deleting; M3U files in `<music>/Playlists` |
| `src/ij_listen.go` | Listen, on an Internet Archive item, a YouTube video and Suno's songs: a song plays on its row (play, pause), fetched into a temporary folder; the user's music is set aside and comes back |
| `src/ij_podcasts.go` | Podcasts (Apple's directory, RSS feeds, episodes) |
| `src/tagwrite.go` | Writes tags into MP3 (ID3v2), FLAC and Ogg Vorbis files (podcast episodes, Album Info) |
| `src/ij_albuminfo.go` | Album Info and Missing Info: an album's tags, filled in online or by hand |
| `src/ij_albummerge.go` | Album Info → Add to Another Album: songs put into another album, kept in the database |
| `src/ij_infosources.go` | Album Info's sources: MusicBrainz releases, iTunes and Deezer albums with their songs, Wikipedia's infobox |
| `src/ij_bluetooth.go` | Settings → Bluetooth (bluetoothctl): paired and new devices, their pictures |
| `src/ij_npextra.go` | Now Playing's card (START) and options row (Shuffle, Repeat) |
| `src/ij_visualizer.go` | The Visualizer screensaver: the sound's spectrum (FFT) and its effects |
| `src/ij_2048.go`, `src/ij_quiz.go` | Apps: 2048 and Guess the Song |
| `src/ij_words.go`, `src/words/` | Apps: Five Letters and its word lists |
| `src/ij_share.go`, `src/ij_share_draw.go`, `third_party/` | Share and Receive (magic-wormhole: wormhole-william, a SPAKE2) |
| `mac/` | betterPod Share for Mac: the Go engine (`core/`) and the SwiftUI app (`App/`), built by `.github/workflows/mac.yml` |
| `src/ij_puzzles.go`, `src/pz_*.go`, `src/puzzles/` | Apps → Puzzles: the front end (`puzzles/pzfe.c` records the drawing, Go replays it) and the puzzles' own C code |
| `src/ij_columns.go` | Two-column menus (Settings): the right column, moving in and out of it |
| `src/ij_vizcolors.go` | The Visualizer's colour schemes (A) and the Spectrogram |
| `src/ij_screensaver.go` | The screensaver: the cover mosaic, its bar, the turned-over card, Settings → Screensaver |
| `src/ij_musicdir.go`, `src/ij_movemusic_view.go` | Music Folder: moving the music to another card, and its screen |
| `src/ij_keyboard.go` | The on-screen QWERTY keyboard |
| `src/ij_youtube.go`, `src/ij_ytdl.go` | YouTube: tools and settings; search, item page, download as albums |
| `src/ij_ytcoverpick.go` | YouTube: choosing the square cover before a download |
| `src/ij_m4a.go`, `src/ij_ytcover.go` | M4A cutting and tagging without ffmpeg; square covers from video pictures |
| `src/ij_covers.go` | Cover search (MusicBrainz, Cover Art Archive), Search All Automatically |
| `src/ij_coversources.go` | The other cover sources: iTunes, Deezer, Wikipedia |
| `src/ij_coverpick.go` | Missing Covers: the albums without a cover, each one's results to choose from |
| `src/ij_update.go` | Settings → Software Update (manifest, download, install, restart) |
| `src/duration.go` | Track lengths read from the file headers |
| `src/knulli.go` | Knulli integration: paths, backlight, system volume, Bluetooth |
| `src/onion.go`, `src/onion.c`, `src/onion_sleep.go` | Onion integration: keys, MI_AO volume and its queue, backlight, battery, memory, the sleep timer |
| `src/cdraw.c`, `src/cdraw.go`, `src/ij_effects.go` | The Miyoo's hottest pixel loops in C; Settings → Effects |
| other `src/*.go` | Player, queue, playback state and settings, inherited from MiyooPod |
| `port/` | Knulli's package: launcher, assets, fonts, bundled libraries (the assets are shared) |
| `onion/betterpod/` | Onion's app folder: `launch.sh`, `config.json`, icon, the Miyoo's SDL2 libraries |
| `tools/` | Knulli's build script and the tools used to recover the v0.20 sources |
| `tools/onion/` | Onion's build, tests, performance tools and release |

## How this repository came to be

The v0.20 source tree was lost. It was rebuilt from the surviving release binary and build cache, file by file. Each function was recompiled and its machine code compared with the original v0.20 object until they matched: 978 functions identical, 53 equivalent. [docs/recovery](docs/recovery) describes the method and the result. Versions 0.21 and later build on the recovered code.

## Credits

- **[iPod.js](https://github.com/tvillarete/ipod-classic-js)** by Tanner Villarete: the interface design, layouts and assets betterPod recreates (MIT).
- **[MiyooPod](https://github.com/danfragoso/miyoopod)** by Danilo Fragoso: the player engine, library scan and original app betterPod is built on (MIT).
- **The Miyoo version** by Emanuele Macri, first released as MacriPod; [steward-fu's SDL2](https://github.com/steward-fu/sdl2) for the Miyoo Mini (zlib) and [Onion](https://github.com/OnionUI/Onion)'s volume, brightness and sleep sequences.
- **[Simon Tatham's Portable Puzzle Collection](https://www.chiark.greenend.org.uk/~sgtatham/puzzles/)** by Simon Tatham and contributors: the puzzles in Apps → Puzzles (MIT, `src/puzzles/LICENCE`).
- **[Inter](https://rsms.me/inter/)** by Rasmus Andersson and **[Noto Sans JP](https://github.com/notofonts/noto-cjk)** (Google, Adobe): the fonts (SIL Open Font License).
- Apple's reference **[ALAC decoder](https://github.com/macosforge/alac)** (Apache 2.0), **[FFmpeg](https://ffmpeg.org)**'s AAC decoder (LGPL 2.1), **[SDL2 and SDL2_mixer](https://libsdl.org)** (zlib).

See [THIRD_PARTY_LICENSES.md](port/betterpod/THIRD_PARTY_LICENSES.md) (and, for the Miyoo, [onion/betterpod/THIRD_PARTY_LICENSES.md](onion/betterpod/THIRD_PARTY_LICENSES.md)) for the full list.

## License

betterPod is released under the [MIT License](LICENSE).

iPod is a trademark of Apple Inc. This is an independent fan project, not affiliated with or endorsed by Apple, Anbernic, Miyoo, the Knulli project or the Onion project.
