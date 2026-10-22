"""rs_profile.py in Pyodide, in headless Chromium, with the plugin's own setup (py/worker.js: the CDN version, the packages, py/pkgs.zip).

    mapstyle/.venv/bin/python spike/profile/run_browser.py monaco.duckdb        # about 40 s with the CDN files in the browser's cache

Serves a temporary folder (this folder's files, pkgs.zip and the database), opens index.html and waits for the worker's result."""
import functools
import http.server
import json
import os
import sys
import tempfile
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

HERE = Path(__file__).resolve().parent
PKGS = HERE.parent.parent / "py" / "pkgs.zip"


def main(db):
    with tempfile.TemporaryDirectory() as tmp:
        for src, name in ((HERE / "index.html", "index.html"), (HERE / "worker.js", "worker.js"), (HERE / "rs_profile.py", "rs_profile.py"),
                          (PKGS, "pkgs.zip"), (Path(db).resolve(), "monaco.duckdb")):
            os.symlink(src, Path(tmp) / name)

        class Quiet(http.server.SimpleHTTPRequestHandler):
            def log_message(self, *a, **k):
                pass
        srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Quiet, directory=tmp))
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        with sync_playwright() as p:
            b = p.chromium.launch()
            pg = b.new_page()
            pg.on("console", lambda m: print("console:", m.text[:160]) if m.type == "error" else None)
            pg.goto(f"http://127.0.0.1:{srv.server_port}/index.html")
            pg.wait_for_function("() => window.__result", timeout=900000, polling=1000)
            r = pg.evaluate("() => window.__result")
            b.close()
        srv.shutdown()
    (HERE / "browser_result.json").write_text(json.dumps(r, indent=1))
    if "error" in r:
        print("ERROR:", r["error"])
        return 1
    sys.path.insert(0, str(HERE))
    from rs_profile import report
    print(f"Pyodide start {r['pyodide_start_s']:.1f} s, packages {r['packages_s']:.1f} s, imports {r['import_s']:.1f} s")
    for i, run in enumerate(r["runs"], 1):
        print(f"--- run {i} in the browser")
        report(run)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
