//go:build darwin

package main

import (
	"bytes"
	"os/exec"
	"strings"
	"testing"

	"golang.org/x/sys/unix"
)

func TestLibraryPreservesFinderMetadataAndACL(t *testing.T) {
	path, _ := libraryFixture(t, ".mp3")
	name, value := "com.betterpod.metadata-test", []byte("keep this Finder metadata")
	if err := unix.Setxattr(path, name, value, 0); err != nil {
		t.Fatal(err)
	}
	if out, err := exec.Command("/bin/chmod", "+a", "everyone allow read,readattr,readextattr,readsecurity", path).CombinedOutput(); err != nil {
		t.Fatalf("ACL setup: %v %s", err, out)
	}
	acl := func() string {
		out, err := exec.Command("/bin/ls", "-le", path).Output()
		if err != nil {
			t.Fatal(err)
		}
		_, rest, _ := strings.Cut(string(out), "\n")
		return rest
	}
	beforeACL := acl()
	c := changeFor(t, path)
	title := "With preserved metadata"
	c.Fields.Title = &title
	if err := saveLibraryTrack(c); err != nil {
		t.Fatal(err)
	}
	got := make([]byte, 100)
	n, err := unix.Getxattr(path, name, got)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got[:n], value) {
		t.Fatal("extended attributes changed")
	}
	if afterACL := acl(); beforeACL != afterACL {
		t.Fatalf("ACL changed: before %q after %q", beforeACL, afterACL)
	}
}
