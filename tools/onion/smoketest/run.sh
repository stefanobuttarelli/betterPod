#!/bin/sh
# Runs inside the smoke test container (see tools/onion/smoketest.sh):
# /app is a copy of App/betterpod, /music the test library, /out the frames.
# Each step presses keys and saves the frame on screen as /out/NN-name.raw.
set -u
export DISPLAY=:99
Xvfb :99 -screen 0 640x480x24 -nolisten tcp > /dev/null 2>&1 &
sleep 2

mkfifo /tmp/onion-input
exec 3<> /tmp/onion-input               # kept open: writes never wait for the app
export LD_LIBRARY_PATH=/opt/sdl          # Debian's SDL2 instead of the Miyoo one
# software renderer: the present thread (main.c) can't use Debian's GL one
export SDL_VIDEODRIVER=x11 SDL_AUDIODRIVER=dummy SDL_RENDER_DRIVER=software
export MIYOOPOD_PLATFORM=miyoo
export MIYOOPOD_MUSIC_DIR=/music MIYOOPOD_DATA_DIR=/app/conf
export MIYOOPOD_ONION_INPUT=/tmp/onion-input
export MIYOOPOD_DUMP_FRAME=/tmp/frame.raw
export MIYOOPOD_AUTO_UPDATE_CHECK=0 MIYOOPOD_KEYLOG=1

cd /app
./betterpod > /out/stdout.txt 2>&1 &
PID=$!

for i in $(seq 1 120); do
  WIN=$(xdotool search --name MiyooPod 2> /dev/null | head -1)
  [ -n "$WIN" ] && break
  sleep 1
done
[ -n "${WIN:-}" ] || { echo "no window"; exit 1; }
xdotool windowfocus "$WIN" 2> /dev/null

n=0
shot() { # shot NAME SECONDS: wait, then keep the frame on screen
  sleep "$2"
  n=$((n + 1))
  cp /tmp/frame.raw "/out/$(printf %02d $n)-$1.raw" 2> /dev/null || echo "no frame for $1"
}
key() { xdotool key --delay 150 "$@"; }
# evdev key on the Onion input (POWER 116, VOLUMEUP 115, VOLUMEDOWN 114)
ev() { perl -e 'print pack("llSSl", 0, 0, 1, $ARGV[0], $ARGV[1])' "$1" "$2" >&3; }

shot home 25
key space;             shot music 6        # A: into Music
key Down space;        shot list 6
key space;             shot list2 6
key space;             shot played 10      # play whatever is selected
key Up;                shot np-volume 3    # Cover Flow's Now Playing: up raises the volume
key Escape;            shot menu-back 5    # MENU = back
key Escape Escape;     shot home-again 5   # back to the main menu
key Down space;        shot music-menu 5   # Music: Now playing and Up Next at the bottom
key Down Down Down Down Down Down Down Down space; shot upnext 5 # Up Next (9th row)
key Escape;            shot music-again 4
ev 115 1; ev 115 0;    shot vol-up 1       # volume key
key Tab;               shot bright-down 1  # L2: brightness
key Return;            shot pause 4        # START: play/pause
key Return;            shot resume 4
ev 116 1; ev 116 0;    shot power-off 4    # POWER: screen off
ev 115 1; ev 115 0;    shot vol-while-off 2
ev 116 1; ev 116 0;    shot power-on 5     # POWER: screen on
key Control_R;         shot hold-on 3      # SELECT: Hold
key Control_R;         shot hold-off 3
key Down Down Down Down Alt_L; shot clear-sheet 3 # Y on the main menu's "Now Playing": Clear Now Playing
key Right space;       shot cleared 4      # Clear (Keep is chosen first): the music stops, the two rows go
key Up Up Up Up space; shot music-again2 4 # Music (Genius took Now Playing's row)
key Down Down Down space; shot albums 5    # Albums
key Alt_L;             shot album-sheet 3  # Y on an album: its options
key Down Down space;   shot share 10       # Share: a code from magic-wormhole's server, waiting
key Escape;            shot share-stopped 3
key Alt_L;             shot album-sheet2 3
key Down Down Down space; shot album-info 5 # Edit Album Info
key space;             shot info-search 20 # Find Info Online (the real sources)
key Escape;            shot album-info2 4
key Escape Escape;     shot back 4
key Escape;            shot home-music 4   # the main menu, on Music
key Down space;        shot podcasts 5     # Podcasts, under Music in the main menu: Your Podcasts
key Down Down Alt_L;   shot pod-sheet 3    # Y on Smoke Cast
key space;             shot pod-confirm 3  # Delete Downloaded Episodes: it asks
key space;             shot pod-deleted 4  # Delete
key Escape;            shot music-again4 4
key Down space;        shot apps 4         # Apps
key Down Down Down space; shot words 5     # Five Letters (its word lists)
key Escape;            shot apps-again 4
key Down Down space;   shot puzzles 4      # Puzzles (Simon Tatham's C code, built for armv7)
key space;             shot mines 8        # Mines: a new puzzle
key space;             shot mines-open 4   # A opens a square
key Escape;            shot puzzles2 4
key Down space;        shot sudoku 8       # Sudoku: its generator, the heaviest
key Escape Escape;     shot apps2 4
key Escape;            shot home-apps 4
key Down space;        shot settings 4     # Settings: Effects there, YouTube not (onionSettings)
ev 116 1;              shot quit-warning 3 # hold POWER: warning, quit at 5 s
sleep 4
ev 116 0
for i in $(seq 1 20); do kill -0 $PID 2> /dev/null || break; sleep 1; done
if kill -0 $PID 2> /dev/null; then
  echo "still running after POWER hold"; kill $PID
else
  wait $PID; echo "exited with $?"
fi
cp /app/miyoopod.log /out/ 2> /dev/null
cp -R /app/conf /out/conf 2> /dev/null
