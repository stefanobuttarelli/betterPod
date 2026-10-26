# Source recovery (v0.20)

The KnulliPod sources were lost with the cloud workspace on 2026-10-26. What survived:

* the v0.20 port zip (assets, fonts, patched libs, stripped arm64 binary);
* the Go build cache entry of the v0.20 `main` package
  (build id `UYgfRcMq7gHzxhoi4Yj4`, same as the binary): export data,
  arm64 object code with file:line for every instruction, cgo C objects
  with DWARF.

Reconstruction aids in this folder / `tools/`:

* `exports.txt` – every type, struct field, constant and method signature (from export data, `tools/expdump`).
* `funcmap.tsv` – function → file and line range (from the binary's pclntab, `tools/symmap`).
* `tools/dwarfvars` – locals/params with declaration lines and inlined calls (from a relinked debug ELF), plus struct field offsets (`structs`).
* `tools/decomp/summarize.py` – per-source-line summary of the object code (calls, strings, constants, field offsets).
* `tools/decomp/compare.py` – compares a rebuilt archive with v0.20 function by function.
* `tools/decomp/build-arm64-archive.sh` – compiles `src/` for arm64 and prints the archive path.

Workflow: rewrite a file, build the archive, `go tool objdump -s '^main\.' ARCHIVE > new.objdump`,
`compare.py v020.objdump new.objdump file.go` until every function reports OK.
