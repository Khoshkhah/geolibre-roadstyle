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


def features_parquet(db, specs, path):
    """The base-map tables of mapstyle.map.load_layers as one Parquet file, as the worker writes it (spike/worker/features_sql.js, the same SQL):
    a row per feature with the layer's index in layers.yaml (``spec``), its name (``layer``, or the one it is merged into), ``osm_id``, ``kind``,
    ``name``, ``bearing``, the tags of a building's height (``levels``, ``height``) and the geometry as WKB, in the order load_layers reads them."""
    import duckdb
    con = duckdb.connect()
    con.execute(f"ATTACH '{Path(db).resolve()}' AS m (READ_ONLY); USE m")
    have = {r[0] for r in con.execute("SELECT table_name FROM information_schema.tables WHERE table_catalog = 'm' AND table_schema = 'features'").fetchall()}
    parts = []
    for i, s in enumerate(specs):
        if not s.get("show", True) or s["table"] not in have:
            continue
        tags = "tags['building:levels'][1], tags['height'][1]" if s.get("height") else "NULL, NULL"
        sql = (f"SELECT {i} AS spec, '{s.get('merge_into') or s['name']}' AS layer, rowid AS rid, CAST(osm_id AS VARCHAR) AS osm_id, kind, "
               f"COALESCE(name, '') AS name, {'CAST(bearing AS DOUBLE)' if s.get('bearing') else 'CAST(NULL AS DOUBLE)'} AS bearing, "
               f"CAST({tags.split(', ')[0]} AS VARCHAR) AS levels, CAST({tags.split(', ')[1]} AS VARCHAR) AS height, ST_AsWKB(geom) AS geometry "
               f"FROM features.{s['table']} WHERE geom IS NOT NULL" + (f" AND ({s['where']})" if s.get("where") else ""))
        try:
            con.execute(f"CREATE TEMP TABLE f{i} AS {sql}")
            parts.append(f"SELECT * FROM f{i}")
        except duckdb.Error:
            continue                                  # load_layers skips a layer whose query fails
    con.execute(f"COPY ({' UNION ALL '.join(parts)} ORDER BY spec, rid) TO '{Path(path).resolve()}' (FORMAT parquet)")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("db", nargs="?", default=str(HERE.parent / "duckOSM" / "monaco.duckdb"))
    ap.add_argument("--out", default=str(HERE / "spike" / "data"))
    a = ap.parse_args(argv)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)

    import mapstyle
    import roadstyle as rs
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
            # step 3 of the full port: the base-map layers (mapstyle's styles/layers.yaml and the 3D heights of load_layers)
            "features": {"layers_yaml": load_style("layers"), "buildings_3d": load_style()["features"]["buildings_3d"]}}
    (out / "look.json").write_text(json.dumps(look, default=_json, indent=1))

    golden = export_inline(a.db, **ARGS)
    (out / "golden.json").write_text(json.dumps(golden, separators=(",", ":"), default=_json))


    # S6: the names and arrows, with the same arguments but arrows and labels on: the slots source and the two layers that read it
    # (the roads and simple sources do not change with them)
    full = export_inline(a.db, **{**ARGS, "arrows": True, "labels": True})
    (out / "golden_slots.json").write_text(json.dumps({"layers": [x for x in full["layers"] if x.get("source") == "slots"],
                                                       "sources": {"slots": full["sources"]["slots"]}}, separators=(",", ":"), default=_json))

    # step 3 of the full port: the whole map (with the base-map layers), and the base-map tables as the worker's SQL reads them
    full = export_inline(a.db, **{**ARGS, "layers": True, "arrows": True, "labels": True})
    (out / "golden_full.json").write_text(json.dumps(full, separators=(",", ":"), default=_json))
    features_parquet(a.db, look["features"]["layers_yaml"]["layers"], out / "features.parquet")

    (out / "versions.txt").write_text(
        f"roadstyle {version('roadstyle')} {Path(rs.__file__).parent}\n"
        f"mapstyle {version('mapstyle')} {Path(mapstyle.__file__).parent}\n"
        f"database {Path(a.db).resolve()}\nexport_inline arguments {ARGS}\n")
    f = golden["sources"]["roads"]["data"]["features"]
    print(f"{len(roads)} roads, {len(f)} road features, {len(golden['sources']['simple']['data']['features'])} pieces, "
          f"{len(golden['layers'])} layers: {[x['id'] for x in golden['layers']]}")


if __name__ == "__main__":
    main()
