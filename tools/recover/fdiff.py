#!/usr/bin/env python3
"""Unified diff of one function: v0.20 vs rebuilt. usage: fdiff.py FUNCNAME [NEWDIS]"""
import re, sys, difflib
name = sys.argv[1]; new = sys.argv[2] if len(sys.argv) > 2 else '/tmp/kpobj/obj.dis'
def load(p):
    out = []; cur = None
    for l in open(p):
        if l.startswith('TEXT '):
            cur = l.split()[1][:-4]; continue
        if cur == name:
            m = re.match(r'\s+(\S+):(\d+)\s+0x[0-9a-f]+\s+[0-9a-f]+\s+(.*)$', l)
            if m: out.append('%s:%s %s' % (m.group(1), m.group(2), re.sub(r'<\d+>', '', re.sub(r'\s+', ' ', m.group(3))).strip()))
    return out
a = load('/home/claude/recover/obj.dis'); b = load(new)
for l in difflib.unified_diff(a, b, 'ref', 'new', n=2, lineterm=''): print(l)
