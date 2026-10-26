# Recovery status

Per file: functions identical to v0.20 machine code / total (tools/recover/cmp.py).
Remaining differences are listed with the reason when known.

| file | identical | notes |
|---|---|---|
| ij_gfx.go | 25/26 | fillRoundedGradient: spill order of `stops` and operand order of one FMUL (same maths) |
| ij_theme.go | recovered verbatim | diffs only from stubbed callees |
| ij_libinfo.go | recovered verbatim | diffs only from stubbed callees |
