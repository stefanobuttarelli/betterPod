#!/bin/sh
# Runs inside the perf container (tools/onion/perf.sh): builds the app for
# the host CPU, runs it in Onion mode on one core with MIYOOPOD_PERF=1 and a
# CPU profile, and walks through the screens that animate. Each scenario is
# marked in the log ("PHASE name") so the per-second PERF lines can be
# grouped by it.
set -u
cd /w
CGO_CFLAGS="-I/usr/include/SDL2" go build -o /tmp/app/betterpod src/*.go || exit 1
cp -R port/betterpod/assets /tmp/app/assets

export DISPLAY=:99
Xvfb :99 -screen 0 640x480x24 -nolisten tcp > /dev/null 2>&1 &
sleep 2
export SDL_VIDEODRIVER=x11 SDL_AUDIODRIVER=dummy SDL_RENDER_DRIVER=software
export MIYOOPOD_PLATFORM=miyoo MIYOOPOD_GOMAXPROCS=1
export MIYOOPOD_MUSIC_DIR=/music MIYOOPOD_DATA_DIR=/tmp/app/conf
export MIYOOPOD_AUTO_UPDATE_CHECK=0 MIYOOPOD_PERF=1 MIYOOPOD_PPROF=/out/cpu.prof
export MIYOOPOD_VSYNC=0
if [ -n "${PERF_EFFECTS:-}" ]; then
  mkdir -p /tmp/app/conf && echo "{\"effects\":\"$PERF_EFFECTS\"}" > /tmp/app/conf/ipodjs_settings.json
fi
cd /tmp/app
./betterpod > /out/stdout.txt 2>&1 &
PID=$!
for i in $(seq 1 60); do
  WIN=$(xdotool search --name MiyooPod 2> /dev/null | head -1)
  [ -n "$WIN" ] && break
  sleep 1
done
xdotool windowfocus "$WIN" 2> /dev/null
sleep 8

phase() { echo "$(date -u '+%Y-%m-%d %H:%M:%S') - PHASE $1" >> /tmp/app/miyoopod.log; }
key() { xdotool key --delay 120 "$@"; }
hold() { xdotool keydown "$1"; sleep "$2"; xdotool keyup "$1"; }

phase idle-home;   sleep 10
key Down;          phase idle-home-music; sleep 6
key space;         phase idle-music; sleep 6
key Down Down Down; phase menu-step; sleep 2
key space;         phase albums-idle; sleep 4
phase scroll;      hold Down 3; hold Up 3
phase sheet;       for i in 1 2 3 4; do hold space 0.8; sleep 0.8; key Control_L; sleep 0.8; done
phase slides;      for i in 1 2 3 4 5 6; do key space; sleep 0.7; key Control_L; sleep 0.7; done
key space;         sleep 1.5
key space;         phase nowplaying; sleep 8
key Control_L;     phase after-np; sleep 3
hold Control_L 0.8; sleep 1
key Up Up Up Up;   key space; phase coverflow-idle; sleep 4
phase coverflow;   hold Right 3; hold Left 3
phase end;         sleep 1
xdotool keydown Control_R key Return keyup Control_R
for i in $(seq 1 20); do kill -0 $PID 2> /dev/null || break; sleep 1; done
kill $PID 2> /dev/null
cp /tmp/app/miyoopod.log /out/
