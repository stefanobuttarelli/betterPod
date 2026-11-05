#!/bin/bash
# Separate local experiment. Does not replace betterPod or its personal data.
LAUNCHER_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
GAMEDIR=${BETTERPOD_RUST_DIR:-$LAUNCHER_DIR/betterpod-rust}
STABLE_DIR="$LAUNCHER_DIR/betterpod"
XDG_DATA_HOME=${XDG_DATA_HOME:-$HOME/.local/share}
controlfolder=""
for folder in /opt/system/Tools/PortMaster /opt/tools/PortMaster "$XDG_DATA_HOME/PortMaster" /roms/ports/PortMaster; do
  if [ -f "$folder/control.txt" ]; then controlfolder=$folder; break; fi
done
if [ -n "$controlfolder" ]; then
  # PortMaster supplies mappings and firmware helpers, not app implementation.
  source "$controlfolder/control.txt"
  [ ! -f "$controlfolder/mod_${CFW_NAME:-}.txt" ] || source "$controlfolder/mod_${CFW_NAME}.txt"
  get_controls
  export SDL_GAMECONTROLLERCONFIG="${sdl_controllerconfig:-${SDL_GAMECONTROLLERCONFIG:-}}"
fi
[ -x "$GAMEDIR/betterpod-rust" ] || { echo "Missing Rust app: $GAMEDIR/betterpod-rust" >&2; exit 1; }
mkdir -p "$GAMEDIR/conf"
export BETTERPOD_RUST_DATA_DIR=${BETTERPOD_RUST_DATA_DIR:-$GAMEDIR/conf}
export BETTERPOD_RUST_ASSETS_DIR=${BETTERPOD_RUST_ASSETS_DIR:-$GAMEDIR/assets}
export BETTERPOD_RUST_FULLSCREEN=${BETTERPOD_RUST_FULLSCREEN:-1}
mkdir -p "$BETTERPOD_RUST_DATA_DIR"

# Import only the library index. Settings, queue, scans and extracted artwork
# are written to the Rust app's own folder. Original files are never migrated.
stable_data="$STABLE_DIR/conf"
if [ -f "$stable_data/datadir.txt" ]; then
  candidate=$(cat "$stable_data/datadir.txt")
  case "$candidate" in /*) [ ! -d "$candidate" ] || stable_data="$candidate" ;; esac
fi
if [ -z "${BETTERPOD_RUST_MUSIC_DIR:-}" ]; then
  if [ -f "$STABLE_DIR/conf/musicdir.txt" ]; then
    candidate=$(cat "$STABLE_DIR/conf/musicdir.txt")
    case "$candidate" in /*) [ ! -d "$candidate" ] || export BETTERPOD_RUST_MUSIC_DIR="$candidate" ;; esac
  fi
fi
if [ -z "${BETTERPOD_RUST_MUSIC_DIR:-}" ]; then
  if [ -d /userdata/music ]; then export BETTERPOD_RUST_MUSIC_DIR=/userdata/music
  elif [ -d "$STABLE_DIR/music" ]; then export BETTERPOD_RUST_MUSIC_DIR="$STABLE_DIR/music"
  else mkdir -p "$GAMEDIR/music"; export BETTERPOD_RUST_MUSIC_DIR="$GAMEDIR/music"; fi
fi
if [ -z "${BETTERPOD_RUST_IMPORT_CACHE:-}" ] && [ -f "$stable_data/.miyoopod_library.json" ]; then
  export BETTERPOD_RUST_IMPORT_CACHE="$stable_data/.miyoopod_library.json"
fi
for protected in "$STABLE_DIR/conf" "$stable_data"; do
  if [ -d "$protected" ] && [ "$(cd "$BETTERPOD_RUST_DATA_DIR" && pwd -P)" = "$(cd "$protected" && pwd -P)" ]; then
    echo "Refusing to use the installed betterPod database for Rust writes" >&2; exit 1
  fi
done
export LD_LIBRARY_PATH="$GAMEDIR/libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/var/run}
export PULSE_SERVER=${PULSE_SERVER:-unix:/var/run/pulse/native}
export BETTERPOD_RUST_IDLE_FILE="/var/run/battery-saver/betterpod-rust-$$.pause"
old_governors=""
for file in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
  [ ! -f "$file" ] || old_governors="$old_governors $file=$(cat "$file" 2>/dev/null)"
done
bright_tool=$(command -v batocera-brightness 2>/dev/null || true)
old_bright=""
[ -z "$bright_tool" ] || old_bright=$("$bright_tool" 2>/dev/null)
paused_es=""
app_pid=""
cleanup() {
  trap - EXIT INT TERM HUP
  if [ -n "$app_pid" ] && kill -0 "$app_pid" 2>/dev/null; then
    kill -TERM "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
  rm -f -- "$BETTERPOD_RUST_IDLE_FILE"
  for pair in $old_governors; do
    file=${pair%%=*}; value=${pair#*=}
    [ ! -f "$file" ] || [ -z "$value" ] || printf '%s\n' "$value" > "$file" 2>/dev/null || true
  done
  case "$old_bright" in
    ''|*[!0-9]*) ;;
    *) "$bright_tool" "$old_bright" >/dev/null 2>&1 || true ;;
  esac
  for pid in $paused_es; do
    [ "$(cat "/proc/$pid/comm" 2>/dev/null)" != emulationstatio ] && [ "$(cat "/proc/$pid/comm" 2>/dev/null)" != emulationstation ] || kill -CONT "$pid" 2>/dev/null || true
  done
  if [ "$(type -t pm_finish)" = function ]; then pm_finish; fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM HUP
for pid in $(pidof emulationstation 2>/dev/null || true); do
  state=$(awk '{print $3}' "/proc/$pid/stat" 2>/dev/null)
  case "$state" in T|t) ;; *) kill -STOP "$pid" 2>/dev/null && paused_es="$paused_es $pid" ;; esac
done
cd "$GAMEDIR" || exit 1
./betterpod-rust > "$GAMEDIR/betterpod-rust.log" 2>&1 &
app_pid=$!
wait "$app_pid"
result=$?
app_pid=""
exit "$result"
