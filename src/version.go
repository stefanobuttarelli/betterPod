package main

import (
	"encoding/json"
	"fmt"
	"io"
	"strconv"
	"strings"
	"time"
)

const VERSION_CHECK_URL = "https://github.com/danfragoso/miyoopod/raw/refs/heads/main/version.json"

type VersionInfo struct {
	Version   string `json:"version"`
	Checksum  string `json:"checksum"`
	URL       string `json:"url"`
	Size      int64  `json:"size"`
	Changelog string `json:"changelog"`
}

// checkVersion fetches the latest version from GitHub and compares with current version
func (app *MiyooPod) checkVersion() string {
	if otaDisabled {
		return "Updates: Settings → Software Update"
	}
	client := getInsecureHTTPClient(5 * time.Second)

	resp, err := client.Get(VERSION_CHECK_URL)
	if err != nil {
		logMsg(fmt.Sprintf("WARNING: Failed to fetch version: %v", err))
		return "Failed to fetch version"
	}
	defer resp.Body.Close()

	if resp.StatusCode != 200 {
		logMsg(fmt.Sprintf("WARNING: Version check returned status: %d", resp.StatusCode))
		return "Failed to fetch version"
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		logMsg(fmt.Sprintf("WARNING: Failed to read version response: %v", err))
		return "Failed to fetch version"
	}

	var remoteVersion VersionInfo
	if err := json.Unmarshal(body, &remoteVersion); err != nil {
		logMsg(fmt.Sprintf("WARNING: Failed to parse version JSON: %v", err))
		return "Failed to fetch version"
	}

	// Compare versions — only prompt if remote is strictly newer
	if !isNewerVersion(APP_VERSION, remoteVersion.Version) {
		logMsg(fmt.Sprintf("INFO: Version check: Up to date (%s, remote: %s)", APP_VERSION, remoteVersion.Version))
		app.UpdateAvailable = false
		return "Up to date"
	}

	logMsg(fmt.Sprintf("INFO: Version check: Update available (current: %s, latest: %s)", APP_VERSION, remoteVersion.Version))
	app.UpdateAvailable = true
	app.UpdateInfo = &remoteVersion
	return fmt.Sprintf("Update available: v%s", remoteVersion.Version)
}

// isNewerVersion returns true if remote is strictly newer than current.
// Versions are dot-separated numbers ("0.91.1", "1.0"), and a beta adds
// "-beta.N" ("1.1-beta.2"): a beta comes before its version ("1.1-beta.2" <
// "1.1") and after the versions before it ("1.0.3" < "1.1-beta.1").
func isNewerVersion(current, remote string) bool {
	return compareVersions(remote, current) > 0
}

// isBetaVersion: a beta's version ("1.1-beta.2").
func isBetaVersion(v string) bool { return strings.Contains(v, "-") }

// compareVersions: -1, 0 or 1 as a is older than, the same as or newer
// than b.
func compareVersions(a, b string) int {
	parse := func(v string) (nums []int, beta int) {
		base, pre, isPre := strings.Cut(strings.TrimSpace(v), "-")
		for _, p := range strings.Split(base, ".") {
			n, _ := strconv.Atoi(p)
			nums = append(nums, n)
		}
		beta = -1 // a release: after all its betas
		if isPre {
			beta = 0
			if i := strings.LastIndexByte(pre, '.'); i >= 0 {
				beta, _ = strconv.Atoi(pre[i+1:])
			}
		}
		return nums, beta
	}
	an, ab := parse(a)
	bn, bb := parse(b)
	for len(an) < len(bn) {
		an = append(an, 0)
	}
	for len(bn) < len(an) {
		bn = append(bn, 0)
	}
	for i := range an {
		if an[i] != bn[i] {
			if an[i] > bn[i] {
				return 1
			}
			return -1
		}
	}
	rank := func(beta int) int { // a release ranks above any beta
		if beta < 0 {
			return 1 << 30
		}
		return beta
	}
	switch ra, rb := rank(ab), rank(bb); {
	case ra > rb:
		return 1
	case ra < rb:
		return -1
	}
	return 0
}
