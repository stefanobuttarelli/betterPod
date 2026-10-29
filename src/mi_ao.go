package main

// setMiAOVolume sets the Miyoo's hardware volume (MI_AO) to percent, with
// Onion's curve (the ioctl is in onion.c, as keymon does it).
func setMiAOVolume(percent int) {
	if isKnulli {
		// no /dev/mi_ao: the volume is the system's (see knulli.go)
		return
	}
	onionSetVolume(float64(clamp(percent, 0, 100)) / 100)
}
