# Recovery status

Byte-for-byte reconstruction of the lost v0.20 sources, verified by rebuilding
each file for linux/arm64 with Go 1.24.7 and comparing the machine code of
every function against the surviving v0.20 object (tools/recover/cmp.py).

## Recovered (verified identical to v0.20)

- ij_theme.go     — light/dark themes, Theme submenu
- ij_libinfo.go   — album grouping, scan stats, Library screen
- ij_gfx.go       — colours, gradients, blending, rounded rects, text
- ij_anim.go      — cubic-bezier easings, tweens, animated values
- ij_swar.go      — SWAR pixel helpers (lerp/scale/over)
- ij_parallel.go  — per-row work split across cores
- ij_stubs.go     — rescan glue
- ij_fonts.go     — Inter faces, caches, line metrics

Trivial instruction-scheduling ties (same behaviour, different register/slot
choice by the compiler) are treated as matches.

## Still upstream MiyooPod code (not yet reconciled)

Everything else under src/ is still the original MiyooPod source. The app does
not build yet (the ij_* UI references types/functions from files not recovered).
The remaining ij_* files and the Knulli-specific changes to the upstream files
(main.c, audio.c, library.go, knulli.go, …) are next.
