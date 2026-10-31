package main

// The telemetry (PostHog) was removed before 1.0: betterPod sends nothing
// about its use. These names are still called from player.go; they do
// nothing.

func TrackSongPlayed(track *Track) {}

func TrackAction(action string, properties map[string]interface{}) {}
