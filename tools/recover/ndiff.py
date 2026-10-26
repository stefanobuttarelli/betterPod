#!/usr/bin/env python3
"""Like fdiff.py but normalizes registers and stack offsets, to show logic
differences only. usage: ndiff.py FUNCNAME [NEWDIS]"""
import re, sys, difflib
name = sys.argv[1]; new = sys.argv[2] if len(sys.argv) > 2 else '/tmp/kpobj/obj.dis'
def norm(ins):
    ins = re.sub(r'<\d+>', '', ins)
    ins = re.sub(r'\b-?\d+\(RSP\)', 'S', ins)
    ins = re.sub(r'\bF\d+\b', 'F', ins)
    ins = re.sub(r'\bR\d+\b', 'R', ins)
    ins = re.sub(r'-?\d+\(PC\)', 'PC', ins)
    ins = re.sub(r'\[0:0\]R_USEIFACE:\S+', '', ins)
    return re.sub(r'\s+', ' ', ins).strip()
def load(p):
    out = []; cur = None
    for l in open(p):
        if l.startswith('TEXT '):
            cur = l.split()[1][:-4]; continue
        if cur == name:
            m = re.match(r'\s+(\S+):(\d+)\s+0x[0-9a-f]+\s+[0-9a-f]+\s+(.*)$', l)
            if m:
                n = norm(m.group(3))
                if re.match(r'^(MOVD|FMOVD|MOVB|MOVBU|STP|LDP) (R|F|S|\(R, R\)), (R|F|S|\(R, R\))$', n):
                    continue  # register/stack shuffles
                out.append('%s:%s %s' % (m.group(1), m.group(2), n))
    return out
a = load('/home/claude/recover/obj.dis'); b = load(new)
for l in difflib.unified_diff(a, b, 'ref', 'new', n=1, lineterm=''): print(l)
