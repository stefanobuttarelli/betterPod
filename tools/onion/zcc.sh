#!/bin/sh
# C compiler for the Miyoo Mini (Onion OS): zig cc for 32-bit ARM, hard
# float, glibc 2.28 like the Miyoo firmware, tuned for the Cortex-A7.
exec zig cc -target arm-linux-gnueabihf.2.28 -mcpu=cortex_a7 "$@"
