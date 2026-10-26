#!/usr/bin/env python3
"""Compact annotated disassembly of the v0.20 object, grouped by source line.
usage: view.py FILE.go [funcregex]   (reads /home/claude/recover/{obj.dis,dwarf.txt,layouts.txt})"""
import re, sys, struct, collections
R = '/home/claude/recover/'
target = sys.argv[1]
fre = re.compile(sys.argv[2]) if len(sys.argv) > 2 else None

def f64(h): return struct.unpack('>d', bytes.fromhex(h.rjust(16, '0')))[0]
def f32(h): return struct.unpack('>f', bytes.fromhex(h.rjust(8, '0')))[0]

# global layouts: name -> [(off, field)]
glob = {}; structs = {}; cur = None; curg = None
for l in open(R + 'layouts.txt'):
    if l.startswith('STRUCT '):
        cur = l.split()[1]; structs[cur] = []; continue
    if l.startswith('GLOBAL '):
        p = l.split(None, 2); glob[p[1]] = p[2].strip(); cur = None; continue
    m = re.match(r'\s+\+(\d+) (\S+) (.*)', l)
    if m and cur: structs[cur].append((int(m.group(1)), m.group(2), m.group(3).strip()))
def gfield(sym, off):
    t = glob.get(sym, '')
    st = structs.get(t) or structs.get(sym)
    if not st: return '%s+%d' % (sym, off)
    best = None
    for o, n, ty in st:
        if o <= off: best = (o, n)
    if best is None: return '%s+%d' % (sym, off)
    o, n = best
    return '%s.%s%s' % (sym, n, '' if o == off else '+%d' % (off - o))

dw = collections.defaultdict(list); cur = None
for l in open(R + 'dwarf.txt'):
    if l.startswith('FUNC '): cur = l.split()[1]; continue
    if cur and l.strip(): dw[cur].append(l.rstrip())

DROP = re.compile(r'(morestack|gcWriteBarrier|runtime\.writeBarrier|arm64HasATOMICS|NOOP|^\?)')
funcs = []; cur = None
for l in open(R + 'obj.dis'):
    if l.startswith('TEXT '):
        p = l.split(); cur = (p[1][:-4], p[-1].split('/')[-1], []); funcs.append(cur); continue
    if cur is None: continue
    m = re.match(r'\s+(\S+):(\d+)\s+0x([0-9a-f]+)\s+[0-9a-f]+\s+(.*)$', l)
    if m: cur[2].append((m.group(1), int(m.group(2)), int(m.group(3), 16), m.group(4).rstrip()))

def clean(ins):
    ins = re.sub(r'\s+', ' ', ins).strip()
    m = re.search(r'\[\d+:\d+\](R_\w+):(.*)$', ins)
    rel = None
    if m:
        rel = re.sub(r'<\d+>', '', m.group(2)).strip()
        ins = ins[:m.start()].strip()
        mm = re.match(r'\$f64\.([0-9a-f]+)', rel)
        if mm: rel = '=%r' % f64(mm.group(1))
        mm = re.match(r'\$f32\.([0-9a-f]+)', rel)
        if mm: rel = '=%rf' % f32(mm.group(1))
        mm = re.match(r'(main\.\w+)\+(\d+)$', rel)
        if mm and mm.group(1) in glob: rel = gfield(mm.group(1), int(mm.group(2)))
        elif rel in glob: rel = gfield(rel, 0)
        if rel.startswith('go:string.'): rel = rel[10:]
        if rel.startswith('type:'): rel = 'T:' + rel[5:]
    if rel:
        op = ins.split(' ')[0]
        if op in ('CALL', 'JMP', 'B') and '0(PC)' in ins: return 'CALL ' + rel
        if op == 'ADRP': return '&' + rel if not rel.startswith('=') else rel
        return ins.replace('0(R27)', '').replace('(R27)', '') + ' <' + rel + '>'
    return ins

for name, f, ins in funcs:
    if name == 'main.init' and any(x[0] == target for x in ins):
        ins = [x for x in ins if x[0] == target]
    elif f != target: continue
    if fre and not fre.search(name): continue
    print('\n==== %s' % name)
    for d in dw.get(name, []): print('  ' + d.strip())
    lastkey = None; buf = []
    def flush():
        if lastkey and buf:
            fl, ln = lastkey
            tag = '%4d' % ln if fl == target else '  (%s:%d)' % (fl, ln)
            print('%s| %s' % (tag, ' ; '.join(buf)))
    start = ins[0][2] if ins else 0
    for fl, ln, addr, i in ins:
        c = clean(i)
        if DROP.search(c): continue
        if re.match(r'(MOVD R\d+, \d+\(RSP\)|MOVD \d+\(RSP\), R\d+|STP .*RSP|LDP .*RSP)', c) and False: continue
        key = (fl, ln)
        if key != lastkey:
            flush(); buf = []; lastkey = key
        buf.append('%x:%s' % ((addr - start) // 4, c) if c.split(' ')[0].startswith('B') or c.split(' ')[0] in ('JMP','CBZ','CBNZ','TBZ','TBNZ') else c)
    flush()
