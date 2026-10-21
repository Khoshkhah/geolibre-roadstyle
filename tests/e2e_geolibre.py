"""The plugin in the real GeoLibre (https://web.geolibre.app), in a headless browser: install the .zip (and a probe plugin, tests/probe,
that gives the test the map), open a database through the plugin's dialog, and check the map:

- the arrows use roadstyle's image (GeoLibre's base map has an image 'oneway' of its own: a name clash),
- the crossings and the traffic signals show with the choice "Show all" at zoom 17,
- the "residential" road type switches its roads off.

Needs the network, playwright, duckdb and a few minutes.

    python build.py --zip
    python tests/e2e_geolibre.py /path/to/monaco.duckdb [screenshots-folder]
"""
import io
import sys
import tempfile
import zipfile
from pathlib import Path

import duckdb
from PIL import Image, ImageChops
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parent.parent
ZIP = ROOT / "dist" / "roadstyle-geolibre.zip"


def changed(a, b):
    d = ImageChops.difference(Image.open(io.BytesIO(a)).convert("RGB"), Image.open(io.BytesIO(b)).convert("RGB"))
    return sum(1 for v in d.getdata() if sum(v) > 40)


def probe_zip(folder):
    out = Path(folder) / "rs-probe.zip"
    with zipfile.ZipFile(out, "w") as z:
        for f in ("plugin.json", "index.js"):
            z.write(ROOT / "tests" / "probe" / f, f)
    return out


def activate(pg, name):
    """Plugins > Installed > name (a plugin that is already active has a tick)"""
    import re
    pg.get_by_role("button", name="Plugins").first.click()
    pg.get_by_text(re.compile(r"^Installed")).first.click()
    items = [e.inner_text().strip() for e in pg.query_selector_all("[role=menuitem], [role=menuitemcheckbox], [role=menu] button")]
    if any(i.startswith(name) and "✓" in i for i in items):
        pg.keyboard.press("Escape")
        pg.keyboard.press("Escape")
        return
    pg.get_by_text(name, exact=True).first.click()
    pg.wait_for_timeout(1500)


def main(db, out=None):
    con = duckdb.connect(str(db), read_only=True)
    con.execute("LOAD spatial")
    lng, lat = con.execute("SELECT ST_X(geom), ST_Y(geom) FROM features.traffic WHERE kind = 'crossing' LIMIT 1").fetchone()
    con.close()
    with tempfile.TemporaryDirectory() as tmp, sync_playwright() as p:
        browser = p.chromium.launch(args=["--use-gl=swiftshader", "--enable-unsafe-swiftshader"])
        pg = browser.new_page(viewport={"width": 1500, "height": 900})
        errors, said = [], []
        pg.on("pageerror", lambda e: errors.append(str(e)[:200]))
        pg.on("console", lambda m: said.append(m.text) if "roadstyle" in m.text.lower() else None)
        pg.goto("https://web.geolibre.app/", wait_until="load")
        pg.wait_for_timeout(15000)
        pg.get_by_role("button", name="Settings").first.click()
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
        base = pg.evaluate("() => { const i = window.__rs.map.getImage('oneway'); return i && i.data.width; }")
        pg.get_by_role("button", name="roadstyle").first.click()
        pg.get_by_text("Open a duckOSM database", exact=False).first.click()
        pg.wait_for_selector("[role=dialog] input[type=file]")
        pg.set_input_files("[role=dialog] input[type=file]", str(db))
        assert pg.eval_on_selector_all("[role=dialog] select", "(ss) => ss.map((s) => s.value)")[3:] == ["off", "off"]      # the defaults: no ocean polygon, no 3D bridge decks
        pg.click("[role=dialog] button[type=submit]")
        pg.wait_for_selector(".rs-pl-progress", timeout=20000)
        pg.wait_for_selector(".rs-pl-progress", state="detached", timeout=400000)
        pg.wait_for_timeout(8000)

        # 1) the arrows: roadstyle's own image, not the base map's
        arrows = pg.evaluate("""() => { const m = window.__rs.map, id = m.getLayoutProperty('rs0-roads-arrows', 'icon-image'), i = m.getImage(id);
                                        return {id, w: i && i.data.width, sdf: i && i.sdf, base: m.getImage('oneway').data.width}; }""")
        print("arrows:", arrows, "(the base map's own 'oneway' image:", base, "px wide)")
        assert arrows["id"] == "rs0-oneway" and arrows["sdf"] is True and arrows["w"] != arrows["base"], arrows

        # 2) the crossings and the traffic signals show with "Show all"
        pg.evaluate("([lng, lat]) => window.__rs.map.jumpTo({center: [lng, lat], zoom: 17.5, pitch: 0, bearing: 0})", [lng, lat])
        pg.wait_for_timeout(6000)
        feat = pg.evaluate("""() => { const m = window.__rs.map, out = {};
            for (const g of ['crossings', 'traffic_signals']) {
              const ids = m.getStyle().layers.filter((l) => l.metadata && l.metadata['roadstyle:group'] === g).map((l) => l.id);
              out[g] = {layers: ids.length, visible: ids.filter((i) => m.getLayoutProperty(i, 'visibility') !== 'none').length,
                        drawn: ids.length ? m.queryRenderedFeatures({layers: ids}).length : 0}; }
            return out; }""")
        print("feature layers:", feat)
        assert feat["crossings"]["layers"] > 0 and feat["crossings"]["visible"] == feat["crossings"]["layers"], feat
        assert feat["crossings"]["drawn"] > 0, feat
        # GeoLibre puts a folder at the place of its first row: the features must be drawn over the roads, as on the mapstyle page
        order = pg.evaluate("""() => { const L = window.__rs.map.getStyle().layers, g = (l) => l.metadata && l.metadata['roadstyle:group'];
            const at = (name, first) => { const k = L.map((l, i) => g(l) === name ? i : -1).filter((i) => i >= 0); return first ? Math.min(...k) : Math.max(...k); };
            return {roadsLast: at('roads', false), crossingsFirst: at('crossings', true), signalsFirst: at('traffic_signals', true), railwaysLast: at('railways', false), roadsFirst: at('roads', true)}; }""")
        groups = pg.evaluate("""() => { const c = {}; for (const l of window.__rs.map.getStyle().layers) { const g = l.metadata && l.metadata['roadstyle:group'];
            if (!g) continue; c[g] = c[g] || {layers: 0, visible: 0}; c[g].layers++; if (l.layout.visibility !== 'none') c[g].visible++; } return c; }""")
        print("groups on the map:", groups)
        assert "ocean" not in groups and "3D bridge decks" not in groups, groups                           # out unless chosen in the dialog
        assert all(groups[g]["visible"] > 0 for g in ("landcover", "water", "buildings", "crossings", "traffic_signals", "roads")), groups
        if "buildings 3D" in groups:                                                                        # mapstyle 0.7.0 and later
            assert groups["buildings 3D"]["visible"] == 1, groups
            n3 = pg.evaluate("""() => { const m = window.__rs.map; m.jumpTo({pitch: 60}); return m.getStyle().layers.filter((l) => l.type === 'fill-extrusion' && (l.metadata || {})['roadstyle:group'] === 'buildings 3D').length; }""")
            assert n3 == 1, n3
        for title in ("Background (monaco)", "Roads (monaco)", "Features (monaco)", "by type"):
            assert pg.get_by_text(title, exact=True).count() > 0, title + " is not in the Layers panel"
        ys = {t: pg.get_by_text(t, exact=True).first.bounding_box()["y"] for t in ("Features (monaco)", "Roads (monaco)", "Background (monaco)")}
        print("folders in the panel, from the top:", sorted(ys, key=ys.get))
        assert ys["Roads (monaco)"] < ys["Features (monaco)"] < ys["Background (monaco)"], ys                  # the panel: Roads (it holds the type rows), Features, Background
        print("draw order:", order)
        assert order["roadsLast"] < order["crossingsFirst"] and order["roadsLast"] < order["signalsFirst"], order      # features over the roads
        assert order["railwaysLast"] < order["roadsFirst"], order                                                      # railways and parking under them
        for name in ("crossings", "traffic_signals"):                          # and their rows are in the Layers panel
            assert pg.get_by_text(name, exact=True).count() > 0, name + " has no row in the Layers panel"
        assert feat["traffic_signals"]["layers"] > 0 and feat["traffic_signals"]["visible"] == feat["traffic_signals"]["layers"], feat
        if out:
            Path(out).mkdir(parents=True, exist_ok=True)
            pg.screenshot(path=str(Path(out) / "crossings.png"))

        # 3) the road type "residential"
        pg.evaluate("() => window.__rs.map.jumpTo({zoom: 14.8})")
        pg.wait_for_timeout(4000)
        clip = {"x": 470, "y": 100, "width": 800, "height": 600}
        before = pg.screenshot(clip=clip)
        row = pg.get_by_text("residential", exact=True).first
        row.scroll_into_view_if_needed()
        box = row.bounding_box()
        pg.mouse.click(box["x"] - 29, box["y"] + box["height"] / 2)      # the eye of the row
        pg.wait_for_timeout(3000)
        after = pg.screenshot(clip=clip)
        n = changed(before, after)
        print("pixels changed when the residential roads are switched off:", n)
        assert n > 1000, "the road type row did not change the map"
        assert any("road type residential hidden" in t for t in said), said[-5:]
        assert not errors, errors
        if out and "buildings 3D" in groups:                                                                   # a picture of the 3D buildings
            pg.evaluate("() => window.__rs.map.jumpTo({center: [7.4255, 43.7385], zoom: 17.3, pitch: 60, bearing: 20})")
            pg.wait_for_timeout(8000)
            Path(out).mkdir(parents=True, exist_ok=True)
            pg.screenshot(path=str(Path(out) / "3d_buildings.png"))

        browser.close()
        print("ok")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else None)
