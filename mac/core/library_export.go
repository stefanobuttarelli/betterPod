package main

/*
#include <stdlib.h>
*/
import "C"

import "encoding/json"

//export BPLibraryRead
func BPLibraryRead(paths *C.char) *C.char {
	var input []string
	if err := json.Unmarshal([]byte(C.GoString(paths)), &input); err != nil {
		return cJSON(LibraryReadResult{Error: err.Error()})
	}
	return cJSON(readLibrary(input))
}

//export BPLibrarySave
func BPLibrarySave(request *C.char) *C.char {
	var input LibrarySaveRequest
	if err := json.Unmarshal([]byte(C.GoString(request)), &input); err != nil {
		return cJSON(LibrarySaveResult{Error: err.Error()})
	}
	return cJSON(saveLibrary(input))
}
