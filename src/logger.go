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

const LOG_PATH = "./miyoopod.log"

var logFile *os.File
var globalApp *MiyooPod // Global reference for checking LocalLogsEnabled

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

	// Determine device model based on resolution
	if w == 640 && h == 480 {
		globalApp.DeviceModel = "miyoo-mini-plus"
	} else if w == 750 && h == 560 {
		// Both v4 and flip have the same resolution
		// We'll label them as v4 by default, could be either
		globalApp.DeviceModel = "miyoo-mini-v4"
	} else {
		globalApp.DeviceModel = fmt.Sprintf("unknown-%dx%d", w, h)
	}

	logMsg(fmt.Sprintf("INFO: Device detected: %s (%dx%d)", globalApp.DeviceModel, w, h))
}
