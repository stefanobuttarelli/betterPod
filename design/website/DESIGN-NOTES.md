# betterPod website design prototype

## Preview and scope

Open `index.html` directly in a browser. No build step or development server is
required. This is a local HTML/CSS design prototype with a small `script.js`
for synchronized Share-video controls and reduced-motion preferences. It has no
framework. It reviews the one-page layout, copy, screenshots and device
compositions; it is not deployed and does not change the console or Mac apps.

For an HTTP preview from the repository root, run
`python3 -m http.server 4173 --bind 127.0.0.1 --directory design/website`
and open `http://localhost:4173`.

## Art direction

Combine the generous spacing and product photography of an Apple product page
with the physical detail of the iPod era: silver surfaces, precise reflections,
glossy blue selection and colourful album art. Modern bento tiles organize the
feature story; their scale follows the importance of each feature.

The hero establishes the hardware and music-player identity first. Cover Flow
and Now Playing carry the visual story. YouTube album downloads, Internet
Archive, metadata and covers, and podcasts receive prominent feature space.
The website uses its own layout; native console frames preserve the app's
established interface.

The centered hero and system typography are deliberate choices from the brief,
even where a general design skill recommends asymmetry or a distinctive display
font. Authentic app gloss and reflections are retained because they are part of
betterPod's identity. They are not added indiscriminately to every website
surface. Native Mac styling remains separate from the console design.

## Feature claims and repository evidence

The user-directed product reference is the current Rust app, version **0.2.21**.
Use `rust/README.md`, `rust/CHANGELOG.md` and implementation evidence below for
feature claims. Go remains the interface reference under `DESIGN.md`, not the
current screenshot or download source. Rust is still labelled a preview; the
website does not convert local fixture evidence into hardware acceptance.

The Rust sources are currently in [PR #18](https://github.com/stefanobuttarelli/betterPod/pull/18),
separate from this website. Source paths below refer to
[capture revision 576ceba](https://github.com/stefanobuttarelli/betterPod/tree/576cebac0daf014f3fb4d0ad0efc83dd1516d081).
The installation link targets the Rust branch until that port reaches `main`.

| Website claim | Source and boundary |
| --- | --- |
| iPod-style player for Anbernic handhelds | `rust/README.md`: Knulli on Allwinner H700; RG35XX Plus is the documented target. RG35XX Pro acceptance remains pending. |
| Cover Flow, Now Playing and lossless audio | `rust/src/app/visual_player.rs`, `rust/src/library.rs`, `rust/src/audio.rs`: current renderer, codec-based lossless detection and native sample-rate switching. No universal bit-perfect or 60 fps guarantee. |
| YouTube songs and albums | `rust/README.md`, `rust/src/app/services/track_list_ui.rs`, `rust/src/downloads/youtube_split.rs`: single songs, a playlist as an album, chapters as tracks, or a chosen online track list to split a full-album video. Optional YouTube tools are required. Source audio is not described as lossless. |
| Internet Archive | `rust/src/app/services/visual.rs`, `rust/src/downloads/archive.rs`: listen before downloading, individual tracks or whole albums, with lossless formats when the source provides them. |
| Metadata and missing artwork | `rust/src/metadata.rs`, `rust/src/app/services.rs`, `rust/src/app/curation_ui.rs`: choose an online release, review matched fields, then save changes. Missing Covers has a user-started **Search All Automatically** action. Downloads receive available source tags; imported libraries are not silently retagged or physically reorganized. |
| Podcasts | `rust/src/podcasts.rs`, `rust/src/app/services/visual.rs`: search shows, save individual episodes or up to three latest episodes, and keep downloaded shows in Your Podcasts. No scheduled automatic-download claim. |
| Share between consoles, or from a Mac | `rust/README.md`, `rust/src/app/services/share_visual.rs`, `mac/NOTES.md`: eight-digit codes for songs and albums. The separate [Share for Mac](https://betterpod.xyz/mac/) link is retained. |
| Rust download | [Rolling Rust release](https://github.com/stefanobuttarelli/betterPod/releases/tag/rust-latest), documented in `rust/README.md`. Do not use the generic latest-release link, which can point to Go. |

## Screenshot provenance

App frames are newly rendered from current Rust 0.2.21 source in an isolated
fixture, with production MSAA4 and unchanged app/renderer implementations.
Still exports are 1280 × 960 (2×); Share animation frames are 640 × 480 (1×).
Both preserve the native 4:3 composition. The main Now Playing reference is
Radiohead's “15 Step.” These are native-renderer fixture captures, not photos
of a live console or copies of the older `docs/images/` screenshots.

The fixture contains six real albums and Apple catalogue artwork, with short
generated silent audio placeholders. It does not contain downloaded commercial
recordings or use the user's music/data. Capture evidence, source hashes and
fixture details are in ignored build output:
`build/website-design/current-captures/capture-provenance.json`.

The console-to-console Share demonstration uses two MP4 frame sequences from
the same current Rust renderer. The local script synchronizes Play, Pause and
Replay and respects reduced motion. Transfer states are demonstrative; no
network music transfer is performed. App geometry, artwork motion and the
five-bar lossless mark are native output, not CSS recreations.

`DESIGN.md` is the interface contract. The original interface derives from
[iPod.js by Tanner Villarete](https://github.com/tvillarete/ipod-classic-js).
Existing third-party attribution remains in the repository.

## Album artwork credits

Artwork was obtained from Apple's catalogue image URLs at 600 × 600. These
are real releases; ownership remains with the respective rights holders.
Catalogue records are retained in ignored `build/website-design/library-art/`.

| Artist | Apple catalogue source |
| --- | --- |
| The Beatles | [Abbey Road (Remastered)](https://music.apple.com/us/album/abbey-road-remastered/1441164426) |
| Daft Punk | [Discovery](https://music.apple.com/us/album/discovery/697194953) |
| Radiohead | [In Rainbows](https://music.apple.com/us/album/in-rainbows/1109714933) |
| Miles Davis | [Kind of Blue](https://music.apple.com/us/album/kind-of-blue/268443092) |
| Daft Punk | [Random Access Memories](https://music.apple.com/us/album/random-access-memories/617154241) |
| Fleetwood Mac | [Rumours](https://music.apple.com/us/album/rumours/594061854) |

## Device image credits

Device photographs are official Anbernic product assets. Original source images
are preserved; cropping and the app-screen placement are presentation treatments
in the prototype. The resulting compositions illustrate betterPod rather than
documenting a photographed installation on each model.

| Device | Official source | Image |
| --- | --- | --- |
| Anbernic RG35XX Plus | [Product page](https://win.anbernic.com/cn/chanpin/329.html) | [Original asset](https://win.anbernic.com/uploads/ueditor/20231213/2-23121319145b50.jpg) |
| Anbernic RG35XX Pro | [Product page](https://win.anbernic.com/cn/chanpin/604.html) | [Original 1500 px asset](https://win.anbernic.com/uploads/ueditor/20250612/2-250612111532H7.jpg) |

Credit: Anbernic. These assets are used for this local design review; no separate
reuse licence or manufacturer endorsement has been established.

The Pro photograph has an inactive display. Its screen inset is a 4:3
approximation based on the panel and body dimensions, to be checked against
an illuminated device before final artwork production.

## Skills used and attribution

Eight external design skills informed the prototype. Their general defaults
were interpreted against the product brief and `DESIGN.md`, including the
intentional overrides described above.

| Skill | Application to this design |
| --- | --- |
| [Unslop](https://raw.githubusercontent.com/heydamir/unslop/main/skills/unslop/SKILL.md) | Product-specific imagery, restrained decoration and a clear visual hierarchy. By Damir Sokolovsky, [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). |
| [Taste](https://raw.githubusercontent.com/Leonxlnx/taste-skill/main/skills/taste-skill/SKILL.md) | Deliberate composition, proportion and visual density. |
| [Better Typography](https://raw.githubusercontent.com/jakubkrehel/skills/main/skills/better-typography/SKILL.md) | Type hierarchy, readable measure and balanced line breaks. |
| [Better Colors](https://raw.githubusercontent.com/jakubkrehel/skills/main/skills/better-colors/SKILL.md) | Neutral surfaces, consistent contrast and a purposeful blue accent. |
| [Better Accessibility](https://raw.githubusercontent.com/jakubkrehel/skills/main/skills/better-accessibility/SKILL.md) | Semantic content, visible focus and readable controls. |
| [Clarify](https://www.ui-skills.com/skills/pbakaus/clarify) | Concrete feature descriptions and clear calls to action. |
| [Adapt](https://www.ui-skills.com/skills/pbakaus/adapt) | Recompose the hierarchy for narrow screens. |
| [Polish](https://www.ui-skills.com/skills/pbakaus/polish) | Consistent alignment, spacing, edges and interaction details. |

Two local motion skills, `css-animations` and `animation-accessibility`, informed
restrained CSS motion and reduced-motion handling. Both draw on
[Animations on the Web](https://animations.dev/). Motion supports interaction;
the content and feature story remain understandable without it.

## Prototype verification

The current revision was reviewed in local Chrome at 1440, 1024, 820, 768,
390 and 320 px widths with no horizontal overflow or page-script errors.
The page opens directly from its local file and all images load. Share Play,
Pause, completion and Replay passed: both videos paused at the same 4.9583 s
timestamp in the synchronization check. Reduced motion keeps both videos
paused and hidden, and switches between native start/completed screenshots.
Native button keyboard activation and the screen-gallery radio controls work.
No full screen-reader or WCAG conformance audit is claimed.

## Before production

- Verify the current Rust build on the intended Knulli release and physical
  hardware, including RG35XX Pro. Native fixture rendering and previous Go
  performance captures do not establish Rust hardware acceptance.
- Confirm the selected release links, installation instructions and current
  supported hardware. Avoid publishing performance or battery-life guarantees
  from a visual prototype.
- Review device-image reuse permission, final export quality, responsive image
  sizing and the accuracy of screen placement before public distribution.
- Check keyboard navigation, focus visibility, contrast, reduced motion, zoom
  and mobile layouts in the final implementation.
- Keep the independent-project attribution: iPod is an Apple trademark;
  betterPod is not affiliated with or endorsed by Apple, Anbernic or Knulli.
