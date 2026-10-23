"""The plugin's map of a database in the real GeoLibre (https://web.geolibre.app), in a headless browser: install dist/roadstyle-geolibre.zip
and the probe plugin (tests/probe: it gives the test the map), open a database through the plugin's dialog three times (the first map, the
same again from the browser's storage, other choices), and check the map: the road layers and the names and arrows come in one style, the
base-map rows are there, the route planner draws a route (and Walk + drive, when the database has it). Prints the times.

    .venv/bin/python build.py
    .venv/bin/python tests/e2e_maps.py monaco.duckdb [screenshots-folder]      # or a bigger area (Tartu: about 64,000 roads)

Needs the network (web.geolibre.app, jsdelivr), playwright and a few minutes."""
import sys
import tempfile
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

sys.path.insert(0, str(Path(__file__).resolve().parent))
from e2e_geolibre import ZIP, activate, probe_zip  # noqa: E402

MENU = "roadstyle"


def open_db(pg, db, menu=MENU, choose=None):
    """The plugin's menu > Open a duckOSM database, with the dialog's choices (mode, theme, paths: None keeps the dialog's); returns the seconds
    from the click on Open to the map (MapLibre idle) and the steps of the progress box."""
    pg.get_by_role("button", name=menu).first.click()
    pg.get_by_text("Open a duckOSM database", exact=False).first.click()
    pg.wait_for_selector("[role=dialog] input[type=file]")
    pg.set_input_files("[role=dialog] input[type=file]", str(db))
    for i, v in enumerate(choose or ()):
        pg.select_option(f"[role=dialog] select >> nth={i}", v)
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
    return time.time() - t0, [s for _, s in steps]


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
        activate(pg, "roadstyle")
        activate(pg, "rs-probe")
        assert pg.evaluate("() => !!(window.__rs && window.__rs.map)"), "the probe has no map"

        print("first map (DuckDB and the Rust module start):")
        t1, _ = open_db(pg, db)
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
            pg.screenshot(path=str(Path(out) / "map.png"))
        print("page errors:", errors[-5:], "| console errors and warnings:", said[-8:])
        assert info["simple"] and info["arrows"] and info["labels"] and info["base"] > 0, info
        assert pl["dom"] and pl["pins"] == 2 and pl["route"] and drawn > 0, (pl, drawn)

        def again(choose, label):
            print(label)
            pg.get_by_role("button", name=MENU).first.click()
            pg.get_by_text("Remove roadstyle layers", exact=False).first.click()
            pg.wait_for_timeout(2000)
            t, steps = open_db(pg, db, choose=choose)
            pg.wait_for_timeout(1500)
            n = pg.evaluate("() => window.__rs.map.getStyle().layers.filter((l) => /^rs\\d+-/.test(l.id)).length")
            assert n > 50 and not errors, (n, errors)
            return t, steps
        t2, steps = again(("all", "osm", "google"), "second map (the same file and choices: from the browser's storage):")
        assert not any("making the map" in s for s in steps), steps        # the box's "the map from the browser's storage" is too short to see
        t3, steps = again(("walking", "grey", "komoot"), "third map (walking, grey, Komoot; DuckDB and the Rust module are warm):")
        assert any("making the map (Rust)" in s for s in steps), steps
        print(f"times: first map {t1:.1f} s, the same again {t2:.1f} s, other choices {t3:.1f} s")
        browser.close()


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else None)
