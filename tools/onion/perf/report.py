"""Groups the MIYOOPOD_PERF lines of a log by the PHASE markers of perf.sh.

    python3 report.py miyoopod.log

fps is frames over the scenario's whole time (a still screen draws none);
busy is the share of that time the main thread spent drawing and handing
frames over, the load the interface puts on its core.
"""
import re
import sys
from datetime import datetime

perf = re.compile(r"PERF: (\d+) frames \((\d+) unchanged\), render ([\d.]+)ms, present ([\d.]+)ms avg, max ([\d.]+)ms, late (\d+)")


def stamp(line):
    t = line.split(" - ", 1)[0].strip()
    for fmt in ("%Y-%m-%d %H:%M:%S.%f", "%Y-%m-%d %H:%M:%S"):
        try:
            return datetime.strptime(t, fmt)
        except ValueError:
            pass
    return None


phases, order, cur = {}, [], None
for line in open(sys.argv[1], errors="replace"):
    if " - PHASE " in line:
        name = line.split(" - PHASE ", 1)[1].strip()
        if cur:
            phases[cur]["end"] = stamp(line)
        cur = name
        phases[cur] = {"start": stamp(line), "end": None, "rows": []}
        order.append(cur)
        continue
    m = perf.search(line)
    if m and cur:
        phases[cur]["rows"].append(tuple(float(x) for x in m.groups()))

print("%-16s %6s %7s %9s %10s %8s" % ("scenario", "fps", "busy %", "render ms", "present ms", "max ms"))
for name in order:
    p = phases[name]
    rows = p["rows"]
    if not p["end"] or not rows:
        continue
    secs = max((p["end"] - p["start"]).total_seconds(), 1)
    frames = sum(r[0] for r in rows)
    render = sum(r[0] * r[2] for r in rows) / max(frames, 1)
    present = sum(r[0] * r[3] for r in rows) / max(frames, 1)
    busy = frames * (render + present) / 1000 / secs * 100
    print("%-16s %6.1f %7.1f %9.2f %10.2f %8.1f" % (name, frames / secs, busy, render, present, max(r[4] for r in rows)))
