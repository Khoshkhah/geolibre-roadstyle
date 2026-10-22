"""Compare two style files as data (docs/design/rust-spike.md, step S0).

    python spike/compare.py golden.json made.json [--tol 1e-9] [--skip metadata,name] [--max 20]

Numbers are equal when they differ by at most --tol (the text of a float can differ). The order of the keys does not matter; the order
of a list does. It prints the first differences as paths, such as ``sources.simple.data.features[812].properties.__rs_s``, and exits 1.
"""
import argparse
import json
import sys


def diff(a, b, tol=1e-9, path="", out=None, limit=20):
    """The paths where ``a`` and ``b`` differ (at most ``limit`` of them), with the two values."""
    out = [] if out is None else out
    if len(out) >= limit:
        return out
    num = lambda x: isinstance(x, (int, float)) and not isinstance(x, bool)   # noqa: E731
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(a.keys() | b.keys()):
            if k not in a or k not in b:
                out.append((f"{path}.{k}".lstrip("."), "missing" if k not in a else "present", "missing" if k not in b else "present"))
            else:
                diff(a[k], b[k], tol, f"{path}.{k}".lstrip("."), out, limit)
    elif isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            out.append((path + ".length", len(a), len(b)))
        for i, (x, y) in enumerate(zip(a, b)):
            diff(x, y, tol, f"{path}[{i}]", out, limit)
    elif num(a) and num(b):
        if abs(a - b) > tol:
            out.append((path, a, b))
    elif a != b or type(a) is not type(b) and not (num(a) and num(b)):
        out.append((path, a, b))
    return out[:limit]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("a")
    ap.add_argument("b")
    ap.add_argument("--tol", type=float, default=1e-9)
    ap.add_argument("--skip", default="", help="top-level keys to leave out, comma separated")
    ap.add_argument("--max", type=int, default=20)
    x = ap.parse_args(argv)
    skip = set(filter(None, x.skip.split(",")))
    docs = [{k: v for k, v in json.load(open(f)).items() if k not in skip} for f in (x.a, x.b)]
    d = diff(*docs, tol=x.tol, limit=x.max)
    if not d:
        print("equal")
        return 0
    for p, u, v in d:
        print(f"{p}: {json.dumps(u)[:80]} != {json.dumps(v)[:80]}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
