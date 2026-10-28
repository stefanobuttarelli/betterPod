#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <dirent.h>
#include <sys/ioctl.h>
#include <linux/input.h>
#include <SDL.h>
extern void GoLogMsg(char* msg);
extern void DetectDevice(int width, int height);

const int RENDER_WIDTH = 640;
const int RENDER_HEIGHT = 480;

static int display_width = 640;
static int display_height = 480;

static SDL_Window *window = NULL;
static SDL_Renderer *renderer = NULL;
static SDL_Texture *texture = NULL;

// --- Logging (through Go) ---------------------------------------------------
void c_log(const char *msg) {
    char buffer[512];
    snprintf(buffer, sizeof(buffer), "[C] INFO: %s", msg);
    GoLogMsg(buffer);
}

void c_logf(const char *fmt, const char *detail) {
    char buffer[512];
    snprintf(buffer, sizeof(buffer), "[C] INFO: ");
    int offset = strlen(buffer);
    snprintf(buffer + offset, sizeof(buffer) - offset, fmt, detail);
    GoLogMsg(buffer);
}

void c_logd(const char *fmt, int val) {
    char buffer[512];
    snprintf(buffer, sizeof(buffer), "[C] INFO: ");
    int offset = strlen(buffer);
    snprintf(buffer + offset, sizeof(buffer) - offset, fmt, val);
    GoLogMsg(buffer);
}

// --- Input -------------------------------------------------------------------
//
// pollEvents() returns one key event at a time: the SDL keycode of a key
// press, -(keycode + 1) for a release, or -1 when nothing is pending. The
// game controller (buttons, sticks, triggers) and the media keys of
// headphones / Bluetooth devices (read directly from /dev/input) are turned
// into the same keycodes the keyboard would give, through a small queue.
//
// Keycodes of the controller buttons (as in MiyooPod's keyboard mapping):
//   A = space, B = left ctrl, X = left shift, Y = left alt,
//   L = 'e', R = 't', START = return, SELECT = right ctrl,
//   MENU (guide) = application, L2 = tab, R2 = backspace.
// Extra keys that no keyboard has use SDLK_SCANCODE_MASK | 0x1000 | n.
#define KP_KEY(n)        (SDLK_SCANCODE_MASK | 0x1000 | (n))
#define KEY_WHEEL_CW     KP_KEY(0x01)   // stick turned clockwise (wheel mode)
#define KEY_WHEEL_CCW    KP_KEY(0x02)   // stick turned counter-clockwise
#define KEY_MEDIA_PP     KP_KEY(0x0a)   // play/pause
#define KEY_MEDIA_PLAY   KP_KEY(0x0b)
#define KEY_MEDIA_PAUSE  KP_KEY(0x0c)
#define KEY_MEDIA_NEXT   KP_KEY(0x0d)
#define KEY_MEDIA_PREV   KP_KEY(0x0e)
#define KEY_MEDIA_STOP   KP_KEY(0x0f)
#define KEY_HP_UNPLUG    KP_KEY(0x10)   // headphones unplugged / BT gone
#define KEY_MEDIA_HOOK   KP_KEY(0x11)   // a wired headset's one button (clicks counted in Go)
#define KEY_MEDIA_VOLUP  KP_KEY(0x12)   // a wired headset's volume buttons
#define KEY_MEDIA_VOLDN  KP_KEY(0x13)
#define KEY_A            SDLK_SPACE
#define KEY_B            SDLK_LCTRL
#define KEY_X            SDLK_LSHIFT
#define KEY_Y            SDLK_LALT
#define KEY_L            SDLK_e
#define KEY_R            SDLK_t
#define KEY_L2           SDLK_TAB
#define KEY_R2           SDLK_BACKSPACE
#define KEY_START        SDLK_RETURN
#define KEY_SELECT       SDLK_RCTRL
#define KEY_MENU         SDLK_APPLICATION
//
// Event queue: SDL may give several events per poll (and a media key or a
// wheel turn becomes a press and a release), Go takes one at a time.
//
//
//
static int evq[128];
static int evq_head, evq_tail;
static int swap_ab = 1;      // Anbernic labels: the bottom button is B
static int stick_wheel = 0;  // stick as a click wheel instead of arrows

static void evq_push(int v) {
    int next = (evq_tail + 1) % 128;
    if (next == evq_head) return;
    evq[evq_tail] = v;
    evq_tail = next;
}

static int evq_pop(void) {
    if (evq_head == evq_tail) return -1;
    int v = evq[evq_head];
    evq_head = (evq_head + 1) % 128;
    return v;
}

static void push_key(int code, int pressed) {
    evq_push(pressed ? code : -code - 1);
}

// Sticks. In wheel mode the angle of the stick is followed while it is
// pushed out: every wheel_step degrees of rotation is one wheel click,
// like the click wheel of an iPod. Otherwise the stick gives the arrows.
//
//
typedef struct {
    float x, y;          // -1..1
    int engaged;         // pushed out far enough to track the angle
    float last_angle;
    float accum;         // degrees turned since the last click
} Wheel;
static Wheel wheels[2];
static float wheel_step = 15.0f;

static void wheel_update(Wheel *w) {
    float mag = sqrtf(w->x * w->x + w->y * w->y);
    if (!w->engaged) {
        if (mag >= 0.55f) {
            w->engaged = 1;
            w->last_angle = atan2f(w->y, w->x);
            w->accum = 0;
        }
        return;
    }
    // back near the centre: stop tracking
    if (mag < 0.35f) {
        w->engaged = 0;
        w->accum = 0;
        return;
    }
    // between the two thresholds the angle is too noisy to follow, but
    // the wheel stays engaged
    if (mag < 0.55f) return;
    float a = atan2f(w->y, w->x);
    float d = a - w->last_angle;
    if (d > (float)M_PI) d -= 2 * (float)M_PI;
    if (d < -(float)M_PI) d += 2 * (float)M_PI;
    w->last_angle = a;
    // a jump (stick flicked through the centre) is not a rotation
    if (fabsf(d) > 0.5f) return;
    // positive angles are clockwise on screen (y grows downwards)
    w->accum += d * 180.0f / (float)M_PI;
    while (w->accum >= wheel_step) {
        evq_push(KEY_WHEEL_CW);
        w->accum -= wheel_step;
    }
    while (w->accum <= -wheel_step) {
        evq_push(KEY_WHEEL_CCW);
        w->accum += wheel_step;
    }
}

// Stick as arrows: press when it goes past 0.55 along an axis, release
// when it comes back under 0.35 (hysteresis, no chattering).
//
static int stick_key[2];

static void stick_arrows(Wheel *w, int idx) {
    float ax = fabsf(w->x), ay = fabsf(w->y);
    int key = 0;
    if (stick_key[idx]) {
        // still held: keep it until the stick is back near the centre
        float m = (stick_key[idx] == SDLK_RIGHT || stick_key[idx] == SDLK_LEFT) ? ax : ay;
        if (m >= 0.35f) return;
    }
    if (ax >= 0.55f || ay >= 0.55f) {
        if (ax > ay) key = w->x > 0 ? SDLK_RIGHT : SDLK_LEFT;
        else key = w->y > 0 ? SDLK_DOWN : SDLK_UP;
    }
    if (key == stick_key[idx]) return;
    if (stick_key[idx]) push_key(stick_key[idx], 0);
    if (key) push_key(key, 1);
    stick_key[idx] = key;
}


static void stick_update(int idx) {
    if (stick_wheel) wheel_update(&wheels[idx]);
    else stick_arrows(&wheels[idx], idx);
}

static int trig_l2, trig_r2;

static void open_controller(int index) {
    if (SDL_IsGameController(index)) {
        SDL_GameController *gc = SDL_GameControllerOpen(index);
        if (gc) c_logf("Game controller opened: %s", SDL_GameControllerName(gc));
    } else {
        SDL_JoystickOpen(index);
        c_logd("Joystick %d opened without a controller mapping", index);
    }
}

static int button_to_key(int button) {
    switch (button) {
    case SDL_CONTROLLER_BUTTON_DPAD_UP:       return SDLK_UP;
    case SDL_CONTROLLER_BUTTON_DPAD_DOWN:     return SDLK_DOWN;
    case SDL_CONTROLLER_BUTTON_DPAD_LEFT:     return SDLK_LEFT;
    case SDL_CONTROLLER_BUTTON_DPAD_RIGHT:    return SDLK_RIGHT;
    case SDL_CONTROLLER_BUTTON_A:             return swap_ab ? KEY_B : KEY_A;
    case SDL_CONTROLLER_BUTTON_B:             return swap_ab ? KEY_A : KEY_B;
    case SDL_CONTROLLER_BUTTON_X:             return swap_ab ? KEY_Y : KEY_X;
    case SDL_CONTROLLER_BUTTON_Y:             return swap_ab ? KEY_X : KEY_Y;
    case SDL_CONTROLLER_BUTTON_LEFTSHOULDER:  return KEY_L;
    case SDL_CONTROLLER_BUTTON_RIGHTSHOULDER: return KEY_R;
    case SDL_CONTROLLER_BUTTON_BACK:          return KEY_SELECT;
    case SDL_CONTROLLER_BUTTON_START:         return KEY_START;
    case SDL_CONTROLLER_BUTTON_GUIDE:         return KEY_MENU;
    case SDL_CONTROLLER_BUTTON_LEFTSTICK:
    case SDL_CONTROLLER_BUTTON_RIGHTSTICK:    return KEY_A;
    }
    return 0;
}

static void handle_axis(int axis, int value) {
    float v = value / 32767.0f;
    switch (axis) {
    case SDL_CONTROLLER_AXIS_LEFTX:  wheels[0].x = v; stick_update(0); break;
    case SDL_CONTROLLER_AXIS_LEFTY:  wheels[0].y = v; stick_update(0); break;
    case SDL_CONTROLLER_AXIS_RIGHTX: wheels[1].x = v; stick_update(1); break;
    case SDL_CONTROLLER_AXIS_RIGHTY: wheels[1].y = v; stick_update(1); break;
    case SDL_CONTROLLER_AXIS_TRIGGERLEFT: {
        int on = value > 16000;
        if (on != trig_l2) { trig_l2 = on; push_key(KEY_L2, on); }
        break;
    }
    case SDL_CONTROLLER_AXIS_TRIGGERRIGHT: {
        int on = value > 16000;
        if (on != trig_r2) { trig_r2 = on; push_key(KEY_R2, on); }
        break;
    }
    }
}

void input_init(void) {
    const char *s = getenv("MIYOOPOD_SWAP_AB");
    if (s && s[0]) swap_ab = s[0] == '1';
    const char *sw = getenv("MIYOOPOD_STICK_WHEEL");
    stick_wheel = sw && sw[0] == '1';
    const char *st = getenv("MIYOOPOD_WHEEL_STEP");
    if (st) { float f = atof(st); if (f >= 5 && f <= 90) wheel_step = f; }
    SDL_GameControllerEventState(SDL_ENABLE);
    for (int i = 0; i < SDL_NumJoysticks(); i++) open_controller(i);
}

// Media keys. Headphone remotes and Bluetooth headsets are separate input
// devices that SDL does not see: the /dev/input/event* devices that have
// media keys (or a headphone jack switch) are opened non-blocking and read
// on every poll. The list is refreshed every 2 seconds, so a headset that
// connects later is picked up. MIYOOPOD_MEDIA_FIFO adds a named pipe that
// receives input_events too (for testing, or a helper that forwards keys
// from elsewhere). MIYOOPOD_MEDIA_KEYS=0 turns all of this off.
//
//
static struct { int fd; int bt; int jack; int logged; char name[24]; } media_dev[16];

// headphone_jack: wired headphones in the jack (1), not (0), or no jack
// device seen (-1). Read when the jack device is opened, then followed.
static int hp_jack = -1;
int headphone_jack(void) { return hp_jack; }
static int media_n;
static Uint32 media_last_scan;
static int media_disabled = -1;

// The key a media key code becomes (0 = not a media key). Fast forward and
// rewind act as next / previous.
//
// A wired headset's buttons come from the audio codec's jack device: the
// one button as KEY_MEDIA (the "hook"), KEY_PHONE or, with drivers that
// give no key types, BTN_0; the volume buttons as BTN_2/BTN_3 or
// KEY_VOLUMEUP/DOWN (those only from the jack device: the console's own
// volume keys are another device, left to Knulli).
static int media_key_for(int code, int jack) {
    switch (code) {
    case KEY_MEDIA: case KEY_PHONE: case BTN_0: case KEY_VOICECOMMAND:
        return KEY_MEDIA_HOOK;
    case BTN_2: return KEY_MEDIA_VOLUP;
    case BTN_3: return KEY_MEDIA_VOLDN;
    case KEY_VOLUMEUP:   return jack ? KEY_MEDIA_VOLUP : 0;
    case KEY_VOLUMEDOWN: return jack ? KEY_MEDIA_VOLDN : 0;
    }
    switch (code) {
    case KEY_PLAYPAUSE:                     return KEY_MEDIA_PP;
    case KEY_PLAYCD: case KEY_PLAY:         return KEY_MEDIA_PLAY;
    case KEY_PAUSECD:                       return KEY_MEDIA_PAUSE;
    case KEY_NEXTSONG: case KEY_FASTFORWARD: return KEY_MEDIA_NEXT;
    case KEY_PREVIOUSSONG: case KEY_REWIND:  return KEY_MEDIA_PREV;
    case KEY_STOPCD:                        return KEY_MEDIA_STOP;
    }
    return 0;
}

static void media_scan(void) {
    const char *fifo = getenv("MIYOOPOD_MEDIA_FIFO");
    DIR *d;
    if (fifo && media_n == 0) {
        int fd = open(fifo, O_RDONLY | O_NONBLOCK);
        if (fd >= 0) {
            media_dev[0].fd = fd; media_dev[0].bt = 1;
            strcpy(media_dev[0].name, "fifo");
            media_n = 1;
        }
    }
    d = opendir("/dev/input");
    struct dirent *e;
    if (!d) return;
    while ((e = readdir(d)) && media_n < 16) {
        char path[64];
        int have = 0, known = 0, i, fd;
        unsigned long keys[(KEY_MAX + 1) / (8 * sizeof(long)) + 0], sw[1];
        struct input_id id;
        if (strncmp(e->d_name, "event", 5) != 0) continue;
        for (i = 0; i < media_n; i++) if (strcmp(media_dev[i].name, e->d_name) == 0) known = 1;
        if (known) continue;
        snprintf(path, sizeof(path), "/dev/input/%s", e->d_name);
        fd = open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        if (fd < 0) continue;
        memset(keys, 0, sizeof(keys)); sw[0] = 0;
        ioctl(fd, EVIOCGBIT(EV_KEY, sizeof(keys)), keys);
        ioctl(fd, EVIOCGBIT(EV_SW, sizeof(sw)), sw);
        {
            const int codes[] = { KEY_PLAYPAUSE, KEY_PLAYCD, KEY_PAUSECD, KEY_PLAY, KEY_NEXTSONG,
                                  KEY_PREVIOUSSONG, KEY_STOPCD, KEY_FASTFORWARD, KEY_REWIND, KEY_MEDIA,
                                  KEY_PHONE, BTN_0, KEY_VOICECOMMAND };
            for (i = 0; i < (int)(sizeof(codes) / sizeof(codes[0])); i++)
                if (keys[codes[i] / (8 * sizeof(long))] & (1UL << (codes[i] % (8 * sizeof(long))))) have = 1;
        }
        if (!have && !(sw[0] & (1UL << SW_HEADPHONE_INSERT))) { close(fd); continue; }
        memset(&id, 0, sizeof(id));
        ioctl(fd, EVIOCGID, &id);
        media_dev[media_n].fd = fd;
        media_dev[media_n].bt = id.bustype == BUS_BLUETOOTH;
        media_dev[media_n].jack = (sw[0] & (1UL << SW_HEADPHONE_INSERT)) != 0;
        if (media_dev[media_n].jack) {
            unsigned long now_sw[1] = {0};
            if (ioctl(fd, EVIOCGSW(sizeof(now_sw)), now_sw) >= 0)
                hp_jack = (now_sw[0] & (1UL << SW_HEADPHONE_INSERT)) != 0;
        }
        media_dev[media_n].logged = 0;
        snprintf(media_dev[media_n].name, sizeof(media_dev[media_n].name), "%s", e->d_name);
        {
            char nm[128] = "?", msg[200];
            ioctl(fd, EVIOCGNAME(sizeof(nm)), nm);
            snprintf(msg, sizeof(msg), "media input: %s (%s)%s%s", e->d_name, nm, media_dev[media_n].bt ? " [bluetooth]" : "",
                     media_dev[media_n].jack ? " [headphone jack]" : "");
            c_log(msg);
        }
        media_n++;
    }
    closedir(d);
}

static void media_poll(void) {
    int i;
    Uint32 now = SDL_GetTicks();
    if (media_disabled < 0) {
        const char *v = getenv("MIYOOPOD_MEDIA_KEYS");
        media_disabled = v && v[0] == '0';
    }
    if (media_disabled) return;
    if (media_last_scan == 0 || now - media_last_scan > 2000) {
        media_last_scan = now;
        media_scan();
    }
    for (i = 0; i < media_n; i++) {
        struct input_event ev[16];
        ssize_t r;
        while ((r = read(media_dev[i].fd, ev, sizeof(ev))) > 0) {
            int n = r / sizeof(struct input_event), j;
            for (j = 0; j < n; j++) {
                if (ev[j].type == EV_KEY && ev[j].value == 1) {
                    int k = media_key_for(ev[j].code, media_dev[i].jack);
                    if (media_dev[i].logged < 20) {
                        // what the headset sends, for the log (a button that
                        // does nothing shows its code here)
                        char msg[120];
                        media_dev[i].logged++;
                        snprintf(msg, sizeof(msg), "media key from %s: code %d%s", media_dev[i].name, ev[j].code, k ? "" : " (not used)");
                        c_log(msg);
                    }
                    if (!k) continue;
                    if (k == KEY_MEDIA_HOOK || k == KEY_MEDIA_VOLUP || k == KEY_MEDIA_VOLDN) {
                        push_key(k, 1); // clicks are counted in Go, no de-duplication
                        push_key(k, 0);
                        continue;
                    }
                    // The same press often arrives twice (from the headset
                    // and from the phone-style AVRCP device): ignore a
                    // repeat of the same kind within 400 ms.
                    static int last_g;
                    static Uint32 last_t;
                    int g = k == KEY_MEDIA_NEXT ? 2 : k == KEY_MEDIA_PREV ? 3 : 1;
                    if (g == last_g && now - last_t < 400) continue;
                    last_g = g; last_t = now;
                    push_key(k, 1);
                    push_key(k, 0);
                } else if (ev[j].type == EV_SW && ev[j].code == SW_HEADPHONE_INSERT) {
                    hp_jack = ev[j].value != 0;
                    if (ev[j].value == 0) { push_key(KEY_HP_UNPLUG, 1); push_key(KEY_HP_UNPLUG, 0); }
                }
            }
        }
        if (r < 0 && errno != EINTR && errno != EAGAIN) {
            // device gone: a Bluetooth headset that disconnects is like
            if (media_dev[i].bt) { push_key(KEY_HP_UNPLUG, 1); push_key(KEY_HP_UNPLUG, 0); }
            close(media_dev[i].fd);
            media_dev[i] = media_dev[media_n - 1];
            media_n--;
            i--;
        }
    }
}

// Backlight of the H700 (sunxi display driver): args[1] is the value.
// A negative value reads the current brightness.
//
int disp_brightness(int value) {
    // sunxi: arg[0] screen, arg[1] value
    unsigned long args[4] = {0};
    int fd = open("/dev/disp", O_RDWR | O_CLOEXEC);
    int r;
    if (fd < 0) return -1;
    if (value < 0) {
        r = ioctl(fd, 0x103 /* DISP_LCD_GET_BRIGHTNESS */, args);
    } else {
        args[1] = value;
        r = ioctl(fd, 0x102 /* DISP_LCD_SET_BRIGHTNESS */, args);
        if (r > 0) r = 0;
    }
    close(fd);
    // The value set is applied by the driver immediately (no fade), and
    // the reading is what was last set (not what the panel shows while a
    // blanking is in progress).
    //
    return r;
}
static void present_pending(void);
int pollEvents() {
    present_pending();
    media_poll();
    int queued = evq_pop();
    if (queued != -1) return queued;

    SDL_Event event;
    while (SDL_PollEvent(&event)) {
        switch (event.type) {
        case SDL_KEYDOWN:
            if (event.key.repeat == 0) evq_push(event.key.keysym.sym);
            break;
        case SDL_KEYUP:
            push_key(event.key.keysym.sym, 0);
            break;
        case SDL_CONTROLLERBUTTONDOWN:
        case SDL_CONTROLLERBUTTONUP: {
            int k = button_to_key(event.cbutton.button);
            if (k) push_key(k, event.type == SDL_CONTROLLERBUTTONDOWN);
            break;
        }
        case SDL_CONTROLLERAXISMOTION:
            handle_axis(event.caxis.axis, event.caxis.value);
            break;
        case SDL_CONTROLLERDEVICEADDED:
            open_controller(event.cdevice.which);
            break;
        case SDL_QUIT:  // quit like SELECT + START
            push_key(KEY_SELECT, 1); push_key(KEY_START, 1);
            push_key(KEY_START, 0); push_key(KEY_SELECT, 0);
            break;
        }
        if (evq_head != evq_tail) break;
    }
    return evq_pop();
}

// --- Display -------------------------------------------------------------------
// The UI may draw on another goroutine (another thread): SDL rendering has
// to happen on the thread that created the renderer, so a frame from
// elsewhere is left in pending_pixels and presented by the next pollEvents.
static SDL_threadID main_thread_id;
static volatile unsigned char *pending_pixels;
static SDL_atomic_t frame_pending;
// renderer has vsync (from SDL_GetRendererInfo)
static int vsync_on;
int c_vsync(void) { return vsync_on; }

// Update rows y0..y1 of the texture and present. MIYOOPOD_DUMP_FRAME=path
// writes every frame (raw RGBA 640x480) to path, for screenshots.
static void present_rows(unsigned char *pixels, int y0, int y1) {
    if (y0 < 0) y0 = 0;
    if (y1 > RENDER_HEIGHT) y1 = RENDER_HEIGHT;
    if (y1 > y0) {
        SDL_Rect r = { 0, y0, RENDER_WIDTH, y1 - y0 };
        SDL_UpdateTexture(texture, &r, pixels + (size_t)y0 * RENDER_WIDTH * 4, RENDER_WIDTH * 4);
    }
    SDL_SetRenderDrawColor(renderer, 0, 0, 0, 255);
    SDL_RenderClear(renderer);
    // Scale the 640x480 texture to fit the actual display resolution
    SDL_RenderCopy(renderer, texture, NULL, NULL);
    SDL_RenderPresent(renderer);
    static const char *dump;
    static int dump_init;
    if (!dump_init) { dump = getenv("MIYOOPOD_DUMP_FRAME"); dump_init = 1; }
    if (dump) {
        char tmp[512];
        snprintf(tmp, sizeof(tmp), "%s.tmp", dump);
        FILE *f = fopen(tmp, "wb");
        if (f) { fwrite(pixels, 1, RENDER_WIDTH * RENDER_HEIGHT * 4, f); fclose(f); rename(tmp, dump); }
    }
    // MIYOOPOD_DUMP_SEQ=dir writes every frame as dir/fNNNNN.raw while the
    // file dir/on exists (to study animations frame by frame)
    static const char *seq;
    static int seq_init, seq_n;
    if (!seq_init) { seq = getenv("MIYOOPOD_DUMP_SEQ"); seq_init = 1; }
    if (seq) {
        char path[512];
        snprintf(path, sizeof(path), "%s/on", seq);
        FILE *on = fopen(path, "rb");
        if (on) fclose(on);
        if (on) {
            snprintf(path, sizeof(path), "%s/f%05d.raw", seq, seq_n++);
            FILE *f = fopen(path, "wb");
        if (f) { fwrite(pixels, 1, RENDER_WIDTH * RENDER_HEIGHT * 4, f); fclose(f); }
        }
    }
}

static void present_frame(unsigned char *pixels) {
    present_rows(pixels, 0, RENDER_HEIGHT);
}

static void present_pending(void) {
    if (texture && SDL_AtomicSet(&frame_pending, 0))
        present_rows((unsigned char *)pending_pixels, 0, RENDER_HEIGHT);
}


int refreshScreenPtr(unsigned char *pixels) {
    if (!texture) return -1;
    if (SDL_ThreadID() == main_thread_id) {
        SDL_AtomicSet(&frame_pending, 0);
        present_frame(pixels);
    } else {
        pending_pixels = pixels;
        SDL_AtomicSet(&frame_pending, 1);
    }
    return 0;
}

int refreshScreenRows(unsigned char *pixels, int y0, int y1) {
    if (!texture) return -1;
    if (SDL_ThreadID() != main_thread_id) return refreshScreenPtr(pixels);
    SDL_AtomicSet(&frame_pending, 0);
    present_rows(pixels, y0, y1);
    return 0;
}


int init() {
    main_thread_id = SDL_ThreadID();
    c_log("SDL2 init (VIDEO | AUDIO)...");
    if (SDL_Init(SDL_INIT_VIDEO | SDL_INIT_AUDIO | SDL_INIT_GAMECONTROLLER) < 0) {
        c_logf("SDL_Init failed: %s", SDL_GetError());
        return -1;
    }
    c_log("SDL_Init OK");

    // Display resolution (the UI renders at 640x480 and is scaled)
    SDL_DisplayMode mode;
    if (SDL_GetCurrentDisplayMode(0, &mode) == 0 && mode.w > 0 && mode.h > 0) {
        display_width = mode.w;
        display_height = mode.h;
    } else {
        display_width = 640;
        display_height = 480;
    }
    c_logd("Display width: %d", display_width);
    c_logd("Display height: %d", display_height);
    DetectDevice(display_width, display_height);

    input_init();

    c_log("Creating window...");
    window = SDL_CreateWindow("MiyooPod",
        SDL_WINDOWPOS_UNDEFINED, SDL_WINDOWPOS_UNDEFINED,
        display_width, display_height,
        SDL_WINDOW_SHOWN | (getenv("MIYOOPOD_WINDOWED") ? 0 : SDL_WINDOW_FULLSCREEN));
    if (!window) {
        c_logf("SDL_CreateWindow failed: %s", SDL_GetError());
        return -1;
    }
    c_log("Window created");

    c_log("Creating renderer...");
    // vsync unless MIYOOPOD_VSYNC=0; fall back to no vsync, then software
    const char *vs = getenv("MIYOOPOD_VSYNC");
    Uint32 fl = SDL_RENDERER_ACCELERATED;
    if (!vs || vs[0] != '0') fl |= SDL_RENDERER_PRESENTVSYNC;
    renderer = SDL_CreateRenderer(window, -1, fl);
    if (!renderer && (fl & SDL_RENDERER_PRESENTVSYNC))
        renderer = SDL_CreateRenderer(window, -1, SDL_RENDERER_ACCELERATED);
    if (renderer) {
        SDL_RendererInfo info;
        if (SDL_GetRendererInfo(renderer, &info) == 0) {
            char b[160];
            vsync_on = (info.flags & SDL_RENDERER_PRESENTVSYNC) != 0;
            snprintf(b, sizeof(b), "renderer: %s, vsync %s", info.name, vsync_on ? "on" : "off");
            c_log(b);
        }
    }
    // no accelerated renderer at all
    if (!renderer) renderer = SDL_CreateRenderer(window, -1, SDL_RENDERER_SOFTWARE);
    SDL_ShowCursor(SDL_DISABLE);

    if (renderer) SDL_RenderSetLogicalSize(renderer, RENDER_WIDTH, RENDER_HEIGHT);
    if (!renderer) {
        c_logf("SDL_CreateRenderer failed: %s", SDL_GetError());
        return -1;
    }
    c_log("Renderer created");

    c_log("Creating texture at 640x480 (ABGR8888)...");
    texture = SDL_CreateTexture(renderer,
        SDL_PIXELFORMAT_ABGR8888, SDL_TEXTUREACCESS_STREAMING,
        RENDER_WIDTH, RENDER_HEIGHT);
    if (!texture) {
        c_logf("SDL_CreateTexture failed: %s", SDL_GetError());
        return -1;
    }
    c_log("Texture created");

    return 0;
}

void quit() {
    if (texture) {
        SDL_DestroyTexture(texture);
    }
    if (renderer) {
        SDL_DestroyRenderer(renderer);
    }
    if (window) {
        SDL_DestroyWindow(window);
    }
    SDL_Quit();
}
