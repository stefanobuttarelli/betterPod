#!/bin/bash
# KnulliPod is now betterPod. This launcher came with the last update to
# reach the KnulliPod installation: it moves it, once, to its new name
#   ports/knullipod  -> ports/betterpod   (the database in conf goes with it)
#   ports/KnulliPod.sh -> ports/betterPod.sh
# and starts betterPod. In EmulationStation, update the game lists once to
# see betterPod in Ports.
PORTS="$(cd "$(dirname "$0")" && pwd)"
OLD="$PORTS/knullipod"
NEW="$PORTS/betterpod"

if [ -d "$OLD" ] && [ ! -e "$NEW" ]; then
  mv "$OLD" "$NEW"
fi
if [ -d "$NEW" ]; then
  [ -f "$NEW/MiyooPod" ] && [ ! -f "$NEW/betterpod" ] && mv "$NEW/MiyooPod" "$NEW/betterpod"
  rm -f "$NEW/MiyooPod" "$NEW/MiyooPod.prev"
  chmod +x "$NEW/betterpod" 2>/dev/null
  if [ -f "$NEW/betterPod.sh" ]; then
    mv -f "$NEW/betterPod.sh" "$PORTS/betterPod.sh"
    chmod +x "$PORTS/betterPod.sh"
  fi
fi
if [ -f "$PORTS/betterPod.sh" ] && [ -x "$NEW/betterpod" ]; then
  sync
  rm -f "$PORTS/KnulliPod.sh"
  exec /bin/bash "$PORTS/betterPod.sh" "$@"
fi
# the move could not be done: start it where it is
BETTERPOD_DIR="$OLD" exec /bin/bash "$OLD/betterPod.sh" "$@"
