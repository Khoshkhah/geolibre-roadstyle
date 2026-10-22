"""Compare the "decks" sources of two styles (docs/design/rust-spike.md, full port step 5).

    python spike/compare_decks.py golden.json made.json [--min-aligned 0.99]

The two lists of deck features are aligned by their keys (chain, ribbon or slab, edges, base, height): GEOS can make no slab, or two, on the inner
side of a sharply bent part, where the port makes one. The rules: 1. at least --min-aligned of the golden features align; 2. every aligned
feature has the same properties (tolerance 0); 3. a polygon that has four corners on both sides (a straight part: GEOS's buffer exactly) has the
same corners within 1e-6 deg, the ring compared from its lowest point in either orientation. The other aligned polygons (a bent part: GEOS's
offset-curve and buffer builders are not ported exactly) are reported: the Hausdorff distance between the two outlines, in metres. Exits 1 if a
rule fails."""
import argparse
import difflib
import json
import math
import statistics
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from compare import diff  # noqa: E402


def ring(f):
    r = [tuple(p) for p in f["geometry"]["coordinates"][0]]
    return r[:-1] if r and r[0] == r[-1] else r


def same_ring(a, b, tol=1e-6):
    if len(a) != len(b):
        return False
    j = a.index(min(a))
    aa = a[j:] + a[:j]
    for r in (b, b[::-1]):
        k = r.index(min(r))
        r = r[k:] + r[:k]
        if all(abs(p[0] - q[0]) <= tol and abs(p[1] - q[1]) <= tol for p, q in zip(aa, r)):
            return True
    return False


def hausdorff_m(fa, fb):
    from shapely.geometry import shape
    a, b = shape(fa["geometry"]).boundary, shape(fb["geometry"]).boundary
    lat = fa["geometry"]["coordinates"][0][0][1]
    return a.hausdorff_distance(b) * 111320 * max(math.cos(math.radians(lat)), 0.5)


def key(f):
    p = f["properties"]
    return json.dumps([p["__rs_chain"], bool(p.get("__rs_casing_slab")), p["__rs_edges"], p["__rs_base"], p["__rs_height"]])


def compare(ga, gb, min_aligned=0.99):
    """(ok, lines of the report)."""
    sm = difflib.SequenceMatcher(a=[key(f) for f in ga], b=[key(f) for f in gb], autojunk=False)
    pairs = [(i, j) for a, b, n in sm.get_matching_blocks() for i, j in zip(range(a, a + n), range(b, b + n))]
    props = [i for i, j in pairs if diff(ga[i]["properties"], gb[j]["properties"], tol=0, limit=1)]
    rect = [(i, j) for i, j in pairs if len(ring(ga[i])) == 4 and len(ring(gb[j])) == 4]
    rect_bad = [i for i, j in rect if not same_ring(ring(ga[i]), ring(gb[j]))]
    bent = sorted(hausdorff_m(ga[i], gb[j]) for i, j in pairs if not (len(ring(ga[i])) == 4 and len(ring(gb[j])) == 4))
    share = len(pairs) / max(len(ga), 1)
    lines = [f"{len(ga)} and {len(gb)} deck features; {len(pairs)} aligned ({share:.1%}): {len(ga) - len(pairs)} only in the first, {len(gb) - len(pairs)} only in the second",
             f"properties of the aligned: {len(pairs) - len(props)} equal, {len(props)} differ",
             f"straight parts (four corners on both sides): {len(rect) - len(rect_bad)} of {len(rect)} equal within 1e-6 deg",
             f"bent parts: {len(bent)}; outline distance median {statistics.median(bent) if bent else 0:.3f} m, "
             f"90 % {bent[int(len(bent) * 0.9)] if bent else 0:.3f} m, largest {bent[-1] if bent else 0:.3f} m"]
    return share >= min_aligned and not props and not rect_bad, lines


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("a")
    ap.add_argument("b")
    ap.add_argument("--min-aligned", type=float, default=0.99)
    x = ap.parse_args(argv)
    ga, gb = (json.load(open(f))["sources"].get("decks", {}).get("data", {}).get("features", []) for f in (x.a, x.b))
    ok, lines = compare(ga, gb, x.min_aligned)
    print("\n".join(lines))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
