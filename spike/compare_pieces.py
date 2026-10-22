"""Compare the "simple" sources of two styles per edge, up to the short-road knife edge (docs/design/rust-spike.md, S4).

    python spike/compare_pieces.py a.json b.json [--tol 2e-7]

A road shorter than its two heads is cut at h0 and n - h1, which are equal in exact arithmetic. In floating point the middle piece is one
last bit long, zero or negative, so its presence depends on the math library (glibc natively, musl-like in Pyodide and in Rust's wasm). Where
it exists it is under 2 cm after rounding, and its cut gets a second seam and lap. On an edge with such a piece, this drops it and keeps one
seam and one lap per casing number. Then every piece must match, with coordinates within ``tol`` degrees (1e-7 is one rounding step, 1.1 cm).
Prints the number of edges that differ, how many the knife edge explains, the others, and exits 1 if any is left."""
import argparse
import collections
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from compare import diff  # noqa: E402

TINY_M = 0.02


def _len_m(c):
    k = 111320 * math.cos(math.radians(c[0][1]))
    return sum(math.hypot((c[i + 1][0] - c[i][0]) * k, (c[i + 1][1] - c[i][1]) * 111320) for i in range(len(c) - 1))


def by_edge(style):
    out = collections.defaultdict(list)
    for f in style["sources"]["simple"]["data"]["features"]:
        out[f["properties"]["__rs_edge"]].append(f)
    return out


def normal(fs):
    """An edge's pieces as data, without the knife edge (see the module text): no main piece under 2 cm, and a seam or lap that repeats
    one of the same kind starting within 1e-6 deg (a cut one last bit from the other) once. Returns (pieces, whether any was dropped)."""
    out, seen, knife = [], [], False
    for f in fs:
        p, c = {k: v for k, v in f["properties"].items() if k != "__rs_s"}, f["geometry"]["coordinates"]
        if p.get("__rs_main") and _len_m(c) < TINY_M:
            knife = True
            continue
        if p.get("__rs_seam"):
            key = (p["__rs_cl"], p["__rs_k"], bool(p.get("__rs_lap")))
            if any(k == key and abs(s[0] - c[0][0]) < 1e-6 and abs(s[1] - c[0][1]) < 1e-6 for k, s in seen):
                knife = True
                continue
            seen.append((key, c[0]))
        out.append({"p": p, "c": c})
    return out, knife


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("a")
    ap.add_argument("b")
    ap.add_argument("--tol", type=float, default=2e-7)
    x = ap.parse_args(argv)
    a, b = (by_edge(json.load(open(f))) for f in (x.a, x.b))
    differ, knife, left = 0, 0, []
    for e in sorted(set(a) | set(b)):
        if json.dumps(a.get(e)) == json.dumps(b.get(e)):
            continue
        differ += 1
        (na, ka), (nb, kb) = normal(a.get(e, [])), normal(b.get(e, []))
        d = diff(na, nb, tol=x.tol, limit=3)
        if d:
            left.append((e, d))
        elif ka or kb:
            knife += 1
    print(f"{len(set(a) | set(b))} edges, {differ} differ: {knife} by the short-road knife edge, {differ - knife - len(left)} by rounding "
          f"within {x.tol:g} deg, {len(left)} otherwise")
    for e, d in left[:10]:
        print(f"  edge {e}: " + "; ".join(f"{p}: {json.dumps(u)[:50]} != {json.dumps(v)[:50]}" for p, u, v in d))
    return 1 if left else 0


if __name__ == "__main__":
    sys.exit(main())
