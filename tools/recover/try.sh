#!/bin/sh
# try.sh FILE FUNC : rebuild and print number of diff lines for FUNC
cd /home/claude/knullipod
./tools/recover/objbuild.sh /tmp/kpobj >/tmp/kpobj/build.log 2>&1 || { tail -5 /tmp/kpobj/build.log; exit 1; }
python3 tools/recover/fdiff.py "$1" | grep -c '^[-+][^-+]'
