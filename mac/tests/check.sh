#!/bin/sh
# Build and check the Mac library feature without packaging or publishing it.
set -eu
cd "$(dirname "$0")/../.."

command -v go >/dev/null
command -v swiftc >/dev/null
check_dir=$(mktemp -d "${TMPDIR:-/tmp}/betterpod-mac-check.XXXXXX")
trap 'rm -rf "$check_dir"' 0
trap 'exit 1' HUP INT TERM

go test ./mac/core
native_arch=$(uname -m)
CGO_ENABLED=1 CC="clang -arch $native_arch -mmacosx-version-min=13.0" \
    go build -buildmode=c-archive -o "$check_dir/libbpshare.a" ./mac/core

swift_target="$native_arch-apple-macos13.0"
swiftc -target "$swift_target" -module-cache-path "$check_dir/swift-module-cache" \
    mac/App/AlbumLookup.swift mac/tests/AlbumLookupTests.swift \
    -o "$check_dir/album-lookup-tests"
"$check_dir/album-lookup-tests"

swiftc -target "$swift_target" -module-cache-path "$check_dir/swift-module-cache" \
    mac/App/AlbumLookup.swift mac/App/MagicLibrary.swift mac/tests/MagicLibraryTests.swift \
    -o "$check_dir/magic-library-tests"
"$check_dir/magic-library-tests"

swiftc -target "$swift_target" -module-cache-path "$check_dir/swift-module-cache" \
    -import-objc-header "$check_dir/libbpshare.h" \
    mac/App/AlbumLookup.swift mac/App/LibraryEngine.swift mac/App/LibraryModel.swift mac/App/LanEngine.swift \
    mac/tests/LibraryModelTests.swift "$check_dir/libbpshare.a" \
    -framework CoreFoundation -framework Security -lresolv \
    -o "$check_dir/library-model-tests"
"$check_dir/library-model-tests"

printf 'Mac library checks passed.\n'
