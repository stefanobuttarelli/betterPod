#!/usr/bin/env python3
"""Compare reconstructed code with the v0.20 object code, function by function.

usage: compare.py ORIG.objdump NEW.objdump [file.go ...]
Prints, per function: OK, DIFF (with differing lines) or MISSING.
"""
import subprocess, sys, re, collections, os
here = os.path.dirname(os.path.abspath(__file__))

def summ(objdump, f):
    out = subprocess.run([sys.executable, os.path.join(here, 'summarize.py'), objdump, '/dev/null', f],
                         capture_output=True, text=True).stdout
    funcs = collections.OrderedDict()
    cur = None
    for l in out.splitlines():
        if l.startswith('== '):
            cur = l[3:].split('  [')[0]
            funcs[cur] = {}
        elif cur and re.match(r'\s*\d+: ', l):
            n, rest = l.split(':', 1)
            toks = [t for t in rest.strip().split(' | ') if t]
            if os.environ.get('STRICT') != '1':
                toks = [re.sub(r'^\[([^\]:]+):\d+\]', r'[\1]', t) for t in toks]
                toks = [re.sub(r'\.stmp_\d+', '.stmp', t) for t in toks]
                toks = [re.sub(r'\.(func|gowrap|deferwrap)\d+', r'.\1N', t) for t in toks]
            # drop pure register-offset noise for robustness
            funcs[cur][int(n)] = sorted(set(toks))
    return funcs

orig_od, new_od = sys.argv[1], sys.argv[2]
files = sys.argv[3:]
if not files:
    files = sorted(set(re.findall(r'^TEXT \S+ \S+/(\S+\.go)$', open(orig_od).read(), re.M)))
verbose = os.environ.get('V') == '1'
tot = collections.Counter()
for f in files:
    a, b = summ(orig_od, f), summ(new_od, f)
    st = collections.Counter()
    lines = []
    for name, la in a.items():
        lb = b.get(name)
        if lb is None:
            st['missing'] += 1
            lines.append(f'  MISSING {name}')
            continue
        if la == lb:
            st['ok'] += 1
            continue
        st['diff'] += 1
        lines.append(f'  DIFF {name}')
        if verbose:
            for n in sorted(set(la) | set(lb)):
                if la.get(n) != lb.get(n):
                    lines.append(f'     {n:5d} orig: {" | ".join(la.get(n, []))}')
                    lines.append(f'     {n:5d}  new: {" | ".join(lb.get(n, []))}')
    for name in b:
        if name not in a:
            st['extra'] += 1
            lines.append(f'  EXTRA {name}')
    tot += st
    print(f'{f}: ok={st["ok"]} diff={st["diff"]} missing={st["missing"]} extra={st["extra"]}')
    if os.environ.get('Q') != '1':
        for l in lines:
            print(l)
print('TOTAL', dict(tot))
