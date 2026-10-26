# Recovering the KnulliPod v0.20 sources

The original source tree was lost when the cloud workspace was reset
(2026-10-26). What survived:

- the v0.20 release zip (`port/`): assets, fonts, prebuilt aarch64
  `libSDL2_mixer-2.0.so.0` (ALAC/AAC/24-bit patches) and `libavcodec_kp` /
  `libavutil_kp`;
- the Go build cache, which still holds the **compiled package archive of the
  exact v0.20 build** (build id `UYgfRcMq7gHzxhoi4Yj4`, matching the release
  binary): `_go_.o` (Go object, with per-instruction file:line) and the cgo C
  objects (`_x005.o` = main.c + audio.c, clang `-O2 -g`, with DWARF).

## Reference material in this folder

- `exports.txt` — declarations from the archive's export data (types with
  fields and tags, method sets, constants with values).
- `funcmap.tsv` — function → file and line range, from the release binary.

Generated locally (not committed, large; regenerate as below):

- `/home/claude/recover/obj.dis` — `go tool objdump` of the v0.20 `_go_.o`.
- `/home/claude/recover/dbg.elf` — the archive relinked with the internal
  linker (`go tool link -linkmode=internal`) so it has DWARF.
- `/home/claude/recover/dwarf.txt` — per function: params, locals (with their
  declaration line), inlined calls (with call line). `tools/dwdump`.
- `/home/claude/recover/layouts.txt` — every main.* struct layout and the type
  of every global. `tools/dwdump ... layouts`.

## Workflow

1. `tools/recover/view.py FILE.go [funcregex]` — compact annotated
   disassembly grouped by source line (strings, globals with field names,
   float constants decoded).
2. Write the Go source for that file so that line numbers match.
3. `tools/recover/objbuild.sh /tmp/kpobj` — regenerates `src/zz_stubs.go`
   (declarations of everything not recovered yet, from DWARF + export data),
   compiles `src/*.go` for linux/arm64 with Go 1.24.7 exactly like v0.20 and
   disassembles the result.
4. `tools/recover/cmp.py FILE.go` — compares the new object with the v0.20
   object function by function (normalized instructions). A file is done when
   every function is identical.

`zz_stubs.go` must be deleted (it is regenerated) once everything is recovered.

## Status

See `STATUS.md`.

## Triage helpers (coarser)

- `tools/decomp/summarize.py` / `compare.py` — per-source-line token summary
  (calls, strings, constants, offsets) and a line-level comparison. Useful to
  see quickly which upstream MiyooPod files were changed in v0.20: on the
  pristine upstream sources it reports 15 files fully identical (about,
  crashhandler, fastdraw, fontsize, glyphatlas, logo, lyrics, playlist,
  posthog, queue, search, seek, theme, togglelogs, volume_brightness).
- `tools/recover/dwdump`, `gdata`, `astfuncs` — the DWARF / data dumpers used
  to produce `dwarf.txt`, `layouts.txt`.
