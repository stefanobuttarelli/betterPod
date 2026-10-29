// Package gospake2 is a small SPAKE2 (symmetric mode, Ed25519 group) with
// the API wormhole-william expects, written for betterPod because the
// original package's home (salsa.debian.org) can't be reached from where
// betterPod is built. It follows python-spake2 (Brian Warner, MIT) byte for
// byte, so it talks to every magic-wormhole client:
//
//   - the password scalar is HKDF-SHA256(pw, salt "", info "SPAKE2 pw"),
//     48 bytes read big-endian, mod L;
//   - S is python-spake2's arbitrary_element(b"symmetric");
//   - the message is "S" + (x·B + pw·S); K = x·(Y − pw·S);
//   - the key is SHA256(SHA256(pw) ‖ SHA256(id) ‖ sorted(msg1, msg2) ‖ K).
package gospake2

import (
	"bytes"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"io"
	"math/big"

	"filippo.io/edwards25519"
	"golang.org/x/crypto/hkdf"
)

type Password []byte
type IdentityS []byte

func NewPassword(pw string) Password    { return Password(pw) }
func NewIdentityS(id string) IdentityS { return IdentityS(id) }

// SPAKE2 is one side of a symmetric exchange.
type SPAKE2 struct {
	pw       Password
	id       IdentityS
	pwScalar *edwards25519.Scalar
	x        *edwards25519.Scalar
	outbound []byte // the element (without the side byte)
	started  bool
	finished bool
}

func SPAKE2Symmetric(pw Password, id IdentityS) SPAKE2 {
	return SPAKE2{pw: pw, id: id, pwScalar: passwordToScalar(pw)}
}

var order, _ = new(big.Int).SetString("7237005577332262213973186563042994240857116359379907606001950938285454250989", 10)

// scalarFromBig: n mod L as a Scalar.
func scalarFromBig(n *big.Int) *edwards25519.Scalar {
	m := new(big.Int).Mod(n, order)
	be := m.FillBytes(make([]byte, 32))
	le := make([]byte, 32)
	for i := range be {
		le[i] = be[31-i]
	}
	s, err := edwards25519.NewScalar().SetCanonicalBytes(le)
	if err != nil {
		panic(err)
	}
	return s
}

func passwordToScalar(pw []byte) *edwards25519.Scalar {
	r := hkdf.New(sha256.New, pw, []byte{}, []byte("SPAKE2 pw"))
	buf := make([]byte, 32+16)
	if _, err := io.ReadFull(r, buf); err != nil {
		panic(err)
	}
	return scalarFromBig(new(big.Int).SetBytes(buf))
}

// sElem: arbitrary_element(b"symmetric") of python-spake2.
var sElem = func() *edwards25519.Point {
	b, _ := hex.DecodeString("6f00dae87c1be1a73b5922ef431cd8f57879569c222d22b1cd71e8546ab8e6f1")
	p, err := new(edwards25519.Point).SetBytes(b)
	if err != nil {
		panic(err)
	}
	return p
}()

// Start: this side's message ("S" + the element).
func (s *SPAKE2) Start() []byte {
	if s.started {
		panic("gospake2: Start called twice")
	}
	s.started = true
	buf := make([]byte, 64)
	if _, err := rand.Read(buf); err != nil {
		panic(err)
	}
	// python: int(hexlify(64 random bytes)) % L, the same distribution
	s.x = scalarFromBig(new(big.Int).SetBytes(buf))
	xb := new(edwards25519.Point).ScalarBaseMult(s.x)
	blind := new(edwards25519.Point).ScalarMult(s.pwScalar, sElem)
	s.outbound = new(edwards25519.Point).Add(xb, blind).Bytes()
	return append([]byte("S"), s.outbound...)
}

var (
	ErrOffSides   = errors.New("gospake2: message from the wrong side")
	ErrReflection = errors.New("gospake2: our own message reflected back")
	ErrBadElement = errors.New("gospake2: bad element")
)

// Finish: the shared key from the other side's message.
func (s *SPAKE2) Finish(msg []byte) ([]byte, error) {
	if !s.started || s.finished {
		return nil, errors.New("gospake2: Finish out of order")
	}
	s.finished = true
	if len(msg) != 33 || msg[0] != 'S' {
		return nil, ErrOffSides
	}
	in := msg[1:]
	y, err := new(edwards25519.Point).SetBytes(in)
	if err != nil {
		return nil, ErrBadElement
	}
	// only elements of the prime-order subgroup (as python-spake2)
	if y.Equal(edwards25519.NewIdentityPoint()) == 1 {
		return nil, ErrBadElement
	}
	lMinus1 := scalarFromBig(new(big.Int).Sub(order, big.NewInt(1)))
	t := new(edwards25519.Point).ScalarMult(lMinus1, y) // (L−1)·Y = −Y in the subgroup
	if t.Add(t, y).Equal(edwards25519.NewIdentityPoint()) != 1 {
		return nil, ErrBadElement
	}
	if bytes.Equal(in, s.outbound) {
		return nil, ErrReflection
	}
	neg := edwards25519.NewScalar().Negate(s.pwScalar)
	unblind := new(edwards25519.Point).ScalarMult(neg, sElem)
	k := new(edwards25519.Point).ScalarMult(s.x, new(edwards25519.Point).Add(y, unblind))
	first, second := in, s.outbound
	if bytes.Compare(first, second) > 0 {
		first, second = second, first
	}
	h := func(b []byte) []byte { d := sha256.Sum256(b); return d[:] }
	var tr []byte
	tr = append(tr, h(s.pw)...)
	tr = append(tr, h(s.id)...)
	tr = append(tr, first...)
	tr = append(tr, second...)
	tr = append(tr, k.Bytes()...)
	key := sha256.Sum256(tr)
	return key[:], nil
}
