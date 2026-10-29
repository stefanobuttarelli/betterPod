package main

// fadvise.go — posix_fadvise on the whole file, for 64-bit and 32-bit ARM
// alike. The binary is built from a list of files, which skips build
// constraints, so the syscall is picked at run time: on 32-bit ARM (the
// Miyoo) it is arm_fadvise64_64, with the advice second, before the
// 64-bit offset and length (0 and 0: the whole file).

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
	case "arm":
		syscall.Syscall6(270, fd, uintptr(advice), 0, 0, 0, 0)
	}
}
