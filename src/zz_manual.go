package main

// Temporary declarations of inline-only helpers whose files are not
// recovered yet (genstubs cannot see them). Delete with zz_stubs.go.

import "unsafe"

func newKenBurns() *KenBurns                             { return &KenBurns{} }
func refreshFramebuffer(p unsafe.Pointer)                {}
func refreshFramebufferRows(p unsafe.Pointer, y0, y1 int) {}
func displayVsync() bool                                 { return false }
