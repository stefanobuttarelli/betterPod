/*
 * pzfe.c — betterPod's front end for Simon Tatham's Portable Puzzle
 * Collection (MIT, see LICENCE): the puzzles draw through drawing_api into
 * a list of commands that the Go side replays on its own canvas.
 */
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <stdio.h>
#include <time.h>
#include "puzzles.h"

extern const struct game mines, solo, net, flood, samegame, fifteen,
    lightup, unruly, towers, keen, pattern, tents;

static const struct game *const pz_games[] = {
    &mines, &solo, &net, &flood, &samegame, &fifteen,
    &lightup, &unruly, &towers, &keen, &pattern, &tents,
};
#define PZ_NGAMES ((int)(sizeof(pz_games) / sizeof(*pz_games)))

enum {
    PZ_TEXT = 1, PZ_RECT, PZ_LINE, PZ_POLY, PZ_CIRCLE, PZ_CLIP, PZ_UNCLIP,
    PZ_BSAVE, PZ_BLOAD, PZ_THICK, PZ_BNEW, PZ_BFREE
};

typedef struct {
    int op, a, b, c, d, e, f, g, off;
    float fl;
} pzcmd;

struct blitter { int id, w, h, x, y; };

struct frontend {
    pzcmd *cmds; int ncmd, capcmd;
    int *ints; int nint, capint;
    char *chars; int nchar, capchar;
    int timer;
    char *status;
    int nextblit;
    float bg[3];
};

static pzcmd *pz_push(frontend *fe, int op)
{
    pzcmd *c;
    if (fe->ncmd == fe->capcmd) {
        fe->capcmd = fe->capcmd ? fe->capcmd * 2 : 256;
        fe->cmds = realloc(fe->cmds, fe->capcmd * sizeof(pzcmd));
    }
    c = &fe->cmds[fe->ncmd++];
    memset(c, 0, sizeof(*c));
    c->op = op;
    return c;
}

static int pz_ints(frontend *fe, const int *v, int n)
{
    int off;
    while (fe->nint + n > fe->capint) {
        fe->capint = fe->capint ? fe->capint * 2 : 1024;
        fe->ints = realloc(fe->ints, fe->capint * sizeof(int));
    }
    off = fe->nint;
    memcpy(fe->ints + off, v, n * sizeof(int));
    fe->nint += n;
    return off;
}

static int pz_chars(frontend *fe, const char *s)
{
    int n = (int)strlen(s) + 1, off;
    while (fe->nchar + n > fe->capchar) {
        fe->capchar = fe->capchar ? fe->capchar * 2 : 1024;
        fe->chars = realloc(fe->chars, fe->capchar);
    }
    off = fe->nchar;
    memcpy(fe->chars + off, s, n);
    fe->nchar += n;
    return off;
}

static void d_text(void *h, int x, int y, int fonttype, int fontsize, int align, int colour, char *text)
{
    frontend *fe = h;
    pzcmd *c = pz_push(fe, PZ_TEXT);
    c->a = x; c->b = y; c->c = fonttype; c->d = fontsize; c->e = align; c->f = colour;
    c->off = pz_chars(fe, text);
}
static void d_rect(void *h, int x, int y, int w, int hh, int colour)
{
    pzcmd *c = pz_push(h, PZ_RECT);
    c->a = x; c->b = y; c->c = w; c->d = hh; c->e = colour;
}
static void d_line(void *h, int x1, int y1, int x2, int y2, int colour)
{
    pzcmd *c = pz_push(h, PZ_LINE);
    c->a = x1; c->b = y1; c->c = x2; c->d = y2; c->e = colour;
}
static void d_poly(void *h, int *coords, int n, int fill, int outline)
{
    frontend *fe = h;
    pzcmd *c = pz_push(fe, PZ_POLY);
    c->a = n; c->b = fill; c->c = outline;
    c->off = pz_ints(fe, coords, n * 2);
}
static void d_circle(void *h, int cx, int cy, int r, int fill, int outline)
{
    pzcmd *c = pz_push(h, PZ_CIRCLE);
    c->a = cx; c->b = cy; c->c = r; c->d = fill; c->e = outline;
}
static void d_update(void *h, int x, int y, int w, int hh) {}
static void d_clip(void *h, int x, int y, int w, int hh)
{
    pzcmd *c = pz_push(h, PZ_CLIP);
    c->a = x; c->b = y; c->c = w; c->d = hh;
}
static void d_unclip(void *h) { pz_push(h, PZ_UNCLIP); }
static void d_start(void *h) {}
static void d_end(void *h) {}
static void d_status(void *h, char *text)
{
    frontend *fe = h;
    free(fe->status);
    fe->status = dupstr(text);
}
static blitter *d_bnew(void *h, int w, int hh)
{
    frontend *fe = h;
    blitter *bl = snew(blitter);
    pzcmd *c;
    bl->id = ++fe->nextblit; bl->w = w; bl->h = hh; bl->x = bl->y = 0;
    c = pz_push(fe, PZ_BNEW);
    c->a = bl->id; c->b = w; c->c = hh;
    return bl;
}
static void d_bfree(void *h, blitter *bl)
{
    pzcmd *c = pz_push(h, PZ_BFREE);
    c->a = bl->id;
    sfree(bl);
}
static void d_bsave(void *h, blitter *bl, int x, int y)
{
    pzcmd *c = pz_push(h, PZ_BSAVE);
    bl->x = x; bl->y = y;
    c->a = bl->id; c->b = x; c->c = y; c->d = bl->w; c->e = bl->h;
}
static void d_bload(void *h, blitter *bl, int x, int y)
{
    pzcmd *c = pz_push(h, PZ_BLOAD);
    if (x == BLITTER_FROMSAVED && y == BLITTER_FROMSAVED) { x = bl->x; y = bl->y; }
    c->a = bl->id; c->b = x; c->c = y; c->d = bl->w; c->e = bl->h;
}
static char *d_fallback(void *h, const char *const *strings, int n)
{
    return dupstr(strings[0]);
}
static void d_thick(void *h, float t, float x1, float y1, float x2, float y2, int colour)
{
    pzcmd *c = pz_push(h, PZ_THICK);
    c->a = (int)(x1 * 16); c->b = (int)(y1 * 16); c->c = (int)(x2 * 16); c->d = (int)(y2 * 16);
    c->e = colour; c->fl = t;
}

static const struct drawing_api pz_api = {
    d_text, d_rect, d_line, d_poly, d_circle, d_update, d_clip, d_unclip,
    d_start, d_end, d_status, d_bnew, d_bfree, d_bsave, d_bload,
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, d_fallback, d_thick,
};

/* ---- what the puzzles ask of a front end ---- */

void fatal(char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    fputc('\n', stderr);
    abort();
}

void frontend_default_colour(frontend *fe, float *output)
{
    output[0] = fe->bg[0]; output[1] = fe->bg[1]; output[2] = fe->bg[2];
}

void activate_timer(frontend *fe) { fe->timer = 1; }
void deactivate_timer(frontend *fe) { fe->timer = 0; }

void get_random_seed(void **randseed, int *randseedsize)
{
    struct pzseed { time_t t; clock_t c; void *p; } *s = smalloc(sizeof(struct pzseed));
    s->t = time(NULL); s->c = clock(); s->p = s;
    *randseed = s;
    *randseedsize = sizeof(*s);
}

/* ---- for the Go side ---- */

int pz_ngames(void) { return PZ_NGAMES; }
const char *pz_name(int i) { return pz_games[i]->name; }

frontend *pz_fe_new(float r, float g, float b)
{
    frontend *fe = snew(frontend);
    memset(fe, 0, sizeof(*fe));
    fe->bg[0] = r; fe->bg[1] = g; fe->bg[2] = b;
    return fe;
}

midend *pz_new(frontend *fe, int i)
{
    return midend_new(fe, pz_games[i], &pz_api, fe);
}

void pz_reset(frontend *fe) { fe->ncmd = fe->nint = fe->nchar = 0; }
int pz_ncmd(frontend *fe) { return fe->ncmd; }
pzcmd *pz_cmds(frontend *fe) { return fe->cmds; }
int *pz_intpool(frontend *fe) { return fe->ints; }
char *pz_charpool(frontend *fe) { return fe->chars; }
int pz_timer_on(frontend *fe) { return fe->timer; }
const char *pz_status(frontend *fe) { return fe->status ? fe->status : ""; }

/* keys: 1-4 up/down/left/right, 5 select, 6 select 2, 7 undo, 8 redo,
 * 9 new game, 10 restart */
int pz_key(midend *me, int k)
{
    int b = 0;
    switch (k) {
    case 1: b = CURSOR_UP; break;
    case 2: b = CURSOR_DOWN; break;
    case 3: b = CURSOR_LEFT; break;
    case 4: b = CURSOR_RIGHT; break;
    case 5: b = CURSOR_SELECT; break;
    case 6: b = CURSOR_SELECT2; break;
    case 7: b = 'u'; break;
    case 8: b = 'r'; break;
    case 9: b = 'n'; break;
    case 10: midend_restart_game(me); return 1;
    }
    return midend_process_key(me, 0, 0, b);
}

struct pzbuf { char *p; int n, cap; };
static void pz_write(void *ctx, void *buf, int len)
{
    struct pzbuf *b = ctx;
    while (b->n + len > b->cap) {
        b->cap = b->cap ? b->cap * 2 : 4096;
        b->p = realloc(b->p, b->cap);
    }
    memcpy(b->p + b->n, buf, len);
    b->n += len;
}
char *pz_save(midend *me, int *len)
{
    struct pzbuf b = {NULL, 0, 0};
    midend_serialise(me, pz_write, &b);
    *len = b.n;
    return b.p;
}
struct pzread { const char *p; int n, pos; };
static int pz_read(void *ctx, void *buf, int len)
{
    struct pzread *r = ctx;
    if (r->pos + len > r->n) return FALSE;
    memcpy(buf, r->p + r->pos, len);
    r->pos += len;
    return TRUE;
}
const char *pz_load(midend *me, const char *data, int len)
{
    struct pzread r = {data, len, 0};
    return midend_deserialise(me, pz_read, &r);
}

/* printing isn't used here */
void document_add_puzzle(document *doc, const game *game, game_params *par,
                         game_state *st, game_state *st2) {}
