package main

/*
#include <stdlib.h>
*/
import "C"
import (
	"fmt"
	"os"
	"time"
)

const LOG_PATH = "./betterpod.log"

var logFile *os.File
var globalApp *BetterPod // Global reference for checking LocalLogsEnabled

func init() {
	var err error
	// the log is appended to on every start: past 1 MB the previous one is
	// kept as .old, so it can't fill the SD card
	if st, err := os.Stat(LOG_PATH); err == nil && st.Size() > 1<<20 {
		os.Rename(LOG_PATH, LOG_PATH+".old")
	}
	logFile, err = os.OpenFile(LOG_PATH, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		panic(err)
	}
}

// logMsg writes to the local log file (when LocalLogsEnabled).
func logMsg(message string) {
	if globalApp != nil && globalApp.LocalLogsEnabled {
		logFile.WriteString(time.Now().Format("2006-01-02 15:04:05.999") + " - " + message + "\n")
	}
}

//export GoLogMsg
func GoLogMsg(cMsg *C.char) {
	msg := C.GoString(cMsg)
	logMsg(msg)
}

//export DetectDevice
func DetectDevice(width, height C.int) {
	if globalApp == nil {
		return
	}

	w := int(width)
	h := int(height)

	globalApp.DisplayWidth = w
	globalApp.DisplayHeight = h

	globalApp.DeviceModel = "anbernic-h700"

	logMsg(fmt.Sprintf("INFO: Device detected: %s (%dx%d)", globalApp.DeviceModel, w, h))
}
