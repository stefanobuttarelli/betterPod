#!/bin/bash
# KnulliPod - lettore musicale in stile iPod per Knulli (PortMaster)
# UI ricreata da iPod.js di Tanner Villarete (MIT); motore audio da MiyooPod di Danilo Fragoso (MIT)

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

# --- Cartella musica -------------------------------------------------------
# Su Knulli la cartella standard e' /userdata/music (accessibile anche via rete
# come "music"). Se non esiste, si usa knullipod/music dentro la cartella ports.
if [ -d "/userdata/music" ]; then
  export MIYOOPOD_MUSIC_DIR="/userdata/music"
else
  mkdir -p "$GAMEDIR/music"
  export MIYOOPOD_MUSIC_DIR="$GAMEDIR/music"
fi
# Libreria, impostazioni e copertine restano nella cartella del port
export MIYOOPOD_DATA_DIR="$GAMEDIR/conf"

# --- Comandi ----------------------------------------------------------------
# Il gamepad e' letto direttamente da SDL (niente gptokeyb, evita input doppi).
export SDL_GAMECONTROLLERCONFIG="$sdl_controllerconfig"
# A e B seguono le etichette Anbernic. Se risultano invertiti, togli il # qui sotto:
# export MIYOOPOD_SWAP_AB=0
# Lo stick funziona come le frecce. Per usarlo ruotandolo come una ghiera:
# export MIYOOPOD_STICK_WHEEL=1
# export MIYOOPOD_WHEEL_STEP=15   # gradi di rotazione per scatto (5-90)
# Luminosita' (in %) durante Hold (SELECT). Di base un quinto di quella attuale:
# export MIYOOPOD_HOLD_BRIGHTNESS=10
# In Hold le frecce su/giu' regolano la luminosita'. Per disattivarlo:
# export MIYOOPOD_HOLD_ADJUST=0
# Volume interno dell'app invece del volume della console:
# export MIYOOPOD_SOFT_VOLUME=1
# Ignora i tasti delle cuffie / niente icona Bluetooth:
# export MIYOOPOD_MEDIA_KEYS=0
# export MIYOOPOD_BT_ICON=0
# Registro dei tempi di disegno (per segnalare scatti):
# export MIYOOPOD_PERF=1

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
