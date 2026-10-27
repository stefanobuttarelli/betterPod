# KnulliPod — sorgenti

Lettore musicale in stile iPod per Knulli (Anbernic H700, aarch64), avviabile da PortMaster.
Guida d'uso e comandi: [port/knullipod/README.md](port/knullipod/README.md).

Interfaccia ricostruita da [iPod.js](https://github.com/tvillarete/ipod-classic-js) (Tanner Villarete, MIT).
Audio e libreria da [MiyooPod](https://github.com/danfragoso/miyoopod) (Danilo Fragoso, MIT).

## Struttura

- `src/`: codice Go e C (SDL2 / SDL2_mixer).
  - `ij_*.go`: l'interfaccia iPod.js.
  - `main.c`: input, display e vsync.
  - `audio.c`: SDL_mixer, con cambio della frequenza di campionamento.
  - `knulli.go`: integrazione con il sistema (volume, luminosità, Bluetooth).
- `port/`: il pacchetto PortMaster, cioè `KnulliPod.sh` e la cartella `knullipod`. Contiene asset, font e le librerie `libSDL2_mixer` / `libavcodec_kp` / `libavutil_kp`.
- `tools/build.sh`: compila e crea `dist/knullipod.zip`.
- `docs/recovery/` e `tools/recover/`: come sono stati ricostruiti i sorgenti della v0.20 partendo dal binario (vedi `STATUS.md`).

## Compilare

Servono:

- Go 1.24
- `zig` (`pip install ziglang`) come compilatore C per aarch64 con glibc 2.28
- gli header SDL2 e `libSDL2-2.0.so` per aarch64, in una cartella con `include/SDL2` e `lib/`

```sh
SDL2_SYSROOT=/percorso/sysroot tools/build.sh
# -> dist/knullipod.zip  (KnulliPod.sh + knullipod/, binario knullipod/MiyooPod)
```

Il binario va compilato dalla lista dei file (`src/*.go`), non come package: i file `.c` sono inclusi da `main.go`.
