//go:build !darwin

package main

// The companion app runs on macOS. Keep the container safety tests portable.
func preserveLibraryFileMetadata(source, destination string) error { return nil }
