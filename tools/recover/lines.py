#!/usr/bin/env python3
"""Compare function declaration lines of a source file with v0.20.
usage: lines.py src/FILE.go"""
import re, subprocess, sys, os
src = sys.argv[1]; base = os.path.basename(src)
here = os.path.dirname(os.path.abspath(__file__))
out = subprocess.run(['go', 'run', '.', os.path.abspath(src)], cwd=os.path.join(here, '..', 'funclines'), capture_output=True, text=True).stdout
mine = {}
for l in out.splitlines():
    f, a, b, n = l.split('\t'); mine[n] = (int(a), int(b))
# reference: DWARF param decl lines (first param line) per function, else first own-file line in obj.dis
ref = {}
cur = None
for l in open('/home/claude/recover/dwarf.txt'):
    if l.startswith('FUNC '):
        cur = l.split()[1]; continue
    m = re.match(r'\s{4}param \S+ .* @(\d+)$', l)
    if cur and m and cur not in ref and int(m.group(1)) > 0:
        ref[cur] = int(m.group(1))
first = {}
curf = None
for l in open('/home/claude/recover/obj.dis'):
    if l.startswith('TEXT '):
        p = l.split(); curf = p[1][:-4]; fl = p[2].split('/')[-1]; continue
    m = re.match(r'\s+(\S+?):(\d+)\s', l)
    if m and curf and m.group(1) == base and curf not in first:
        first[curf] = int(m.group(2))
bad = 0
for n, (a, b) in sorted(mine.items(), key=lambda x: x[1]):
    r = ref.get(n) or first.get(n)
    if r is None:
        print(f'  ?    {a:5d} {n} (not in v0.20 object)'); continue
    flag = 'ok ' if r == a else 'BAD'
    if r != a: bad += 1
    print(f'  {flag} {a:5d} ref {r:5d} {n}')
print('mismatched:', bad)
