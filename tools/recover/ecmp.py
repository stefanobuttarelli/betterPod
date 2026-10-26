#!/usr/bin/env python3
"""Equivalence check: per function, compares the multiset of normalized
instructions (registers, stack slots, branch offsets abstracted) for each
source line. 'same' = identical code, 'equiv' = same instructions per line
in a different order/allocation, 'diff' = real difference (lines listed).
usage: ecmp.py [-v] FILE.go|all [NEWDIS]"""
import re, sys, collections
args = [a for a in sys.argv[1:] if not a.startswith('-')]
verbose = '-v' in sys.argv
target = args[0]; new = args[1] if len(args) > 1 else '/tmp/kpobj/obj.dis'
SHUF = re.compile(r'^(MOVD|FMOVD|MOVB|MOVBU|MOVW|MOVWU|MOVH|MOVHU|STP|LDP) (R|F|S|ZR|\(R, R\)|\(ZR, ZR\)), (R|F|S|\(R, R\))$')
def norm(ins):
    ins = re.sub(r'<\d+>', '', ins)
    ins = re.sub(r'\[0:0\]R_USEIFACE:\S+', '', ins)
    ins = re.sub(r'main\.\.stmp_\d+', 'stmp', ins)
    ins = re.sub(r'-?\d+\(RSP\)', 'S', ins)
    ins = re.sub(r'\bF\d+\b', 'F', ins)
    ins = re.sub(r'\bR\d+\b', 'R', ins)
    ins = re.sub(r'-?\d+\(PC\)', 'PC', ins)
    ins = re.sub(r'\$-?\d+, RSP', '$N, RSP', ins)
    return re.sub(r'\s+', ' ', ins).strip()
def load(path):
    funcs = collections.OrderedDict(); cur = None
    for l in open(path):
        if l.startswith('TEXT '):
            p = l.split(); cur = p[1][:-4]; funcs[cur] = (p[-1].split('/')[-1], [], []); continue
        if cur is None: continue
        m = re.match(r'\s+(\S+):(\d+)\s+0x[0-9a-f]+\s+[0-9a-f]+\s+(.*)$', l)
        if m:
            raw = re.sub(r'<\d+>', '', re.sub(r'\s+', ' ', m.group(3))).strip()
            funcs[cur][1].append('%s:%s %s' % (m.group(1), m.group(2), raw))
            n = norm(m.group(3))
            if SHUF.match(n) or n in ('NOOP', '?'): continue
            funcs[cur][2].append('%s:%s %s' % (m.group(1), m.group(2), n))
    return funcs
ref = load('/home/claude/recover/obj.dis'); got = load(new)
stat = collections.Counter(); out = []
for name, (f, raw, ns) in ref.items():
    if target != 'all' and f != target: continue
    if name not in got: stat['missing'] += 1; out.append('MISSING ' + name); continue
    g = got[name]
    if g[1] == raw: stat['same'] += 1; continue
    ca, cb = collections.Counter(ns), collections.Counter(g[2])
    if ca == cb: stat['equiv'] += 1; continue
    stat['diff'] += 1
    lines = collections.defaultdict(lambda: [collections.Counter(), collections.Counter()])
    for k, v in ca.items(): lines[k.split(' ')[0]][0][k] += v
    for k, v in cb.items(): lines[k.split(' ')[0]][1][k] += v
    bad = sorted((l for l, (x, y) in lines.items() if x != y), key=lambda s: (s.split(':')[0], int(s.split(':')[1])))
    out.append('DIFF %s  lines: %s' % (name, ' '.join(bad[:12]) + (' …' if len(bad) > 12 else '')))
    if verbose:
        for l in bad:
            x, y = lines[l]
            for k in sorted(set(x) | set(y)):
                if x[k] != y[k]: out.append('    %s ref=%d new=%d' % (k, x[k], y[k]))
print(dict(stat))
for l in out: print(l)
