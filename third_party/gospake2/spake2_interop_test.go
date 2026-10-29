package gospake2

import (
	"encoding/hex"
	"os"
	"os/exec"
	"strings"
	"testing"
)

// TestPythonInterop runs python-spake2 (when installed) on the other side.
func TestPythonInterop(t *testing.T) {
	if _, err := exec.LookPath("python3"); err != nil {
		t.Skip("no python3")
	}
	if exec.Command("python3", "-c", "import spake2").Run() != nil {
		t.Skip("no python spake2")
	}
	for _, pw := range []string{"4-82100937", "7-guitarist-revenge", ""} {
		id := "lothar.com/wormhole/text-or-file-xfer"
		s := SPAKE2Symmetric(NewPassword(pw), NewIdentityS(id))
		m := s.Start()
		py := `import sys,binascii
from spake2 import SPAKE2_Symmetric
s=SPAKE2_Symmetric(sys.argv[1].encode(),idSymmetric=sys.argv[2].encode())
m=s.start(); k=s.finish(binascii.unhexlify(sys.argv[3]))
print(m.hex(), k.hex())`
		cmd := exec.Command("python3", "-c", py, pw, id, hex.EncodeToString(m))
		cmd.Dir = os.TempDir()
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("python: %v %s", err, out)
		}
		f := strings.Fields(string(out))
		pm, _ := hex.DecodeString(f[0])
		key, err := s.Finish(pm)
		if err != nil {
			t.Fatal(err)
		}
		if hex.EncodeToString(key) != f[1] {
			t.Fatalf("pw %q: keys differ", pw)
		}
	}
	// a wrong password gives another key
	a := SPAKE2Symmetric(NewPassword("1-2"), NewIdentityS("x"))
	b := SPAKE2Symmetric(NewPassword("1-3"), NewIdentityS("x"))
	ma, mb := a.Start(), b.Start()
	ka, _ := a.Finish(mb)
	kb, _ := b.Finish(ma)
	if hex.EncodeToString(ka) == hex.EncodeToString(kb) {
		t.Fatal("different passwords, same key")
	}
}
