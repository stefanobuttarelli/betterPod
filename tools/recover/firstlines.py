#!/usr/bin/env python3
"""First and last own-file line of each function: v0.20 vs rebuilt.
usage: firstlines.py FILE.go [NEWDIS]"""
import re, sys
f = sys.argv[1]; new = sys.argv[2] if len(sys.argv) > 2 else '/tmp/kpobj/obj.dis'
def load(p):
    d = {}; cur = None; cf = None
    for l in open(p):
        if l.startswith('TEXT '):
            q = l.split(); cur = q[1][:-4]; cf = q[2].split('/')[-1]; continue
        m = re.match(r'\s+(\S+?):(\d+)\s', l)
        if m and cur and cf == f and m.group(1) == f:
            n = int(m.group(2)); a = d.setdefault(cur, [n, n, n]); a[1] = min(a[1], n); a[2] = max(a[2], n)
    return d
r = load('/home/claude/recover/obj.dis'); g = load(new)
for k in r:
    a = r[k]; b = g.get(k)
    if b is None: print('MISSING', k); continue
    if a != b: print('%-50s ref first=%d min=%d max=%d | new first=%d min=%d max=%d  (shift %+d)' % (k, a[0], a[1], a[2], b[0], b[1], b[2], a[0]-b[0]))
