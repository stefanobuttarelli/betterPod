# Changelog

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
