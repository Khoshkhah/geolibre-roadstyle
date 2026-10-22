"""Step 1 of the full port (docs/design/rust-spike.md): the worker in headless Chromium, from the duckOSM file to the style, and its checks.

    mapstyle/.venv/bin/python spike/worker/run_browser.py monaco.duckdb [--no-build]

1. The roads table the worker wrote in SQL equals spike/data/roads.parquet (from Python): the same rows, in the same order.
2. The style equals the golden files: the layers (made in Rust: the roads, step 2, and the base map, step 3), roads and images with tolerance 0,
   the base-map sources within 1e-12, simple within one rounding step (2e-7), slots within one (1e-6).
Needs the network (duckdb-wasm from jsdelivr), cargo, wasm-bindgen and the fixture (spike/data, made with the roadstyle branch fix/short-road-one-cut)."""
import argparse
import functools
import http.server
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

HERE = Path(__file__).resolve().parent
SPIKE = HERE.parent
DATA = SPIKE / "data"
_spec = importlib.util.spec_from_file_location("s4_wasm", SPIKE / "wasm" / "run_browser.py")    # the wasm module's build (S4)
_s4 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_s4)
PKG, build = _s4.PKG, _s4.build


def run(db, times_only=False):
    with tempfile.TemporaryDirectory() as tmp:
        for src, name in ((HERE / "index.html", "index.html"), (HERE / "worker.js", "worker.js"), (HERE / "roads_sql.js", "roads_sql.js"), (HERE / "features_sql.js", "features_sql.js"), (HERE / "planner_sql.js", "planner_sql.js"),
                          (PKG, "pkg"), (DATA / "look.json", "look.json"), (Path(db).resolve(), "map.duckdb")):
            os.symlink(src, Path(tmp) / name)

        class Quiet(http.server.SimpleHTTPRequestHandler):
            def log_message(self, *a, **k):
                pass
        srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Quiet, directory=tmp))
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as p:
            b = p.chromium.launch()
            pg = b.new_page()
            pg.on("console", lambda m: print("console:", m.text[:200]) if m.type == "error" else None)
            pg.goto(f"http://127.0.0.1:{srv.server_port}/index.html" + ("?times" if times_only else ""))
            pg.wait_for_function("() => window.__result", timeout=600000, polling=500)
            r = pg.evaluate("() => window.__result")
            b.close()
        srv.shutdown()
    return r


def same_rows(made, ref):
    """The rows of two Parquet files, in file order, by the reference's columns: (equal, a message)."""
    import duckdb
    con = duckdb.connect()
    cols = [c for c, *_ in con.execute(f"DESCRIBE SELECT * FROM '{ref}'").fetchall()]
    missing = [c for c in cols if c not in [c2 for c2, *_ in con.execute(f"DESCRIBE SELECT * FROM '{made}'").fetchall()]]
    if missing:
        return False, f"columns missing: {missing}"
    sel = ", ".join(f'"{c}"' for c in cols)
    a = con.execute(f"SELECT {sel} FROM '{made}'").fetchall()
    b = con.execute(f"SELECT {sel} FROM '{ref}'").fetchall()
    if len(a) != len(b):
        return False, f"{len(a)} rows, the reference has {len(b)}"
    bad = [(i, c, x[k], y[k]) for i, (x, y) in enumerate(zip(a, b)) for k, c in enumerate(cols) if x[k] != y[k]]
    if bad:
        return False, f"{len(bad)} values differ, first: row {bad[0][0]} {bad[0][1]}: {str(bad[0][2])[:40]} != {str(bad[0][3])[:40]}"
    return True, f"{len(a)} rows, {len(cols)} columns, equal"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("db")
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--times-only", action="store_true", help="only the times and the sizes (a big area: no checks)")
    a = ap.parse_args(argv)
    if not a.no_build:
        build()
    r = run(a.db, a.times_only)
    if r.get("error"):
        print("ERROR:", r["error"], r.get("times"))
        return 1
    t = r["times"]
    print("times (ms): " + ", ".join(f"{k} {v}" for k, v in t.items()) + f"; the page had the style after {r['pageMs']} ms")
    if a.times_only:
        print(f"the style: {r['styleMB']} MB")
        return 0
    made_pq, made = DATA / "worker_roads.parquet", DATA / "made_worker.json"
    made_pq.write_bytes(bytes(r["parquet"]))
    made.write_text(r["style"])
    ok, msg = same_rows(made_pq, DATA / "roads.parquet")
    print("1. the roads table from SQL against roads.parquet (Python):", msg)
    rc = 0 if ok else 1
    for mode, pq in r.get("byMode", {}).items():                 # the dialog's modes: the access column (fixture --choices)
        ref = DATA / "choices" / f"roads_{mode}.parquet"
        if ref.exists():
            (DATA / "worker_roads_mode.parquet").write_bytes(bytes(pq))
            ok, msg = same_rows(DATA / "worker_roads_mode.parquet", ref)
            print(f"1. the roads of mode {mode} from SQL against {ref.name}:", msg)
            rc = rc or (0 if ok else 1)
    ov = ",".join(f"sources.ov{i}" for i in range(sum(1 for k in json.loads((DATA / "golden_full.json").read_text())["sources"] if k.startswith("ov"))))
    for what, ref, only, tol in (("layers (roads and base map)", "golden_full.json", "layers", "0"), ("roads", "golden.json", "sources.roads", "0"),
                                 ("base-map sources", "golden_full.json", ov, "1e-12"), ("images", "golden_full.json", "metadata.roadstyle:images", "0"),
                                 ("planner data", "golden_full.json", "metadata.roadstyle:planner", "0"),
                                 ("simple", "golden.json", "sources.simple", "2e-7"),
                                 ("slots", "golden_slots.json", "sources.slots", "1e-6")):
        c = subprocess.run([sys.executable, str(SPIKE / "compare.py"), str(DATA / ref), str(made), "--only", only, "--tol", tol],
                           capture_output=True, text=True)
        print(f"2. {what} against {ref} (tolerance {tol}):", c.stdout.strip().replace("\n", "; ")[:400])
        rc = rc or c.returncode
    return rc


if __name__ == "__main__":
    sys.exit(main())
