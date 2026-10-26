#!/usr/bin/env python3
"""Compare a rebuilt object with the v0.20 one, function by function.
usage: cmp.py [-v] FILE.go [NEWDIS]  (FILE.go may be 'all')"""
import re, sys, collections
args = [a for a in sys.argv[1:] if not a.startswith('-')]
verbose = '-v' in sys.argv
target = args[0]
new = args[1] if len(args) > 1 else '/tmp/kpobj/obj.dis'
def load(path):
    funcs = collections.OrderedDict(); cur = None
    for l in open(path):
        if l.startswith('TEXT '):
            p = l.split(); cur = p[1][:-4]; funcs[cur] = (p[-1].split('/')[-1], []); continue
        if cur is None: continue
        m = re.match(r'\s+(\S+):(\d+)\s+0x[0-9a-f]+\s+[0-9a-f]+\s+(.*)$', l)
        if m:
            ins = re.sub(r'<\d+>', '', re.sub(r'\s+', ' ', m.group(3))).strip()
            funcs[cur][1].append('%s:%s %s' % (m.group(1), m.group(2), ins))
    return funcs
ref = load('/home/claude/recover/obj.dis'); got = load(new)
stat = collections.Counter(); bad = []
for name, (f, ins) in ref.items():
    if target != 'all' and f != target: continue
    if name not in got: stat['missing'] += 1; bad.append((name, 'missing', None)); continue
    g = got[name][1]
    if g == ins: stat['same'] += 1; continue
    i = 0
    while i < min(len(g), len(ins)) and g[i] == ins[i]: i += 1
    stat['diff'] += 1; bad.append((name, 'diff', (i, ins[i:i+6], g[i:i+6])))
print(dict(stat))
for name, k, d in bad:
    if k == 'missing': print('MISSING', name); continue
    i, a, b = d
    line = a[0].split(' ')[0] if a else (b[0].split(' ')[0] if b else '?')
    print('DIFF %s at #%d %s' % (name, i, line))
    if verbose:
        for x in a: print('   ref:', x)
        for x in b: print('   new:', x)
