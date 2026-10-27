#!/bin/bash
# KnulliPod - an iPod-style music player for Knulli (PortMaster-style port)
# UI recreated from iPod.js by Tanner Villarete (MIT); player engine from MiyooPod by Danilo Fragoso (MIT)
# https://github.com/stefanobuttarelli/knullipod

XDG_DATA_HOME=${XDG_DATA_HOME:-$HOME/.local/share}

if [ -d "/opt/system/Tools/PortMaster/" ]; then
  controlfolder="/opt/system/Tools/PortMaster"
elif [ -d "/opt/tools/PortMaster/" ]; then
  controlfolder="/opt/tools/PortMaster"
elif [ -d "$XDG_DATA_HOME/PortMaster/" ]; then
  controlfolder="$XDG_DATA_HOME/PortMaster"
else
  controlfolder="/roms/ports/PortMaster"
fi

source $controlfolder/control.txt
[ -f "${controlfolder}/mod_${CFW_NAME}.txt" ] && source "${controlfolder}/mod_${CFW_NAME}.txt"
get_controls

GAMEDIR="/$directory/ports/knullipod"
cd "$GAMEDIR"

> "$GAMEDIR/log.txt" && exec > >(tee "$GAMEDIR/log.txt") 2>&1

# --- Music folder ----------------------------------------------------------
# On Knulli the standard folder is /userdata/music (the "music" network share).
# If it doesn't exist, knullipod/music inside the ports folder is used.
if [ -d "/userdata/music" ]; then
  export MIYOOPOD_MUSIC_DIR="/userdata/music"
else
  mkdir -p "$GAMEDIR/music"
  export MIYOOPOD_MUSIC_DIR="$GAMEDIR/music"
fi
# Library, settings and artwork stay in the port folder (Settings > Database
# can move them elsewhere)
export MIYOOPOD_DATA_DIR="$GAMEDIR/conf"

# --- Controls --------------------------------------------------------------
# The gamepad is read directly through SDL (no gptokeyb, no double input).
export SDL_GAMECONTROLLERCONFIG="$sdl_controllerconfig"
# A and B follow the Anbernic labels. If they are swapped, remove the # below:
# export MIYOOPOD_SWAP_AB=0
# The stick works like the arrows. To turn it like a click wheel instead:
# export MIYOOPOD_STICK_WHEEL=1
# export MIYOOPOD_WHEEL_STEP=15   # degrees of rotation per step (5-90)
# Brightness (in %) during Hold (SELECT). Default: a fifth of the current one:
# export MIYOOPOD_HOLD_BRIGHTNESS=10
# In Hold, up/down set the brightness. To turn that off:
# export MIYOOPOD_HOLD_ADJUST=0
# Use the app's internal volume instead of the console volume:
# export MIYOOPOD_SOFT_VOLUME=1
# Ignore headphone buttons / no Bluetooth icon:
# export MIYOOPOD_MEDIA_KEYS=0
# export MIYOOPOD_BT_ICON=0
# Log drawing times (to report stutter):
# export MIYOOPOD_PERF=1
# Disable vertical sync:
# export MIYOOPOD_VSYNC=0
# Original MiyooPod interface:
# export MIYOOPOD_CLASSIC_UI=1

export LD_LIBRARY_PATH="$GAMEDIR/libs:$LD_LIBRARY_PATH"

$ESUDO chmod +x "$GAMEDIR/MiyooPod"
if [ "$(type -t pm_platform_helper)" = "function" ]; then
  pm_platform_helper "$GAMEDIR/MiyooPod"
fi

./MiyooPod

if [ "$(type -t pm_finish)" = "function" ]; then
  pm_finish
fi
printf "\033c" > /dev/tty0 2>/dev/null
