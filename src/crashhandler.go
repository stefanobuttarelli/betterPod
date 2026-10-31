package main

import (
	"fmt"
	"os"
	"os/signal"
	"runtime"
	"syscall"
)

// installCrashHandler sets up signal handlers for fatal signals (SIGSEGV, SIGABRT, SIGBUS).
// When a C-level crash occurs, Go's signal handler fires. We write the stack trace
// to the log, then re-raise the signal to get default behavior.
func installCrashHandler() {
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGSEGV, syscall.SIGABRT, syscall.SIGBUS)

	go func() {
		sig := <-sigChan

		// Capture stack trace of all goroutines
		buf := make([]byte, 16384)
		n := runtime.Stack(buf, true) // true = all goroutines
		stackTrace := string(buf[:n])

		crashMsg := fmt.Sprintf("Fatal signal: %v", sig)

		// Write to log file immediately (most reliable)
		if logFile != nil {
			logFile.WriteString(fmt.Sprintf("\n\nFATAL CRASH: %s\n%s\n", crashMsg, stackTrace))
			logFile.Sync()
		}

		// Re-raise the signal with default handler to get normal crash behavior
		signal.Reset(sig.(syscall.Signal))
		syscall.Kill(syscall.Getpid(), sig.(syscall.Signal))
	}()
}
