#!/bin/sh
# mac/build.sh — betterPod Share for the Mac, on a Mac (Xcode's command
# line tools, Go 1.22+): the Go engine as a C archive per architecture, the
# Swift app linked with it, one universal binary, the .app (signed ad hoc:
# not notarized yet) and its zip:
#   mac/build/betterPod Share.app
#   mac/build/betterPod-Share-mac-v<version>.zip
set -e
cd "$(dirname "$0")/.."
V=$(cat mac/VERSION)
OUT=mac/build
rm -rf "$OUT"
mkdir -p "$OUT"
MIN=13.0
for arch in arm64 amd64; do
	sarch=$arch
	[ $arch = amd64 ] && sarch=x86_64
	mkdir -p "$OUT/$arch"
	CGO_ENABLED=1 GOOS=darwin GOARCH=$arch \
		CC="clang -arch $sarch -mmacosx-version-min=$MIN" \
		go build -trimpath -ldflags=-s -buildmode=c-archive -o "$OUT/$arch/libbpshare.a" ./mac/core
	swiftc -O -target $sarch-apple-macos$MIN \
		-import-objc-header "$OUT/$arch/libbpshare.h" \
		mac/App/*.swift "$OUT/$arch/libbpshare.a" \
		-framework CoreFoundation -framework Security -lresolv \
		-o "$OUT/$arch/betterPodShare"
done
APP="$OUT/betterPod Share.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create "$OUT/arm64/betterPodShare" "$OUT/amd64/betterPodShare" -output "$APP/Contents/MacOS/betterPod Share"
sed "s/@VERSION@/$V/g" mac/App/Info.plist > "$APP/Contents/Info.plist"
cp mac/App/Resources/share_*.png "$APP/Contents/Resources/"
# the icon, at every size
IS="$OUT/AppIcon.iconset"
mkdir -p "$IS"
for s in 16 32 128 256 512; do
	sips -z $s $s mac/App/Resources/AppIcon.png --out "$IS/icon_${s}x${s}.png" >/dev/null
	d=$((s * 2))
	sips -z $d $d mac/App/Resources/AppIcon.png --out "$IS/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$IS" -o "$APP/Contents/Resources/AppIcon.icns"
codesign --force --deep --sign - "$APP"
codesign --verify --verbose "$APP"
(cd "$OUT" && ditto -c -k --keepParent "betterPod Share.app" "betterPod-Share-mac-v$V.zip")
ls -l "$OUT"/*.zip
