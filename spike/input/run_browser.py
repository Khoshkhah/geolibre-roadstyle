"""Condition 3 of docs/design/rust-spike.md: run the duckdb-wasm probe (index.html) in headless Chromium for some duckdb-wasm versions, and the
same SQL natively, and compare the rows.

    mapstyle/.venv/bin/python spike/input/run_browser.py monaco.duckdb [1.32.0 1.33.1-dev65.0 ...]

Needs spike/input/roads.sql (make_sql.py) and the network (jsdelivr). A new browser for each version: a cold cache, as on a first visit."""
import functools
import http.server
import os
import sys
import tempfile
import threading
from pathlib import Path

from playwright.sync_api import sync_playwright

HERE = Path(__file__).resolve().parent
CHECK = ("SELECT count(*), md5(string_agg(edge_id || '|' || coalesce(highway, '') || '|' || coalesce(name, '') || '|' || hex(geometry), ',' "
         "ORDER BY edge_id)) FROM 'roads.parquet'")


def native(db):
    import duckdb
    with tempfile.TemporaryDirectory() as tmp:
        os.chdir(tmp)
        con = duckdb.connect()
        con.execute(f"ATTACH '{Path(db).resolve()}' AS monaco (READ_ONLY); USE monaco")
        for s in (HERE / "roads.sql").read_text().split(";\n\n"):
            if s.strip().rstrip(";"):
                con.execute(s.strip().rstrip(";"))
        n, md5 = con.execute(CHECK).fetchone()
    os.chdir(HERE)
    return n, md5


def probe(db, versions):
    with tempfile.TemporaryDirectory() as tmp:
        for src, name in ((HERE / "index.html", "index.html"), (HERE / "roads.sql", "roads.sql"), (Path(db).resolve(), "monaco.duckdb")):
            os.symlink(src, Path(tmp) / name)

        class Quiet(http.server.SimpleHTTPRequestHandler):
            def log_message(self, *a, **k):
                pass
        srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Quiet, directory=tmp))
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        out = []
        with sync_playwright() as p:
            for v in versions:
                b = p.chromium.launch()
                pg = b.new_page()
                pg.goto(f"http://127.0.0.1:{srv.server_port}/index.html?v={v}")
                pg.wait_for_function("() => window.__result", timeout=300000, polling=500)
                out.append(pg.evaluate("() => window.__result"))
                b.close()
        srv.shutdown()
    return out


def main(db, versions):
    n, md5 = native(db)
    print(f"native: {n} rows, md5 {md5}")
    for r in probe(db, versions):
        if "error" in r:
            print(f"{r['v']}: {r.get('duckdb', '?')}  ERROR {r['error'][:300]}")
            continue
        same = "the same rows as native" if (r["rows"], r["md5"]) == (n, md5) else f"DIFFERENT rows ({r['rows']}, {r['md5']})"
        print(f"{r['v']} ({r['duckdb']}, {r['bundle']}): module {r['wasmMB']:.1f} MB, {r['wasmWireMB']:.1f} MB on the wire; start {r['startMs']} ms; "
              f"attach {r['attachMs']} ms; roads SQL + Parquet {r['sqlMs']} ms; copy {r['copyMs']} ms; {r['parquetMB']:.2f} MB; {same}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1], sys.argv[2:] or ["1.32.0", "1.33.1-dev57.0", "1.33.1-dev65.0"]))
