# Local Wi-Fi Library

Wi-Fi Library lets the Mac companion edit this experimental Rust player's music library. It is independent from Share and does not change the classic Go application. All development and checks remain local; deployment to a console is a separate step.

## Use

Open Music or Settings → Wi-Fi Library → Start Connection on the player. In the Mac app's betterPod workspace, find the player and enter its eight-digit code. Both devices must be on the same LAN. The player also displays an address and port for networks that block discovery.

The Mac lists the player's approved tracks in pages and fetches a selected cover on demand. It shares the local editor's natural sorting, draft, per-track/bulk fields and multi-service metadata lookup. Magic identifies selected tracks and prepares metadata, covers and cross-folder album joins. Its manual tools let users correct a failed match within the same review flow. Save Changes writes the draft on the player. It does not download or upload audio, rename files, delete tracks or modify playlists.

Records contain physical file tags. Optional display album fields explain a private logical grouping on the player. Plain album-field edits clear the relevant grouping overrides; explicit Merge and Magic can join tracks from different folders into one logical album. Joining does not infer new disc positions or replace artwork. Changing artwork clears a chosen-cover override. Other private choices are preserved. Successful writes trigger the ordinary library refresh and artwork invalidation.

The active or paused decoder's path is protected. Saves also reject symlinks, paths outside the approved music/download roots, tracks absent from the snapshot, and files whose size or modification time changed. Each file is staged and replaced after a successful write. A batch reports individual failures. Stopping a session cancels remaining writes before publication; a completed file remains saved. A lost reply can mean a write already succeeded, so the Mac requires reload rather than retrying it automatically.

The service starts only from the explicit menu action. It stops on request, shutdown, root moves and file-transfer maintenance. Pairing lasts ten minutes, with five unsuccessful attempts per code. One authenticated Mac is served at a time. An established connection has a thirty-minute idle timeout and an eight-hour maximum lifetime. No code or session key is persisted.

## Wire protocol, version 1

Discovery uses an IPv4 UDP broadcast or direct local query to port 45841. The request is the literal ASCII text `betterpod-library-discover-v1`. A reply contains `{v:1,id,name,port}` and no pairing secret. The client uses the reply's source address, accepts local/private/link-local addresses, and pins the confirmed encrypted session ID to the selected discovery result. Manual local addresses are supported without discovery.

TCP uses a four-byte big-endian frame length. Handshake frames are capped separately; authenticated frames are capped at 16 MiB. Both peers exchange JSON `{v:1,spake:<base64>}` using symmetric SPAKE2 with the Ed25519 group, the eight-digit password and identity `betterpod.xyz/library/v1`. The pinned Rust and Go implementations use the existing Share-compatible SPAKE2 construction.

HKDF-SHA256 derives separate 32-byte keys from the shared secret with empty salt and info `betterpod-library-v1/client` or `betterpod-library-v1/server`. Subsequent frames contain ChaCha20-Poly1305 ciphertext, including its authentication tag. Each direction has its own counter starting at zero. The 12-byte nonce is four zero bytes followed by the big-endian 64-bit counter. No additional authenticated data is used. A failed tag, repeated frame, exhausted counter or transport failure ends the client session.

The first encrypted request is `{op:"confirm",name:<Mac name>}`. The server confirms with `{ok:true,name,session:<discovery id>}` before serving any library operation.

| Operation | Request | Reply |
| --- | --- | --- |
| list | `offset`, `limit` up to 256 | `tracks`, `total`, string `revision`, `capabilities` |
| read | `paths`, up to 256 approved paths | `tracks`, `skipped`, `capabilities` |
| cover | approved track `path` | optional base64 `cover` |
| save | `tracks`, up to 50 sparse changes | `saved`, `failures`, optional `warnings`, `joined` |
| disconnect | no fields | `ok` |

Records use the Mac local editor's field names: `path`, `fileName`, `format`, `writable`, `title`, `artist`, `album`, `albumArtist`, `genre`, `year`, `track`, `trackTotal`, `disc`, `size`, `modified`, optional `warning`, `displayAlbum`, `displayAlbumArtist`, `displayAlbumID` and `albumGroup`. `modified` is signed Unix nanoseconds. List replies omit artwork and keep an unreadable placeholder in its page position. A changing revision during pagination requires loading again.

A save change contains `path`, sparse `fields`, expected `size` and `modified`, and optionally base64 `cover` or `removeCover:true`. Empty strings or zero numeric values explicitly clear fields. Missing fields preserve existing tags. Covers are JPEG/PNG, at most 8 MiB and 40 megapixels. The Mac splits artwork-heavy batches below the frame cap. General operation errors are `{error:<message>}`; file failures also identify their path.

Players advertising `album-join-v1` accept `joinAlbum:true` on a save change with explicit `fields.album` and `fields.albumArtist`. After the checked tag write, the player commits a private group derived from the normalized album and album artist. This stable identity joins matching tracks across separate folders and requests without changing their audio, order or covers. `joined` contains only paths whose group commit succeeded. A tag write can succeed while the group commit fails; it then has a warning, is absent from `joined`, and the Mac keeps the join pending. Older players omit the capability; the Mac does not send them the extension.

## Local checks

Run `sh mac/tests/check.sh` for the local editor, Go transport tests, lookup fixtures and Swift editor model. Network tests require permission to open localhost sockets; a restricted run reports skips for unavailable socket/codec capabilities.

Run `sh mac/tests/check-wifi.sh` for the real Rust-server/Go-client integration. It builds a fixture server with the existing local Rust/SDL dependencies and uses marked temporary music/data folders. Pairing data is passed through a subprocess pipe. The test exercises real encrypted framing and metadata operations against disposable audio files, with no console connection or release.

Focused Rust tests live alongside `wifi_library`, and console menu/lifecycle checks live in `app_wifi_library`. These checks do not establish physical Wi-Fi discovery reliability, Knulli suspend behavior or audible continuity under SD-card I/O. Those remain hardware acceptance checks.
