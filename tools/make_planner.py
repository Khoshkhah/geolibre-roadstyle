"""Make the plugin's route planner files from duckOSM's planner template (the panel of ``duckosm route-map``).

    python tools/make_planner.py [path/to/duckosm/templates/planner.html]

The default path is the template of the installed duckosm. It writes ``planner/panel.css``, ``planner/panel.html`` and ``planner/planner.js``.
The panel and its code are the template's own, so the plugin shows the same panel as ``duckosm route-map``. Only the places where the template
talks to its page are changed, and each change must match once (else this tool stops: the template has changed, and the change must be looked at):

- the page's layout rules (``#map``, ``body``, the fixed position of the panel, the phone layout) go: GeoLibre's right panel holds the panel;
- ``map``, ``RM`` and a host object ``H`` are the arguments of ``mount(map, RM, H)``, in place of the page's globals;
- ``H.roadsData()``, ``H.roadIds()``, ``H.roadProps(ids)``, ``H.Marker`` and ``H.recolor`` replace the roadstyle page's functions.
"""
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent


def once(text, old, new):
    if text.count(old) != 1:
        raise SystemExit(f"make_planner: expected one match of {old[:70]!r}, found {text.count(old)}: the template has changed")
    return text.replace(old, new, 1)


def main(path=None):
    if path is None:
        import duckosm
        path = Path(duckosm.__file__).parent / "templates" / "planner.html"
    src = Path(path).read_text(encoding="utf-8")
    css = src[src.index("<style>") + len("<style>"):src.index("</style>")].strip("\n")
    markup = src[src.index("</style>") + len("</style>"):src.index("<script>const RM")].strip("\n")
    js = src[src.index("<script>\n(function () {") + len("<script>\n"):src.rindex("</script>")].strip("\n")

    # the CSS: no layout of the page
    css = re.sub(r"^#map\{right:[^\n]*\n", "", css, flags=re.M)
    css = once(css, "#rm-panel{position:fixed;top:0;right:0;bottom:0;width:var(--w);box-sizing:border-box;padding:18px 18px 28px;overflow-y:auto;",
               "#rm-panel{position:relative;width:auto;min-height:100%;box-sizing:border-box;padding:18px 18px 28px;")
    css = once(css, "border-left:1px solid var(--line);", "")
    css = re.sub(r"@media \(max-width:700px\)\{.*?\n\}\n?", "", css, flags=re.S)
    if "#map" in css or "position:fixed" in css.replace(".rm-tag", ""):
        raise SystemExit("make_planner: a page layout rule is left in the CSS")

    # the code: a function of the map, the data and the host
    body = once(js, "(function () {", "")
    body = body.rstrip()
    if not body.endswith("})();"):
        raise SystemExit("make_planner: the script does not end with })();")
    body = body[:-len("})();")].rstrip()
    body = once(body, 'const d = map.getSource("roads")._data, fs = (d.geojson || d).features;', "const d = H.roadsData(), fs = (d.geojson || d).features;")
    body = once(body, "const ids = rsQuery(() => true), props = rsGetProps(ids);", "const ids = H.roadIds(), props = H.roadProps(ids);")
    body = once(body, "ps = rsGetProps(ks.map((k) => fid[k]));", "ps = H.roadProps(ks.map((k) => fid[k]));")
    body = once(body, "rsColor(null);", "H.recolor(null);")
    body = once(body, "new maplibregl.Marker(", "new H.Marker(")
    body = once(body, """for (const l of res.legs) l.edges.forEach((k, i) => lines.push({type: "Feature", properties: {color: COLORS[l.mode]},
      geometry: {type: "LineString", coordinates: cut(GEO[k], l.parts[i][0], l.parts[i][1])}}));""",
                """for (const l of res.legs) l.edges.forEach((k, i) => {
      if (!GEO[k]) return console.warn("roadstyle: a road of the route has no line on the map (edge_id " + D.eid[k] + ")");     // one bad road must not hide the route
      lines.push({type: "Feature", properties: {color: COLORS[l.mode]}, geometry: {type: "LineString", coordinates: cut(GEO[k], l.parts[i][0], l.parts[i][1])}});
    });""")
    body = once(body, 'document.addEventListener("rs:select", (e) => {', 'document.addEventListener("rs:select", (e) => {\n      if (!alive) return;')
    body = "\n  let alive = true;" + body
    body = re.sub(r"\n  \(function wait\(\) \{.*?\n  \}\)\(\);\s*$", "\n  init();\n", body, flags=re.S)
    if "wait()" in body:
        raise SystemExit("make_planner: the wait loop is left in the script")
    body += '''
  return {
    route: (a, b) => window.rmRoute(a, b),
    restore() { drawLayers(); drawRadius(); if (window.rmLast && window.rmLast.res) paint(window.rmLast.res); },     // a new base map took the layers away
    dispose() {                                                    // the markers, the route layers and the panel's listeners go
      for (const k of ["A", "B"]) if (markers[k]) { markers[k].remove(); markers[k] = null; }
      for (const id of ["rm-radius-fill", "rm-radius-line", "rm-route-casing", "rm-route", "rm-access"]) if (map.getLayer(id)) map.removeLayer(id);
      for (const id of ["rm-radius", "rm-access", "rm-route"]) if (map.getSource(id)) map.removeSource(id);
      alive = false; delete window.rmRoute; delete window.rmLast; delete window.rmSteps;
    },
  };
'''
    header = ("// GENERATED by tools/make_planner.py from duckOSM's templates/planner.html (the panel of `duckosm route-map`). Do not edit: change the tool or the template.\n"
              "// mount(map, RM, H): map = the MapLibre map, RM = the planner data (mapstyle's planner_data), H = the host: roadsData(), roadIds(), roadProps(ids), recolor(x), Marker.\n")
    out = HERE / "planner"
    out.mkdir(exist_ok=True)
    (out / "panel.css").write_text("/* GENERATED by tools/make_planner.py from duckOSM's templates/planner.html. Do not edit. */\n" + css + "\n")
    (out / "panel.html").write_text(markup + "\n")
    (out / "planner.js").write_text(header + "export function mount(map, RM, H) {\n" + body + "}\n")
    print("wrote", [str(p.relative_to(HERE)) for p in sorted(out.iterdir())])


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else None)
