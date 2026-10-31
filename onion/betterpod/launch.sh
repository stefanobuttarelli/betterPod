#!/bin/sh
# betterPod for Onion OS (Miyoo Mini, Mini Plus, Mini v4, Mini Flip): an
# iPod-style music player, recreated from iPod.js by Tanner Villarete; player
# engine: MiyooPod by Danilo Fragoso. All MIT.
# https://github.com/stefanobuttarelli/betterPod

appdir=$(cd "$(dirname "$0")" && pwd)
cd "$appdir"

# --- Music and data --------------------------------------------------------
# Music: Onion's music folder. Library, settings and artwork: conf/ in this
# folder (Settings > Database can move them, e.g. next to the music).
export MIYOOPOD_PLATFORM=miyoo
export MIYOOPOD_MUSIC_DIR=/mnt/SDCARD/Media/Music
export MIYOOPOD_DATA_DIR="$appdir/conf"
mkdir -p "$MIYOOPOD_MUSIC_DIR" "$MIYOOPOD_DATA_DIR"

# --- Options (remove the # to use one) ---------------------------------------
# (Settings > Effects chooses Full or Light animations; Light is the default)
# Brightness (in %) during Hold (SELECT). Default: a fifth of the current one:
# export MIYOOPOD_HOLD_BRIGHTNESS=10
# In Hold, up/down set the brightness. To turn that off:
# export MIYOOPOD_HOLD_ADJUST=0
# No automatic daily check for new versions:
# export MIYOOPOD_AUTO_UPDATE_CHECK=0
# Audio buffer in frames (default 2048: MI_AO keeps six, about 0.28 s).
# Higher if the music still clicks, at the cost of a slower pause:
# export MIYOOPOD_AUDIO_CHUNK=4096
# Up to +3 dB at full volume, like Onion (loud recordings may crackle):
# export MIYOOPOD_VOLUME_BOOST=1
# Let the Miyoo audio driver drop buffers again when MI_AO is busy:
# export MIYOOPOD_AO_TIMEOUT=0
# Present frames on the main thread (if the screen misbehaves):
# export MIYOOPOD_PRESENT_THREAD=0
# Tracks up to this size (MB) play from memory, bigger ones from the card:
# export MIYOOPOD_RAM_TRACK_MB=24
# Memory for album artwork (MB):
# export MIYOOPOD_ART_CACHE_MB=16
# CPU cores for the interface (default 1, the other one plays the music):
# export MIYOOPOD_GOMAXPROCS=2
# Log drawing times every second (to report stutter; see miyoopod.log):
# export MIYOOPOD_PERF=1
# Original MiyooPod interface:
# export MIYOOPOD_CLASSIC_UI=1

# --- System ------------------------------------------------------------------
export LD_LIBRARY_PATH="$appdir/libs:/customer/lib:/mnt/SDCARD/miyoo/lib:$LD_LIBRARY_PATH"
export SDL_VIDEODRIVER=mmiyoo
export SDL_AUDIODRIVER=mmiyoo
export EGL_VIDEODRIVER=mmiyoo

# Onion's audio server holds the sound device: stop it, with Onion's own
# script (it keeps the volume). Onion starts it again with its menu.
if [ -f /mnt/SDCARD/.tmp_update/script/stop_audioserver.sh ]; then
    . /mnt/SDCARD/.tmp_update/script/stop_audioserver.sh
else
    killall -9 audioserver audioserver.mod 2> /dev/null
    sleep 1
fi

# keymon would suspend the app on POWER (and stop the music) and owns the
# volume keys: betterPod handles them itself while it runs. Onion starts
# keymon again as soon as you are back in its menu.
killall -9 keymon 2> /dev/null

# Full speed for the interface; betterPod drops to powersave while the
# screen is off, and to the lowest clock while it sleeps (onion_sleep.go).
governor=/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor
old_governor=$(cat "$governor" 2> /dev/null)
min_freq=/sys/devices/system/cpu/cpu0/cpufreq/scaling_min_freq
old_min_freq=$(cat "$min_freq" 2> /dev/null)
echo performance > "$governor" 2> /dev/null
# the backlight (betterPod sets it to 0 with the screen off)
duty=/sys/class/pwm/pwmchip0/pwm0/duty_cycle
old_duty=$(cat "$duty" 2> /dev/null)

chmod +x ./betterpod 2> /dev/null
./betterpod

[ -n "$old_min_freq" ] && echo "$old_min_freq" > "$min_freq" 2> /dev/null
[ -n "$old_governor" ] && echo "$old_governor" > "$governor" 2> /dev/null

# The screen panel on again, however betterPod ended (it turns the panel off
# with the screen, as Onion's sleep does: gpio4).
echo 4 > /sys/class/gpio/export 2> /dev/null
echo out > /sys/devices/gpiochip0/gpio/gpio4/direction 2> /dev/null
echo 1 > /sys/devices/gpiochip0/gpio/gpio4/value 2> /dev/null
echo 4 > /sys/class/gpio/unexport 2> /dev/null
# and the backlight as it was (after a crash with the screen off it stayed 0)
case "$old_duty" in
  ''|0|*[!0-9]*) ;;
  *) echo "$old_duty" > "$duty" 2> /dev/null ;;
esac
sync
