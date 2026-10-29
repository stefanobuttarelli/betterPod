# betterPod for Onion OS

An iPod-style music player for the Miyoo Mini, Mini Plus, Mini v4 and Mini Flip running Onion OS. It plays your own music from the SD card.

betterPod is by Stefano Buttarelli, with the Miyoo version by Emanuele Macri: a native recreation of [iPod.js](https://github.com/tvillarete/ipod-classic-js) by Tanner Villarete. The player engine and the Miyoo libraries come from [MiyooPod](https://github.com/danfragoso/miyoopod) by Danilo Fragoso. All MIT-licensed: see `LICENSE`, `LICENSE.ipod.js`, `LICENSE.MiyooPod` and `THIRD_PARTY_LICENSES.md`.

Source code and updates: https://github.com/stefanobuttarelli/betterPod

## Installation

1. Unzip `betterpod-v…-onion.zip` (from the repository's `release` folder) at the root of the SD card. It holds `App/betterpod`, which ends up next to Onion's other apps.
2. Put your music in `Media/Music` on the card.
   - **Formats:** MP3, FLAC, Ogg Vorbis, WAV and AIFF. AAC and ALAC (`.m4a`) can't be played on the Miyoo and are skipped.
   - **Folders and tags:** `Artist/Album/` folders with tagged files work best. Untagged files get artist, album and track number from the folders and the file name.
   - **Cover art:** embedded art, or `cover.jpg` / `folder.jpg` in the album folder.
3. Start **betterPod** from Apps.

**Coming from MacriPod** (this version's earlier name): copy `App/MacriPod/conf` to `App/betterpod/conf` to keep your library, settings and podcasts, then delete `App/MacriPod`.

The first start scans the music folder; later starts load the saved library at once. Settings → Library → **Rescan now** picks up new music.

## Controls

| Button | On the iPod | Action |
|---|---|---|
| D-pad | Click wheel | Down/right = forward, up/left = back; hold to keep scrolling |
| A | Centre button | Select; hold 0.5 s for options (Play Next / Add to End of Queue) |
| B or MENU | MENU | Back; hold 0.5 s to return to the main menu |
| START or X | Play/Pause | Play/Pause |
| L / R | ⏮ / ⏭ | Previous / next track |
| Y | | Queue the selected song or album: Play Next or Add to End of Queue |
| SELECT | Hold switch | Lock the controls and dim the screen; the music keeps playing |
| Volume keys (Mini Plus) | | Volume |
| MENU + volume keys, L2 / R2 | | Brightness |
| POWER | | Tap: screen off/on, the music keeps playing. Hold 5 s: quit |
| SELECT + START | | Quit |

- **Now Playing:** up/right raise the volume, down/left lower it; the centre button switches to the scrubber, and then the same keys seek; pressed again, it shows Shuffle and Repeat. START turns the cover over to the album's songs and X plays and pauses. Y opens **Up Next**, the songs that play after this one; at its end, **Clear Now Playing** stops the music and empties the queue (also: hold A on Now Playing in the menus).
- **Keyboard** (Search, Internet Archive, Weather): the d-pad moves between the keys and A types; B deletes (held, it closes it), X types a space, Y switches to 123, L / R move the cursor, START searches.
- **Hold:** up/down set the brightness, and betterPod remembers it. After 5 seconds the Now Playing screen comes up.
- While betterPod runs it handles POWER and the volume keys itself, so the screen can go off without stopping the music. Onion takes them back as soon as you quit.
- **Sleep, as in Onion:** with nothing playing, Onion's sleep timer (Settings → System → Sleep timer, 5 minutes by default) without a key turns the screen off, and a minute later the Miyoo powers off. With the screen off by POWER and nothing playing, it powers off after the sleep timer and one more minute. Never while music plays, while charging or during a download.

## Internet Archive

Music → **Internet Archive** searches archive.org (START runs the search). Each item shows its license as archive.org records it. Download FLAC, WAV, AIFF or MP3 for the whole album, or pick a single track: the files go to `Media/Music/Internet Archive/<creator>/<title>/` with the cover, and the library is updated at the end. A download that stopped can be resumed from the item's page. It needs Wi-Fi (Mini Plus, v4, Flip).

## Apps, weather and covers

- **Apps:** Brick, Solitaire, 2048, Guess the Song (a quiz on your own music), twelve of Simon Tatham's Puzzles, and Weather.
- **Apps → Weather:** search for a place and see its 10-day forecast (from Open-Meteo, over Wi-Fi). The last forecast stays saved, so it also shows offline.
- **Settings → Library → Missing Covers:** the albums without a cover, each with what MusicBrainz, iTunes, Deezer and Wikipedia find, to choose from, or Search All Automatically. For a soundtrack ripped from a game, Wikipedia gives the game's box.
- **Settings → Library → Browse Files:** the files and folders in `Media/Music`, with their sizes; Y deletes, always asking first.

## Podcasts

**Podcasts**, in the main menu under Music, searches Apple's podcast directory (Wi-Fi). Choose a podcast to see its newest episodes; choosing one downloads it, and **Download the 3 Latest** gets the newest you don't have. Episodes go to `Media/Music/Podcasts/<podcast>/` with their tags and the podcast's artwork, so each podcast is an album; a downloaded episode plays when you choose it. The podcasts you download from stay listed under the search field. Y (or holding A) on a downloaded episode offers **Delete Download**, and on a podcast **Delete Downloaded Episodes** (both ask first) or **Remove from Your Podcasts**. Only MP3 episodes play on the Miyoo.

## Album Info

Hold A (or press Y) on an album and choose **Edit Album Info**, or open Settings → Library → **Missing Info** for the albums without an artist or track numbers.

- **Find Info Online** asks MusicBrainz, iTunes, Deezer and Wikipedia (Wi-Fi). Choosing a result fills in the album and its songs; for a soundtrack ripped from a game, Wikipedia gives the composer or the game's developer.
- Every field can be typed: START keeps the text, holding B leaves it as it was.
- **Save Changes** writes the tags into the MP3, FLAC and Ogg files (WAV and AIFF have none) and scans the library again.

## Effects

Settings → **Effects** chooses how much the interface animates:

- **Light** (the default): the album covers beside the menus stand still and cross-fade quickly, the Now Playing preview doesn't pan, the Weather icons don't move and option sheets appear at once, so a screen you aren't touching isn't redrawn at all. This keeps the Miyoo's processor free for the music.
- **Full:** the slow Ken Burns pans and animated weather icons. Prettier, but heavier.

Menu slides, Cover Flow and Now Playing look the same in both.

## Updates

Settings → Software Update checks GitHub over Wi-Fi and installs a new version in place. betterPod also checks by itself once a day and shows **Update Available** in the main menu. Nothing is downloaded without asking, and your library and settings (`conf/`) are never touched. You can also update by hand: unzip the new version over the old one.

## Files

- `conf/`: library, artwork, settings and game saves (Settings → Database can move them).
- `miyoopod.log`: the log, for bug reports.
- `launch.sh`: advanced options, commented out.
