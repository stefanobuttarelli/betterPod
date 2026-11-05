package main

// fadvise.go — posix_fadvise on the whole file for Knulli's arm64 build
// and native amd64 tests. The app is built from a file list, so the syscall
// is selected at run time rather than through platform build constraints.

import (
	"runtime"
	"syscall"
)

func fadvise(fd uintptr, advice int) {
	switch runtime.GOARCH {
	case "arm64":
		syscall.Syscall6(223, fd, 0, 0, uintptr(advice), 0, 0)
	case "amd64":
		syscall.Syscall6(221, fd, 0, 0, uintptr(advice), 0, 0)
	}
}
