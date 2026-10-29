module salsa.debian.org/vasudev/gospake2

go 1.21

require (
	filippo.io/edwards25519 v1.1.0
	golang.org/x/crypto v0.31.0
)

replace filippo.io/edwards25519 => github.com/FiloSottile/edwards25519 v1.1.0

replace golang.org/x/crypto => github.com/golang/crypto v0.31.0

replace golang.org/x/sys => github.com/golang/sys v0.28.0
