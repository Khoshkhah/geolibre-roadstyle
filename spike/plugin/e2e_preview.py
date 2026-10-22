"""The Rust preview in the real GeoLibre (https://web.geolibre.app), in a headless browser: install dist/roadstyle-rust-preview.zip and the probe
plugin (tests/probe: it gives the test the map), open a database through the preview's dialog with the default choices, and check the map: the
road layers and the names and arrows come in one style, the base-map rows are there, the route planner draws a route. Prints the time from the
click on Open to the map.

    mapstyle/.venv/bin/python spike/plugin/build_preview.py
    mapstyle/.venv/bin/python spike/plugin/e2e_preview.py monaco.duckdb [screenshots-folder]

Needs the network (web.geolibre.app, jsdelivr), playwright and a few minutes."""
import sys
import tempfile
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT / "tests"))
from e2e_geolibre import activate, probe_zip  # noqa: E402

ZIP = ROOT / "dist" / "roadstyle-rust-preview.zip"


def open_db(pg, db, menu="roadstyle (Rust)"):
    """The plugin's menu > Open a duckOSM database, the default choices; returns the seconds from the click on Open to the map (MapLibre idle)."""
    pg.get_by_role("button", name=menu).first.click()
    pg.get_by_text("Open a duckOSM database", exact=False).first.click()
    pg.wait_for_selector("[role=dialog] input[type=file]")
    pg.set_input_files("[role=dialog] input[type=file]", str(db))
    t0 = time.time()
    pg.click("[role=dialog] button[type=submit]")
    pg.wait_for_selector(".rs-pl-progress", timeout=20000)
    steps, last = [], None
    while pg.query_selector(".rs-pl-progress"):
        txt = pg.evaluate("() => (document.querySelector('.rs-pl-progress') || {}).innerText || ''").strip().replace("\n", " | ")[:90]
        if txt != last:
            steps.append((round(time.time() - t0, 1), txt))
            last = txt
        pg.wait_for_timeout(200)
    t_box = time.time() - t0
    pg.wait_for_function("() => window.__rs.map.loaded()", timeout=300000)
    for t, s in steps:
        print(f"  {t:6.1f} s  {s}")
    print(f"  {t_box:6.1f} s  the progress box is gone; {time.time() - t0:.1f} s  the map is idle")
    return time.time() - t0


def main(db, out=None):
    with tempfile.TemporaryDirectory() as tmp, sync_playwright() as p:
        browser = p.chromium.launch(args=["--use-gl=swiftshader", "--enable-unsafe-swiftshader"])
        pg = browser.new_page(viewport={"width": 1500, "height": 900})
        errors, said = [], []
        pg.on("pageerror", lambda e: errors.append(str(e)[:300]))
        pg.on("console", lambda m: said.append(m.text[:300]) if m.type in ("error", "warning") else None)
        pg.goto("https://web.geolibre.app/", wait_until="load")
        pg.get_by_role("button", name="Settings").first.click(timeout=90000)
        pg.get_by_text("Manage Plugins", exact=True).first.click()
        pg.locator("[role=dialog] button", has_text="Settings").last.click()
        for z in (ZIP, probe_zip(tmp)):
            with pg.expect_file_chooser() as fc:
                pg.get_by_role("button", name="Choose .zip…").click()
            fc.value.set_files(str(z))
            pg.wait_for_timeout(3500)
        pg.locator("[role=dialog] button", has_text="Close").last.click()
        pg.wait_for_timeout(800)
        activate(pg, "roadstyle (Rust preview)")
        activate(pg, "rs-probe")
        assert pg.evaluate("() => !!(window.__rs && window.__rs.map)"), "the probe has no map"

        print("first map (DuckDB and the Rust module start):")
        t1 = open_db(pg, db)
        pg.wait_for_timeout(3000)
        info = pg.evaluate("""() => { const m = window.__rs.map, ls = m.getStyle().layers.filter((l) => /^rs\\d+-/.test(l.id));
            return {layers: ls.length, arrows: ls.some((l) => /roads-arrows$/.test(l.id)), labels: ls.some((l) => /roads-labels$/.test(l.id)),
                    simple: ls.some((l) => /roads-simple$/.test(l.id)), base: ls.filter((l) => /-ov\\d+-/.test(l.id)).length,
                    rows: window.__rs.app.listLayers().map((l) => l.name)}; }""")
        print("layers:", {k: v for k, v in info.items() if k != "rows"}, "| rows:", info["rows"][:12])
        pl = pg.evaluate("""() => ({dom: !!document.getElementById('rm-panel'), pins: document.querySelectorAll('.rm-mk').length,
            steps: (window.rmSteps || []).length, route: !!(window.rmLast && window.rmLast.res)})""")
        pg.wait_for_timeout(1500)
        drawn = pg.evaluate("() => window.__rs.map.getLayer('rm-route') ? window.__rs.map.queryRenderedFeatures({layers: ['rm-route']}).length : 0")
        print("route planner:", pl, "| route pieces drawn:", drawn)
        wd = pg.evaluate("""() => { const s = document.getElementById('rm-mode'), i = [...s.options].findIndex((o) => o.textContent === 'Walk + drive');
            if (i < 0) return null; s.value = String(i); s.dispatchEvent(new Event('change'));
            const f = window.rmLast.res; document.querySelector('#rm-weight button[data-w=length]').click(); const g = window.rmLast.res;
            return {fastest: [Math.round(f.time), Math.round(f.len)], shortest: [Math.round(g.time), Math.round(g.len)]}; }""")
        print("walk + drive (s, m):", wd)                          # Shortest: shorter; Fastest: faster
        if out:
            Path(out).mkdir(parents=True, exist_ok=True)
            pg.screenshot(path=str(Path(out) / "rust_preview.png"))
        print("page errors:", errors[-5:], "| console errors and warnings:", said[-8:])
        assert info["simple"] and info["arrows"] and info["labels"] and info["base"] > 0, info
        assert pl["dom"] and pl["pins"] == 2 and pl["route"] and drawn > 0, (pl, drawn)

        print("second map (the same file; DuckDB and the Rust module are warm; the preview keeps no style in the browser's storage):")
        pg.get_by_role("button", name="roadstyle (Rust)").first.click()
        pg.get_by_text("Remove roadstyle layers", exact=False).first.click()
        pg.wait_for_timeout(2000)
        t2 = open_db(pg, db)
        print(f"times: first map {t1:.1f} s, second map {t2:.1f} s")
        browser.close()


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else None)
