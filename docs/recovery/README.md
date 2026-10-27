# Recovering the KnulliPod v0.20 sources

The original v0.20 source tree was lost on 2026-10-26, when the workspace it lived in was reset. This folder documents how the sources in this repository were rebuilt and checked. The result is in [STATUS.md](STATUS.md).

## What survived

- **The v0.20 release zip** (now `port/`). It holds the assets and fonts, the prebuilt aarch64 `libSDL2_mixer-2.0.so.0` (with the ALAC/AAC/24-bit patches) and `libavcodec_kp` / `libavutil_kp`.
- **The release binary.** It is stripped, but still carries Go's symbol table and line table.
- **The Go build cache.** It still held the compiled package archive of the exact v0.20 build (build id `UYgfRcMq7gHzxhoi4Yj4`, matching the release binary):
  - `_go_.o`: the Go object, with file:line for each instruction;
  - `_x005.o`: the cgo C object (`main.c` + `audio.c`, clang `-O2 -g`, with DWARF).

## Method

The starting point was the upstream [MiyooPod](https://github.com/danfragoso/miyoopod) sources. Each file was then rewritten until the compiler produced the same machine code as v0.20.

1. **Reference material.** The v0.20 archive was relinked with Go's internal linker to get DWARF back. Three dumps came out of it:
   - every function's parameters, locals and inlined calls, each with its line;
   - every struct layout and the type of every global;
   - the declarations in the export data: types, fields, tags, method sets and constant values.

   `exports.txt` and `funcmap.tsv` (function → file and line range) are kept here.
2. **Annotated disassembly.** `tools/recover/view.py FILE.go` prints the v0.20 code of a file grouped by source line, with strings, globals and float constants decoded.
3. **Rebuild exactly like v0.20.** `tools/recover/objbuild.sh OUT` compiles `src/*.go` for linux/arm64 with Go 1.24.7 and the same cgo flags, then disassembles the result.
   - While files were missing, it generated `src/zz_stubs.go` with the declarations not recovered yet, built from DWARF and export data.
   - Once nothing was missing, the stub file stopped being generated.
4. **Compare.**
   - `tools/recover/ecmp.py FILE.go` (or `all`) compares each function against v0.20. Registers and stack slots are normalized, and the instructions are compared line by line.
   - `fdiff.py` and `firstlines.py` show where a function differs or where its lines have shifted.
   - The C object is compared the same way.

   A file was done when its functions came out identical, or equivalent (same instructions, different register choice).

Line numbers are part of the comparison: every statement had to land on the same line as in v0.20. That is why some comments and blank lines in the recovered files are placed the way they are.

## Tools

| Tool | Purpose |
| --- | --- |
| `tools/recover/view.py` | Annotated v0.20 disassembly per source line |
| `tools/recover/objbuild.sh` | Rebuild the package object like v0.20 |
| `tools/recover/ecmp.py`, `fdiff.py`, `firstlines.py` | Function-level comparison, diffs, line offsets |
| `tools/recover/dwdump`, `gdata`, `astfuncs` | DWARF and data dumpers (locals, layouts, globals) |
| `tools/decomp/` | Coarser per-line triage. On the pristine upstream sources it showed that 15 upstream files were unchanged in v0.20 |
| `tools/dwarfvars`, `tools/funclines`, `tools/symmap`, `tools/expdump` | Helpers for DWARF variables, line tables, symbols and export data |

The large intermediate files (the full disassembly, the relinked ELF with DWARF, the dumps) are not committed. They can be regenerated from the v0.20 build archive with the tools above.
