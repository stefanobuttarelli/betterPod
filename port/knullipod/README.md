# KnulliPod

An iPod-style music player for Anbernic handhelds running Knulli, launched from Ports. It plays your own music, stored on the console.

The interface is a native recreation of [iPod.js](https://github.com/tvillarete/ipod-classic-js) by Tanner Villarete. It includes:

- split-screen menus with previews and a Ken Burns pan;
- lists, a 3D Now Playing screen with scrubber and volume, and Cover Flow;
- Search with the wheel keyboard, pop-up menus and About;
- the Brick and Solitaire games.

Sizes, colours, icons and animation timings come from the original code.

Playback and the library scan come from [MiyooPod](https://github.com/danfragoso/miyoopod) by Danilo Fragoso. Both projects are MIT-licensed (see `LICENSE.ipod.js` and `LICENSE.MiyooPod`). The Inter font is under the SIL Open Font License. The Apple Lossless decoder is Apple's reference implementation (Apache 2.0). See `THIRD_PARTY_LICENSES.md`.

Source code, screenshots and updates: https://github.com/stefanobuttarelli/knullipod

## Installation

1. Copy `KnulliPod.sh` and the `knullipod` folder to `/userdata/roms/ports/`.
2. Put your music in `/userdata/music`, the `music` share on the network.
   - **Formats:** FLAC, ALAC and AAC (iTunes / Apple Music `.m4a`, including HE-AAC), WAV, AIFF, MP3 and OGG. DRM-protected `.m4a` files (old iTunes purchases) can't be played.
   - **Folders and tags:** use `Artist/Album/` folders with tagged files. For untagged files (usually WAV), artist, album and track number come from the folders and the file name.
   - **Cover art:** embedded art is read automatically (JPEG, PNG and WebP). Files without it use an image in the album folder (`cover.jpg`, `folder.jpg`, `front.png`…).
3. In EmulationStation, update the game lists and start **KnulliPod** from Ports.

## Controls

| Button | On the iPod | Action |
|---|---|---|
| D-pad or stick (left or right) | Click wheel | Down/right = forward, up/left = back; hold to keep scrolling |
| A, or a stick click | Centre button | Select. Hold 0.5 s: options (Play Next / Play Later) |
| B | MENU | Back. Hold 0.5 s: back to the main menu |
| START or X | Play/Pause | Play/Pause |
| L / R | ⏮ / ⏭ | Previous / next track |
| Y | | Options for the selected song or album |
| SELECT | Hold switch | Lock/unlock the controls and dim the screen; the music keeps playing |
| MENU (function key) | | Tap: screen off/on. Hold 5 s: quit |
| SELECT + START | | Quit |

- **Search keyboard:** L and R jump six keys at a time.
- **Now Playing:**
  - up/down change the volume;
  - the centre button switches to the scrubber, and then up/down move through the track.
- **Screen:** it doesn't turn off by itself, as in iPod.js. Press MENU (function key) to turn it off, and again to turn it back on.

**Hold:** press SELECT to put the iPod in your pocket.

- All buttons are ignored and a padlock shows in the status bar. The screen dims while the music keeps playing.
- Up/down set the brightness (a small bar shows), and KnulliPod remembers it for next time. The default is a fifth of the normal brightness.
- An accidental press shows the "Hold" badge.
- After 5 seconds in Hold, the Now Playing screen comes up, with the clock in the middle of the status bar.
- Press SELECT again to unlock. SELECT + START quits even in Hold.

**Solitaire:**

- the wheel moves the hand, and A picks up or drops cards;
- holding A undoes a move;
- B opens the pause menu (Resume, Undo, Redeal, Quit);
- the game is saved.

**Brick:** left/right move the paddle, and A launches the ball.

## Audio quality

- Each track plays at its original sample rate (44.1, 48, 96, 192 kHz…) in 32-bit float, with no conversion in the app. 24-bit FLAC and ALAC reach the system mixer untouched.
- The app volume is the console volume. The signal always stays at 100%. The volume keys of the console or of Bluetooth headphones also move the volume bar in Now Playing.

## Now Playing

- When paused, the cover straightens up; it tilts again when the music starts.
- Shuffle and Repeat are shown as icons in the status bar.
- Changing the volume from the console or the headphones outside Now Playing shows a volume overlay.

## Headphones

- Headphone buttons work, wired (if the console supports them) and Bluetooth: play/pause, next and previous. They also work in Hold. The volume of Bluetooth headphones is handled by the system.
- Unplugging the headphones, or a Bluetooth disconnect, pauses the music, as on the iPod.
- With Bluetooth headphones connected, a Bluetooth icon shows in the status bar.

## Options (in `KnulliPod.sh`)

- `MIYOOPOD_SWAP_AB=0`: swap A and B, if they are reversed on your console.
- `MIYOOPOD_STICK_WHEEL=1`: turn the stick like the iPod click wheel instead of using it as arrows. Spinning faster scrolls faster.
- `MIYOOPOD_WHEEL_STEP=15`: with the wheel on, degrees of rotation per step.
- `MIYOOPOD_HOLD_BRIGHTNESS=10`: brightness in % during Hold, until you change it with the arrows (default: a fifth of the current one).
- `MIYOOPOD_HOLD_ADJUST=0`: the arrows don't change the brightness in Hold (Hold is completely inert).
- `MIYOOPOD_SOFT_VOLUME=1`: use the app's internal volume instead of the console's.
- `MIYOOPOD_MEDIA_KEYS=0`: ignore headphone buttons.
- `MIYOOPOD_BT_ICON=0`: no Bluetooth icon and no pause on disconnect.
- `MIYOOPOD_PERF=1`: log the drawing times every second (useful if something stutters).
- `MIYOOPOD_VSYNC=0`: disable vertical sync.
- `MIYOOPOD_CLASSIC_UI=1`: go back to the original MiyooPod interface.

## Notes

- **Where data is kept:** the library, settings (volume, shuffle, repeat), artwork and the Solitaire game are kept in `knullipod/conf`, unless you choose another folder in Settings → Database.
- **Settings → Database:** chooses the database folder (library, artwork, settings, statistics, saves). The quick choices are:
  - the port folder (default);
  - the music folder (`/userdata/music/.knullipod`, hidden so the scan skips it);
  - a `knullipod` folder on each card or drive mounted under `/media`.

  "Choose a folder…" browses to any folder, where you can also create a new `knullipod` folder.

  - If there's no database in the chosen folder, the current one is moved there ("Move database here").
  - If there already is one (for example a card used on another console), you can use it as it is ("Use database there") or replace it with the current one ("Replace with current").

  The choice is stored in `knullipod/conf/datadir.txt`. If the folder is missing at start-up (card removed), KnulliPod uses the port folder and says so on the same screen.
- **Settings → Software Update:** with Wi-Fi on, "Check for updates" looks for a new version on GitHub. "Install" downloads it, checks it, replaces the app's files and restarts KnulliPod. Your library, settings and saves are not touched, and the previous binary is kept as `knullipod/MiyooPod.prev`.
- **Log:** the app log is `knullipod/miyoopod.log`.
- **Settings → Theme:** light (as iPod.js), dark, or automatic (dark from 20:00 to 7:00).
- **Shuffle and Repeat:** in Settings they open a submenu, with a tick on the active option.
- **Settings → Library** shows:
  - the music folder;
  - how many songs, albums and artists were found;
  - the formats, and the unsupported files (for example Opus);
  - the space used and free;
  - when the last scan ran.

  "Rescan now" starts a new scan from there.
- **Albums:** they are grouped by folder and album name, so they stay whole even when tracks have different artists or no "album artist" tag. CD1, CD2, Disc 1… subfolders count as one album.
- **Main menu:** while music plays, the cover, song, artist and album show at the bottom.
- **Cover Flow:** flip a cover to see its tracks with their lengths; the playing one has a ▶ before its title.
- **Not included:** Settings leaves out the iPod.js items that make no sense on the console (streaming service, sign-in, iPod body theme and haptics).
