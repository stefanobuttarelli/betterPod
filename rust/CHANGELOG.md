# betterPod Rust: changelog

The separate Rust app for Knulli. A version here whose number is in `Cargo.toml` and has no release yet is published by `.github/workflows/rust.yml`, with its section as the notes.

## v0.2.3 (2026-11-08)

- The console's volume keys show in the app at once: the volume bar in the header, Now Playing's volume row, the screensaver. The app follows PipeWire's events (as betterPod does) instead of reading the volume every 2 seconds without showing it.
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
