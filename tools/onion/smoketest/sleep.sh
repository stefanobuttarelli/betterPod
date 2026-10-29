#!/bin/sh
# Runs inside the smoke test container (see tools/onion/sleeptest.sh).
set -u
export DISPLAY=:99
Xvfb :99 -screen 0 640x480x24 -nolisten tcp > /dev/null 2>&1 &
sleep 2
export LD_LIBRARY_PATH=/opt/sdl
export SDL_VIDEODRIVER=x11 SDL_AUDIODRIVER=dummy SDL_RENDER_DRIVER=software
export MIYOOPOD_PLATFORM=miyoo
export MIYOOPOD_MUSIC_DIR=/music MIYOOPOD_DATA_DIR=/app/conf
export MIYOOPOD_AUTO_UPDATE_CHECK=0
export MIYOOPOD_SLEEP_SCALE=3
: > /out/result.txt

# run NAME MINUTES SECONDS [POWER_AT]: start betterPod with the timer at
# MINUTES, press POWER POWER_AT s after the window shows, and wait up to
# SECONDS for it to quit by itself
run() {
  name=$1 mins=$2 secs=$3 power=${4:-}
  rm -f /tmp/.offOrder /tmp/onion-input /app/miyoopod.log
  mkfifo /tmp/onion-input
  exec 3<> /tmp/onion-input
  export MIYOOPOD_ONION_INPUT=/tmp/onion-input MIYOOPOD_SLEEP_MINUTES=$mins
  cd /app
  ./betterpod > "/out/$name-stdout.txt" 2>&1 &
  pid=$!
  for i in $(seq 1 120); do
    xdotool search --name MiyooPod > /dev/null 2>&1 && break
    sleep 1
  done
  # the library scan first (the timer waits for it)
  for i in $(seq 1 120); do
    grep -q "Library scan complete\|Library loaded" /app/miyoopod.log 2> /dev/null && break
    sleep 1
  done
  start=$(date +%s)
  if [ -n "$power" ]; then
    sleep "$power"
    perl -e 'print pack("llSSl", 0, 0, 1, 116, 1)' >&3
    sleep 0.3
    perl -e 'print pack("llSSl", 0, 0, 1, 116, 0)' >&3
  fi
  quit=no
  for i in $(seq 1 "$secs"); do
    kill -0 $pid 2> /dev/null || { quit=yes; break; }
    sleep 1
  done
  took=$(( $(date +%s) - start ))
  kill -0 $pid 2> /dev/null && { kill $pid; sleep 3; }
  cp /app/miyoopod.log "/out/$name.log" 2> /dev/null
  off=no; [ -f /tmp/.offOrder ] && off=yes
  echo "$name: quit=$quit after ${took}s, offOrder=$off" >> /out/result.txt
  exec 3>&-
}

run auto 1 40
run power 2 60 2
run never 0 25

grep -q "auto: quit=yes.*offOrder=yes" /out/result.txt || echo "FAIL auto" >> /out/result.txt
grep -q "power: quit=yes.*offOrder=yes" /out/result.txt || echo "FAIL power" >> /out/result.txt
grep -q "never: quit=no.*offOrder=no" /out/result.txt || echo "FAIL never" >> /out/result.txt
