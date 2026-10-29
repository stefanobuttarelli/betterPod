# Third-party licenses

betterPod is licensed under the MIT License (`LICENSE`). It includes and depends on the components below. Each is distributed under its own license, and the copyright notices stay with their authors.

## Projects it is based on

| Project | Author | License | Used for |
| --- | --- | --- | --- |
| [iPod.js](https://github.com/tvillarete/ipod-classic-js) | Tanner Villarete | MIT (`LICENSE.ipod.js`) | Interface design, layouts, icons and images in `assets/ipod` |
| [MiyooPod](https://github.com/danfragoso/miyoopod) | Danilo Fragoso | MIT (`LICENSE.MiyooPod`) | Player engine, library scan, the Miyoo libraries in `libs/` |
| [Simon Tatham's Portable Puzzle Collection](https://www.chiark.greenend.org.uk/~sgtatham/puzzles/) | Simon Tatham and contributors | MIT (`LICENSE.puzzles`) | Apps → Puzzles: twelve of its puzzles, compiled in |
| [2048](https://github.com/gabrielecirulli/2048) | Gabriele Cirulli | MIT | Apps → 2048: the game's rules (the code is betterPod's own) |
| [SCOWL](http://wordlist.aspell.net/) and ENABLE | Kevin Atkinson and others; Alan Beale, Mendel Cooper and others | SCOWL's permissive licence (`LICENSE.scowl`); ENABLE is public domain | Apps → Five Letters: the answers and the accepted guesses |
| [Onion](https://github.com/OnionUI/Onion) | the Onion team | MIT | The volume and brightness curves, the sleep timer and the screen-off sequence follow its sources |

## Fonts (`assets/`)

| Font | License |
| --- | --- |
| Inter (`assets/fonts`) | SIL Open Font License 1.1 (`assets/fonts/COPYRIGHT-Inter.txt`) |
| Noto Sans JP (`assets/fonts/NotoSansJP-*.otf`, from [Noto CJK](https://github.com/notofonts/noto-cjk)) | SIL Open Font License 1.1 (`assets/fonts/LICENSE-NotoSansCJK.txt`) |

## Certificates (`assets/cacert.pem`)

| Component | License | Notes |
| --- | --- | --- |
| Mozilla CA certificate bundle (as packaged by [certifi](https://github.com/certifi/python-certifi)) | MPL 2.0 | Used by Software Update to verify the HTTPS connection to GitHub (the Miyoo has no system certificates) |

## Bundled native libraries (`libs/`)

The Miyoo Mini builds shipped with MiyooPod 0.0.8, unchanged.

| Library | License | Notes |
| --- | --- | --- |
| SDL2 (`libSDL2.so`) | zlib | [steward-fu's SDL2](https://github.com/steward-fu/sdl2) with the Miyoo Mini video and audio drivers (`mmiyoo`) |
| SDL2_mixer (`libSDL2_mixer.so`) | zlib | FLAC through dr_flac (public domain / MIT-0) and Ogg Vorbis through stb_vorbis (public domain / MIT), built in |
| mpg123 (`libmpg123.so.0`) | LGPL 2.1 | MP3 decoding. A separate shared library, so it can be replaced; source at [mpg123.de](https://www.mpg123.de/) |
| SwiftShader EGL / OpenGL ES (`libSDL2_EGL.so`, `libSDL2_GLESv2.so`) | Apache 2.0 | Needed by the SDL2 build; source at [swiftshader](https://github.com/google/swiftshader) |
| json-c (`libSDL2_json-c.so`) | MIT | Needed by the SDL2 build |
| zlib (`libSDL2_z.so`) | zlib | Needed by the SDL2 build |

## Go modules (compiled into `betterpod`)

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

iPod is a trademark of Apple Inc. betterPod is an independent fan project, not affiliated with or endorsed by Apple, Miyoo or the Onion project.
