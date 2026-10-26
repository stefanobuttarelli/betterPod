# Third-Party Licenses

MiyooPod itself is licensed under the [MIT License](LICENSE). It bundles and
depends on the following third-party components, each distributed under its own
license. Copyright and license notices for these components are retained by
their respective authors.

## Bundled Native Libraries (`libs/`)

| Component | License |
| --- | --- |
| SDL2 | zlib License |
| SDL2_mixer | zlib License |
| zlib (`libSDL2_z`) | zlib License |
| json-c (`libSDL2_json-c`) | MIT License |
| libmpg123 | LGPL 2.1 |
| dr_flac (statically linked in SDL2_mixer) | Public Domain (Unlicense) / MIT-0 |
| stb_vorbis (statically linked in SDL2_mixer) | Public Domain (Unlicense) / MIT |

### Note on libmpg123 (LGPL 2.1)

libmpg123 is licensed under the GNU Lesser General Public License, version 2.1.
It is dynamically linked (as a shared `.so`), which satisfies the LGPL
requirement that users be able to replace or relink the library. Its license
text and source are available from https://www.mpg123.de/.

## Apple Lossless (ALAC) decoder

KnulliPod's SDL_mixer build (`libs/libSDL2_mixer-2.0.so.0`) includes Apple's
reference ALAC decoder (https://github.com/macosforge/alac), Copyright (c) 2011
Apple Inc., licensed under the Apache License 2.0 (see
`libs/LICENSE.ALAC-Apache-2.0.txt`). The C++ decoder class was ported to C and
an MP4 demuxer and SDL_mixer music interface were added; FLAC decoding was
changed to keep full 24-bit resolution.

## FFmpeg (AAC decoder, LGPL 2.1)

`libs/libavcodec_kp.so.60` and `libs/libavutil_kp.so.58` are a minimal build of
FFmpeg 6.1.2 (https://ffmpeg.org, source: https://github.com/FFmpeg/FFmpeg,
tag n6.1.2) containing only the AAC decoder, licensed under the GNU Lesser
General Public License 2.1 (see `libs/LICENSE.FFmpeg-LGPL-2.1.txt`). They are
separate shared libraries and can be replaced. Configure flags:
`--disable-everything --disable-autodetect --enable-decoder=aac
--enable-parser=aac --disable-programs --disable-doc --disable-avformat
--disable-swscale --disable-swresample --disable-avdevice --disable-avfilter
--disable-network --enable-shared --disable-static --enable-pic
--build-suffix=_kp`.

## Go Module Dependencies

| Module | License |
| --- | --- |
| github.com/dhowden/tag | BSD-2-Clause |
| github.com/fogleman/gg | MIT License |
| github.com/google/uuid | BSD-3-Clause |
| github.com/skip2/go-qrcode | MIT License |
| golang.org/x/image | BSD-3-Clause |
| github.com/golang/freetype | FreeType License / GPLv2 |

Full license texts for each dependency are available in their respective source
repositories.
