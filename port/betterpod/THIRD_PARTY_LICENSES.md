# Third-party licenses

betterPod is licensed under the [MIT License](https://github.com/stefanobuttarelli/betterPod/blob/main/LICENSE). It includes and depends on the components below. Each is distributed under its own license, and the copyright notices stay with their authors.

## Projects betterPod is based on

| Project | Author | License | Used for |
| --- | --- | --- | --- |
| [iPod.js](https://github.com/tvillarete/ipod-classic-js) | Tanner Villarete | MIT (`LICENSE.ipod.js`) | Interface design, layouts, icons and images in `assets/ipod` |
| [MiyooPod](https://github.com/danfragoso/miyoopod) | Danilo Fragoso | MIT (`LICENSE.MiyooPod`) | Player engine, library scan, original app |
| [Simon Tatham's Portable Puzzle Collection](https://www.chiark.greenend.org.uk/~sgtatham/puzzles/) | Simon Tatham and contributors | MIT (`LICENSE.puzzles`) | Apps → Puzzles: twelve of its puzzles, compiled in |
| [2048](https://github.com/gabrielecirulli/2048) | Gabriele Cirulli | MIT | Apps → 2048: the game's rules (the code is betterPod's own) |

## Fonts (`assets/`)

| Font | License |
| --- | --- |
| Inter (`assets/fonts`) | SIL Open Font License 1.1 (`assets/fonts/COPYRIGHT-Inter.txt`) |
| Noto Sans JP (`assets/fonts/NotoSansJP-*.otf`, from [Noto CJK](https://github.com/notofonts/noto-cjk)) | SIL Open Font License 1.1 (`assets/fonts/LICENSE-NotoSansCJK.txt`) |

## Certificates (`assets/cacert.pem`)

| Component | License | Notes |
| --- | --- | --- |
| Mozilla CA certificate bundle (as packaged by [certifi](https://github.com/certifi/python-certifi)) | MPL 2.0 | Used by Software Update to verify the HTTPS connection to GitHub when the system certificates are missing |

## Bundled native libraries (`libs/`)

| Library | License | Notes |
| --- | --- | --- |
| SDL2_mixer (`libSDL2_mixer-2.0.so.0`) | zlib | Patched build: ALAC and AAC support, 24-bit FLAC. MP3 through minimp3 (CC0), Ogg Vorbis through stb_vorbis (public domain / MIT), FLAC through dr_flac (public domain / MIT-0), all built in |
| Apple ALAC decoder (inside SDL2_mixer) | Apache 2.0 (`libs/LICENSE.ALAC-Apache-2.0.txt`) | See below |
| FFmpeg AAC decoder (`libavcodec_kp.so.60`, `libavutil_kp.so.58`) | LGPL 2.1 (`libs/LICENSE.FFmpeg-LGPL-2.1.txt`) | See below |

SDL2 itself is not bundled: betterPod uses the one provided by Knulli.

### Apple Lossless (ALAC) decoder

The SDL_mixer build includes Apple's [reference ALAC decoder](https://github.com/macosforge/alac), Copyright (c) 2011 Apple Inc., under the Apache License 2.0. Changes:

- the C++ decoder class was ported to C;
- an MP4 demuxer and an SDL_mixer music interface were added;
- FLAC decoding was changed to keep the full 24-bit resolution.

### FFmpeg (AAC decoder, LGPL 2.1)

`libavcodec_kp.so.60` and `libavutil_kp.so.58` are a minimal build of [FFmpeg](https://ffmpeg.org) 6.1.2 ([source](https://github.com/FFmpeg/FFmpeg), tag `n6.1.2`) containing only the AAC decoder.

- **License:** GNU Lesser General Public License 2.1.
- **Replaceable:** they are separate shared libraries, so they can be swapped for another build.
- **Configure flags:** `--disable-everything --disable-autodetect --enable-decoder=aac --enable-parser=aac --disable-programs --disable-doc --disable-avformat --disable-swscale --disable-swresample --disable-avdevice --disable-avfilter --disable-network --enable-shared --disable-static --enable-pic --build-suffix=_kp`.

## Go modules (compiled into `MiyooPod`)

| Module | License |
| --- | --- |
| [github.com/dhowden/tag](https://github.com/dhowden/tag) | BSD-2-Clause |
| [github.com/fogleman/gg](https://github.com/fogleman/gg) | MIT |
| [github.com/golang/freetype](https://github.com/golang/freetype) | FreeType License (dual-licensed with GPLv2; used under the FreeType License) |
| [github.com/google/uuid](https://github.com/google/uuid) | BSD-3-Clause |
| [github.com/skip2/go-qrcode](https://github.com/skip2/go-qrcode) | MIT |
| [golang.org/x/image](https://pkg.go.dev/golang.org/x/image) | BSD-3-Clause |
| [golang.org/x/text](https://pkg.go.dev/golang.org/x/text) | BSD-3-Clause |
| The Go standard library | BSD-3-Clause |

The full license texts are in each project's repository.

## Trademarks

iPod is a trademark of Apple Inc. betterPod is an independent fan project, not affiliated with or endorsed by Apple, Anbernic or the Knulli project.
