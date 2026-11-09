# Design contract

This file is mandatory for UI work. The console product is the established
iPod-style betterPod interface implemented in Go. The Rust rewrite must port
that interface and its behavior. A rewrite, a new feature or a different
rendering engine does not authorize a redesign.

Explicit user instructions take precedence. Otherwise, use the Go component
and flow before inventing a replacement. When a detail is unspecified here,
read the reference implementation and compare the actual screens.

## Scope

- **Console:** Go and Rust share the existing iPod visual language, controls,
  navigation, labels, artwork treatment and motion.
- **Mac companion:** use native macOS SwiftUI navigation, tables, menus and
  controls, with Liquid Glass where supported, as requested for that product.
  The Share scene retains its established artwork. Do not transplant Mac
  controls, fonts, glass surfaces or window layouts into the console.
- Shared library and metadata rules do not imply identical platform layouts.

## Source of truth

| Component | Go reference |
| --- | --- |
| Coordinates and drawing | `src/ij_gfx.go`: `DPR`, `px`; `src/ij_ui.go`: `drawStack`, `drawHeader`, `renderFrame` |
| Views, rows, selection and scrolling | `src/ij_views.go`: `View`, `Item`, `rowHeight`, `ensureVisible`, `renderList` |
| Colors and themes | `src/ij_theme.go`: `UITheme`, `lightTheme`, `darkTheme`; `src/ij_views.go`: `selGrad`, `selGradInactive` |
| Fonts and metrics | `src/ij_fonts.go`: `FontWeight`, `fontFiles`, `face`, `sizedFace`, `lineBox` |
| Search and its options button | `src/ij_views.go`: `searchField`, `drawSearchBar`, `drawGearButton`; `src/ij_ui.go`: `gearKey` |
| YouTube search and details | `src/ij_ytdl.go`: `youtubeSearchView`, `ytSearchSettingsView`, `ytItemView`, `startYTDownload` |
| Contextual actions | `src/ij_sheet.go`: `showSheetTitled`, `Sheet.render`, `sheetCard` |
| Keyboard | `src/ij_keyboard.go`: layouts, dimensions and `keyboardKey` |
| Artwork | `src/ij_ytcover.go`: `squareCover`, `framedCover`, `blurFillSquare`; the existing artwork assets and cover renderers |
| Download progress | `src/ij_archive.go`: `startDownloadJob`, `iaDownloadView`, `IADownloadView.Render`; shared `drawProgressBar` and `drawJobButton` |
| Activity indication | `src/ij_loading.go`: `drawSpinnerIn`, `spinnerStep` |
| Motion | `src/ij_anim.go`, `src/ij_effects.go`; component-specific Go transitions |

Use source functions and tokens, not approximate replacements that merely
resemble an iPod. Existing reference captures and `rust/PARITY.md` supplement
the code; they are finite evidence, not permission to assume untested parity.

## Layout, typography and color

- Go lays out a **320 × 240 CSS-pixel canvas** and renders at **640 × 480**
  with `DPR = 2`. Rust device-pixel coordinates must preserve that scale.
  The standard header is 20 CSS pixels high. Distinguish row bounds from the
  bounds of controls inside them.
- Keep split menus, contextual previews, full-page views, header, mini-player
  reservations and viewport clipping in their existing places. Do not replace
  console screens with dashboards, grids of cards or desktop dialogs.
- Use the existing Inter weight mappings and Noto Japanese/CJK fallback.
  Preserve source baselines, line boxes, truncation and marquee behavior.
  Standard list labels use W700 at 14 CSS pixels, subtitles W400 at 12,
  values W400 at 14 and durations W600 at 14; individual components retain
  their own source sizes. Do not substitute system/SF fonts on the console.
- Consume theme roles for background, text, secondary text, header, artwork
  borders and sheets. Preserve the glossy header and blue selected-row
  gradient (`#3cb8ff` to `#347ab5`) with white selected text. Inactive columns
  use the established inactive treatment. No separate palette per new screen.
- Keep established icon assets, silhouettes, dimensions, gradients and shadows.
  Higher raster resolution may improve edge quality while preserving geometry.
- Header rows are noninteractive. Chevrons, checked values, thumbnails,
  speaker marks, separators and scrollbars keep their established meaning.
  Keep the entire selected item visible when rows or keyboards change.

## Navigation and search

- Preserve the existing view stacks and Back destination. Do not insert
  intermediary menus, dialogs or history lists into an existing action.
- A selects. B goes back in ordinary navigation. Long presses, Y/options,
  wheel/D-pad navigation, keyboard actions and playback controls follow Go.
  Knulli retains ownership of system volume and POWER.
- Search options belong to the **round gear beside the search field**.
  Right focuses that button, Left returns to the query and A activates the
  focused control. Up/Down resumes list navigation. Use the same control for
  features such as Weather that already have it in Go.
- YouTube Search Settings is a **full page** of checked Videos and
  Playlists & Albums choices. Retain the query and rerun the underlying search
  when the kind changes. Selecting a kind stays in Settings; Back returns to
  the search. `Showing: …` is only the thin, nonselectable source header.
  It must never replace the gear with a selectable settings row.
- Keep the dedicated Videos search video-only, as in Go. Preserve result
  selection, scroll, query, recent searches and cancellation ownership.
- Contextual actions use the existing bottom sheet over a dimmed screen,
  with its separate Cancel card. Settings pages are not action sheets.
- **Requested console updates:** Left/Right in Now Playing cycles the time,
  scrubber and shuffle/repeat controls. Sharing-name entry uses Enter. Menu
  scrollbars appear during navigation and fade shortly afterward. Hold notices
  are centered. Music → Search includes Lossless albums with the wave icon.
- Cover Flip exits to Now Playing while music is active, otherwise to the
  previous screen. Hold permits the automatic screensaver. After ten seconds
  without input, Cover Flow follows the playing album.
- Wi-Fi Library uses a dedicated console page with a large pairing code,
  connection status, Mac instructions and one Start/Stop button. Retain the
  console header, typography, theme and glossy blue button.

## Artwork and YouTube detail actions

- Preserve the source order and presentation: metadata/thumbnail header,
  inline Listen, Watch Video or Watch the Videos, Cover, download actions,
  then the track/chapter list where applicable.
- Playlist tracks remain individual songs. Listen and Watch operate on their
  child queue. A parent playlist URL must not be treated as one video file.
- Keep the established labels: Download as Album and Download as One Song
  where those actions apply. Source-video and audio-only options retain their
  existing contexts rather than being added indiscriminately to every page.
- **Requested override:** remove the custom Start / Center / End cover dialog.
  Download must not force a crop-choice step. Keep an automatic square-cover
  preview in the existing artwork treatment; do not introduce another crop UI
  or mandatory confirmation in its place.
- Cover is actionable before a YouTube download and for existing library
  albums. An optional source search selects replacement art; it must preserve
  aspect ratio and must not add a mandatory step to downloading.

## Async foreground actions

This is a product-wide interaction rule, including actions added during the
Rust port:

1. When an action needs to await a result, mark the **pressed item** busy
   immediately, before dispatching work. Use a small themed spinner derived
   from Go's 12-spoke activity indicator. Preserve row geometry and context.
2. While that foreground operation is pending, block list movement, repeated
   activation and switching to another action. Keep audio and system controls
   responsive. Do not leave a silent, apparently unresponsive selection.
3. **B cancels the pending operation**, removes the loader and returns control
   to the same list, selection and scroll position. It must stop the owned
   worker/process where supported, not merely hide its feedback.
4. Success opens or updates the intended destination. Failure removes the
   loader and provides readable feedback and a usable retry path.
5. Each operation has an owner, request identity and generation. Late results
   from cancelled, superseded or closed screens must not navigate, overwrite
   another page or clear another operation's loader.

Passive thumbnail requests and explicitly background jobs do not lock browsing.
Committed writes are not undone by cancelling remaining work; partial results
must stay visible and be reported honestly. Avoid empty placeholder pages,
unrelated global loading messages and fake progress percentages.

## Downloads and background work

- Pressing Download opens the **current download's dedicated Go-style job
  view**, using the existing artwork/title/creator, status, glossy progress
  bar, byte/speed/ETA information and job button. It must not land in a list
  mixing the new job with previous downloads.
- Use one primary job button: Cancel while running, then Done or Try Again
  according to the outcome. Keep normal Go button and hint styling.
  **Requested override:** the one actionable button is always selected blue,
  including Cancel, so its A-button action is visible while work is running.
- Downloads from all sources share one bounded FIFO queue. A new request opens
  its own job view and shows its queue position while waiting. When the current
  job finishes, fails or is cancelled, the next starts automatically. Downloads
  lists active and waiting jobs in execution order. Cancelling a waiting job
  removes only that job; an identical active/waiting request reopens its view.
- **B leaves the job view and the download continues in the background.**
  The view must say so in its normal hint area. Only Cancel stops that job.
  This is distinct from B cancelling a foreground loader.
- Downloads stays reachable from Music and Videos while active or recent
  jobs exist. Its standard list clearly distinguishes active, completed,
  cancelled and failed jobs. Selecting one returns to that job's progress and
  outcome; it does not start another transfer.
- Y on the Downloads list opens the existing action sheet with Remove from
  List and Clear Download List. Removing active/queued work is labelled Cancel
  and Remove; clearing the entire list confirms cancellation of remaining work.
  Saved media is preserved. An empty visible list removes those Downloads rows,
  including when only failed/completed records were cleared. Late callbacks
  must not restore removed history or release a stopping worker's idle guard.
- Try Again must retain completed valid tracks and request the missing work.
  Do not present duplicate downloads as a faithful retry. Do not overwrite
  existing or externally changed files to achieve that behavior.
- Cancellation preserves finished files and refreshes the library. Completion,
  failure and partial completion must not be confused with one another.

## Motion, performance and resources

- Reuse source timing and easing: `durSlide`, `durPanel`, `durPreview`,
  `delayMenuHide`, `easeCSS` and `easeInOut`. **Requested override:** Rust always
  uses Full effects; remove the Effects menu and Full/Light preference, including
  the influence of a previously saved Light value.
  New backend code must not change how input or transitions feel.
- Network, metadata resolution, image decoding, filesystem operations and
  serialization run off the UI/audio threads. Feedback begins before that
  work and cancellation releases its owned resources.
- Reuse resolved metadata instead of repeatedly launching yt-dlp/Deno for
  opening, previewing and downloading the same video. Keep large provider JSON
  out of duplicated UI rows. Bound caches and process groups; report the
  actual failure reason in diagnostics rather than disguising every failure
  as insufficient memory.
- Preserve audio quality, codec choice and buffer behavior. Do not remove
  memory protections, lower audio quality or change system settings to conceal
  a regression. Compare resource use under the same workload.

## Required verification

- Read the matching Go component and flow before implementing a console UI.
  Reuse it or port its actual geometry and behavior.
- Compare deterministic **Go and Rust captures at 640 × 480** for affected
  screens, including selected, loading, error and Back states as applicable.
  Inspect the pictures; row-label tests alone do not prove design fidelity.
- Test the actual input route, cancellation, stale callbacks, partial work
  and reopening a background job. Keep fixture tests separate from claims
  about live providers, audible playback or hardware performance.
- State any remaining difference explicitly. Do not silently normalize a
  reference mismatch into a new design. Keep generated captures and logs in
  ignored build output, not in application data or release manifests.
