package main

type Key int

// SDL2 keycodes as mapped by the steward-fu Miyoo Mini SDL2 driver
// (SDL_event_mini.c maps Linux input keycodes → SDL keycodes)
const (
	NONE Key = -1

	// D-pad (SDLK_UP/DOWN/LEFT/RIGHT = SDL_SCANCODE | 1<<30)
	UP    Key = 1073741906 // SDLK_UP
	DOWN  Key = 1073741905 // SDLK_DOWN
	LEFT  Key = 1073741904 // SDLK_LEFT
	RIGHT Key = 1073741903 // SDLK_RIGHT

	// Face buttons
	A Key = 32         // SDLK_SPACE  (KEY_SPACE)
	B Key = 1073742048 // SDLK_LCTRL (KEY_LEFTCTRL)
	X Key = 1073742049 // SDLK_LSHIFT (KEY_LEFTSHIFT)
	Y Key = 1073742050 // SDLK_LALT (KEY_LEFTALT)

	// Shoulder buttons
	L  Key = 101 // SDLK_e (KEY_E)
	L2 Key = 9   // SDLK_TAB (KEY_TAB)
	R  Key = 116 // SDLK_t (KEY_T)
	R2 Key = 8   // SDLK_BACKSPACE (KEY_BACKSPACE)

	// System buttons
	SELECT Key = 1073742052 // SDLK_RCTRL (KEY_RIGHTCTRL)
	START  Key = 13         // SDLK_RETURN (KEY_ENTER)
	MENU   Key = 27         // SDLK_ESCAPE (KEY_ESC)
	POWER  Key = 0          // SDLK_UNKNOWN / SDLK_FIRST (Onion OS maps KEY_POWER to SDLK_FIRST which is 0)
)

// Knulli (Anbernic H700) extra keys, made up by main.c (SDL has no keycodes
// for them): the lock switch, media keys from Bluetooth / USB headsets and
// headphone unplug, and the analog stick used as a click wheel.
const (
	LOCK            Key = 0x40000065 // SDLK_APPLICATION (Function / Home)
	WHEEL_CW        Key = 0x40001001
	WHEEL_CCW       Key = 0x40001002
	MEDIA_PLAYPAUSE Key = 0x4000100A
	MEDIA_PLAY      Key = 0x4000100B
	MEDIA_PAUSE     Key = 0x4000100C
	MEDIA_NEXT      Key = 0x4000100D
	MEDIA_PREV      Key = 0x4000100E
	MEDIA_STOP      Key = 0x4000100F
	HP_UNPLUG       Key = 0x40001010
	MEDIA_HOOK      Key = 0x40001011 // a wired headset's one button: 1, 2 or 3 clicks
	MEDIA_VOLUP     Key = 0x40001012
	MEDIA_VOLDN     Key = 0x40001013
)

// Onion (Miyoo Mini) keys read by onion.c from /dev/input/event0: the
// volume keys of the Mini Plus. POWER comes as LOCK.
const (
	VOL_UP   Key = 0x40001020
	VOL_DOWN Key = 0x40001021
)
