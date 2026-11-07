//go:build darwin

package main

/*
#include <copyfile.h>
#include <errno.h>
#include <stdlib.h>

static int bpCopyFileMetadata(const char *source, const char *destination) {
    if (copyfile(source, destination, NULL,
        COPYFILE_ACL | COPYFILE_XATTR | COPYFILE_NOFOLLOW) == 0) return 0;
    return errno;
}
*/
import "C"

import (
	"fmt"
	"syscall"
	"unsafe"
)

// Copy ACLs and extended attributes, including Finder tags and resource forks.
// Keep the staged file's new timestamps and mode; do not follow symbolic links.
func preserveLibraryFileMetadata(source, destination string) error {
	src, dst := C.CString(source), C.CString(destination)
	defer C.free(unsafe.Pointer(src))
	defer C.free(unsafe.Pointer(dst))
	if code := C.bpCopyFileMetadata(src, dst); code != 0 {
		return fmt.Errorf("Could not preserve the file's Finder metadata: %w", syscall.Errno(code))
	}
	return nil
}
