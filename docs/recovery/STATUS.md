# Recovery status

The v0.20 sources were rebuilt from the surviving v0.20 binary. Each file was
rebuilt for linux/arm64 with Go 1.24.7, and the machine code of every function
was compared against the v0.20 object (`tools/recover/ecmp.py all`).

## Result

- 978 Go functions are identical and 53 are equivalent: the same
  instructions, with a different register or stack slot chosen by the
  compiler.
- 11 Go functions differ only in compiler scheduling or stack-slot choices
  inside identical source logic: `scroll`, `drawHeader`, `Run`,
  `backsideTexture`, `sprite`, `drawHoldBadge`, `OnCenter`, `selectCurrent`,
  `drawKeyboard`, `drawIconPreview` and `AboutView.Render`.
- The two `_cgoexp_*` exports differ only by the package hash in their names.
- C (`main.c`, `audio.c`): every function is identical except three:
  - `pollEvents`: the media-key detection loop is vectorised differently. It
    tests the same set of key codes.
  - `audio_init` and `audio_set_rate`: one instruction is scheduled
    differently and one relocation offset differs.
- The package-level initializers (`main.init`) are identical.
- The rebuilt binary is 8,094,088 bytes; the original is 8,094,120. Its
  `.text` section is 72 bytes smaller. Both have the same NEEDED libraries
  and the same RUNPATH (`$ORIGIN/libs`).

The only `gofmt` deviation left is the one-line `NowPlayingView.Animating` in
`ij_nowplaying.go`. Reformatting it would move every following line of the
file, so it was kept as it was in v0.20.

## Build

See the top-level README.md (`tools/build.sh`).
