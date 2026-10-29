# Changelog

## v0.71.1 (2026-10-29)

### Fixed
- **A received album is in the library at once**, without scanning the whole music folder: only its own folder (Shared/<album> in the music folder) is read and added, and the library is saved. Listen Now is ready in a moment.

## v0.71 (2026-10-29)

### Changed
- **Share, when it's received:** the green pedestal sinks away, the cover turns over once while it rises to the middle and grows, then RECEIVED, the title and the artist, and two buttons: **Listen Now** and **Receive Another** (← → to choose, A). Listen Now says "Adding…" for the few seconds the songs take to enter the library; no more "Adding it to your library" under "Now in Music".

### Fixed
- **The keyboard:** with nothing left to delete, B closes it and goes back.

## v0.70 (2026-10-29)

### Added
- **Settings → Sharing → Device Name:** the name your friends see when you share with them (without one, a friend's betterPod).
- **Buttons look like buttons.** Wherever betterPod names a button it draws it: A, B, X and Y in a circle, L, R, L2, R2, START and SELECT in a rounded rectangle, the arrows in a little square and the d-pad as a cross (the hints of Share, 2048, Five Letters, Puzzles, Guess the Song, the screensaver's settings, "press A to…" lines).

### Changed
- **Share:**
  - next to the number of songs, how big it is (on both sides);
  - while it sends or receives, the laser stays on the edge between what has gone (or arrived) and what is still to come, instead of sweeping;
  - receiving shows the speed too;
  - when it's received, Y opens the album in the library (A still plays it). The songs are in the music folder, in Shared/<album>, and in the library under the album's own name.

## v0.69.2 (2026-10-29)

### Fixed
- **betterPod knows its own version again.** Since 0.67.1 it said it was 0.67, so Software Update kept offering the update you already had.

## v0.69.1 (2026-10-29)

### Changed
- **Share:** the laser over the cover moves slower and more smoothly (a sweep takes 7 seconds while waiting, 3 while sending).

## v0.69 (2026-10-29)

### Added
- **Share:** send an album or a song to another betterPod, with an 8-digit code.
  - **To send:** an album's or a song's options (Y) → Share. The code shows; the cover stands on a white pedestal, and a laser goes up and down over it while it is sent.
  - **To receive:** Music → Receive, set the 8 digits (↑ ↓ change a digit, ← → move), A. The cover arrives first and is built line by line on the other pedestal, then the songs; they go in `Shared/` in the music folder and into the library, and Play plays them.
  - **Over the internet or the same Wi-Fi.** The two consoles meet on the public servers of magic-wormhole through the code; the files go straight from one to the other when the network allows it, else through its relay. Either way they are encrypted from one console to the other, and a wrong code just ends the share.
  - Before receiving, betterPod checks there is room on the card. Only music, pictures, lyrics and cue sheets are written.
  - A few little things to find.

## v0.68.1 (2026-10-29)

### Fixed
- **Cover Flow loads the covers in the shelf's order**, left to right (numbers first, then A to Z), instead of from the far end. The covers asked for by the screen are now loaded first in, first out.

## v0.68 (2026-10-29)

### Added
- **Apps → Five Letters:** guess a five-letter English word in six tries.
  - Tiles turn green (right letter, right place), orange (in the word, elsewhere) or grey (not in the word), one after the other; the on-screen keyboard keeps the colour of each letter.
  - **Keys:** the arrows move on the keyboard, A types the key (ENTER and ⌫ too), Y deletes a letter, START sends the word. When the game is over, A or START gives a new word.
  - Words that aren't in the list shake and say so. Games played, the share won and the streak are shown at the left and kept; the game in progress is kept too.
  - About 2,100 common words to guess (from SCOWL, without plurals and past tenses), and about 8,600 accepted as guesses (SCOWL and ENABLE).

## v0.67.2 (2026-10-29)

### Changed
- **Apps → Puzzles:** each puzzle has its own icon on the right of the list (a minefield, a Sudoku grid, a network of pipes, a tent by a tree…), so you can tell them apart at a glance.

## v0.67.1 (2026-10-29)

### Changed
- **About and Software Update** carry the new betterPod logo and the two-colour wordmark, with the tagline "The mid-2000s music player, reborn on retro handhelds", the site (betterpod.xyz) and the community (r/betterPod).
  - The credits list Puzzles (Simon Tatham) and 2048 (Gabriele Cirulli), and a note says betterPod is an independent fan project, not affiliated with Apple.
- The Settings preview shows the new glossy gear.

### Fixed
- **Guess the Song:** a title too long for its row now scrolls when it is selected (and on the right answer after the reveal), instead of being cut.

## v0.67 (2026-10-29)

### Added
- **Apps → Puzzles:** twelve puzzles from Simon Tatham's Portable Puzzle Collection (MIT). Their own code runs inside betterPod, drawn as big as the screen allows.
  - **The puzzles:** Mines, Sudoku, Net, Flood, Same Game, Fifteen, Light Up, Unruly, Towers, Keen, Pattern (nonogram) and Tents.
  - **Keys:**
    - the arrows move the cursor (in Fifteen they slide);
    - A does the puzzle's action, Y the second one (flag, mark, turn back);
    - L undoes, R redoes, START starts a new puzzle.
  - **Sudoku, Towers and Keen:** R chooses the digit (or "clear"), A places it, Y turns pencil marks on and off.
  - The line under the puzzle shows its keys and its own state (time, mines marked, moves).
  - Each puzzle is kept when you leave, and comes back as it was.

### Fixed
- **X is play / pause again in 2048.** It started a new game there; now START does.

## v0.66 (2026-10-29)

### Added
- **Apps → 2048**, after Gabriele Cirulli's game (MIT).
  - The arrows slide the tiles; two equal tiles merge.
  - Glossy tiles, from white through orange, red and yellow to green, blue and purple, with slide, merge and appear animations.
  - Score and best score.
  - X starts a new game; B goes back, and the game is kept for next time.
- **Apps → Guess the Song**, a quiz on your own music.
  - A part from the middle of one of your songs plays; pick its title among four with UP / DOWN and A.
  - Ten rounds, 15 seconds each: the faster the answer, the more points (up to 100). The cover, the right title and the artist show after each answer. The best score is kept.
  - What you were listening to is put aside and comes back where it was, playing or paused, when you leave.
  - Podcast episodes are left out.

## v0.65 (2026-10-29)

### Changed
- **Games is now Apps**, in the main menu and as its title. It holds Brick, Solitaire and Weather, and not all of them are games.
- **A new icon for Apps:** four small glossy app tiles (Brick's bricks, Solitaire's spade, Weather's sun, and a "+" for what comes next) instead of the die.

## v0.64.1 (2026-10-29)

### Fixed
- **Wi-Fi icon: the biggest arc was cut at its ends.** The icon was too narrow for it. It is now 18 px wide, and all three arcs show whole with their rounded ends.

## v0.64 (2026-10-29)

### Added
- **The main menu says betterPod.** iPod is Apple's name, so the main menu now says betterPod. **Settings → Turn it Apple** brings back "iPod" with On; Off is the default.

### Fixed
- **A freeze when picking a song as another one ends.** The song picked loads in the background. If the previous one ended at that moment, or was stopped by the new one (which also says "finished"), the track end loaded the next song at the same time. Two loads that change the sample rate could hang the sound and the screen. Now songs load one at a time, and a song stopped by a new one is not a track end.

### Changed
- The README's screenshots show betterPod and the two-column Settings.

## v0.63.4 (2026-10-29)

### Fixed
- **The Wi-Fi icon shows on more consoles.** It looked only at /proc/net/wireless, which some Wi-Fi drivers leave empty. Now:
  - the wireless interface is found by name, and "connected" means it has a link;
  - the signal is read from /proc/net/wireless, else from `iw`, else from `wpa_cli`;
  - when the console is connected but no signal can be read, the icon shows full.
- The log says what was found when it changes ("Wi-Fi: wlan0:up/1 signal 3/3"), to check a console where the icon still doesn't show.

## v0.63.3 (2026-10-29)

### Fixed
- **Visualizer: the bar at the top no longer shows the song's title.** It comes up for a moment while the brightness or the volume changes, and now shows only the clock there. The song has its own block at the bottom.

## v0.63.2 (2026-10-29)

### Changed
- **Settings: a lighter shadow on the right column.** It is about a third as dark and half as wide as before.

## v0.63.1 (2026-10-29)

### Changed
- **The repository is now github.com/stefanobuttarelli/betterPod.** Software Update looks there first. The old address still leads to it, so installations that haven't updated yet keep finding updates.
- The download links in the README point to the release folder, so they don't go stale with each version.

## v0.63 (2026-10-29)

### Changed
- **Settings in two columns.** The right side used to show the same picture for every row. Now it shows the menu of the row the cursor is on, under its own header bar. The left column lies over it, with a soft shadow falling on it.
  - **A or RIGHT** move the cursor into the right column; the left cursor turns grey. **B or LEFT** bring it back.
  - **Columns:** Shuffle, Repeat, Theme, Library, Database, Podcasts and Screensaver show their choices there.
  - **Podcasts** is now a column with On / Off, instead of switching straight away.
  - **Summaries:** About, YouTube, Bluetooth and Software Update show one there, and A opens their page as before.

## v0.62 (2026-10-29)

### Added
- **Visualizer: the Spectrogram.** A fifth effect, after Stars. The sound scrolls from right to left as a band of heat, with low notes at the bottom and high ones at the top, on the same 40 Hz–16 kHz scale as the bars. Small marks show 100, 1k and 10k. A frame of sound takes a column, and the screen holds about 5 seconds.
- **Visualizer: A changes the colours**, for every effect. Each press shows the name for a moment, and the choice is kept.
  - **Classic:** each effect's own colours (blue bars, rainbow hues, a magma heat).
  - **Album:** made from the cover of the song that plays, and it follows the song.
  - **Magma, Aurora, Sunset, Ocean, Phosphor, Mono.**

## v0.61.1 (2026-10-29)

### Fixed
- **Track lists: the current song shows a pause mark while paused.** Before, the triangle stayed even when the music was paused. This covers the album's card in Cover Flow and Now Playing and the other track lists. The mark changes as soon as you press play or pause.

## v0.61 (2026-10-29)

### Changed
- **KnulliPod is now betterPod.** betterPod is the product; this is its version for Knulli, and versions for other systems can follow.
  - The name changes in the app (About, Software Update), in the documentation and in the files.
  - The installation moves once to its new name: `ports/knullipod` becomes `ports/betterpod`, `KnulliPod.sh` becomes `betterPod.sh`, and the binary is `betterpod`.
    - The update reaches KnulliPod as usual and restarts it. The next time it is started from Ports, its launcher moves it, with the database.
    - In EmulationStation, update the game lists once: Ports then shows **betterPod**.
  - A database folder chosen in Settings → Database takes the new name too: `.knullipod` in the music folder becomes `.betterpod`, and `knullipod` on a card becomes `betterpod`. New ones are made with the new name.
  - A music move that was stopped goes on as before.
- **Software Update** reads `release/latest-knulli.json`, one manifest per system. It looks in the repository `betterpod`, then under its old name `knullipod`, so updates keep coming before and after the repository is renamed.

## v0.60.2 (2026-10-29)

### Changed
- **The album's songs are the back of the cover.** START in Now Playing, and A on an album in Cover Flow, now turn the cover over as one thing. The card is on the cover's back: it starts the size of the cover when the cover is edge-on, and grows to the whole card as it finishes the turn. Turning back, it shrinks into the cover and the cover lands. There is no fade between card and cover any more, so two pictures never show at once.

## v0.60.1 (2026-10-29)

### Fixed
- **Cover Flow's Now Playing: START stays in Now Playing.** The cover turns over to the album's songs and back without going to Cover Flow. The header still reads Now Playing and the other covers stay hidden. **B** on the card turns it back to Now Playing too, as START does, and **A** plays the song picked.

### Changed
- **Smoother turn of the cover** (Now Playing, Cover Flow's Now Playing and Cover Flow):
  - The song's info fades as the cover starts to turn.
  - The card comes only once the cover has turned away, so the two never show at once.
  - Turning back, the card goes quickly and the info returns as the cover lands.

## v0.60 (2026-10-29)

### Added
- **Status icons in the bar at the top, beside the battery, grey like the Bluetooth mark:**
  - **Headphones:** shown when the sound goes to headphones, Bluetooth ones (with the Bluetooth mark beside them) or wired ones in the jack.
  - **Wi-Fi:** shown while connected. Its three arcs fill with the signal (strong, medium, weak; only the dot when it is very weak). It is read every 5 seconds from the system, without running programs.
  - The screensaver's bar shows them too, in white.

## v0.59.3 (2026-10-29)

### Changed
- **Visualizer: the song comes up by itself.** When the song changes (it ends, L / R, or one picked elsewhere) or the music plays again after a pause, the song's block rises at the bottom for 5 seconds, then goes down again. Pausing does not bring it up.
  - Brought up with **UP / DOWN**, it stays until UP / DOWN again and just changes with the song.

## v0.59.2 (2026-10-29)

### Changed
- **Visualizer: the big clock (START) is frosted glass.** Thin digits (Inter Display Light) show the effect behind them, blurred and lightened, with a bright rim. The dark patch behind the clock is gone, and the date has a soft shadow so it reads on any effect. The clock also sits a little higher, so the date clears the floor of the Bars effect.
- The font is cut down to the digits and the colon (4 KB).

## v0.59.1 (2026-10-29)

### Changed
- **Visualizer: the whole screen, no bar at the top.** The effects fill 640×480. The bar shows only for a moment while the brightness (L2 / R2) or the volume changes. The effect's name now shows at the top, so the song can come up at the bottom.

## v0.59 (2026-10-29)

### Added
- **Screensaver: the Visualizer.** Settings → Screensaver → **Style** chooses **Cover Flip** (the covers, as before) or **Visualizer**, effects that move with the music being heard.
  - **The effects:**
    - **Bars:** the spectrum in blue bars with falling caps and a reflection.
    - **Waves:** the sound's wave, glowing, its trails drifting outwards.
    - **Tunnel:** the bands round a ring, the picture turning into the middle, like the old Infinity.
    - **Stars:** sparks thrown from the middle, bursts on the beats.
  - **The keys:**
    - **LEFT / RIGHT** change the effect (its name shows for a moment, and the choice is kept).
    - **START** shows a big clock with the date.
    - **UP / DOWN** bring up the song at the bottom, as Now Playing's block shows it, on a dark fade instead of its background.
    - **B** leaves (to Now Playing while a song plays), **X** plays and pauses, **L / R** change song, **L2 / R2** set the brightness.
  - **The sound:** it is taken from what the console is playing (the mixer's output). An FFT gives 40 bands in decibels, with automatic gain, a quick rise and a slower fall, and the beats of the bass. It follows what is heard, not the ~90 ms chunks the audio is sent in.
  - **Light on the battery:** 30 frames a second. The trail effects draw at half size and are doubled onto the screen, the bars from ready-made pictures. With nothing playing it says so, and stops drawing after a moment.

## v0.58.3 (2026-10-29)

A review of the last releases: bugs, cleaner code, less work per frame.

### Fixed
- **Move Music, safer:**
  - **Never over someone else's files.** A move goes only into an empty music folder on the other card, or into one this same move started (a `.knullipod-move` note there says from where), so files already on the card aren't overwritten.
  - **Checked before it starts.** On a FAT/exFAT card it refuses names the card can't hold, two names that differ only in upper/lower case (one would overwrite the other), and files over 4 GB (FAT32), before copying anything.
  - **A repair now really copies again.** A copy that doesn't match is removed and copied again. Before, it could be skipped as "already there". If any had to be repaired, every file is checked once more.
  - **Switched before anything is deleted.** The card is flushed, the music folder (and the database, if it lived inside) is switched and remembered on disk, and only then are the originals deleted. A file that changed since it was copied is kept. A file that vanished meanwhile no longer fails the whole move.
  - **The database's folder** inside the music folder is copied when switching, not as it changes during the copy.
  - **The same card:** a card is recognised by its UUID, so another card in the same slot doesn't count as it.
  - **Smaller things.**
    - Only the folders the move emptied are removed.
    - Playlists over 16 MB stop the move instead of being cut short, and only lines starting with the old folder are rewritten.
    - Downloads can't start during a move.
    - Stop isn't offered while the originals are being deleted.
    - The card is looked at every 2 s, not once per file.
    - The space needed counts only what's left to copy, and files already there don't count in the speed.
- **Screensaver:**
  - The stick (wheel mode) no longer leaves a repeat running that scrolled the menus by itself after leaving.
  - A key before it shows puts the brightness back.
  - A song change can't leave the same album twice.
  - A new library (a rescan) closes it.
  - Starting it while it leaves keeps Now Playing's cover.
  - Leaving during the fly-in flies back.
  - Now Playing's card closes when it starts.
  - The brightness set with L2 / R2 is saved when the key is let go, not 25 times a second.
- **Now Playing:**
  - The cover cross-fades from the first frame of a new album.
  - START with no song plays and pauses as before.
- **Cover Flow:** START on an album's back reached from the flow no longer opens Now Playing for that album. START with the options row open turns the cover over. Up and Down on the options row work as on the other Now Playing.
- **YouTube cover:** the loading stripes move (they were still). The automatic cover no longer replaces an existing one when a download goes on.

### Faster
- **Now Playing's cover is kept and only drawn again when it changes.** Before, its 3D picture was redrawn every frame.
  - The card's list uses a kept buffer, and the buffers are freed when the card closes.
  - The cover isn't drawn under the card.
  - The art isn't fetched when the cover is turned away.
- **The screensaver:**
  - It looks up the playing album once per song.
  - It picks covers under one lock.
  - Its warm-up no longer redraws the screen, and runs only when the screensaver can start by itself.
  - The picked cover is scaled from the small picture.
  - It doesn't work with the screen off.
- **The brightness bar (L2 / R2, Hold)** no longer redraws the whole screen for 1.5 s. The brightness is read from the screen only when a key is first pressed.
- **The YouTube cover's square** is made from a smaller copy of the picture as it moves, and its little picture is made away from the screen.
- **A search's songs:** each album is sorted once.

### Removed
- The old Now Playing cover code (its own perspective drawing, now Cover Flow's) and other unused functions.

## v0.58.2 (2026-10-28)

### Fixed
- **Screensaver: B goes to Now Playing only while a song plays.** With the song paused (or none), B goes back to the screen it started over (a YouTube download, a list…), as it was.

## v0.58.1 (2026-10-28)

### Changed
- **One Now Playing.** The Now Playing reached from the menus now draws its cover exactly as Cover Flow's Now Playing does: the same 3D cover, turned 35° while playing and flat and smaller when paused, with the same reflection. Both have the same keys.
- **START turns the cover over like Cover Flow.** It turns straight from where it is (turned, while playing) to the back, half a turn, with the same card coming in, and back the same way to the turned pose. It is flat only when paused. The song's lines and the rows fade out while the card shows.
- **Cover Flow's Now Playing:** START turns the cover over to its album's songs too, and START there goes back to Now Playing with the song going on. A, A shows the Shuffle / Repeat row there as well, and B closes it first.

## v0.58 (2026-10-28)

### Added
- **Now Playing: START turns the cover over** to the back of the album, the same card as Cover Flow's (its size and place, the album and artist on blue scrolling when long, the songs).
  - **UP / DOWN** pick a song, **A** plays it and the card turns back to the cover.
  - **START** or **B** turn it back, and **Y** opens the song's options.
  - Playing, the turned cover first turns flat, then over.
  - START no longer pauses on Now Playing; X does.
- **Now Playing: the options row.** **A** goes from the progress to the scrubber (as before), and a second **A** brings in a row of two buttons as wide as the bar, **Shuffle** and **Repeat**, each with its icon and value.
  - **LEFT / RIGHT** (or the wheel) pick one, and **A** changes it: Shuffle goes Off → Songs → Albums, Repeat goes Off → All → One.
  - **B** goes back to the progress, as does 8 s without a key.

## v0.57.1 (2026-10-28)

### Fixed
- **Screensaver ↔ Now Playing: one cover only.** While the cover flies, Now Playing's own is hidden and its screen drawn without it. Before, the old picture stayed under the flying one, so two covers showed. Playing, Now Playing's turned cover first turns flat, the flying cover takes over from it exactly where it is, and coming back it lands flat on it and then turns again. The flying cover's size and place are measured on the screen.
- **Covers always there, quickly.**
  - A cover the screen asks for now goes ahead of the background work (the thumbnails' prefetch, the screensaver's warm-up). That work waits while there is one, and takes at most one of the two decoding places. Before, a cover could wait seconds behind it.
  - When a song starts, Now Playing's covers are got ready: its album's, and those of the songs before and after it. So are the ones of an album turned over in the screensaver.
  - While Now Playing's cover is being made, the same cover at another size already in memory stands in, and is replaced by the real one.

### Added
- **Screensaver bar: the artist and the album take turns** after the title (6 s the artist, 4 s the album), the album coming in from below, so everything gets read.

## v0.57 (2026-10-28)

### Added
- **Screensaver ↔ Now Playing, one cover.** From Now Playing, its cover flies smoothly into the big square while the mosaic fades in around it. With a song on, **B** leaves the screensaver to Now Playing: the big cover flies back to its place there while the mosaic fades out, and hands over to Now Playing's own cover. Without a song, B just closes it.
- **Screensaver: Y as anywhere.** On a picked cover, Y opens the album's options (Play Next, Add to End of Queue, Edit Album Info). On the turned-over card it opens the song's. A screen opened from there closes the screensaver.
- **Screensaver bar: the song changes with a slide.** The next song comes in from below as the old one goes up; going back, it comes down from above.

### Fixed
- **A song found by Library search plays on with its album.** Next and previous now move to the album's next and previous songs, instead of finding nothing.
- **A key held when the screensaver started no longer counts as a long press** (B held could take you back to the main menu under it).

## v0.56.1 (2026-10-28)

### Fixed
- **Screensaver: the last column can be picked too.** Its covers are cut by the edge of the screen; picked, the cover comes in whole (moved left, inside the screen) and A turns it over like the others.

## v0.56 (2026-10-28)

### Added
- **L2 / R2 set the brightness anywhere:** L2 down, R2 up, in steps of 5% (1% at the bottom). Held, they keep going. The level shows for a moment in the header, a sun and a bar where the title is, like the volume. In Hold they set Hold's brightness (as UP / DOWN do). In the screensaver they set the console's brightness, unless Dim the Screen is on: then they set the screensaver's own, which goes back as it was when it closes.

## v0.55.2 (2026-10-28)

### Changed
- **Screensaver bar: paused, the song goes away.** Its title and artist slide down out of the bar, and when it plays again they slide back up next to the clock.

## v0.55.1 (2026-10-28)

### Fixed
- **Screensaver: the volume shows in the bar again** when it changes (the console's keys, the headphones'), as in the header. The clock and the song slide up and the speaker (or headphones) and the bar come in from below, in white. It shows even when Now Playing is under the screensaver.

## v0.55 (2026-10-28)

### Added
- **Screensaver: the song playing in the bar,** next to the clock: its title in white and the artist in grey, cut short with "…" when there's no room.

## v0.54.3 (2026-10-28)

### Fixed
- **Screensaver: no cover is ever shown twice.** A cover only turns over to an album that isn't on the screen. When the big cover changes to an album already showing, that small cover turns over to another. With fewer albums than cells, the last cells stay dark.
- **Covers ready from the start:** the screensaver's covers are got ready in the background right after the library's thumbnails, not just before it starts. They are kept on disk at its two sizes (made too whenever a cover is first decoded), so later starts only read them. The big cover of each new song's album is got ready when the song changes.

## v0.54.2 (2026-10-28)

### Fixed
- **YouTube download:** while the cover is made and the audio downloaded, the progress no longer says "Track 1 of N". Those steps are for the whole video, and the tracks come after, when it is cut at the chapters ("Track 2 of 5 · Making the tracks"). For a playlist, each video still shows its own track.

## v0.54.1 (2026-10-28)

### Fixed
- **Screensaver: the covers no longer load in bits.** They are got ready at the screensaver's two sizes in their own memory, a little before it starts and in the background after (made once from the Cover Flow size and kept on disk). The mosaic appears when its covers are ready, and a cover turns over only to an album that is ready.
- **Smoother:** covers growing, flying or turning are scaled straight onto the screen, without a new picture each frame. Between flips nothing is drawn.
- **L2 / R2 set the brightness** in the screensaver whether or not Dim the Screen is on. It goes back as it was when the screensaver closes.
- **The bar is laid out like the header:** the clock on the left, and play / pause, the padlock and the battery where the header has them. The battery is drawn as a white outline.
- **The turned-over cover is as wide as Cover Flow's,** going down past the bottom of the screen like it. The album and artist scroll when they are long, and so does the selected song.
- **The picked cover has only its white border,** no shadow.

## v0.54 (2026-10-28)

### Added
- **Screensaver: a mosaic of album covers.** Settings → **Screensaver** starts it after 1, 2, 5 or 10 minutes without a key (or **Never**). **SELECT + Y** starts it by hand, anywhere.
  - **The layout:** 110 px covers in 4 rows, under a black bar with the clock, play / pause, the battery and, in Hold, the padlock, all white. The album playing is the big cover in the top left (2×2); with nothing playing all are the same size.
  - **Movement:** from Now Playing, its cover flies into the big square. Every 5 s one to three covers turn over and show another album, never the same as a neighbour. When the song changes to another album, the big cover turns over to it.
  - **The arrows** pick a cover: it grows a little, with a white border. **A** turns it over in the middle of the screen with its songs, like Cover Flow. Choosing a song plays it, and the cover flies back into the big square. **B** turns it back, or leaves the screensaver. **X / START** play and pause, **L / R** change song.
  - **Dim the Screen** (Settings → Screensaver) lowers the brightness while it shows. **L2 / R2** change it, shown for a moment in the bar.
  - **With Hold on:** Now Playing after 5 s, then the screensaver after its time. The keys do nothing; UP / DOWN still set Hold's brightness, shown in the bar.

### Changed
- **Hold's brightness shows in the header** (a sun and a bar, like the volume), no more in the middle of the screen.

## v0.53 (2026-10-28)

### Added
- **YouTube cover: zoom.** On the Cover screen **R** zooms in (a smaller square) and **L** zooms out, down to a sixth of the picture. Held, they keep zooming. The arrows now move the square in all four directions, so any part of the picture can become the cover. The keys under the picture say so: "◀ ▶ ▲ ▼ Move · L R Zoom · A Use This · Y More".

### Fixed
- **The square starting from Whole Picture began in the top-left corner,** not in the middle. A rectangle was built the wrong way round.

## v0.52.2 (2026-10-28)

### Fixed
- **A screen could stay half slid in** (seen opening the YouTube Cover screen) until a key was pressed. When a frame took a while to draw (a big picture being got ready), the slide's time ran out before its last frame, and with nothing else moving the screen wasn't drawn again. Now the frame after any animation ends is always drawn, so a slide, a sheet or a fade always finishes where it should.

## v0.52.1 (2026-10-28)

### Fixed
- **YouTube cover:** choosing the Cover row opens its screen at once. While the picture is got ready (scaling it and finding the cover take a moment on the console), the whole screen shows the old iPod's moving 45° stripes, with "Getting the picture ready…", instead of the screen freezing. Choosing Whole Picture shows the stripes in the cover's square while the blurred cover is made. The automatic cover is now worked out once per picture, not each time the page is drawn.

## v0.52 (2026-10-28)

### Added
- **Move the music to another memory card:** Settings → Library → **Music Folder**. It lists the cards, with their free space, and the one the music is on now ("Here"). Choosing another one asks first: **Move Music**, **Copy Music (keep the originals)**, or **Use the Music Already There** when a copy is there.
  - **Safe steps:** every file is copied, then read back from the new card and compared (CRC-32, from the card, not the cache). Only then, when moving, are the originals deleted. A file that doesn't match is copied again.
  - **Interruptions lose nothing.** Each file is written as name.kpart and renamed once whole. Moving again after a stop skips what's already there. A card taken out pauses it ("Paused · put the card back"), and it goes on when the card is back. It checks it's the same card, so it never writes into the empty folder a card leaves behind. A full card stops it.
  - **Afterwards:** the library uses the new folder (kept in musicdir.txt, also after a restart), and the database goes with it if it was inside. Playlists' lines with the old folder are rewritten. Now Playing is cleared and the library is scanned again.
  - **The screen:**
    - **Copying:** the albums travel from one card's pile to the other's, the one being copied flying across on an arc as its files are copied. Under it are the album, the file, the GB left on one side and arrived on the other, and a bar with the time left and the speed.
    - **Checking:** each album is lifted with a blue outline, and the checked ones get a ✓.
    - **When it's all done,** the albums open into a Cover Flow line.
    - **B** asks before stopping, or hides the screen while it goes on.
  - **Formatting a new card** is left to Knulli (Developer Options → Format A Disk).

## v0.51 (2026-10-28)

### Added
- **Choose the square cover before a YouTube download.** A video's or playlist's page has a **Cover** row: the cover it will get, with what the automatic cover does ("the cover found in the picture", "whole picture, blurred sides"…).
  - **Choosing it** shows the whole picture with the square on it, darker outside, and the cover as it will be beside it.
  - **Keys:** left / right move the square (up / down on a tall picture; held, they keep moving). A uses it, B goes back as it was.
  - **Y** offers Whole Picture (nothing cut off, blurred sides) and Automatic.
  - The cover is made from the choice when the download runs, and replaces an earlier one.

## v0.50 (2026-10-28)

### Changed
- **Clear Now Playing asks inside the Now Playing block,** with no sheet over the screen. Y (or holding A) on the Now Playing row of the main menu or of Music:
  - **The question:** the song goes up and out of the block, and "Clear Now Playing?" comes up from below, with **Keep** (chosen first, so a press by mistake changes nothing) and **Clear** (red).
  - **Keys:** left / right choose, A does it, B keeps.
  - **Keep,** or 5 seconds without a key, brings the song back down.
  - **Clear** stops the music, and the block goes down and away.
  - Where the block isn't on screen, a sheet still asks.
- **The header's icons are closer together:** 4 px apart instead of 8 (play, Bluetooth, battery, padlock, shuffle and repeat).

## v0.49.4 (2026-10-28)

### Fixed
- **Cover Flow's Now Playing cover looks square too.** It's drawn by Cover Flow, not as in Now Playing, so 0.49.3 didn't change it. Tilted, it's now 9 px wider; its perspective is milder, so 9 px are enough. Measured on screen it's 248×247, as in Now Playing; before it was 234×247. Paused, it's still square.

## v0.49.3 (2026-10-28)

### Fixed
- **Now Playing's tilted cover looks square.** In perspective its far side shrinks, so a square cover looked taller than wide (218×246 px on screen). Tilted, it's now 18 px wider, growing to the right, and looks square (247×247). It widens with the tilt, so paused (flat) it is still square.

## v0.49.2 (2026-10-28)

### Fixed
- **Y on Cover Flow's Now Playing opens Up Next,** as on the other Now Playing. B goes back to Cover Flow's Now Playing, and B again to the album's songs. The keys go to Up Next while it's open, and the menus no longer slide in under Cover Flow when it closes.

## v0.49.1 (2026-10-28)

### Fixed
- **The volume bar in the header is shorter on full screens.** At most 150 px instead of 240, so it no longer runs across the whole screen (Cover Flow, full lists). It's unchanged on the half-width menus.

## v0.49 (2026-10-28)

### Added
- **Settings → Bluetooth:** connect the headphones without leaving KnulliPod.
  - **Your devices:** the paired devices, with Connected (and the battery, when they tell it) or Not Connected. A on one connects or disconnects it. Y on one: Connect / Disconnect, Forget This Device (asks first).
  - **Find New Devices** searches for 12 seconds, with a bar. New devices show as they're found; A on one pairs it, trusts it (so it reconnects by itself) and connects it.
  - **Bluetooth On/Off.**
  - **The sound** goes to the headphones once they're connected (PipeWire's default output).
  - **The list** is read again every 4 seconds while the screen is open, so a change made elsewhere shows. Errors are in words ("not found · is it on and close by?").
  - **A picture for each device:** over-ear headphones, earbuds, speaker, game controller or Bluetooth, from what the device says it is and its name.
  - **The Settings row** shows the device connected.
  - It uses Knulli's `bluetoothctl`. Without it, the screen says so.
- **The volume shows in the header,** on every screen but Now Playing (which has its own bar). When it changes from the console's keys, the headphones or the wheel, the title slides up and the volume comes in: an icon and a bar, shorter on the half-width menus and longer on full screens. It goes again after 1.5 s.
  - **The icon** is a speaker for the console's speaker and headphones for wired or Bluetooth headphones (the jack's state is followed).
  - It replaces the box in the middle of the screen.

## v0.48.2 (2026-10-28)

### Fixed
- **The options over Cover Flow work.** Y on an album in Cover Flow opened Play Next / Add to End of Queue / Edit Album Info. But the arrows and A still moved and opened Cover Flow underneath, and B closed Cover Flow's album instead of the sheet. Now a sheet takes the keys first, over Cover Flow too.
- **The song in the header shows only where the song isn't shown already.** It no longer shows on the menus with the Now Playing block at the bottom (the main menu, Music, an album's songs…), or while the preview on the right is Now Playing. It shows on the other screens, and in Cover Flow.

## v0.48.1 (2026-10-28)

### Fixed
- **Wired earphones' button works, as on the iPod.**
  - **What it does:** one click plays or pauses, two clicks go to the next song, three to the previous one.
  - **Why it didn't work:** the button comes from the headphone jack as KEY_MEDIA, KEY_PHONE or (with drivers that give no key types) BTN_0. KEY_MEDIA was taken as a play/pause key, but its repeats within 400 ms were dropped, and the other codes were ignored.
- **The volume buttons of wired earphones change the volume** (BTN_2/BTN_3, or the volume keys of the jack device; the console's own volume keys stay Knulli's).
- **Diagnostics:** the log now names the headphone-jack device, and the first 20 key codes each media device sends, so a button that still does nothing shows its code.

## v0.48 (2026-10-28)

### Added
- **Album Info** (by Emanuele Macrì, PR #3): an album's tags, filled in online or by hand, and written into its files.
  - **Where:** Y (or holding A) on an album, in the lists or in Cover Flow → Edit Album Info. Settings → Library → **Missing Info** lists the albums without an artist or track numbers.
  - **Find Info Online** asks MusicBrainz, iTunes, Deezer and Wikipedia. Choosing a result fills in album, album artist, year and genre, and pairs its songs with yours (titles, artists, numbers). For a game rip, Wikipedia gives the composer or the game's developer.
  - **By hand:** every field can be typed, for the whole album or for one song.
  - **Save Changes** writes the tags into the MP3, FLAC and Ogg Vorbis files and scans the library again.

### Changed (on top of the PR, from its review)
- **Only the fields you changed are written.** Everything else in each file stays exactly as it was: dates with a month, totals, discs as "1/2", other spellings like "ALBUM ARTIST", several values. Before, saving one field rewrote them all from what the library had read, so some were lost.
- **Songs are paired only when it's sure:**
  - First by the same title.
  - Then by the same number, if the lengths agree.
  - Then by the same length.
  - Never by position: songs without numbers are listed alphabetically, so that gave the wrong titles. An edition with an extra intro no longer shifts every title by one.
- **The confirmation says what changes** ("Write into 12 files? Titles of 9 · Numbers · Year. It can't be undone."), in red. The old values of each file go into the log first.
- **One save at a time:** tag writes (Album Info and Podcasts) can't overlap. An album can't be opened again while it's being saved.
- **Safer screens and data:**
  - After a save, only its own screens close.
  - A field that differs on each song opens empty, so pressing START by mistake no longer gives every song the first one's value.
  - Results that load after you leave are dropped.
  - A read-only card stops at the first file with a short message.
  - Values from the internet are cleaned (no control characters, at most 200 characters).
  - Deezer's errors are reported.
  - Compilations ("Various Artists") no longer count as missing an artist.
  - M4A is named among the files that can't take tags.

## v0.47 (2026-10-28)

### Added
- **Clear Now Playing** (by Emanuele Macrì, PR #4). It stops the music and empties the queue, so Now Playing and Up Next leave the menus (and the session saved for the next start) until something is played again.
  - **Where:** Y (or holding A) on the Now Playing row of the main menu or of Music. It's also the last row of Up Next, where it asks first.
  - **Up Next:** when only the song playing is left, it shows "Nothing up next" above it.

### Changed
- **Up and right raise the volume on Now Playing; down and left lower it** (PR #4). Before, the arrows followed the click wheel, where down is clockwise and raised it. The same goes for moving through the song, and for Cover Flow's Now Playing. The lists are unchanged.

### Fixed
- **The preview on the right follows when a row goes away.** After Clear Now Playing, the cursor moves onto Settings, and the preview used to stay blank until you moved.

## v0.46 (2026-10-28)

### Added
- **Pictures in the Internet Archive results.** Each item shows its own picture, its cover when it has one (archive.org's thumbnail, loaded three at a time after the results). They stop loading when you search again or leave.
- **Recent Searches** in Internet Archive, YouTube and Podcasts: the searches of the last 7 days, newest first, under the search field.
  - **Choosing one** runs it again. The field above opens the keyboard for a new search.
  - **The keyboard** no longer opens by itself when there are recent searches; it still does the first time.
  - **Y on one** removes it, or clears them all.
  - **Podcasts** shows the last 3, above Your Podcasts.
  - They're kept in `recent_searches.json` in the data folder.

## v0.45.3 (2026-10-28)

### Fixed
- **MENU + volume (brightness) no longer turns the screen off.** MENU on its own locks the screen. But Knulli also uses it with the volume keys to change the brightness, and it does that itself, so the app never saw those keys. Letting go of MENU then counted as a press on its own and locked the screen (backlight off, music still playing).
  - Now a MENU press is the lock only if nothing else happened while it was down: no other key, and no change of brightness. The brightness is checked a few times a second while MENU is held, and again when it's let go.
  - Holding MENU for a while to change the brightness no longer shows the exit warning or quits the app after 5 seconds.
- **Waking the console from sleep turns the screen on.** If the app was locked when the console went to sleep, it's unlocked on waking. Before, the screen stayed black while the music went on. A backlight left at 0 is also put back.

## v0.45.2 (2026-10-28)

### Fixed
- **The song in the header now shows on every screen but Now Playing.** When a song starts or changes, the header's title slides away for a few seconds and the song comes in with its little cover.
  - **Cover Flow:** it shows there too, where there was no way to tell which song was playing. It doesn't show on Cover Flow's own Now Playing.
  - **Lists with the mini player at the bottom** (an album's songs, Albums, Artists…): it shows there too, at the top where you're looking, even though the mini player already has the song.

## v0.45.1 (2026-10-28)

### Fixed
- **Weather: the rain line no longer contradicts the picture.** A day drawn with showers could say "Rain 0%". The picture and the chance come from different forecast models in Open-Meteo, and a missing chance (null) was read as 0.
  - A missing chance is now treated as unknown, not 0.
  - The day's amount is asked too. A wet day with no chance gives "Rain 2 mm" (or "Snow…") instead of 0%. When the chance is known, both are shown ("Rain 60% · 3 mm"), or only the chance if the line is too long.
  - A wet day with neither number says nothing about rain.

## v0.45 (2026-10-28)

### Fixed
- **The Solitaire icon is no longer cut off.** The tilted queen on the left went past the edge of its square. The fan of three cards is a little smaller now, so it shows whole.

## v0.44 (2026-10-28)

### Changed
- **Cancel is grey and Done is blue** on the download screens (Internet Archive, YouTube, Podcasts) and on Search All Automatically. A button looks blue only when the work is over (Done, Try Again), so it's clear when it has finished.
- **Software Update shows every note whole.** A long note wraps under its bullet instead of ending in "…", and all the notes are listed, not only the first three. When they don't fit, Down scrolls them to the end before it moves to Not Now, Up scrolls them back, and a thin bar shows where you are.

## v0.43 (2026-10-28)

### Changed
- **A YouTube video's screen keeps "YouTube" in the header.** The video itself is the first row: its thumbnail, title, channel and length (or the number of videos for a playlist). The loading bar, the download buttons and the chapters come below it. A video opened from a link gets its thumbnail once it's loaded.

## v0.42 (2026-10-28)

### Added
- **Settings → Podcasts (On/Off)** takes Podcasts out of the main menu, or puts it back under Music. It's kept after a restart, and the episodes already downloaded stay in Music. The main menu's cursor stays on the entry it was on.

### Changed
- **The hints name the button:** "Press A to show Playlists & Albums" instead of "Press for…", and "press A to retry" / "press A to try again" where a row can be pressed again.

## v0.41 (2026-10-28)

### Added
- **Podcasts** in the main menu, under Music, as on the iPod (by Emanuele Macrì, PR #2).
  - **Finding one:** search Apple's podcast directory (no key or account). The podcasts you download from stay listed under the search field.
  - **Episodes:** the newest 200 from the podcast's feed, newest first, with date, length and size. "Download the 3 Latest" gets the ones you don't have yet.
  - **Downloading** uses the Internet Archive screen (progress, time left, Cancel, Try Again). Episodes go into `Music/Podcasts/<podcast>/`, tagged, with the podcast's artwork as `cover.jpg`, so each podcast is an album. A downloaded episode plays when chosen.
  - **Deleting:** Y on an episode or on a podcast deletes what was downloaded, after asking.

### Changed (on top of the PR, from its review)
- **Only what KnulliPod downloaded counts.** Each podcast remembers the files it downloaded. Only those show as Downloaded, and only those are deleted. A `Music/Podcasts` folder of your own is left alone.
- **Try Again skips the episodes already done** instead of downloading them again.
- **A stopped episode is continued only if the server still has the same file.** It checks the file's ETag or date and its total size. Podcast hosts often put in different ads on each download, and joining two versions would break the file. Otherwise the episode starts over.
- **Every episode gets its own file name.** Two episodes with the same title on the same day become "… (2)". The episode's real title is used, not the shorter iTunes one.
- **The file decides its format, not the feed.** An M4A listed as MP3 is saved and tagged as M4A, and its cover goes inside. An error page or an Opus or raw AAC file is reported instead of being kept.
- **Tagged before it gets its name.** An episode is tagged while it's still `.part`, so a finished name is always a whole, tagged file.
- **Feeds listed oldest first** are sorted, so "Download the 3 Latest" really gets the latest.
- **Safer tag writing:**
  - A damaged ID3 tag is left as it is instead of being rewritten without its later frames (like the cover).
  - Tags whose frame sizes older iTunes wrote the wrong way are read correctly.
- **Smaller fixes:**
  - HTML entities like `&rsquo;` and any charset in feeds are read properly.
  - Made-up sizes and lengths are ignored.
  - `http://` feeds are tried over `https://` first.
  - The artwork is checked to be an image (a PNG is made JPEG).
  - Thumbnails stop when you leave the screen.
  - The episode list reads its folder once per refresh, not twice per episode.
  - A failed episode shows its own error rather than a later retry's.
- **Deleting in Browse Files and in Podcasts** now uses the same code.

## v0.40 (2026-10-28)

### Added
- **Missing Covers also searches iTunes, Deezer and Wikipedia** (by Emanuele Macrì, from [MacriPod](https://github.com/emanuelemacri/macripod), PR #1). None of them needs a key or an account.
  - **The sources:** iTunes for covers up to 1200 px (searches spaced by 3 s, as Apple asks), and Deezer up to 1000 px. Wikipedia gives the picture at the top of an album's article, or the game's box for a soundtrack ripped from a game.
  - **Choosing an album** asks MusicBrainz and the other three at the same time. Each result shows its source, and the checked results come first, with the very same title at the top.
  - **Search All Automatically:** when MusicBrainz has nothing, or nothing with a cover, it tries the others with the same checks. A result with the very same title wins over one that only shares the main title.
  - **Soundtracks by genre:** songs tagged Soundtrack, Game, Score, OST or VGM make their album count as a soundtrack.

## v0.39 (2026-10-28)

### Changed
- **A real search field** on top of every search (Library, Internet Archive, YouTube, Weather, Missing Covers), like the first iPhone's search bar, instead of a list row.
  - **Look:** a white rounded field with a magnifying glass on a light grey bar (dark in the dark theme).
  - **Text:** what you typed, or a grey placeholder ("Search YouTube or paste a link") after the cursor while it's empty. A long text keeps its end in view while typing.
  - **Note:** how many results, on the right.
  - **Selected:** the field gets a blue ring.

### Added
- **Opening a YouTube video shows what's happening** instead of a still "Loading…".
  - **The steps,** in words: asking YouTube, reading the page, getting YouTube's player, solving YouTube's challenge (the slow part), reading the formats.
  - **The seconds** elapsed.
  - **A bar** that moves on with each step and keeps creeping while one lasts.

## v0.38.2 (2026-10-28)

### Added
- **YouTube search results show the video's thumbnail** on the left of each row (a square from its middle), so you can tell which video it is. They load three at a time after the results, small ones first, and stop when you leave or search again.

## v0.38.1 (2026-10-28)

A review of the YouTube code.

### Fixed
- **The tools are checked before they're used.**
  - yt-dlp is checked against the SHA-256 published with its release (SHA2-256SUMS), and Deno against its .sha256sum.
  - Every tool must be a program for this console's CPU and must run.
  - An update is put in place only once checked, so a failed update leaves the old tools working.
- **yt-dlp starts faster:** the unpacked build is used (the one-file build unpacked itself on every start). The one-file build stays as a fallback.
- **Cancel stops everything at once:** yt-dlp, the Python it starts and the JavaScript runtime. Leaving a search or a video page stops its yt-dlp too.
- **The progress bar moves with the real yt-dlp** (it printed no progress without --progress).
- **Downloads:**
  - a playlist is downloaded by one yt-dlp, not one per video, and a video uses the information already loaded;
  - Try Again continues the partial download and keeps the tracks already made;
  - private and deleted videos in a playlist are left out, and a video that fails no longer stops the others;
  - free space is checked first.
- **Clearer messages:**
  - "No connection to YouTube" when offline;
  - "YouTube took too long to answer" on a timeout;
  - YouTube's request to confirm you're not a bot is named as such;
  - a video page that fails to load can be retried.
- **Links typed without https://** open, and a video link from a playlist or a mix opens the video.
- **Chapters without an end** end where the next one starts.
- **Covers:** a square picture is used as it is (never cropped), and the bars around a cover are found faster.
- **Cutting M4A:**
  - a damaged or unusual file gives an error instead of a crash;
  - only the sound track is taken from files that also have video;
  - a file cut short is reported;
  - the copy goes in large blocks (thousands of times fewer disk operations on long videos).
- **File names** are kept short enough in bytes for the card.

## v0.38 (2026-10-28)

### Added
- **YouTube** (off by default: Settings → YouTube). Search videos or playlists and download them as albums.
  - **Turning it on** downloads the tools from GitHub the first time: yt-dlp for ARM64 and a JavaScript runtime (QuickJS-ng, or Deno if that fails), about 40 MB, kept in the data folder. The same page shows their version, updates them (YouTube changes often: update when downloads fail) and removes them.
  - **Searching:** Music → Search → YouTube. The keyboard opens; Videos or Playlists & Albums; a YouTube link opens that video or playlist.
  - **A video with chapters** downloads as an album with a track per chapter ("01." numbering removed from the titles), or as one song. **A playlist** downloads as an album with a track per video.
  - **The tracks:** M4A, YouTube's AAC audio as it is (not re-encoded), with title, artist, album, track number, year and cover inside. They go in Music/YouTube/Artist/Album.
  - **Cutting at the chapters** is done by KnulliPod itself (no ffmpeg): one download, then the audio is split at the AAC frames.
  - **Names:** "Artist - Album" titles are split, and "(Full Album)", "(Official Video)" and the like are removed. YouTube Music's own artist and album are used when the video has them.
  - **A square cover** from the video's 16:9 picture:
    - the album cover inside the bars, cut out exactly, when the video is a still cover on a plain background;
    - the square picture, for YouTube Music tracks;
    - otherwise the whole picture, sharp, on a blurred and darkened version of itself.
  - **The download screen** is the same as Internet Archive's (progress, time left, cancel, Try Again).
  - **Only download what you may:** YouTube's terms don't allow downloading. Use it for your own or freely licensed videos.

### Changed
- **Music → Search** is a menu: Library (the search in your music), Internet Archive (moved here from the Music menu) and YouTube when it's on.

## v0.37 (2026-10-28)

A quality pass over the code of the last versions: bugs, speed and battery, smoother animations.

### Fixed
- **Holding B with an options sheet open froze the app.** It now goes back to the main menu. Buttons pressed while a sheet slides away are no longer lost.
- **A rescan with music playing could crash the app.** The new library is now built on the side and swapped in whole, and the menus show the old one until then. The queue and the queued marks follow the new library, and songs that are gone leave the queue.
- **Deleting songs that were in the queue stopped playback.** They now leave the queue at once.
- **Browse Files can't delete outside the music folder,** even through a link to a folder elsewhere.
- **A second rescan asked for during one was lost.** It now runs when the first ends.
- **Screens could stay on "Searching…" or "Loading…" for good** when a result came back while the app was busy. Results can no longer be dropped. Finding a cover could also hang its search for good.
- **Internet Archive downloads:**
  - without a connection, they stop after two tracks in a row fail, instead of trying every track for half an hour;
  - resuming an album only needs room for what is left;
  - a stale partial file is started over instead of being kept;
  - a PNG cover is saved as cover.png.
- **Covers:** a busy MusicBrainz is asked again after a pause, and a cover that fails to download tries the next candidate.
- **Play Next / Add to End of Queue after the queue has ended** plays the songs, instead of doing nothing.
- **Up Next:** choosing a song checks it is still there and goes to Now Playing. Skipped songs lose their queue mark. A long queue shows its first 300 songs, and Clear Up Next is instant even with Shuffle All.
- **The header ticker:**
  - a song that starts while the previous one slides out waits its turn, instead of jumping;
  - it doesn't start while locked;
  - it can no longer stay stuck.
- **Weather:** a saved forecast without days no longer risks a crash, and the strip resets when an update brings fewer days.
- **Folders that can't be read in full** follow the same rules as the others (hidden and system folders stay skipped), and quitting stops a scan at once.
- **Browse Files:** Select Several keeps the cursor on the right file, and after a delete the folders above show their new sizes.

### Faster, and easier on the battery
- **Nothing is redrawn while the screen is locked** (it used to redraw 60 times a second with the screen off).
- **Animations that nobody sees no longer keep the screen redrawing.** This covers:
  - scrolling titles during their pause;
  - a finished animation;
  - the toast;
  - the Weather icon behind a full screen;
  - the download screen.
- **The keyboard is drawn again only when its selection changes.** The Now Playing block's fabric is built once. The queue icons are drawn once.
- **Weather:**
  - the day tiles are images that just slide when you scroll, instead of redrawing the whole screen every frame;
  - the animated icons are drawn in the background, and the still icon shows meanwhile;
  - at most two animations are kept in memory, and freed when unused.
- **Browse Files reads folders in the background** (a "Loading…" row meanwhile). Folder sizes fill in as they're measured and stop when you leave.
- **All network requests share one connection pool,** so connections are reused and the certificate bundle is read once.
- **Internet Archive downloads no longer wake the screen** on every piece received.

### Changed
- **The Weather picture in Games shows only the icon** (a different one each time), without its name.
- **Song changes in Now Playing and in the block move more evenly.** The motion was eased twice.

## v0.36 (2026-10-28)

### Added
- **Browse Files** (Settings → Library): a little Finder for the music folder.
  - **The list:** folders first, then files, with their size (folder sizes come in a moment later), the number of items and the kind of file. Unfinished downloads (.part) and macOS leftovers ("._") are named as such. The top row shows the free space on the card.
  - **Moving around:** A opens a folder.
  - **Deleting:** Y (or holding A) on a row offers Delete. Select Several lets A tick more rows, and Delete Selected removes them together.
  - **Safety:** deleting always asks first. Nothing outside the music folder can be deleted, and the library is scanned again afterwards.
- **Pictures for the games** on the right of the Games list: a wall of glossy bricks with the ball and the paddle (Brick), three fanned cards (Solitaire), and for Weather one of its animated icons, a different one each time, with its name.

### Changed
- **Destructive options are red** in the sheets (Delete, Remove, Clear Up Next).
- **Every sheet is titled** with the row it is about.
- **The preview follows the selected row** as soon as a menu opens or comes back (before, it waited for a move).

## v0.35.1 (2026-10-28)

### Added
- **Up Next in the Music menu**, under Now Playing, while a song is playing.

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
