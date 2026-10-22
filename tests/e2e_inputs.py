"""The plugin's other inputs, in the real GeoLibre (https://web.geolibre.app) in a headless browser: the URL of a roadstyle style (inline data), the
URL of a tiles export (rs.export_style), and the link parameter ?roadstyle=URL. The database input is tests/e2e_geolibre.py.

    python tests/e2e_inputs.py /path/to/monaco.duckdb [screenshots-folder]

It needs roadstyle and mapstyle (pip), playwright with Chromium and the network. It installs dist/roadstyle-geolibre.zip.
"""
import json
import os
import re
import sys
import tempfile
from pathlib import Path

from playwright.sync_api import sync_playwright

sys.path.insert(0, str(Path(__file__).resolve().parent))
from e2e_geolibre import ZIP, activate, probe_zip  # noqa: E402

HOST = "https://rs-test.example"            # a made-up https address: the test browser answers it from a folder (GeoLibre's page may not read from http://localhost)


def route_folder(ctx, folder):
    """Answer every request to HOST from ``folder`` (with CORS headers), also those of MapLibre's workers."""
    types = {".json": "application/json", ".pbf": "application/x-protobuf", ".png": "image/png", ".pmtiles": "application/octet-stream"}

    def handle(route):
        path = Path(folder) / route.request.url[len(HOST):].lstrip("/").split("?")[0]
        if route.request.method == "OPTIONS":
            return route.fulfill(status=204, headers={"access-control-allow-origin": "*", "access-control-allow-headers": "*"})
        if not path.is_file():
            return route.fulfill(status=404, headers={"access-control-allow-origin": "*"}, body="")
        route.fulfill(status=200, body=path.read_bytes(), headers={"access-control-allow-origin": "*", "content-type": types.get(path.suffix, "application/octet-stream"),
                                                                    **({"content-encoding": "gzip"} if path.suffix == ".pbf" and path.read_bytes()[:2] == b"\x1f\x8b" else {})})
    ctx.route(HOST + "/**", handle)


def make_files(db, folder):
    import mapstyle
    from mapstyle.export import export_inline
    import roadstyle as rs
    from roadstyle.export import export_style
    style = export_inline(db)
    (folder / "style_inline.json").write_text(json.dumps(style))
    roads = mapstyle.load_roads(db)
    import duckdb
    con = duckdb.connect(str(db), read_only=True)
    lv = con.execute("SELECT edge_id, casing_level, fill_level FROM visualization.edge_levels").df()
    con.close()
    roads["edge_id"] = roads["edge_id"].astype("int64")
    edges = roads.merge(lv, on="edge_id", how="left")[["edge_id", "highway", "oneway", "tunnel", "bridge", "casing_level", "fill_level", "geometry"]]
    edges["edge_id"] = edges["edge_id"].astype(str)
    (folder / "edges.geojson").write_text(edges.to_json())          # what a GeoLibre data layer of the edges holds
    export_style(roads, folder / "tiles_export", base_url=f"{HOST}/tiles_export")
    return roads


def menu(pg, item):
    pg.get_by_role("button", name="roadstyle").first.click()
    pg.get_by_text(item, exact=False).first.click()


def state(pg):
    return pg.evaluate("""() => { const m = window.__rs.map, L = m.getStyle().layers.filter((l) => l.id.startsWith('rs')), g = (l) => l.metadata && l.metadata['roadstyle:group'];
        const groups = {}; for (const l of L) { const k = g(l) || 'none'; groups[k] = (groups[k] || 0) + 1; }
        const roads = L.filter((l) => /roads-(simple|fill)$/.test(l.id));
        const first = L.findIndex((l) => g(l) === 'roads'), all = m.getStyle().layers.map((l) => l.id);
        return {layers: L.length, groups, roadsVisible: roads.map((l) => m.getLayoutProperty(l.id, 'visibility')),
                drawn: roads.map((l) => m.queryRenderedFeatures({layers: [l.id]}).length), rows: window.__rs.app.listLayerGroups().map((x) => x.name)}; }""")


def main(db, out=None):
    with tempfile.TemporaryDirectory() as tmp, sync_playwright() as p:
        tmp = Path(tmp)
        make_files(db, tmp)
        browser = p.chromium.launch(args=["--use-gl=swiftshader", "--enable-unsafe-swiftshader"])
        ctx = browser.new_context(viewport={"width": 1500, "height": 900})
        route_folder(ctx, tmp)
        pg = ctx.new_page()
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
        print("app keys:", pg.evaluate("() => Object.keys(window.__rs.app).sort().join(' ')")[:900])

        for name, url in (("inline", f"{HOST}/style_inline.json"), ("tiles", f"{HOST}/tiles_export/style.json")):
            menu(pg, "Add a roadstyle layer")
            pg.wait_for_selector("[role=dialog] input")
            pg.fill("[role=dialog] input", url)
            pg.click("[role=dialog] button[type=submit]")
            pg.wait_for_timeout(8000)
            pg.evaluate("() => window.__rs.map.jumpTo({center: [7.4262, 43.7385], zoom: 16.5, pitch: 0, bearing: 0})")
            pg.wait_for_timeout(8000)
            s = state(pg)
            print(name, s)
            if s["layers"] == 0:
                print("roadstyle said:", said[-6:], "| errors:", errors[-3:])
                print("toasts:", pg.evaluate("() => [...document.querySelectorAll('.rs-pl-toast, [role=alert], [role=status]')].map((e) => e.textContent.slice(0, 200))"))
                if out:
                    pg.screenshot(path=str(Path(out) / f"input_{name}_failed.png"))
            assert s["layers"] > 0 and any(d > 0 for d in s["drawn"]), (name, s)
            if out:
                pg.screenshot(path=str(Path(out) / f"input_{name}.png"))
            menu(pg, "Remove roadstyle layers")
            pg.wait_for_timeout(2500)
            assert state(pg)["layers"] == 0, "the layers are not removed"
        # "Style a layer with roadstyle": a GeoJSON data layer of the edges (with their drawing positions), and the plugin's default look
        pg.evaluate("""async (u) => { const g = await (await fetch(u)).json(); window.__rs.app.addGeoJsonLayer("monaco edges", g); }""", f"{HOST}/edges.geojson")
        pg.wait_for_timeout(8000)
        pg.evaluate("() => window.__rs.map.jumpTo({center: [7.4262, 43.7385], zoom: 16.5, pitch: 0, bearing: 0})")
        menu(pg, "Style a layer with roadstyle")
        pg.wait_for_selector("[role=dialog] select")
        pg.click("[role=dialog] button[type=submit]")
        pg.wait_for_timeout(10000)
        st = pg.evaluate("""() => { const m = window.__rs.map, L = m.getStyle().layers.filter((l) => l.id.startsWith('rslook-'));
            return {layers: L.length, drawn: L.slice(0, 12).map((l) => m.queryRenderedFeatures({layers: [l.id]}).length).reduce((a, b) => a + b, 0)}; }""")
        print("style a layer:", st, "| said:", said[-2:])
        if out:
            pg.screenshot(path=str(Path(out) / "input_look.png"))
        assert st["layers"] > 0 and st["drawn"] > 0, st
        menu(pg, "Remove roadstyle layers")
        pg.wait_for_timeout(2500)
        # the link parameter: GeoLibre opened with ?roadstyle=URL draws the style when the plugin starts
        pg.goto(f"https://web.geolibre.app/?roadstyle={HOST}/style_inline.json", wait_until="load")
        pg.wait_for_timeout(15000)
        for _ in range(3):
            if pg.evaluate("() => !!(window.__rs && window.__rs.map)") or pg.get_by_role("button", name="Plugins").count() == 0:
                break
            pg.wait_for_timeout(3000)
        try:
            activate(pg, "roadstyle")
            activate(pg, "rs-probe")
        except Exception as e:                                # the plugins stay active after a reload in this browser, or not
            print("activate after the reload:", str(e)[:120])
        pg.wait_for_timeout(15000)
        s = pg.evaluate("() => window.__rs && window.__rs.map ? window.__rs.map.getStyle().layers.filter((l) => l.id.startsWith('rs')).length : -1")
        print("link parameter, layers on the map:", s, "| said:", said[-3:])
        print("page errors:", errors[-5:])
        assert not errors, errors
        print("ok")
        browser.close()


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else None)
