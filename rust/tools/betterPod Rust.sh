#!/bin/bash
# betterPod Rust: a separate app. Does not replace betterPod or its personal data.
LAUNCHER_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
GAMEDIR=${BETTERPOD_RUST_DIR:-$LAUNCHER_DIR/betterpod-rust}
STABLE_DIR="$LAUNCHER_DIR/betterpod"
XDG_DATA_HOME=${XDG_DATA_HOME:-$HOME/.local/share}
controlfolder=""
if command -v knulli-version >/dev/null 2>&1; then
  # Modern Knulli configgen already supplies SDL mappings. Its legacy
  # PortMaster control can attempt to patch an obsolete Python path, so
  # this independent app does not source it or alter firmware files.
  if [ -z "${SDL_GAMECONTROLLERCONFIG:-}" ]; then
    controller_db="${SDL_GAMECONTROLLERCONFIG_FILE:-}"
    if [ ! -f "$controller_db" ] || [ ! -r "$controller_db" ]; then
      controller_db=/tmp/gamecontrollerdb.txt
    fi
    if [ -f "$controller_db" ] && [ -r "$controller_db" ]; then
      export SDL_GAMECONTROLLERCONFIG_FILE="$controller_db"
      export SDL_GAMECONTROLLERCONFIG="$(cat -- "$controller_db")"
    fi
  fi
else
  for folder in /opt/system/Tools/PortMaster /opt/tools/PortMaster "$XDG_DATA_HOME/PortMaster" /roms/ports/PortMaster; do
    if [ -f "$folder/control.txt" ]; then controlfolder=$folder; break; fi
  done
  if [ -n "$controlfolder" ]; then
    # Other firmware keeps the existing optional PortMaster integration.
    source "$controlfolder/control.txt"
    [ ! -f "$controlfolder/mod_${CFW_NAME:-}.txt" ] || source "$controlfolder/mod_${CFW_NAME}.txt"
    get_controls
    export SDL_GAMECONTROLLERCONFIG="${sdl_controllerconfig:-${SDL_GAMECONTROLLERCONFIG:-}}"
  fi
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
bright_tool=$(command -v knulli-brightness 2>/dev/null || command -v batocera-brightness 2>/dev/null || true)
old_bright=""
# Both firmware helpers read a percentage with no arguments; a single
# numeric argument restores it ("get" is not their command-line API).
[ -z "$bright_tool" ] || old_bright=$("$bright_tool" 2>/dev/null)
frontend_state="$BETTERPOD_RUST_DATA_DIR/.frontend-audio.json"
app_log="$GAMEDIR/betterpod-rust.log"
app_pid=""
cleanup() {
  trap - EXIT INT TERM HUP
  # A frozen frontend audio client can hold up the app's SDL shutdown.
  # Resume it and restore only its own streams before waiting for the app.
  "$GAMEDIR/betterpod-rust" --frontend-audio release "$frontend_state" "$$" >> "$app_log" 2>&1 || true
  if [ -n "$app_pid" ] && kill -0 "$app_pid" 2>/dev/null; then
    kill -TERM "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
  "$GAMEDIR/betterpod-rust" --frontend-audio finish "$frontend_state" "$$" >> "$app_log" 2>&1 || true
  rm -f -- "$BETTERPOD_RUST_IDLE_FILE"
  for pair in $old_governors; do
    file=${pair%%=*}; value=${pair#*=}
    [ ! -f "$file" ] || [ -z "$value" ] || printf '%s\n' "$value" > "$file" 2>/dev/null || true
  done
  case "$old_bright" in
    ''|*[!0-9]*) ;;
    *) "$bright_tool" "$old_bright" >/dev/null 2>&1 || true ;;
  esac
  if [ "$(type -t pm_finish)" = function ]; then pm_finish; fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM HUP
es_pids=$(pidof emulationstation 2>/dev/null || true)
: > "$app_log"
# pidof supplies numeric IDs. The Rust helper validates process start times,
# parks their verified audio streams, then freezes the matching frontends.
"$GAMEDIR/betterpod-rust" --frontend-audio prepare "$frontend_state" "$$" $es_pids >> "$app_log" 2>&1 || exit 1
cd "$GAMEDIR" || exit 1
./betterpod-rust >> "$app_log" 2>&1 &
app_pid=$!
wait "$app_pid"
result=$?
app_pid=""
exit "$result"
