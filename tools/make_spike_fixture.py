"""Make the files of the Rust spike (docs/design/rust-spike.md, step S0) from a duckOSM database.

    mapstyle/.venv/bin/python tools/make_spike_fixture.py [monaco.duckdb] [--out spike/data]

Writes to the output folder:
    roads.parquet   the roads as roadstyle gets them from mapstyle (after the stored levels), the geometry as WKB
    look.json       the resolved look: palette, settings, the render arguments, the theme and the config in force
    golden.json     mapstyle's export_inline for the same arguments (the answer the Rust code must give)
    versions.txt    the versions and the places of roadstyle and mapstyle, and the arguments

Needs mapstyle and roadstyle (the venv of the mapstyle project has them).
"""
import argparse
import dataclasses
import inspect
import json
from importlib.metadata import version
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
# the spike scope: no arrows, no street names, no 3D decks, no base-map layers
ARGS = dict(layers=False, arrows=False, labels=False, view_3d=False)


def _json(o):
    """numpy values and tuples that json cannot write."""
    if hasattr(o, "item"):
        return o.item()
    if isinstance(o, (set, frozenset, tuple)):
        return sorted(o, key=str) if isinstance(o, (set, frozenset)) else list(o)
    raise TypeError(f"{type(o)} is not JSON")


def _class_styles(palette, config):
    """``{"classes": {class: [plain, link]}, "fallback": [plain, link]}``: each class's style as roadstyle's ClassStyler resolves it (no tunnel and
    no bridge: the web renderer's styler gets no tunnel or bridge column). A pure function of the class, so it is look data, not code to port."""
    from roadstyle.palettes import FALLBACK, PALETTES
    from roadstyle.style import _apply
    from roadstyle.stylers import _dash_str

    table = PALETTES[palette]

    def res(s):
        out = []
        for link in (False, True):
            p = _apply(s, link, False, False, config)
            out.append({"fill": p.fill, "width": p.width, "casing": p.casing, "casing_width": p.casing_width, "dash": _dash_str(p.dash),
                        "opacity": p.opacity, "casing_opacity": p.casing_opacity})
        return out
    return {"classes": {c: res(s) for c, s in table.items()}, "fallback": res(table.get(FALLBACK) or next(iter(table.values())))}


def _pyodide_spatial(con):
    """mapstyle.map._spatial as it runs in the plugin (Pyodide, sys.platform "emscripten"): no spatial extension; ST_AsGeoJSON, ST_Area and
    ST_Centroid are shapely's over DuckDB's core ST_AsWKB. So the golden files are what the plugin draws: for example the background layer is the
    whole boundary (ST_Difference is not there), not the boundary without the sea."""
    import shapely
    con.create_function("_ms_geojson", lambda b: json.dumps(shapely.geometry.mapping(shapely.from_wkb(b))), ["BLOB"], "VARCHAR")
    con.create_function("_ms_area", lambda b: float(shapely.from_wkb(b).area), ["BLOB"], "DOUBLE")
    con.create_function("_ms_centroid", lambda b: shapely.to_wkb(shapely.from_wkb(b).centroid), ["BLOB"], "BLOB")
    con.execute("CREATE OR REPLACE TEMP MACRO ST_AsGeoJSON(g) AS _ms_geojson(ST_AsWKB(g))")
    con.execute("CREATE OR REPLACE TEMP MACRO ST_Area(g) AS _ms_area(ST_AsWKB(g))")
    con.execute("CREATE OR REPLACE TEMP MACRO ST_Centroid(g) AS ST_GeomFromWKB(_ms_centroid(ST_AsWKB(g)))")


def _theme_features(theme="osm"):
    """The "features" part of mapstyle's style in the theme's colours (render_map passes load_theme(theme)'s style to feature_overlays)."""
    from mapstyle.style import load_theme
    return load_theme(theme)[0]["features"]


def _feature_images(theme="osm"):
    """Every image feature_overlays can put in roadstyle:images (the landcover textures, the point layers' icons), as data URLs: it is run on
    one placeholder feature per layer, so each layer is present. The Rust code keeps the images of the layers a map has."""
    from mapstyle.map import feature_overlays
    from mapstyle.style import load_style, load_theme
    specs = [s for s in load_style("layers")["layers"] if s.get("show", True) and not s.get("merge_into")]
    fake = {s["name"]: {"type": "FeatureCollection", "features": [{"type": "Feature", "properties": {"kind": "x"}, "geometry": None}]} for s in specs}
    return feature_overlays(fake, None, load_theme(theme)[0])[1]["images"]


def _oneway_image():
    """The arrow icon roadstyle's export_inline puts in roadstyle:images (the sprite, an SDF image at the profile's pixel ratio)."""
    import base64
    from importlib.resources import files
    from roadstyle import _settings
    png = files("roadstyle").joinpath("data/sprite.png").read_bytes()
    return {"url": "data:image/png;base64," + base64.b64encode(png).decode(), "sdf": True, "pixelRatio": _settings.theme_value("arrows.icon")["pixel_ratio"]}


def features_parquet(db, specs, path):
    """The base-map tables of mapstyle.map.load_layers as one Parquet file, as the worker writes it (spike/worker/features_sql.js, the same SQL):
    a row per feature with the layer's index in layers.yaml (``spec``), its name (``layer``, or the one it is merged into), ``osm_id``, ``kind``,
    ``name``, ``bearing``, the tags of a building's height (``levels``, ``height``) and the geometry as WKB, in the order load_layers reads them."""
    import duckdb
    con = duckdb.connect()
    con.execute(f"ATTACH '{Path(db).resolve()}' AS m (READ_ONLY); USE m")
    tables = con.execute("SELECT table_schema, table_name FROM information_schema.tables WHERE table_catalog = 'm'").fetchall()
    have = {t for s, t in tables if s == "features"}
    main = {t for s, t in tables if s == "main"}
    nets = " UNION ".join(f'SELECT osm_id FROM "{s}"."{t}"' for s, t in tables      # _network_ways: the ways a mode network holds
                          if s in ("driving", "walking", "cycling") and t in ("edges", "private_edges", "edge_id_map"))
    parts = []
    for i, s in enumerate(specs):
        if not s.get("show", True) or s["table"] not in (main if s.get("schema") == "main" else have):
            continue
        if s.get("outside_networks") and not nets:
            continue
        kind = "'" + s["name"] + "'" if s.get("schema") else "kind"     # a table outside features.*: the kind is the layer's name

        def sql(levels, height):
            return (f"SELECT {i} AS spec, '{s.get('merge_into') or s['name']}' AS layer, rowid AS rid, CAST(osm_id AS VARCHAR) AS osm_id, {kind} AS kind, "
                    f"COALESCE(name, '') AS name, {'CAST(bearing AS DOUBLE)' if s.get('bearing') else 'CAST(NULL AS DOUBLE)'} AS bearing, "
                    f"CAST({levels} AS VARCHAR) AS levels, CAST({height} AS VARCHAR) AS height, ST_AsWKB(geom) AS geometry "
                    f"FROM {s.get('schema', 'features')}.{s['table']} WHERE geom IS NOT NULL" + (f" AND ({s['where']})" if s.get("where") else "")
                    + (f" AND osm_id NOT IN ({nets})" if s.get("outside_networks") else ""))
        try:
            try:
                tags = ("tags['building:levels'][1]", "tags['height'][1]") if s.get("height") else ("NULL", "NULL")
                con.execute(f"CREATE TEMP TABLE f{i} AS {sql(*tags)}")
            except duckdb.Error:
                if not s.get("height"):
                    raise
                con.execute(f"CREATE TEMP TABLE f{i} AS {sql('NULL', 'NULL')}")     # an older file has no tags: the height of the kind
            parts.append(f"SELECT * FROM f{i}")
        except duckdb.Error:
            continue                                  # load_layers skips a layer whose query fails
    con.execute(f"COPY ({' UNION ALL '.join(parts)} ORDER BY spec, rid) TO '{Path(path).resolve()}' (FORMAT parquet)")


def planner_parquet(db, path):
    """The tables of mapstyle.map.planner_data as one Parquet file, as the worker writes it (spike/worker/planner_sql.js, the same SQL): a row per
    record with its ``part`` and the order Python reads it in (``grp``, ``ord``: the reading order of a table or view). Parts: ``mode`` (a mode with edges and edge_graph), ``end`` (an
    edge's nodes, length and junction: edges then private_edges, each mode in order), ``graph`` (a mode's edges by edge_id), ``next`` (a mode's turns,
    its own rows where edge_graph has ``uses``), ``mm`` (walk + drive is there), ``mme`` / ``mmt`` (mm.edges, mm.transfers)."""
    import duckdb
    con = duckdb.connect()
    con.execute(f"ATTACH '{Path(db).resolve()}' AS m (READ_ONLY); USE m")
    have = {tuple(r) for r in con.execute("SELECT table_schema, table_name FROM information_schema.tables WHERE table_catalog = 'm'").fetchall()}
    cols = {tuple(r) for r in con.execute("SELECT table_schema, table_name, column_name FROM information_schema.columns WHERE table_catalog = 'm'").fetchall()}
    modes = ["driving", "walking", "cycling"]
    parts = []
    for i, m in enumerate(modes):
        if (m, "edges") in have and (m, "edge_graph") in have:
            parts.append(f"SELECT 'mode' AS part, {i} AS grp, 0 AS ord, '{m}' AS mode")
    for ti, tb in enumerate(("edges", "private_edges")):
        for i, m in enumerate(modes):
            if (m, tb) in have:
                jn = "junction" if (m, tb, "junction") in cols else "CAST(NULL AS VARCHAR)"
                parts.append(f"SELECT 'end' AS part, {ti * 10 + i} AS grp, row_number() OVER () AS ord, * FROM (SELECT edge_id AS e, source AS s, "
                             f"target AS t, CAST(length_m AS DOUBLE) AS len, {jn} AS junction FROM {m}.{tb})")
    own = {"driving": "car", "walking": "walk", "cycling": "bike"}
    for i, m in enumerate(modes):
        if (m, "edges") in have and (m, "edge_graph") in have:
            parts.append(f"SELECT 'graph' AS part, {i} AS grp, row_number() OVER (ORDER BY edge_id) AS ord, '{m}' AS mode, edge_id AS e, "
                         f"CAST(cost_s AS DOUBLE) AS cost, CAST(length_m AS DOUBLE) AS len FROM {m}.edges")
            uses = f" WHERE uses = '{own[m]}'" if (m, "edge_graph", "uses") in cols else ""
            parts.append(f"SELECT 'next' AS part, {i} AS grp, row_number() OVER () AS ord, * FROM (SELECT '{m}' AS mode, from_edge AS e, to_edge AS e2 "
                         f"FROM {m}.edge_graph{uses})")
    if ("mm", "edges") in have and ("mm", "transfers") in have:
        parts.append("SELECT 'mm' AS part, 0 AS grp, 0 AS ord")
        parts.append("SELECT 'mme' AS part, 0 AS grp, row_number() OVER () AS ord, * FROM (SELECT mode, source AS s, target AS t, edge_id AS e, "
                     "CAST(cost_s AS DOUBLE) AS cost FROM mm.edges)")
        parts.append("SELECT 'mmt' AS part, 0 AS grp, row_number() OVER () AS ord, * FROM (SELECT node_id AS s, from_mode AS fm, to_mode AS tm, "
                     "CAST(cost_s AS DOUBLE) AS cost FROM mm.transfers)")
    con.execute(f"COPY ({' UNION ALL BY NAME '.join(parts)} ORDER BY part, grp, ord) TO '{Path(path).resolve()}' (FORMAT parquet)")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("db", nargs="?", default=str(HERE.parent / "duckOSM" / "monaco.duckdb"))
    ap.add_argument("--out", default=str(HERE / "spike" / "data"))
    a = ap.parse_args(argv)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)

    import mapstyle
    import mapstyle.map as mm
    import roadstyle as rs
    mm._spatial = _pyodide_spatial                     # as in the plugin's Pyodide
    from roadstyle import _settings
    from roadstyle import render_web as rw
    from mapstyle.export import export_inline
    from mapstyle.map import render_map

    got = {}

    def grab(roads, palette, settings, kw, config):    # what mapstyle gives to rs.export_inline
        got.update(roads=roads, palette=palette, settings=settings, kw=kw)
        return None

    render_map(a.db, None, _export=grab, **ARGS)
    roads, settings, kw = got["roads"], got["settings"], got["kw"]

    roads.to_parquet(out / "roads.parquet")            # GeoParquet: geometry as WKB

    prev = list(_settings._EXTRA)                      # the look as render_edges applies it: the settings over the ones in force
    rs.use_settings(*prev, settings)
    try:
        theme, config = _settings.theme(), dataclasses.asdict(rw.CONFIG)
        styles = _class_styles(got["palette"], rw.CONFIG)
        head_m = inspect.signature(rw.render).parameters["head_m"].default
        # S6: a slot's rank (the z_order table) and how far a name keeps from a crossing road (its drawn width at zoom 20, _crossing_half_m)
        slots_look = {"z_order": dict(rw.ROAD_Z), "width_z20": {c: rw.class_width_px(c, 20) for c in rw._CLASSES},
                      "width_z20_fallback": rw.class_width_px(None, 20)}
        roads_model = _settings.roads()        # step 2 of the full port: the road tables the layer builders read (render_web._load_road_model)
    finally:
        rs.use_settings(*prev)
    from mapstyle.style import load_style
    look = {"palette": got["palette"], "settings": settings,
            "args": {k: v for k, v in kw.items() if k != "overlays"},      # name, tooltip, level columns, ... (the overlays are the base map)
            "theme": theme, "config": config,
            # what the Rust code reads (S2): the class styles as the styler resolves them, and the render's defaults
            "styles": styles, "head_m": head_m, "slots": slots_look, "roads_model": roads_model,
            # step 3 of the full port: the base-map layers (mapstyle's styles/layers.yaml and the 3D heights of load_layers), the look of
            # feature_overlays (the theme's "features" style) and its images, and the arrow image of roadstyle's export
            "features": {"layers_yaml": load_style("layers"), "buildings_3d": load_style()["features"]["buildings_3d"],
                         "style": _theme_features(), "images": _feature_images()},
            "oneway_image": _oneway_image()}
    (out / "look.json").write_text(json.dumps(look, default=_json, indent=1))

    golden = export_inline(a.db, **ARGS)
    (out / "golden.json").write_text(json.dumps(golden, separators=(",", ":"), default=_json))


    # S6: the names and arrows, with the same arguments but arrows and labels on: the slots source and the two layers that read it
    # (the roads and simple sources do not change with them)
    full = export_inline(a.db, **{**ARGS, "arrows": True, "labels": True})
    (out / "golden_slots.json").write_text(json.dumps({"layers": [x for x in full["layers"] if x.get("source") == "slots"],
                                                       "sources": {"slots": full["sources"]["slots"]}}, separators=(",", ":"), default=_json))

    # step 3 of the full port: the whole map (with the base-map layers), and the base-map tables as the worker's SQL reads them
    full = export_inline(a.db, **{**ARGS, "layers": True, "arrows": True, "labels": True}, planner=True)      # step 4: with the planner data
    (out / "golden_full.json").write_text(json.dumps(full, separators=(",", ":"), default=_json))
    features_parquet(a.db, look["features"]["layers_yaml"]["layers"], out / "features.parquet")
    planner_parquet(a.db, out / "planner.parquet")
    # step 5: the style exactly as the plugin gets it: roadstyle's export_inline defaults to view_3d=True (the 3D bridge decks)
    full3d = export_inline(a.db, layers=True, planner=True)
    (out / "golden_3d.json").write_text(json.dumps(full3d, separators=(",", ":"), default=_json))

    (out / "versions.txt").write_text(
        f"roadstyle {getattr(rs, '__version__', None) or version('roadstyle')} {Path(rs.__file__).parent}\n"
        f"mapstyle {getattr(mapstyle, '__version__', None) or version('mapstyle')} {Path(mapstyle.__file__).parent}\n"
        f"database {Path(a.db).resolve()}\nexport_inline arguments {ARGS}\n")
    f = golden["sources"]["roads"]["data"]["features"]
    print(f"{len(roads)} roads, {len(f)} road features, {len(golden['sources']['simple']['data']['features'])} pieces, "
          f"{len(golden['layers'])} layers: {[x['id'] for x in golden['layers']]}")


if __name__ == "__main__":
    main()
