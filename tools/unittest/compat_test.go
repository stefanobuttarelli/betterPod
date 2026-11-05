//go:build ignore

package main

import (
	"os"
	"testing"
)

func TestLegacyKnulliEnvironment(t *testing.T) {
	const current = "BETTERPOD_COMPAT_TEST"
	const legacy = "MIYOOPOD_COMPAT_TEST"
	old, exists := os.LookupEnv(current)
	os.Unsetenv(current)
	t.Cleanup(func() {
		if exists {
			os.Setenv(current, old)
		} else {
			os.Unsetenv(current)
		}
	})
	t.Setenv(legacy, "legacy")
	if got := envGet(current); got != "legacy" {
		t.Fatalf("old launcher value lost: %q", got)
	}
	t.Setenv(current, "current")
	if got := envGet(current); got != "current" {
		t.Fatalf("new prefix must win: %q", got)
	}
	t.Setenv(current, "")
	if got := envGet(current); got != "" {
		t.Fatalf("explicit empty value must win: %q", got)
	}
}
