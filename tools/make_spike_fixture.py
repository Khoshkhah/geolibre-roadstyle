"""Make the files of the Rust spike (docs/design/rust-spike.md, step S0) from a duckOSM database.

    mapstyle/.venv/bin/python tools/make_spike_fixture.py [monaco.duckdb] [--out spike/data]

Writes to the output folder:
    roads.parquet   the roads as roadstyle gets them from mapstyle (after the stored levels), the geometry as WKB
    look.json       the resolved look: palette, settings, the render arguments, the theme and the config in force
    golden.json     mapstyle's export_inline for the same arguments (the answer the Rust code must give)
    versions.txt    the versions and the places of roadstyle and mapstyle, and the arguments
    --choices: the dialog's choices (mode, theme, paths) as well:
    looks.json      the look of each of the 64 choices, each part once: {"parts": {key: [values]}, "choices": {"mode|theme|paths": {key: index}}}
    choices/roads_<mode>.parquet   the roads of each mode (the column access depends on the mode)
    choices/<mode>_<theme>_<paths>.json.gz   export_inline with the base map and the names and arrows, for the 16 choices of CHOICES16

Needs mapstyle and roadstyle (the venv of the mapstyle project has them).
"""
import argparse
import sys
import dataclasses
import inspect
import json
from importlib.metadata import version
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE))
from build import MODES, PATHS, THEMES, _json, look_of  # noqa: E402  the looks are the plugin's own (build.py)
# the spike scope: no arrows, no street names, no 3D decks, no base-map layers
ARGS = dict(layers=False, arrows=False, labels=False, view_3d=False)


# 16 choices in which each pair of values (mode and theme, mode and paths, theme and paths) occurs once: an orthogonal array OA(16, 3, 4, 2)
CHOICES16 = [(MODES[i], THEMES[j], PATHS[(i + j) % 4]) for i in range(4) for j in range(4)]


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


def features_parquet(db, specs, path):
    """The base-map tables of mapstyle.map.load_layers as one Parquet file, as the worker writes it (worker/features_sql.js, the same SQL):
    a row per feature with the layer's index in layers.yaml (``spec``), its name (``layer``, or the one it is merged into), ``osm_id``, ``kind``,
    ``name``, ``bearing``, the tags of a building's height (``levels``, ``roof_levels``, ``height``, ``min_height``, ``min_level``) and the geometry as WKB, in the order load_layers reads them."""
    import duckdb
    con = duckdb.connect()
    con.execute(f"ATTACH '{Path(db).resolve()}' AS m (READ_ONLY); USE m")
    tables = con.execute("SELECT table_schema, table_name FROM information_schema.tables WHERE table_catalog = 'm'").fetchall()
    have = {t for s, t in tables if s == "features"}
    main = {t for s, t in tables if s == "main"}
    nets = " UNION ".join(f'SELECT osm_id FROM "{s}"."{t}"' for s, t in tables      # _network_ways: the ways a mode network holds
                          if s in ("driving", "walking", "cycling") and t in ("edges", "private_edges", "edge_id_map"))
    parts = []
    tags, none = (lambda k: f"tags['{k}'][1]"), (lambda k: "NULL")     # the height tags of a building (none: an older file or not a building)
    for i, s in enumerate(specs):
        if not s.get("show", True) or s["table"] not in (main if s.get("schema") == "main" else have):
            continue
        if s.get("outside_networks") and not nets:
            continue
        kind = "'" + s["name"] + "'" if s.get("schema") else "kind"     # a table outside features.*: the kind is the layer's name

        def sql(tag):
            return (f"SELECT {i} AS spec, '{s.get('merge_into') or s['name']}' AS layer, rowid AS rid, CAST(osm_id AS VARCHAR) AS osm_id, {kind} AS kind, "
                    f"COALESCE(name, '') AS name, {'CAST(bearing AS DOUBLE)' if s.get('bearing') else 'CAST(NULL AS DOUBLE)'} AS bearing, "
                    f"CAST({tag('building:levels')} AS VARCHAR) AS levels, CAST({tag('roof:levels')} AS VARCHAR) AS roof_levels, "
                    f"CAST({tag('height')} AS VARCHAR) AS height, CAST({tag('min_height')} AS VARCHAR) AS min_height, "
                    f"CAST({tag('building:min_level')} AS VARCHAR) AS min_level, ST_AsWKB(geom) AS geometry "
                    f"FROM {s.get('schema', 'features')}.{s['table']} WHERE geom IS NOT NULL" + (f" AND ({s['where']})" if s.get("where") else "")
                    + (f" AND osm_id NOT IN ({nets})" if s.get("outside_networks") else ""))
        try:
            try:
                con.execute(f"CREATE TEMP TABLE f{i} AS {sql(tags if s.get('height') else none)}")
            except duckdb.Error:
                if not s.get("height"):
                    raise
                con.execute(f"CREATE TEMP TABLE f{i} AS {sql(none)}")     # an older file has no tags: the height of the kind
            parts.append(f"SELECT * FROM f{i}")
        except duckdb.Error:
            continue                                  # load_layers skips a layer whose query fails
    con.execute(f"COPY ({' UNION ALL '.join(parts)} ORDER BY spec, rid) TO '{Path(path).resolve()}' (FORMAT parquet)")


def planner_parquet(db, path):
    """The tables of mapstyle.map.planner_data as one Parquet file, as the worker writes it (worker/planner_sql.js, the same SQL): a row per
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


def write_choices(db, out):
    """--choices: looks.json (the 64 looks, each part once), the roads of each mode, and the golden files of CHOICES16."""
    import gzip
    from mapstyle.export import export_inline
    parts, choices = {}, {}
    (out / "choices").mkdir(exist_ok=True)
    for m in MODES:
        for t in THEMES:
            for p in PATHS:
                look, roads = look_of(db, m, t, p)
                idx = {}
                for k, v in look.items():
                    vals = parts.setdefault(k, [])
                    if v not in vals:
                        vals.append(v)
                    idx[k] = vals.index(v)
                choices[f"{m}|{t}|{p}"] = idx
        roads.to_parquet(out / "choices" / f"roads_{m}.parquet")
    (out / "looks.json").write_text(json.dumps({"parts": parts, "choices": choices}, separators=(",", ":")))
    print("looks.json:", {k: len(v) for k, v in parts.items()}, f"{(out / 'looks.json').stat().st_size / 1e6:.2f} MB")
    for m, t, p in CHOICES16:
        g = export_inline(db, mode=m, theme=t, paths=p, **{**ARGS, "layers": True, "arrows": True, "labels": True})
        with gzip.open(out / "choices" / f"{m}_{t}_{p}.json.gz", "wt") as f:
            json.dump(g, f, separators=(",", ":"), default=_json)
        print("golden", m, t, p)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("db", nargs="?", default=str(HERE.parent / "duckOSM" / "monaco.duckdb"))
    ap.add_argument("--out", default=str(HERE / "spike" / "data"))
    ap.add_argument("--choices", action="store_true", help="also the dialog's choices: looks.json, the roads of each mode, 16 golden files")
    a = ap.parse_args(argv)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)

    import mapstyle
    import mapstyle.map as mm
    import roadstyle as rs
    mm._spatial = _pyodide_spatial                     # as in the plugin's Pyodide
    from mapstyle.export import export_inline

    look, roads = look_of(a.db)
    roads.to_parquet(out / "roads.parquet")            # GeoParquet: geometry as WKB
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

    if a.choices:
        write_choices(a.db, out)
    (out / "versions.txt").write_text(
        f"roadstyle {getattr(rs, '__version__', None) or version('roadstyle')} {Path(rs.__file__).parent}\n"
        f"mapstyle {getattr(mapstyle, '__version__', None) or version('mapstyle')} {Path(mapstyle.__file__).parent}\n"
        f"database {Path(a.db).resolve()}\nexport_inline arguments {ARGS}\n")
    f = golden["sources"]["roads"]["data"]["features"]
    print(f"{len(roads)} roads, {len(f)} road features, {len(golden['sources']['simple']['data']['features'])} pieces, "
          f"{len(golden['layers'])} layers: {[x['id'] for x in golden['layers']]}")


if __name__ == "__main__":
    main()
