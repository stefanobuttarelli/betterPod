// onion.c — Miyoo Mini (Plus, v4, Flip) hardware under Onion OS, for
// betterPod on the Miyoo. Included by main.go before main.c and audio.c (one C file for
// the whole app, see main.go), so main.c and audio.c can test onion_mode.
//
// - POWER and the volume keys of the Mini Plus are read straight from
//   /dev/input/event0: the SDL2 driver for the Miyoo (steward-fu) reads the
//   same device without grabbing it and has no keycodes for them. They
//   reach Go through the same queue as every other key (main.c), as LOCK
//   (power) and VOL_UP / VOL_DOWN (keys.go).
// - The volume is MI_AO's hardware volume, set exactly as Onion's keymon
//   does it (src/common/system/volume.h in the Onion repository).
// - MI_AO_SendFrame is wrapped so the SDL2 audio driver waits for room in
//   MI_AO's queue instead of dropping buffers (see below).

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/ioctl.h>
#include <linux/input.h>
#include <SDL.h>

void c_log(const char *msg);
static void push_key(int code, int pressed);

// 1 on Onion: set from Go (onion.go) before init() and audio_init()
static int onion_mode;

void onion_set_mode(int on) { onion_mode = on; }

// Keycodes pushed to Go: LOCK is SDLK_APPLICATION (the Knulli function key,
// same handling), the volume keys are made up like the other extra keys of
// main.c (SDLK_SCANCODE_MASK | 0x1000 | n).
#define ONION_KEY_LOCK      SDLK_APPLICATION
#define ONION_KEY_VOL_UP    (SDLK_SCANCODE_MASK | 0x1000 | 0x20)
#define ONION_KEY_VOL_DOWN  (SDLK_SCANCODE_MASK | 0x1000 | 0x21)

static int onion_input_fd = -2; // -2: not opened yet

// onion_input_poll reads the keys SDL doesn't report (called by pollEvents).
static void onion_input_poll(void) {
    struct input_event ev[16];
    ssize_t r;
    int n, j;
    if (!onion_mode) return;
    if (onion_input_fd == -2) {
        // MIYOOPOD_ONION_INPUT: another device or a FIFO of input_events
        // (tools/onion/smoketest.sh presses POWER and the volume keys so)
        const char *dev = getenv("MIYOOPOD_ONION_INPUT");
        onion_input_fd = open(dev && dev[0] ? dev : "/dev/input/event0", O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        c_log(onion_input_fd >= 0 ? "onion: POWER and volume keys from /dev/input/event0"
                                  : "onion: can't open /dev/input/event0 (no POWER / volume keys)");
    }
    if (onion_input_fd < 0) return;
    while ((r = read(onion_input_fd, ev, sizeof(ev))) > 0) {
        n = r / sizeof(struct input_event);
        for (j = 0; j < n; j++) {
            if (ev[j].type != EV_KEY) continue;
            switch (ev[j].code) {
            case KEY_POWER:
                // press and release only: the app times the hold itself
                if (ev[j].value == 1 || ev[j].value == 0) push_key(ONION_KEY_LOCK, ev[j].value);
                break;
            case KEY_VOLUMEUP:
            case KEY_VOLUMEDOWN: {
                int k = ev[j].code == KEY_VOLUMEUP ? ONION_KEY_VOL_UP : ONION_KEY_VOL_DOWN;
                // the kernel's auto-repeat (value 2) steps again, like a press
                push_key(k, ev[j].value != 0);
                break;
            }
            }
        }
    }
}

// onion_drop_sdl_key: keys that come from onion_input_poll, or that the
// SDL driver reports as SDLK_UNKNOWN, are not taken from SDL a second time.
static int onion_drop_sdl_key(int sym) {
    if (!onion_mode) return 0;
    switch (sym) {
    case SDLK_UNKNOWN:
    case SDLK_POWER:
    case SDLK_VOLUMEUP:
    case SDLK_VOLUMEDOWN:
    case SDLK_MUTE:
        return 1;
    }
    return 0;
}

// --- MI_AO_SendFrame -----------------------------------------------------
// The Miyoo SDL2 audio driver (MMIYOO_PlayDevice) sends each buffer to MI_AO
// with a 1 ms timeout, then sleeps for the buffer's length minus 10 ms. That
// never follows the DAC's clock: when MI_AO's queue (6 buffers) is full the
// buffer is dropped, and when the thread wakes late the queue runs dry —
// clicks and crackles either way. Defined in the executable (and exported,
// see tools/onion/build.sh), this MI_AO_SendFrame is the one libSDL2.so
// calls: it waits up to MIYOOPOD_AO_TIMEOUT ms (default 1000) for room, so
// the queue paces the driver and stays full. MIYOOPOD_AO_TIMEOUT=0 keeps
// the driver's own timeout.
#if defined(__arm__)
typedef int (*onion_ao_send_fn)(int, int, void *, int);
static volatile int onion_ao_sent, onion_ao_failed;

int MI_AO_SendFrame(int dev, int chn, void *frame, int ms) {
    static onion_ao_send_fn real;
    static int timeout = -2;
    int r;
    if (!real) real = (onion_ao_send_fn)dlsym(RTLD_NEXT, "MI_AO_SendFrame");
    if (!real) return -1;
    if (timeout == -2) {
        const char *e = getenv("MIYOOPOD_AO_TIMEOUT");
        timeout = e && e[0] ? atoi(e) : 1000;
    }
    if (onion_mode && timeout > 0 && ms >= 0 && ms < timeout) ms = timeout;
    r = real(dev, chn, frame, ms);
    if (r == 0) onion_ao_sent++;
    else onion_ao_failed++;
    return r;
}

// onion_ao_stats: buffers MI_AO took, and (in *failed) the ones it refused.
int onion_ao_stats(int *failed) {
    *failed = onion_ao_failed;
    return onion_ao_sent;
}
#else
int onion_ao_stats(int *failed) {
    *failed = 0;
    return 0;
}
#endif

// --- MI_AO volume --------------------------------------------------------
#define MI_AO_SETVOLUME 0x4008690b
#define MI_AO_GETVOLUME 0xc008690c
#define MI_AO_SETMUTE   0x4008690d
#define ONION_RAW_MIN   -60
#define ONION_RAW_MAX   30

// onion_ao_get_raw returns MI_AO's volume in dB (-60 = mute), or -1000 when
// /dev/mi_ao can't be opened.
int onion_ao_get_raw(void) {
    int fd = open("/dev/mi_ao", O_RDWR | O_CLOEXEC);
    int buf2[] = {0, 0};
    uint64_t buf1[] = {sizeof(buf2), (uintptr_t)buf2};
    if (fd < 0) return -1000;
    ioctl(fd, MI_AO_GETVOLUME, buf1);
    close(fd);
    return buf2[1];
}

// onion_ao_set_raw sets MI_AO's volume in dB, muting at the bottom of the
// range like keymon. It returns the value set, or -1000.
int onion_ao_set_raw(int value) {
    int fd = open("/dev/mi_ao", O_RDWR | O_CLOEXEC);
    int buf2[] = {0, 0};
    uint64_t buf1[] = {sizeof(buf2), (uintptr_t)buf2};
    int prev;
    if (fd < 0) return -1000;
    if (value > ONION_RAW_MAX) value = ONION_RAW_MAX;
    if (value < ONION_RAW_MIN) value = ONION_RAW_MIN;
    ioctl(fd, MI_AO_GETVOLUME, buf1);
    prev = buf2[1];
    if (value != prev) {
        buf2[1] = value;
        ioctl(fd, MI_AO_SETVOLUME, buf1);
        if (prev <= ONION_RAW_MIN && value > ONION_RAW_MIN) {
            buf2[1] = 0;
            ioctl(fd, MI_AO_SETMUTE, buf1);
        } else if (prev > ONION_RAW_MIN && value <= ONION_RAW_MIN) {
            buf2[1] = 1;
            ioctl(fd, MI_AO_SETMUTE, buf1);
        }
    }
    close(fd);
    return value;
}
