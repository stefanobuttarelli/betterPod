module github.com/danfragoso/miyoopod

go 1.22.2

require (
	github.com/dhowden/tag v0.0.0-20240417053706-3d75831295e8
	github.com/fogleman/gg v1.3.0
	github.com/google/uuid v1.6.0
	github.com/skip2/go-qrcode v0.0.0-20200617195104-da1b6568686e
	golang.org/x/image v0.15.0
)

require (
	filippo.io/edwards25519 v1.1.0 // indirect
	github.com/klauspost/compress v1.17.11 // indirect
	golang.org/x/crypto v0.31.0 // indirect
	golang.org/x/sys v0.28.0 // indirect
	nhooyr.io/websocket v1.8.17 // indirect
	salsa.debian.org/vasudev/gospake2 v0.0.0-20210510093858-d91629950ad1 // indirect
)

require (
	github.com/golang/freetype v0.0.0-20170609003504-e2365dfdc4a0
	github.com/psanford/wormhole-william v1.0.8
	golang.org/x/text v0.21.0
)

replace golang.org/x/image => github.com/golang/image v0.15.0

replace golang.org/x/text => github.com/golang/text v0.14.0

replace github.com/psanford/wormhole-william => ./third_party/wormhole-william

replace salsa.debian.org/vasudev/gospake2 => ./third_party/gospake2

replace filippo.io/edwards25519 => github.com/FiloSottile/edwards25519 v1.1.0

replace golang.org/x/crypto => github.com/golang/crypto v0.31.0

replace golang.org/x/sys => github.com/golang/sys v0.28.0

replace nhooyr.io/websocket => github.com/coder/websocket v1.8.11
