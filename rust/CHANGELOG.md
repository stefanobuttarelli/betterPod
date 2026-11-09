# betterPod Rust: changelog

The separate Rust app for Knulli. A version here whose number is in `Cargo.toml` and has no release yet is published by `.github/workflows/rust.yml`, with its section as the notes.

## Unreleased

- Covers saved from the Mac now take precedence over older folder artwork and imported Go cover records. The choice survives rescans, cache reuse and reconnection; removing an embedded cover does not restore an old YouTube thumbnail. Existing folder artwork and cached images are preserved.

## v0.2.10 (2026-11-09)

- Prevent a direct SSH launch from suspending EmulationStation while it still owns the display. This unsafe handoff could freeze the Mali renderer and bypass physical controller setup. Console tests now return through the normal Ports launch; ordinary launch and exit preserve the firmware's display and input handoff.
- YouTube downloads show live progress and refresh Albums and Artists automatically. Playlist retries reuse verified completed tracks. Downloads is inside Music and Videos.
- YouTube video playback reuses resolved formats and reads the metadata cache with buffered I/O. Cold extraction skips unused subtitle URL expansion while preserving media formats. Measured on the test console: warm playback startup about 1.1–1.4 seconds; cold details about 19–21 seconds, depending on the video and network.
- Fix YouTube cover selection before download and replacement on existing albums. Preserve thumbnail proportions, show hours in long durations, and show loading feedback when opening results.
- Correct Now Playing time/repeat controls, pause/resume cover transitions and the pause indicator during automatic track changes. Keep Cover Flow artwork cached and follow the playing album after ten idle seconds.
- Cover Flip updates the playing album, cover and Back destination, returns to Now Playing during active playback, and starts automatically under Hold. Without active music it returns to the previous screen.
- Show Enter for sharing-name entry, center Hold notices, hide idle scrollbars, and add the compact Lossless search row with its wave icon. Clip search-field and gear gloss to their rounded outlines.
- Remove Effects Full/Light and always use Full. Improve Wi-Fi Library pairing and connection screens, battery selection and the animated shutdown loader.

## v0.2.9 (2026-11-08)

- A test release for Software Update from 0.2.8. No other changes.

## v0.2.8 (2026-11-08)

- Software Update no longer stops on "Directory not empty (os error 39)". When the card keeps a staging folder (`betterpod-rust/.rust-update`) that cannot be emptied, the update moves it aside, logs what was left inside, and goes on. A later update removes the old folder when the card allows it. A cleanup problem after the new files are in place is no longer reported as a failed update.

## v0.2.7 (2026-11-08)

- YouTube videos open again where they stopped with "Not enough free memory to open YouTube". The app refused or stopped yt-dlp and Deno on fixed memory limits, and a 64 MB cap on Deno was too small for YouTube's check. Those limits are gone, as in betterPod. If RAM really runs out, the kernel now stops yt-dlp and Deno first, so the app and its music keep going. The log still records how much memory they used.

## v0.2.6 (2026-11-08)

- Software Update installs again after an earlier update: it stopped with "Directory not empty (os error 39)". The folder an update is prepared in (`betterpod-rust/.rust-update`) could not be removed on the console's card, stayed behind, and the next update stopped on it. It is now removed reliably, also when an earlier version left it.
- The battery icon shows the console's real level everywhere. The app now reads the battery the way betterPod does (`axp2202-battery` on the H700) before any other one. The Software Update and Moving Music screens showed a full battery whatever the level.

## v0.2.5 (2026-11-08)

- Check for Updates no longer says "Update worker is busy" while the check made at start-up is still trying GitHub again: it waits for that check's answer.
- When GitHub keeps failing, the message says so ("GitHub is not answering (504). Try again in a minute").

## v0.2.4 (2026-11-08)

- Software Update tries again when GitHub answers with a temporary error (500 to 504, as "http status: 504") or the connection drops: four tries, a few seconds apart, before it reports a failure.

## v0.2.3 (2026-11-08)

- The console's volume keys show in the app at once: the volume bar in the header, Now Playing's volume row, the screensaver. The app follows PipeWire's events (as betterPod does) instead of reading the volume every 2 seconds without showing it.
- The library scan counts live on Settings → Library: the songs found, then "N of M songs" as they are read. It showed 0 (or the previous count) until the end.
- The battery icon shows the console's battery. With a Bluetooth headset connected, the headset's battery could be shown instead (97 % on the console, a quarter empty in the app).

## v0.2.2 (2026-11-07)

- Weather: going back from a forecast left a blank white screen (the search page slid away instead of coming back). Now the search comes back as it should.
- Software Update and About show the Rust version (they said 1.1, the Go app's).
- Beta Versions works: it also reads the Rust beta channel and offers the newer version.
- The update notes are plain text.

## v0.2.1 (2026-11-07)

- Quit from the main menu: Y on Settings opens a sheet with **Quit betterPod**, so there is no need to go to the end of Settings (as in betterPod 1.2).

## v0.2.0 (2026-11-07)

The first published build of the separate Rust app for Knulli (Anbernic H700). It is the betterPod 1.1 feature set rewritten in Rust, by Emanuele Macri. betterPod (the Go app) stays as it is, with its own updates.

- Now Playing: the track card keeps its rows while it flips. It turns back in place when you leave it, and Cover Flow opens at the front afterwards.
- The Shuffle / Repeat buttons have their outline; the header shows pause only when paused.
- Software Update has a channel of its own: later versions come through it.
