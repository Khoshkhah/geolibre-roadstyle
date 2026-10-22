"""Spike S4 (docs/design/rust-spike.md): build the wasm module, run it in a worker in headless Chromium, compare its style with the golden
file (tolerance 0) and print the times next to the Pyodide numbers of spike/profile.

    mapstyle/.venv/bin/python spike/wasm/run_browser.py [--runs 3] [--no-build]

Needs cargo with the wasm32-unknown-unknown target, the wasm-bindgen program (0.2.129, as in Cargo.toml) and the fixture (spike/data)."""
import argparse
import functools
import gzip
import http.server
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
CRATE = SPIKE / "rust"
PKG = CRATE / "target" / "pkg"
DATA = SPIKE / "data"


def build():
    subprocess.run(["cargo", "build", "--release", "--offline", "--target", "wasm32-unknown-unknown", "--lib", "-q"], cwd=CRATE, check=True)
    subprocess.run(["wasm-bindgen", "--target", "web", "--out-dir", str(PKG),
                    str(CRATE / "target" / "wasm32-unknown-unknown" / "release" / "roadstyle_spike.wasm")], check=True)


def run(runs):
    with tempfile.TemporaryDirectory() as tmp:
        for name in ("index.html", "worker.js"):
            os.symlink(HERE / name, Path(tmp) / name)
        os.symlink(PKG, Path(tmp) / "pkg")
        for name in ("roads.parquet", "look.json", "layers.json"):
            os.symlink(DATA / name, Path(tmp) / name)

        class Quiet(http.server.SimpleHTTPRequestHandler):
            def log_message(self, *a, **k):
                pass
        srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Quiet, directory=tmp))
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as p:
            b = p.chromium.launch()
            pg = b.new_page()
            pg.on("console", lambda m: print("console:", m.text[:200]) if m.type == "error" else None)
            pg.goto(f"http://127.0.0.1:{srv.server_port}/index.html?runs={runs}")
            pg.wait_for_function("() => window.__result", timeout=600000, polling=200)
            r = pg.evaluate("() => window.__result")
            b.close()
        srv.shutdown()
    return r


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--no-build", action="store_true")
    a = ap.parse_args(argv)
    if not a.no_build:
        build()
    r = run(a.runs)
    if "error" in r:
        print("ERROR:", r["error"])
        return 1
    style = r.pop("style")
    made = SPIKE / "data" / "made_wasm.json"
    made.write_text(style)
    wasm = (PKG / "roadstyle_spike_bg.wasm").read_bytes()
    print(f"wasm: {len(wasm) / 1e6:.2f} MB, {len(gzip.compress(wasm, 9)) / 1e6:.2f} MB with gzip")
    print(f"start (fetch + compile + instantiate, local server): {r['initMs']:.0f} ms")
    print("makeStyle runs: " + ", ".join(f"{t:.0f} ms" for t in r["runs"]) + f"  ({len(style) / 1e6:.1f} MB of JSON)")
    print(f"JSON.parse in the worker: {r['parseMs']:.0f} ms; the page had the result after {r['pageMs']:.0f} ms (start to end)")
    prof = SPIKE / "profile" / "browser_result.json"
    if prof.exists():
        py = json.loads(prof.read_text())
        st = py["runs"][0]["stages"]
        same = sum(v for k, v in st.items() if k.startswith(("read: roads", "roads:", "simple:")))
        print(f"Pyodide, the same stages (first map): {same:.2f} s; Pyodide start + packages: {py['pyodide_start_s'] + py['packages_s']:.1f} s")
    # the reference: the style Pyodide makes (what the plugin draws today), else the native one; see compare_pieces.py for the knife edge
    ref = DATA / "golden_pyodide.json" if (DATA / "golden_pyodide.json").exists() else DATA / "golden.json"
    c = subprocess.run([sys.executable, str(SPIKE / "compare.py"), str(ref), str(made), "--only", "layers,sources.roads", "--tol", "0"],
                       capture_output=True, text=True)
    print(f"layers and roads against {ref.name} (tolerance 0):", c.stdout.strip())
    d = subprocess.run([sys.executable, str(SPIKE / "compare_pieces.py"), str(ref), str(made)], capture_output=True, text=True)
    print(f"simple against {ref.name}, per edge:", d.stdout.strip())
    s = subprocess.run([sys.executable, str(SPIKE / "compare.py"), str(DATA / "golden_slots.json"), str(made), "--only", "sources.slots",
                        "--tol", "1e-6"], capture_output=True, text=True)
    print("slots against golden_slots.json (native, tolerance 1e-6: one rounding step):", s.stdout.strip())
    return c.returncode or d.returncode or s.returncode


if __name__ == "__main__":
    sys.exit(main())
