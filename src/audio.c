#include "SDL.h"
#include "SDL_mixer.h"
#include <stdlib.h>
#include <string.h>


static Mix_Music *current_music = NULL;
static volatile int music_finished_flag = 0;
static double cached_duration = 0.0;
static int cur_rate = 0;        // rate the device is open at (0 = closed)
static int last_volume = 128;   // re-applied after the device is reopened

// Bigger chunks at high sample rates keep the callback rate (and the CPU
// cost) about the same as 4096 frames at 44.1/48 kHz.
// MIYOOPOD_AUDIO_CHUNK overrides it (frames). On Onion the default is 2048
// (46 ms): MI_AO queues 6 buffers and, with MI_AO_SendFrame waiting for room
// (onion.c), stays full, so about 0.28 s of sound is always buffered while
// a pause still takes effect quickly.
static int chunk_for(int rate) {
    const char *e = getenv("MIYOOPOD_AUDIO_CHUNK");
    if (e && atoi(e) >= 256 && atoi(e) <= 65536) return atoi(e);
    if (onion_mode) return 2048;
    return rate <= 48000 ? 4096 : rate <= 96000 ? 8192 : 16384;
}

// Memory buffer for current track - eliminates SD card I/O during playback
// (owned here: freed on the next load, on a rate change and on quit)
//
static void *current_music_data = NULL;

static void on_music_finished() {
    music_finished_flag = 1;
}

// --- the visualizer's tap ---------------------------------------------------
// The mixed output (mono) goes into a ring as each chunk is handed to the
// device; the reader takes the samples being heard now: the chunk's start
// plus the time since it was handed over (chunks are up to ~90 ms long, so
// reading only at chunk boundaries would jerk).
#define VIS_N 32768
static float vis_ring[VIS_N];
static volatile unsigned vis_pos;      // samples written so far
static volatile unsigned vis_cb_start; // ring position where the last chunk starts
static volatile Uint64 vis_cb_t;       // when it was handed over (perf counter)
static int vis_f32 = 1, vis_ch = 2, vis_rate = 44100;

static void vis_postmix(void *udata, Uint8 *stream, int len) {
    (void)udata;
    unsigned p = vis_pos;
    int n;
    if (vis_f32) {
        const float *s = (const float *)stream;
        n = len / (int)sizeof(float) / vis_ch;
        for (int i = 0; i < n; i++) {
            float v = s[i * vis_ch];
            if (vis_ch > 1) v = (v + s[i * vis_ch + 1]) * 0.5f;
            vis_ring[(p + i) & (VIS_N - 1)] = v;
        }
    } else {
        const Sint16 *s = (const Sint16 *)stream;
        n = len / (int)sizeof(Sint16) / vis_ch;
        for (int i = 0; i < n; i++) {
            float v = s[i * vis_ch];
            if (vis_ch > 1) v = (v + s[i * vis_ch + 1]) * 0.5f;
            vis_ring[(p + i) & (VIS_N - 1)] = v / 32768.0f;
        }
    }
    vis_cb_start = p;
    vis_cb_t = SDL_GetPerformanceCounter();
    vis_pos = p + (unsigned)n;
}

static void vis_hook(void) {
    int freq = 0, ch = 0; Uint16 fmt = 0;
    Mix_QuerySpec(&freq, &fmt, &ch);
    vis_f32 = fmt == AUDIO_F32SYS;
    vis_ch = ch > 0 ? ch : 2;
    vis_rate = freq > 0 ? freq : 44100;
    Mix_SetPostMix(vis_postmix, NULL);
}

// audio_vis_read: the n samples (mono, -1..1) heard now. It returns 0 when
// nothing new came for half a second (paused, stopped: out is silence).
int audio_vis_read(float *out, int n) {
    Uint64 now = SDL_GetPerformanceCounter();
    Uint64 t = vis_cb_t;
    unsigned start = vis_cb_start, pos = vis_pos;
    double dt = t ? (double)(now - t) / (double)SDL_GetPerformanceFrequency() : 1e9;
    if (dt > 0.5 || n > VIS_N / 2) {
        memset(out, 0, sizeof(float) * (size_t)n);
        return 0;
    }
    unsigned end = start + (unsigned)(dt * vis_rate);
    if ((int)(end - pos) > 0) end = pos;
    for (int i = 0; i < n; i++) out[i] = vis_ring[(end - (unsigned)n + (unsigned)i) & (VIS_N - 1)];
    return 1;
}

int audio_vis_rate(void) { return vis_rate; }

int audio_init() {
    c_log("audio_init entered");

    c_log("calling Mix_OpenAudio...");
    if (onion_mode) {
        // MI_AO plays 16-bit: mixing in float would only add a conversion
        if (Mix_OpenAudio(44100, AUDIO_S16SYS, 2, chunk_for(44100)) < 0) {
            c_logf("Mix_OpenAudio failed: %s", SDL_GetError());
            return -1;
        }
    // Float output first (no clipping in the mixer); fall back to the
    // default 16-bit format.
    } else if (Mix_OpenAudio(44100, AUDIO_F32SYS, 2, chunk_for(44100)) < 0) {
        c_logf("Mix_OpenAudio (f32) failed: %s", SDL_GetError());
        if (Mix_OpenAudio(44100, MIX_DEFAULT_FORMAT, 2, chunk_for(44100)) < 0) {
            c_logf("Mix_OpenAudio failed: %s", SDL_GetError());
            return -1;
        }
    }
    cur_rate = 44100;
    vis_hook();
    {
        int freq = 0, ch = 0; Uint16 fmt = 0; char b[96];
        Mix_QuerySpec(&freq, &fmt, &ch);
        snprintf(b, sizeof(b), "audio device: %d Hz, format 0x%x, %d ch (driver %s)", freq, fmt, ch, SDL_GetCurrentAudioDriver());
        c_log(b);
    }
    c_log("Mix_OpenAudio OK");

    int flags = MIX_INIT_MP3 | MIX_INIT_FLAC | MIX_INIT_OGG;
    int initted = Mix_Init(flags);
    if ((initted & MIX_INIT_MP3) == 0)  c_logf("Mix_Init MP3 failed: %s", Mix_GetError());
    else c_log("Mix_Init MP3 OK");
    if ((initted & MIX_INIT_FLAC) == 0) c_logf("Mix_Init FLAC failed: %s", Mix_GetError());
    else c_log("Mix_Init FLAC OK");
    if ((initted & MIX_INIT_OGG) == 0)  c_logf("Mix_Init OGG failed: %s", Mix_GetError());
    else c_log("Mix_Init OGG OK");

    Mix_HookMusicFinished(on_music_finished);
    return 0;
}

static void free_current(void) {
    if (current_music) {
        Mix_HaltMusic();
        Mix_FreeMusic(current_music);
        current_music = NULL;
    }
    if (current_music_data) {
        free(current_music_data);
        current_music_data = NULL;
    }
}

// Reopen the audio device at the sample rate of the track (no resampling
// for 48/88.2/96 kHz files). The current track is unloaded: the caller
int audio_set_rate(int rate) {
    int freq = 0, ch = 0;
    Uint16 fmt = 0;
    if (rate < 8000 || rate > 384000 || rate == cur_rate) return 0;
    if (onion_mode) return 0; // MI_AO stays at 44.1 kHz (SDL_mixer resamples)
    free_current();
    Mix_CloseAudio();
    if (Mix_OpenAudio(rate, AUDIO_F32SYS, 2, chunk_for(rate)) < 0) {
        char b[160]; snprintf(b, sizeof(b), "Mix_OpenAudio(%d) failed: %s, back to 44100", rate, SDL_GetError());
        c_log(b);
        rate = 44100; if (Mix_OpenAudio(rate, AUDIO_F32SYS, 2, 4096) < 0 &&
            Mix_OpenAudio(44100, MIX_DEFAULT_FORMAT, 2, 4096) < 0) {
            c_logf("Mix_OpenAudio failed: %s", SDL_GetError());
            cur_rate = 0;
            return -1;
        }
    }
    cur_rate = rate;
    vis_hook();
    Mix_QuerySpec(&freq, &fmt, &ch);
    { char b[96]; snprintf(b, sizeof(b), "audio device: %d Hz, format 0x%x, %d ch", freq, fmt, ch); c_log(b); }
    Mix_HookMusicFinished(on_music_finished);
    Mix_VolumeMusic(last_volume);
    music_finished_flag = 0;
    return 0;
}

// Load from file path (streaming from SD card - fallback)
int audio_load(const char *path) {
    if (current_music) {
        Mix_FreeMusic(current_music);
        current_music = NULL;
    }
    if (current_music_data) {
        free(current_music_data);
        current_music_data = NULL;
    }

    current_music = Mix_LoadMUS(path);
    if (!current_music) {
        c_logf("Mix_LoadMUS failed: %s", Mix_GetError());
        return -1;
    }

    cached_duration = Mix_MusicDuration(current_music);
    return 0;
}

// Load audio from a memory buffer (data is C-allocated, caller gives ownership).
// Eliminates SD card I/O during playback - SDL_mixer reads from RAM.
// Returns 0 on success, -1 on failure. On failure, caller must NOT free data (we do).
int audio_load_mem(void *data, int size) {
    if (current_music) {
        Mix_FreeMusic(current_music);
        current_music = NULL;
    }
    if (current_music_data) {
        free(current_music_data);
        current_music_data = NULL;
    }

    current_music_data = data;

    SDL_RWops *rw = SDL_RWFromMem(current_music_data, size);
    if (!rw) {
        c_logf("SDL_RWFromMem failed: %s", SDL_GetError());
        free(current_music_data);
        current_music_data = NULL;
        return -1;
    }

    current_music = Mix_LoadMUS_RW(rw, 1);
    if (!current_music) {
        c_logf("Mix_LoadMUS_RW failed: %s", Mix_GetError());
        free(current_music_data);
        current_music_data = NULL;
        return -1;
    }

    cached_duration = Mix_MusicDuration(current_music);
    c_log("audio_load_mem OK (playing from RAM)");
    return 0;
}

int audio_play() {
    if (!current_music) return -1;
    music_finished_flag = 0;
    if (Mix_PlayMusic(current_music, 0) < 0) {
        c_logf("Mix_PlayMusic failed: %s", Mix_GetError());
        return -1;
    }
    return 0;
}

void audio_pause() {
    Mix_PauseMusic();
}

void audio_resume() {
    Mix_ResumeMusic();
}

void audio_toggle_pause() {
    if (Mix_PausedMusic()) {
        audio_resume();
    } else {
        audio_pause();
    }
}

void audio_stop() {
    Mix_HaltMusic();
    music_finished_flag = 0;
    cached_duration = 0.0;
}

int audio_is_playing() {
    return Mix_PlayingMusic() && !Mix_PausedMusic();
}

int audio_is_paused() {
    return Mix_PausedMusic();
}

double audio_get_position() {
    if (!current_music || !Mix_PlayingMusic()) return 0.0;
    return Mix_GetMusicPosition(current_music);
}

double audio_get_duration() {
    return cached_duration;
}

// Get duration of an audio file without loading it into the player
double audio_get_file_duration(const char *path) {
    Mix_Music *temp_music = Mix_LoadMUS(path);
    if (!temp_music) {
        fprintf(stderr, "Failed to load music for duration: %s - Error: %s\n", path, Mix_GetError());
        return 0.0;
    }

    double duration = Mix_MusicDuration(temp_music);
    if (duration < 0) {
        fprintf(stderr, "Mix_MusicDuration failed for: %s - Error: %s\n", path, Mix_GetError());
        duration = 0.0;
    }

    Mix_FreeMusic(temp_music);

    return duration;
}

int audio_seek(double position) {
    if (!current_music) return -1;
    return Mix_SetMusicPosition(position);
}

void audio_set_volume(int volume) {
    last_volume = volume;
    Mix_VolumeMusic(volume);
}

int audio_check_finished() {
    if (music_finished_flag) {
        music_finished_flag = 0;
        return 1;
    }
    return 0;
}

typedef struct {
    double position;
    double duration;
    int is_playing;
    int is_paused;
    int finished;
} AudioState;

void audio_get_state(AudioState *state) {
    state->position = 0.0;
    state->duration = cached_duration;
    state->is_playing = 0;
    state->is_paused = 0;
    state->finished = 0;

    if (current_music && Mix_PlayingMusic()) {
        state->position = Mix_GetMusicPosition(current_music);
        state->is_playing = !Mix_PausedMusic();
        state->is_paused = Mix_PausedMusic();
    }

    if (music_finished_flag) {
        music_finished_flag = 0;
        state->finished = 1;
    }
}

void audio_quit() {
    if (current_music) {
        Mix_FreeMusic(current_music);
        current_music = NULL;
    }
    if (current_music_data) {
        free(current_music_data);
        current_music_data = NULL;
    }
    Mix_CloseAudio();
    Mix_Quit();
}
