#!/usr/bin/env python3
"""Per-line digest of a function/file from `go tool objdump` output of a Go object.
usage: digest.py obj.dis dwarf.txt FILE.go [funcregex]"""
import re, sys, struct, collections

dis, dwf, target = sys.argv[1], sys.argv[2], sys.argv[3]
fre = re.compile(sys.argv[4]) if len(sys.argv) > 4 else None

NOISE = re.compile(r'runtime\.(morestack|gcWriteBarrier|writeBarrier|duff|panicIndex|panicSlice|goPanic|panicdivide|panicshift|panicunsafe|racefunc|arm64Has)')
def f64(h):
    return struct.unpack('>d', bytes.fromhex(h.rjust(16, '0')))[0]
def f32(h):
    return struct.unpack('>f', bytes.fromhex(h.rjust(8, '0')))[0]

# dwarf locals
dw = collections.defaultdict(list); cur = None
for l in open(dwf):
    if l.startswith('FUNC '): cur = l.split()[1]; continue
    if cur and l.strip(): dw[cur].append(l.rstrip())

funcs = []; cur = None
for l in open(dis):
    if l.startswith('TEXT '):
        p = l.split(); name = p[1][:-4]; f = p[-1].split('/')[-1]
        cur = (name, f, []) ; funcs.append(cur); continue
    if cur is None: continue
    m = re.match(r'\s+(\S+):(\d+)\s+0x[0-9a-f]+\s+[0-9a-f]+\s+(.*)$', l)
    if m: cur[2].append((m.group(1), int(m.group(2)), m.group(3)))

def toks(ins):
    out = []
    for r in re.findall(r'R_\w+:(\S+)', ins):
        r = re.sub(r'<\d+>', '', r)
        if NOISE.search(r): continue
        m = re.match(r'\$f64\.([0-9a-f]+)', r)
        if m: out.append('f%g' % f64(m.group(1))); continue
        m = re.match(r'\$f32\.([0-9a-f]+)', r)
        if m: out.append('f32:%g' % f32(m.group(1))); continue
        if r.startswith('go:string.'): out.append(r[10:]); continue
        if r.startswith('type:'): out.append('T(' + r[5:] + ')'); continue
        if r.startswith('go:itab.'): out.append('itab(' + r[8:] + ')'); continue
        if r.startswith('main.') or '.' in r: out.append(('CALL ' if 'CALL' in ins.split()[0] or 'R_CALL' in ins else '') + r)
    m = re.match(r'(F?MOV[DSW]?|FMOVD|FMOVS)\s+\$\(([-0-9.e+]+)\)', ins)
    if m: out.append('f' + m.group(2))
    m = re.match(r'CMPW?\s+\$(-?\d+)', ins)
    if m: out.append('cmp ' + m.group(1))
    if re.match(r'CALL \(R', ins): out.append('CALL indirect')
    return out

for name, f, ins in funcs:
    if f != target and not (fre and fre.search(name)): continue
    if fre and not fre.search(name): continue
    print('=' * 8, name)
    for d in dw.get(name, []): print('   ', d.strip())
    byline = collections.OrderedDict(); order = []
    lastown = None
    for fl, ln, i in ins:
        key = (fl, ln) if fl == target else ('<' + fl, ln)
        t = toks(i)
        if key not in byline: byline[key] = []; order.append(key)
        for x in t:
            if not byline[key] or byline[key][-1] != x: byline[key].append(x)
    # print merged by own lines in ascending order, inline lines in place
    seen = set()
    for k in order:
        if k in seen: continue
        seen.add(k)
        fl, ln = k
        if fl.startswith('<'):
            if byline[k]: print('      (%s:%d) %s' % (fl[1:], ln, ' | '.join(byline[k])))
            continue
        print('  %4d: %s' % (ln, ' | '.join(byline[k])))
