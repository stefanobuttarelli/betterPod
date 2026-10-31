// cdraw.c — the hot pixel loops of the iPod interface in C (see cdraw.go):
// blit, blitShiftX, darkenColumns, dimRect and Cover Flow's texture mapping.
//
// Same arithmetic as the Go versions, pixel for pixel (tools/onion/unittest
// compares them): Go's compiler doesn't vectorise, clang does (NEON on the
// Miyoo's Cortex-A7), and there are no bounds checks. Pixels are RGBA,
// premultiplied, little-endian (byte 0 = R).

#include <stdint.h>

// no fused multiply-add: the same rounding as Go's float64 code on arm
#pragma STDC FP_CONTRACT OFF

// floor(u / 255) for u <= 65278, in 16-bit friendly arithmetic
static inline uint32_t cd_div255(uint32_t u) { return (u + 1 + (u >> 8)) >> 8; }

static inline uint32_t cd_sat8(uint32_t v) { return v > 255 ? 255 : v; }

// the SWAR helpers of ij_swar.go (Go's precedence written out)
static inline uint32_t cd_lerp32(uint32_t a, uint32_t b, uint32_t t) {
    uint32_t it = 256 - t;
    uint32_t rb = ((((a & 0xff00ff) * it) + ((b & 0xff00ff) * t)) >> 8) & 0xff00ff;
    uint32_t ag = ((((a >> 8) & 0xff00ff) * it) + (((b >> 8) & 0xff00ff) * t)) & 0xff00ff00;
    return rb | ag;
}

static inline uint32_t cd_scale32(uint32_t c, uint32_t s) {
    uint32_t rb = (((c & 0xff00ff) * s) >> 8) & 0xff00ff;
    uint32_t ag = (((c >> 8) & 0xff00ff) * s) & 0xff00ff00;
    return rb | ag;
}

static inline uint32_t cd_over32(uint32_t src, uint32_t dst) {
    uint32_t a = src >> 24;
    if (a >= 255) return src;
    if (a == 0) return dst;
    return src + cd_scale32(dst, 256 - (a + (a >> 7)));
}

// cd_blend: one pixel of blit at full opacity (0 < sa < 255).
static inline uint32_t cd_blend(uint32_t sp, uint32_t dp) {
    uint32_t sa = sp >> 24, ia = 255 - sa;
    uint32_t r = cd_sat8((sp & 0xff) + cd_div255((dp & 0xff) * ia + 127));
    uint32_t g = cd_sat8(((sp >> 8) & 0xff) + cd_div255(((dp >> 8) & 0xff) * ia + 127));
    uint32_t b = cd_sat8(((sp >> 16) & 0xff) + cd_div255(((dp >> 16) & 0xff) * ia + 127));
    uint32_t a = cd_sat8(sa + cd_div255((dp >> 24) * ia + 127));
    return r | g << 8 | b << 16 | a << 24;
}

// cd_blit: blit() of ij_gfx.go for an already clipped w x h area.
// a is the opacity in 0..255. Rows are 4-byte aligned (image.RGBA).
void cd_blit(uint8_t *dst, int dstride, const uint8_t *src, int sstride, int w, int h, uint32_t a) {
    int x, y;
    for (y = 0; y < h; y++) {
        uint32_t *d = (uint32_t *)(dst + (long)y * dstride);
        const uint32_t *s = (const uint32_t *)(src + (long)y * sstride);
        if (a >= 255) {
            // opaque pixels are copied whole, clear ones skipped: the
            // interface's layers are mostly one or the other, so this loop
            // stays scalar (vectorised, it would blend every pixel)
#pragma clang loop vectorize(disable)
            for (x = 0; x < w; x++) {
                uint32_t sp = s[x], sa = sp >> 24;
                if (sa == 255) d[x] = sp;
                else if (sa != 0) d[x] = cd_blend(sp, d[x]);
            }
        } else {
            for (x = 0; x < w; x++) {
                uint32_t sp = s[x], dp = d[x];
                uint32_t sa, ia, r, g, b, al;
                if ((sp >> 24) == 0) continue;
                sa = cd_div255((sp >> 24) * a);
                if (sa == 0) continue;
                ia = 255 - sa;
                r = cd_sat8(cd_div255((sp & 0xff) * a + 127) + cd_div255((dp & 0xff) * ia + 127));
                g = cd_sat8(cd_div255(((sp >> 8) & 0xff) * a + 127) + cd_div255(((dp >> 8) & 0xff) * ia + 127));
                b = cd_sat8(cd_div255(((sp >> 16) & 0xff) * a + 127) + cd_div255(((dp >> 16) & 0xff) * ia + 127));
                al = cd_sat8(sa + cd_div255((dp >> 24) * ia + 127));
                d[x] = r | g << 8 | b << 16 | al << 24;
            }
        }
    }
}

// cd_shiftx: the source rows ya..yb of blitShiftX (ij_previews.go); source
// row sy lands on destination row y + sy.
void cd_shiftx(uint32_t *dst, int dst32, const uint32_t *src, int sst, int w, int y,
               int ix, int xa, int xb, int ya, int yb, uint32_t wf, uint32_t op32) {
    int sy, dxp;
    for (sy = ya; sy < yb; sy++) {
        const uint32_t *srow = src + (long)sy * sst;
        uint32_t *drow = dst + (long)(y + sy) * dst32;
        for (dxp = xa; dxp <= xb; dxp++) {
            int sx = dxp - ix;
            uint32_t p0 = 0, p1 = 0, c;
            if (sx < w) p0 = srow[sx];
            if (sx > 0 && sx - 1 < w) p1 = srow[sx - 1];
            c = cd_lerp32(p0, p1, wf);
            if (op32 < 256) c = cd_scale32(c, op32);
            drow[dxp] = cd_over32(c, drow[dxp]);
        }
    }
}

// cd_darken: darkenColumns (ij_gfx.go), row by row over an already clipped
// area of ncols x nrows pixels starting at base. m[i] multiplies the colour
// of column i (by m/256; 256 leaves the column alone).
void cd_darken(uint8_t *base, int stride, int ncols, int nrows, const uint16_t *m) {
    int x, y;
    for (y = 0; y < nrows; y++) {
        uint8_t *row = base + (long)y * stride;
        for (x = 0; x < ncols; x++) {
            uint32_t k = m[x];
            uint8_t *p = row + x * 4;
            if (k >= 256) continue;
            p[0] = (uint8_t)((p[0] * k) >> 8);
            p[1] = (uint8_t)((p[1] * k) >> 8);
            p[2] = (uint8_t)((p[2] * k) >> 8);
        }
    }
}

// cd_drawtex: the rows ya..yb of drawTex (ij_coverflow.go).
// hi: the inverse homography (9); tm: su, ou, sv, ov, u0, v0, u1, v1,
// reflFrom, reflMax, reflStart.
void cd_drawtex(uint32_t *dp, int dst32, int x0, int x1, int ya, int yb,
                const uint32_t *sp, int sst, int iw, int ih,
                const double *hi, const double *tm, uint32_t op, double dpr) {
    const double tsu = tm[0], tou = tm[1], tsv = tm[2], tov = tm[3];
    const double u0 = tm[4], v0 = tm[5], u1 = tm[6], v1 = tm[7];
    const double reflFrom = tm[8], reflMax = tm[9], reflStart = tm[10];
    const double reflH = reflFrom;
    int py;
    for (py = ya; py < yb; py++) {
        double sy = ((double)py + 0.5) / dpr;
        double sx0 = ((double)x0 + 0.5) / dpr;
        double du = hi[0] / dpr, dv = hi[3] / dpr, dw = hi[6] / dpr;
        double U = hi[0] * sx0 + hi[1] * sy + hi[2];
        double V = hi[3] * sx0 + hi[4] * sy + hi[5];
        double W = hi[6] * sx0 + hi[7] * sy + hi[8];
        uint32_t *row = dp + (long)py * dst32;
        double u = U / W, v = V / W;
        int px0;
        for (px0 = x0; px0 < x1; px0 += 8) {
            int n = 8, k;
            double U2, V2, W2, u2, v2, su, sv, uu, vv;
            if (px0 + 8 > x1) n = x1 - px0;
            U2 = U + du * (double)n; V2 = V + dv * (double)n; W2 = W + dw * (double)n;
            u2 = U2 / W2; v2 = V2 / W2;
            su = (u2 - u) / (double)n; sv = (v2 - v) / (double)n;
            uu = u; vv = v;
            for (k = 0; k < n; k++, uu += su, vv += sv) {
                uint32_t alpha = op, fx, fy, c;
                double vs = vv, tx, ty;
                int ix, iy, ix1, iy1, o;
                long r0, r1;
                if (uu < u0 || uu >= u1 || vv < v0 || vv >= v1) continue;
                if (reflFrom > 0 && vv >= reflFrom) {
                    double f = (2 * reflFrom - vv) / reflH;
                    if (f < reflStart) continue;
                    alpha = (uint32_t)((double)op * reflMax * (f - reflStart) / (1 - reflStart));
                    if (alpha == 0) continue;
                    vs = 2 * reflFrom - vv;
                }
                tx = uu * tsu + tou - 0.5;
                ty = vs * tsv + tov - 0.5;
                if (tx < 0) tx = 0;
                if (ty < 0) ty = 0;
                ix = (int)tx; iy = (int)ty;
                fx = (uint32_t)((tx - (double)ix) * 256);
                fy = (uint32_t)((ty - (double)iy) * 256);
                ix1 = ix + 1; iy1 = iy + 1;
                if (ix >= iw - 1) { ix = iw - 1; ix1 = iw - 1; fx = 0; }
                if (iy >= ih - 1) { iy = ih - 1; iy1 = ih - 1; fy = 0; }
                r0 = (long)iy * sst; r1 = (long)iy1 * sst;
                c = cd_lerp32(cd_lerp32(sp[r0 + ix], sp[r0 + ix1], fx),
                              cd_lerp32(sp[r1 + ix], sp[r1 + ix1], fx), fy);
                if (alpha < 256) c = cd_scale32(c, alpha);
                if ((c >> 24) == 0) continue;
                o = px0 + k;
                row[o] = cd_over32(c, row[o]);
            }
            U = U2; V = V2; W = W2; u = u2; v = v2;
        }
    }
}

// cd_dim: dimRect of ij_gfx.go, black over w x h pixels: the colour times
// m/256, the alpha toward 255 by the same amount.
void cd_dim(uint8_t *dst, int dstride, int w, int h, uint32_t m) {
    int x, y;
    for (y = 0; y < h; y++) {
        uint32_t *d = (uint32_t *)(dst + (long)y * dstride);
        for (x = 0; x < w; x++) {
            uint32_t p = d[x];
            uint32_t a = 255 - (((255 - (p >> 24)) * m) >> 8);
            d[x] = (cd_scale32(p, m) & 0xffffff) | a << 24;
        }
    }
}
