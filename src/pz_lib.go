package main

// pz_lib.go — Simon Tatham's Portable Puzzle Collection (MIT, see
// puzzles/LICENCE): its shared code, compiled here. Each puzzle is in a
// pz_<name>.go of its own (the puzzles' static names would clash in one
// file); the front end is in ij_puzzles.go.

/*
#cgo CFLAGS: -DCOMBINED -w
#cgo LDFLAGS: -lm
#include "puzzles/midend.c"
#include "puzzles/drawing.c"
#include "puzzles/misc.c"
#include "puzzles/malloc.c"
#include "puzzles/random.c"
#include "puzzles/version.c"
#include "puzzles/tree234.c"
#include "puzzles/dsf.c"
#include "puzzles/divvy.c"
#include "puzzles/combi.c"
#include "puzzles/latin.c"
#include "puzzles/maxflow.c"
*/
import "C"
