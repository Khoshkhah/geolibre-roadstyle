"""Tests of the plugin in a stand-in for GeoLibre (tests/host.html: a MapLibre map and the host calls the plugin uses).

    python -m pytest tests -q                    # fast tests (a few seconds; needs playwright and chromium, and the network for MapLibre)
    RS_TEST_DB=/path/to/monaco.duckdb python -m pytest tests -q -m slow    # also makes a map of a database with Pyodide (1-2 minutes)
"""
import functools
import http.server
import json
import os
import re
import shutil
import subprocess
import threading
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
sync_api = pytest.importorskip("playwright.sync_api")


@pytest.fixture(scope="module")
def base():
    """The repo folder served over http (a module needs a JavaScript MIME type, a file:// page cannot import one)."""
    class Quiet(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *args):
            pass

    handler = functools.partial(Quiet, directory=str(ROOT))
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    yield f"http://127.0.0.1:{srv.server_address[1]}"
    srv.shutdown()


@pytest.fixture(scope="module")
def browser():
    with sync_api.sync_playwright() as p:
        try:
            b = p.chromium.launch(args=["--use-gl=swiftshader", "--enable-unsafe-swiftshader"])
        except sync_api.Error as e:
            pytest.skip(f"playwright browser missing: {e}")
        yield b
        b.close()


@pytest.fixture
def host(browser, base):
    """A page with the stand-in host and the plugin activated; errors of the page are collected in page.errors."""
    page = browser.new_page(viewport={"width": 1000, "height": 700})
    page.errors = []
    page.on("pageerror", lambda e: page.errors.append(str(e)))
    page.goto(f"{base}/tests/host.html")
    page.wait_for_function("() => window.ready === true", timeout=60000)
    page.base = base
    yield page
    page.close()


def add_style(page, path="/tests/style.json"):
    page.evaluate("""async (p) => { await rs.plugin.handleUrlParameters(rs.app, new URLSearchParams("roadstyle=" + p));
                                    await new Promise((r) => rs.map.once("idle", r)); }""", path)


def test_versions_match():
    manifest = json.loads((ROOT / "plugin.json").read_text())["version"]
    code = re.search(r'const VERSION = "([^"]+)"', (ROOT / "index.js").read_text()).group(1)
    assert manifest == code


@pytest.mark.skipif(shutil.which("node") is None, reason="node is not installed")
@pytest.mark.parametrize("f", ["index.js", "py/worker.js", "popup.js"])
def test_syntax(f):
    subprocess.run(["node", "--check", str(ROOT / f)], check=True)


def test_menu(host):
    ids = host.evaluate("() => rs.menu.items.map((i) => i.id)")
    assert ids == ["db", "redraw", "add", "look", "remove"]
    assert host.errors == []


def test_popup_text_is_escaped(host):
    """A tag value with HTML must show as text, not run (the page's popup.js alone puts the values in as HTML)."""
    add_style(host)
    pt = host.evaluate("() => { const p = rs.map.project([18.06, 59.33]); const r = rs.map.getCanvas().getBoundingClientRect(); return [p.x + r.left, p.y + r.top]; }")
    host.mouse.click(*pt)
    host.wait_for_selector(".maplibregl-popup-content", timeout=10000)
    text = host.inner_text(".maplibregl-popup-content")
    assert '<img src=x onerror="window.__pwn=1">' in text and "A&B <b>x</b>" in text
    assert host.evaluate("() => window.__pwn") is None
    assert host.query_selector(".maplibregl-popup-content img") is None and host.query_selector(".maplibregl-popup-content b b") is None
    assert host.errors == []


def test_add_dialog_and_remove(host):
    n = host.evaluate("() => rs.map.getStyle().layers.length")
    host.evaluate("() => rs.menu.items.find((i) => i.id === 'add').onSelect()")
    host.wait_for_selector("[role=dialog]")
    host.keyboard.press("Escape")                                      # cancel: nothing is added
    assert host.query_selector("[role=dialog]") is None
    assert host.evaluate("() => rs.map.getStyle().layers.length") == n
    host.evaluate("() => rs.menu.items.find((i) => i.id === 'add').onSelect()")
    host.fill("[role=dialog] input", f"{host.base}/tests/style.json")
    host.keyboard.press("Enter")
    host.wait_for_function("(n) => rs.map.getStyle().layers.length > n", arg=n, timeout=10000)
    assert host.evaluate("() => rs.rows.length") >= 1
    host.evaluate("() => rs.menu.items.find((i) => i.id === 'remove').onSelect()")
    host.wait_for_function("(n) => rs.map.getStyle().layers.length === n", arg=n, timeout=10000)
    assert host.evaluate("() => rs.rows.length") == 0
    assert host.errors == []


def test_bad_url_is_a_message_not_a_crash(host):
    host.evaluate("() => rs.menu.items.find((i) => i.id === 'add').onSelect()")
    host.fill("[role=dialog] input", f"{host.base}/tests/no-such-file.json")
    host.keyboard.press("Enter")
    host.wait_for_selector("[role=alert]", timeout=10000)
    assert "404" in host.inner_text("[role=alert]")


def test_ocean_and_decks_are_left_out_unless_chosen(host):
    """The ocean polygon (the blue sea) and the 3D bridge decks: out by default, each a row of its own when chosen. The decks are drawn at 1."""
    out = host.evaluate("""async () => {
        const mod = await import('/index.js');
        const st = await (await fetch('/tests/style.json')).json();
        st.sources.ocean = {type: 'geojson', data: {type: 'FeatureCollection', features: []}};
        st.layers.unshift({id: 'ov0-fill', type: 'fill', source: 'ocean', metadata: {'roadstyle:group': 'ocean'}, paint: {'fill-color': '#00f'}});
        st.layers.push({id: 'ov4-3d', type: 'fill-extrusion', source: 'roads', metadata: {'roadstyle:group': 'buildings 3D'},
                        paint: {'fill-extrusion-color': '#ddd', 'fill-extrusion-height': 9, 'fill-extrusion-opacity': 0.9}});
        await mod.__test.addStyleObject(JSON.parse(JSON.stringify(st)), 'a', 'style');
        await mod.__test.addStyleObject(JSON.parse(JSON.stringify(st)), 'b', 'style', {ocean: true, decks: true});
        const has = (id) => rs.map.getLayer(id) != null, names = rs.rows.filter((r) => !r.metadata.group.startsWith('road type')).map((r) => r.name);
        return {default: [has('rs0-ov0-fill'), has('rs0-roads-bridge-decks'), rs.map.getSource('rs0-decks') != null, rs.map.getSource('rs0-ocean') != null],
                chosen: [has('rs1-ov0-fill'), has('rs1-roads-bridge-decks')], opacity: rs.map.getPaintProperty('rs1-roads-bridge-decks', 'fill-extrusion-opacity'),
                b3d: rs.map.getPaintProperty('rs1-ov4-3d', 'fill-extrusion-opacity'), names};
    }""")
    assert out["default"] == [False, False, False, False] and out["chosen"] == [True, True] and out["opacity"] == 1
    assert out["b3d"] == 0.9                                                         # the 3D buildings keep mapstyle's opacity: only the decks are made solid
    assert out["names"].count("ocean") == 1 and out["names"].count("3D bridge decks") == 1
    assert host.errors == []


def test_bridges_stay_drawn_when_zooming_in_without_the_decks(host):
    """roadstyle's flat layers skip a bridge from zoom 16, where its 3D deck starts. Without the decks (the default) the plugin draws the
    bridge at every zoom; with the decks the split stays."""
    out = host.evaluate("""async () => {
        const mod = await import('/index.js');
        const st = await (await fetch('/tests/style.json')).json();
        const split = ['any', ['<', ['zoom'], 16.0], ['!', ['to-boolean', ['get', '__rs_bridge']]]];
        st.layers.find((l) => l.id === 'roads').filter = ['all', ['!', ['to-boolean', ['get', '__rs_dup']]], split];
        await mod.__test.addStyleObject(JSON.parse(JSON.stringify(st)), 'a', 'style');
        await mod.__test.addStyleObject(JSON.parse(JSON.stringify(st)), 'b', 'style', {decks: true});
        return {off: JSON.stringify(rs.map.getFilter('rs0-roads')), on: JSON.stringify(rs.map.getFilter('rs1-roads'))};
    }""")
    assert "__rs_bridge" not in out["off"] and "zoom" not in out["off"]
    assert "__rs_bridge" in out["on"] and "zoom" in out["on"]
    assert host.errors == []


def test_features_that_the_page_starts_hidden_are_shown(host):
    """GeoLibre ignores the starting visibility of a row, so the plugin shows every group: the user switches it off in the panel."""
    vis = host.evaluate("""async () => {
        const mod = await import('/index.js');
        const st = await (await fetch('/tests/style.json')).json();
        st.layers.push({id: 'ov-crossings', type: 'line', source: 'roads', layout: {visibility: 'none'},
                        metadata: {'roadstyle:group': 'crossings'}, paint: {'line-color': '#00f'}});
        await mod.__test.addStyleObject(st, 'a', 'style');
        return rs.map.getLayoutProperty('rs0-ov-crossings', 'visibility'); }""")
    assert vis == "visible"
    assert host.errors == []


def test_three_folders(host):
    """Background (under the roads), Roads (the roads and the road types) and Features (over the roads), named with the map's name."""
    host.evaluate("""async () => {
        const mod = await import('/index.js');
        const st = await (await fetch('/tests/style.json')).json(); st.name = 'monaco';
        st.sources.s = {type: 'geojson', data: {type: 'FeatureCollection', features: []}};
        st.layers.unshift({id: 'ov0-fill', type: 'fill', source: 's', metadata: {'roadstyle:group': 'water'}, paint: {'fill-color': '#00f'}});
        st.layers.push({id: 'ov5-fill', type: 'fill', source: 's', metadata: {'roadstyle:group': 'crossings'}, paint: {'fill-color': '#000'}});
        await mod.__test.addStyleObject(st, 'a', 'style'); }""")
    assert host.evaluate("() => rs.groups.map((g) => g.name)") == ["Background (monaco)", "Roads (monaco)", "Features (monaco)"]    # no folder inside a folder
    rows = host.evaluate("() => rs.groups.map((g) => g.ids.map((id) => rs.rows.find((r) => r.id === id).name))")
    assert rows[0] == ["water"] and rows[2] == ["crossings"]
    assert "roads" in rows[1] and "bridges" in rows[1] and "tertiary" in rows[1] and "crossings" not in rows[1] and "water" not in rows[1]
    order = host.evaluate("() => rs.map.getStyle().layers.map((l) => l.id).filter((i) => /ov0-fill|rs0-roads$|ov5-fill/.test(i))")
    assert order == ["rs0-ov0-fill", "rs0-roads", "rs0-ov5-fill"]                     # draw order: background, roads, features
    # the roads row is registered after the type rows: GeoLibre lists the last row on top, so the Roads folder starts with the roads
    names = host.evaluate("() => rs.rows.map((r) => r.name)")
    assert names.index("roads") > names.index("bridges")
    assert host.errors == []


def test_opacity_of_a_row_fades_its_layers(host):
    """The slider 'Opacity' of GeoLibre (a row's, and a folder's) reaches the layers: the style's own opacity times the value. An opacity that
    depends on the zoom is scaled in its stops (MapLibre takes a zoom expression only at the top)."""
    add_style(host)
    host.evaluate("() => { window.__maperr = []; rs.map.on('error', (e) => window.__maperr.push(String(e.error && e.error.message))); }")
    v = host.evaluate("""() => { const row = rs.rows.find((r) => r.name === 'roads'), m = rs.map;
        const get = () => [m.getPaintProperty('rs0-roads', 'line-opacity'), m.getPaintProperty('rs0-roads-halo', 'line-opacity')];
        row.paintBridge.setOpacity(0.5); const half = get();
        row.paintBridge.setOpacity(0.25); const quarter = get();
        row.paintBridge.setOpacity(1); const back = get();
        return {half, quarter, back, errors: window.__maperr}; }""")
    assert v["half"] == [0.5, ["interpolate", ["linear"], ["zoom"], 14, 0.1, 17, 0.5]]
    assert v["quarter"] == [0.25, ["interpolate", ["linear"], ["zoom"], 14, 0.05, 17, 0.25]]
    assert v["back"] == [1, ["interpolate", ["linear"], ["zoom"], 14, 0.2, 17, 1]]      # the style's own values again
    assert v["errors"] == [] and host.errors == []


def type_rows(page):
    return page.evaluate("() => rs.rows.filter((r) => r.metadata.group === 'road type').map((r) => r.name)")


def toggle(page, name, visible):
    page.evaluate("([n, v]) => rs.rows.find((r) => r.metadata.group === 'road type' && r.name === n).paintBridge.setVisibility(v)", [name, visible])


def rendered(page):
    """the roads' lines on screen, by name"""
    page.evaluate("() => rs.map.jumpTo({center: [18.06, 59.333], zoom: 13.5, pitch: 0})")
    page.wait_for_function("() => rs.map.areTilesLoaded()")
    return sorted(page.evaluate("() => [...new Set(rs.map.queryRenderedFeatures({layers: ['rs0-roads']}).map((f) => f.properties.name))]"))


def test_road_type_rows_filter_the_roads(host):
    add_style(host)
    # GeoLibre lists the last row on top, so the rows are registered from the end: it shows bridges, tertiary, residential, footway
    assert type_rows(host) == ["footway", "residential", "tertiary", "bridges"]
    assert len(rendered(host)) == 3
    toggle(host, "tertiary", False)
    assert "Bro" not in rendered(host) and len(rendered(host)) == 2
    f = host.evaluate("() => rs.map.getFilter('rs0-roads-casing')")
    assert f[0] == "all" and f[1] == ["!=", ["get", "highway"], "none"]               # the layer's own filter stays
    toggle(host, "footway", False)
    assert rendered(host) == ["<img src=x onerror=\"window.__pwn=1\">"]
    toggle(host, "tertiary", True); toggle(host, "footway", True)
    assert len(rendered(host)) == 3
    assert host.evaluate("() => rs.map.getFilter('rs0-roads')") in (None, [])         # no filter again, as the style had it
    toggle(host, "bridges", False)                                                    # the bridge road goes, and its deck
    assert "Bro" not in rendered(host)
    assert host.errors == []


def test_removing_a_map_removes_its_rows_and_a_new_one_has_a_new_prefix(host):
    add_style(host)
    host.evaluate("() => rs.menu.items.find((i) => i.id === 'remove').onSelect()")
    host.wait_for_function("() => rs.rows.length === 0")
    add_style(host)
    assert host.evaluate("() => rs.map.getLayer('rs1-roads') != null && rs.map.getLayer('rs0-roads') == null")
    assert host.evaluate("() => rs.groups.length") >= 1
    assert host.errors == []


def test_database_dialog_has_the_colour_choices(host):
    host.evaluate("() => rs.menu.items.find((i) => i.id === 'db').onSelect()")
    host.wait_for_selector("[role=dialog]")
    labels = host.eval_on_selector_all("[role=dialog] label", "(els) => els.map((e) => e.textContent)")
    assert labels[0].startswith("duckOSM database") and "Colours" in labels and "Footpaths and cycleways" in labels
    assert host.eval_on_selector_all("[role=dialog] select:nth-of-type(1) option", "(os) => os.map((o) => o.value)")
    themes = host.evaluate("() => [...document.querySelectorAll('[role=dialog] select')].map((s) => [...s.options].map((o) => o.value))")
    assert themes[1] == ["osm", "google", "grey"] and themes[3] == ["off", "on"] and themes[4] == ["off", "on"] and len(themes) == 5
    host.keyboard.press("Escape")
    host.evaluate("() => rs.menu.items.find((i) => i.id === 'redraw').onSelect()")       # nothing to redraw yet: a message
    host.wait_for_selector(".rs-pl-toast")
    assert "Open a duckOSM database first" in host.inner_text(".rs-pl-toast")


def test_project_state_keeps_styles_not_databases(host):
    add_style(host)
    assert host.evaluate("() => rs.plugin.getProjectState()") == {"urls": ["/tests/style.json"]}
    host.evaluate("() => rs.plugin.applyProjectState(rs.app, {urls: ['monaco.duckdb', 5, '/tests/style.json']})")   # a file name is not opened
    host.wait_for_timeout(500)
    assert host.query_selector("[role=alert]") is None
    assert host.evaluate("() => rs.plugin.getProjectState().urls.length") == 1


def test_zip_install_carries_its_files(browser, base, tmp_path):
    """A plugin installed from the .zip has no URL for its files: its index.js carries them (blob URLs, made once)."""
    import importlib.util
    spec = importlib.util.spec_from_file_location("rs_build", ROOT / "build.py")
    build = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(build)
    z = build.write_plugin_zip(ROOT, tmp_path / "plugin.zip")
    out = ROOT / "out" / "_ziptest"
    out.mkdir(parents=True, exist_ok=True)
    import zipfile
    (out / "index.js").write_bytes(zipfile.ZipFile(z).read("index.js"))
    page = browser.new_page()
    errs = []
    page.on("pageerror", lambda e: errs.append(str(e)))
    page.goto(f"{base}/tests/host.html?plugin=/out/_ziptest/index.js&noassets=1")
    page.wait_for_function("() => window.ready === true", timeout=60000)
    page.evaluate("() => { window.__blobs = 0; const o = URL.createObjectURL; URL.createObjectURL = (b) => { window.__blobs++; return o.call(URL, b); }; }")
    add_style(page)
    for _ in range(2):                                                # the popup files come from the .zip, one blob URL for each
        pt = page.evaluate("() => { const p = rs.map.project([18.06, 59.33]); const r = rs.map.getCanvas().getBoundingClientRect(); return [p.x + r.left, p.y + r.top]; }")
        page.mouse.click(*pt)
        page.wait_for_selector(".maplibregl-popup-content", timeout=10000)
    assert page.evaluate("() => window.__blobs") <= 2                # popup.css and popup.js, not one for each click
    assert errs == []
    page.close()


# ---- slow: Pyodide and a real database -------------------------------------------------------------------------------------------

DB = os.environ.get("RS_TEST_DB")
slow = pytest.mark.skipif(not DB or not Path(DB or "x").exists(), reason="set RS_TEST_DB to a duckOSM .duckdb file")


def open_db(page, **choices):
    """the menu item, then the dialog: the file and the choices (selects by their label's order: mode, theme, paths, ocean, decks)"""
    page.evaluate("() => rs.menu.items.find((i) => i.id === 'db').onSelect()")
    page.wait_for_selector("[role=dialog]")
    page.set_input_files("[role=dialog] input[type=file]", DB)
    for i, key in enumerate(("mode", "theme", "paths", "ocean", "decks")):
        if key in choices:
            page.select_option(f"[role=dialog] select >> nth={i}", choices[key])
    page.click("[role=dialog] button[type=submit]")


def style_fingerprint(page):
    return page.evaluate("() => JSON.stringify(rs.map.getStyle().layers.filter((l) => l.id.startsWith('rs')).map((l) => [l.id, l.paint]))")


@pytest.mark.slow
@slow
def test_database_map_and_retry_after_a_failed_start(host):
    """The first start fails (the Python packages are cut off); the second try must send them again and succeed."""
    failed = []

    def cut(route):
        if not failed:
            failed.append(1)
            route.fulfill(status=200, body=b"not a zip")
        else:
            route.continue_()

    host.route("**/py/pkgs.zip", cut)
    open_db(host)
    host.wait_for_selector("[role=alert]", timeout=240000)             # the first try fails with a message
    assert host.query_selector(".rs-pl-progress") is None
    host.evaluate("() => document.querySelector('[role=alert] button').click()")
    n = host.evaluate("() => rs.map.getStyle().layers.length")
    open_db(host)                                                      # the second try works
    host.wait_for_function("(n) => rs.map.getStyle().layers.length > n", arg=n, timeout=360000)
    assert host.evaluate("() => rs.plugin.getProjectState().urls") == []          # a database's map is not saved by address
    types = host.evaluate("() => rs.rows.filter((r) => r.metadata.group === 'road type').map((r) => r.name)")
    assert "residential" in types or "footway" in types                            # the road types of the real data
    first = style_fingerprint(host)
    count = host.evaluate("() => rs.map.getStyle().layers.length")
    host.evaluate("() => rs.menu.items.find((i) => i.id === 'redraw').onSelect()")      # the same file, other colours
    host.wait_for_selector("[role=dialog]")
    host.select_option("[role=dialog] select >> nth=1", "grey")
    host.click("[role=dialog] button[type=submit]")
    host.wait_for_function("(f) => JSON.stringify(rs.map.getStyle().layers.filter((l) => l.id.startsWith('rs')).map((l) => [l.id, l.paint])) !== f",
                           arg=first, timeout=120000)
    host.wait_for_selector(".rs-pl-progress", state="detached", timeout=120000)
    ids = host.evaluate("() => rs.map.getStyle().layers.map((l) => l.id).filter((i) => i.startsWith('rs'))")
    assert ids and all(i.startswith("rs1-") for i in ids)                              # the first map (rs0-) was removed, not added to
    assert abs(host.evaluate("() => rs.map.getStyle().layers.length") - count) <= 3     # the grey theme has a few layers less or more
    host.evaluate("() => rs.menu.items.find((i) => i.id === 'remove').onSelect()")
    assert host.errors == []
