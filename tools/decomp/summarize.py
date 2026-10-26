#!/usr/bin/env python3
"""Summarize Go arm64 objdump (of a .o, with relocations) per source line,
merged with DWARF locals (from tools/dwarfvars). Used to rebuild lost sources.

usage: summarize.py main.objdump dwarf.txt FILE.go > FILE.summary
"""
import re, sys, struct, collections

objdump, dwarftxt, want = sys.argv[1], sys.argv[2], sys.argv[3]
structs = {}
if len(sys.argv) > 4:
    cur = None
    for l in open(sys.argv[4]):
        if l.startswith('STRUCT '):
            cur = l.split()[1].replace('main.', '')
            structs[cur] = {}
        elif cur:
            p = l.split()
            structs[cur].setdefault(int(p[0]), []).append(p[1])

def f64(h):
    return struct.unpack('>d', bytes.fromhex(h.rjust(16, '0')))[0]

def fmtf(v):
    if v == int(v) and abs(v) < 1e15:
        return str(int(v)) + '.'
    return repr(v)

# ---- DWARF ----
dw = collections.defaultdict(list)
cur = None
for l in open(dwarftxt):
    if l.startswith('FUNC '):
        cur = l[5:].strip().replace(' (abstract)', '')
        continue
    if cur:
        dw[cur].append(l.rstrip())

# ---- objdump ----
funcs = []  # (name, file, [(file, line, text)])
for l in open(objdump):
    if l.startswith('TEXT '):
        m = re.match(r'TEXT (\S+)\(SB\) (\S+)', l)
        funcs.append((m.group(1), m.group(2).split('/')[-1], []))
        continue
    m = re.match(r'\s+(\S+?):(\d+)\s+0x[0-9a-f]+\s+[0-9a-f]+\s+(.*)$', l)
    if m and funcs:
        funcs[-1][2].append((m.group(1), int(m.group(2)), m.group(3)))

# index own-file line ranges for naming inlined helpers
ranges = collections.defaultdict(list)
for name, file, ins in funcs:
    ls = [ln for f, ln, _ in ins if f == file]
    if ls:
        ranges[file].append((min(ls), max(ls), name))

def owner(file, line):
    best = None
    for a, b, n in ranges.get(file, []):
        if a <= line <= b and (best is None or b - a < best[1] - best[0]):
            best = (a, b, n)
    return best[2].replace('main.', '') if best else None

NOISE = ('runtime.morestack', 'runtime.gcWriteBarrier', 'runtime.panicIndex',
         'runtime.panicSlice', 'runtime.panicdivide', 'runtime.panicshift',
         'runtime.panicunsafeslice', 'runtime.racefunc', 'runtime.duffzero',
         'runtime.duffcopy', 'runtime.wbZero', 'runtime.wbMove')

def tokens(text):
    out = []
    strs = re.findall(r'R_[A-Z0-9_]+:go:string\.(".*?")\s*$', text)
    for x in strs:
        out.append(x)
    if strs:
        text = text.split('R_', 1)[0]
    for r in re.findall(r'R_[A-Z0-9_]+:(\S+)', text):
        if r.startswith('go:string.'):
            out.append('"' + r[len('go:string."'):-1] + '"' if r.endswith('"') else r)
        elif r.startswith('$f64.'):
            out.append(fmtf(f64(r[5:])))
        elif r.startswith('$f32.'):
            out.append(fmtf(struct.unpack('>f', bytes.fromhex(r[5:]))[0]) + 'f')
        elif r.startswith('go:itab.'):
            out.append('itab(' + r[8:].replace('main.', '') + ')')
        elif r.startswith('type:'):
            out.append('T(' + r[5:].replace('main.', '') + ')')
        elif any(r.startswith(n) for n in NOISE) or r.startswith('gclocals') or r.startswith('go:cuinfo'):
            continue
        elif 'R_CALL' in text and r in text.split('R_CALL', 1)[1]:
            out.append('call ' + r.replace('main.', ''))
        else:
            out.append('&' + r.replace('main.', ''))
    m = re.match(r'(FMOVD|FMOVS) \$([-0-9.e+]+),', text)
    if m:
        out.append(m.group(2))
    m = re.match(r'(CMP|CMPW|ADD|SUB|AND|ORR|TST|TSTW) \$(-?\d+),', text)
    if m and 'RSP' not in text and m.group(2) not in ('0', '1', '8', '16', '24'):
        out.append('#' + m.group(2))
    m = re.match(r'(?:MOV[A-Z]*|FMOV[DS]|LDP|STP|LDAR[A-Z]*|STLR[A-Z]*)\s.*?(-?\d+)\((R\d+)\)', text)
    if m and m.group(2) not in ('R28',) :
        out.append('@' + m.group(1))
    return out

for name, file, ins in funcs:
    if file != want:
        continue
    own = [ln for f, ln, _ in ins if f == file]
    print(f'== {name.replace("main.", "")}  [{file}:{min(own) if own else "?"}-{max(own) if own else "?"}]')
    for d in dw.get(name, []):
        d = d.replace('main.', '')
        if 'inline sync.' in d or 'inline internal/' in d or ' ? @0' in d:
            continue
        print('   ' + d)
    cands = []
    for d in dw.get(name, []):
        for tn in re.findall(r'\*?main\.([A-Za-z0-9_]+)', d):
            if tn in structs and tn not in cands:
                cands.append(tn)
    def ann(x):
        if not x.startswith('@'):
            return x
        try:
            off = int(x[1:])
        except ValueError:
            return x
        hits = []
        for tn in cands:
            for fn in structs[tn].get(off, []):
                hits.append(tn + '.' + fn)
        return x + ('(' + '/'.join(hits[:3]) + ')' if hits else '')
    byline = collections.OrderedDict()
    last = None
    pend = {}  # reg -> value being built by MOVD/MOVK
    for f, ln, text in ins:
        mm = re.match(r'MOVD \$(-?\d+), (R\d+)', text)
        mk = re.match(r'MOVK \$\((\d+)<<(\d+)\), (R\d+)', text)
        extra = None
        if mm:
            pend[mm.group(2)] = int(mm.group(1)) & 0xffffffffffffffff
            v = int(mm.group(1))
            if v not in (0, 1, 8, 16, 24) and 'RSP' not in text:
                extra = '#' + str(v)
        elif mk and mk.group(3) in pend:
            sh = int(mk.group(2)); v = pend[mk.group(3)]
            v = (v & ~(0xffff << sh)) | (int(mk.group(1)) << sh)
            pend[mk.group(3)] = v
            bs = v.to_bytes(8, 'little').rstrip(b'\0')
            if bs and all(32 <= c < 127 for c in bs) and len(bs) >= 2:
                extra = "#'" + bs.decode() + "'"
            else:
                sv = v - (1 << 64) if v >> 63 else v
                extra = '#' + str(sv) + ('' if sh < 48 else ' (%s)' % fmtf(struct.unpack('<d', v.to_bytes(8,'little'))[0]))
        if f == file:
            last = ln
            key = ln
            pre = ''
        else:
            o = owner(f, ln)
            key = last
            pre = f'[{o or f}:{ln}] '
        t = tokens(text)
        if extra:
            # replace partial constants from this register on this line
            t.append(extra)
        if f != file and not f.startswith(('ij_', 'knulli', 'library', 'mpv', 'ui.go', 'player', 'queue')):
            t = [x for x in t if x.startswith('call ') or x.startswith('"')]
        if key is None:
            continue
        lst = byline.setdefault(key, [])
        for x in t:
            x = pre + ann(x)
            if x not in lst:
                lst.append(x)
        if f != file and not t:
            tag = f'[{owner(f, ln) or f}]'
            if tag not in lst and not f.endswith(('mutex.go', 'mgc.go')):
                lst.append(tag)
    for ln in sorted(byline):
        print(f'{ln:5d}: ' + ' | '.join(byline[ln]))
    print()
