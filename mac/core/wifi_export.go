package main

/*
#include <stdlib.h>
*/
import "C"

import "encoding/json"

//export BPLanDiscover
func BPLanDiscover() *C.char {
	return cJSON(lanDiscover())
}

//export BPLanConnect
func BPLanConnect(request *C.char) *C.char {
	var input LanConnectRequest
	if err := json.Unmarshal([]byte(C.GoString(request)), &input); err != nil {
		return cJSON(LanConnectResult{Error: "The Wi-Fi connection request is unreadable"})
	}
	return cJSON(lanConnectRegistered(input))
}

//export BPLanRequest
func BPLanRequest(id C.int, request *C.char) *C.char {
	reply, err := lanRequest(int(id), []byte(C.GoString(request)))
	if err != nil {
		return cJSON(map[string]string{"error": err.Error()})
	}
	return C.CString(string(reply))
}

//export BPLanDisconnect
func BPLanDisconnect(id C.int) {
	lanDisconnect(int(id))
}
