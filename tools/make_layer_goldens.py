"""Golden layers for many subsets of one database's roads (docs/design/rust-spike.md, full port step 2: the layers in Rust).

    PYTHONPATH=<roadstyle>/src mapstyle/.venv/bin/python tools/make_layer_goldens.py [monaco.duckdb] [--out spike/data/layers]

The layers depend on the look and on flags of the data (tunnels, twin pairs, dash values, reverse pairs, ...). For each subset this writes
``<out>/<name>/roads.parquet`` (the subset of the roads as roadstyle gets them) and ``<out>/<name>/golden.json`` (the style's layers, made as
mapstyle.export.export_inline makes them, with arrows and labels, without the base-map layers and the 3D decks), and prints the flags of each subset."""
import argparse
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent

SUBSETS = {          # name -> a mask on the roads (a GeoDataFrame)
    "all": lambda r: r.index >= 0,
    "first100": lambda r: r.index < 100,
    "footway": lambda r: r["highway"] == "footway",
    "service": lambda r: r["highway"] == "service",
    "residential_directed": lambda r: (r["highway"] == "residential") & r["is_directed"],
    "no_tunnel": lambda r: r["tunnel"].isna(),
    "no_bridge_no_tunnel": lambda r: r["tunnel"].isna() & r["bridge"].isna(),
    "bridges": lambda r: r["bridge"].notna(),
    "tunnels": lambda r: r["tunnel"].notna(),
    "directed": lambda r: r["is_directed"],
    "oneway": lambda r: r["oneway"],
    "no_name": lambda r: r["name"] == "",
}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("db", nargs="?", default=str(HERE.parent / "duckOSM" / "monaco.duckdb"))
    ap.add_argument("--out", default=str(HERE / "spike" / "data" / "layers"))
    a = ap.parse_args(argv)

    import roadstyle as rs
    from mapstyle.export import _restyle
    from mapstyle.map import render_map

    got = {}

    def grab(roads, palette, settings, kw, config):
        got.update(roads=roads, palette=palette, settings=settings, kw=kw, config=config)

    render_map(a.db, None, _export=grab, layers=False, arrows=True, labels=True, view_3d=False)
    roads, kw = got["roads"], {k: v for k, v in got["kw"].items() if k not in ("basemap", "basemaps")}
    for name, mask in SUBSETS.items():
        sub = roads[mask(roads)].reset_index(drop=True)
        d = Path(a.out) / name
        d.mkdir(parents=True, exist_ok=True)
        sub.to_parquet(d / "roads.parquet")
        style = rs.export_inline(sub, palette=got["palette"], settings=got["settings"], **kw)      # as mapstyle.export.export_inline's make()
        images = style["metadata"]["roadstyle:images"]
        _restyle(style, got["config"])
        style["metadata"]["roadstyle:images"] = {**style["metadata"]["roadstyle:images"], **images}
        (d / "golden.json").write_text(json.dumps({"layers": style["layers"]}, separators=(",", ":")))
        p = [f["properties"] for f in style["sources"]["roads"]["data"]["features"]]
        flags = {"tunnel": any(x["__rs_tunnel"] for x in p), "twin": any("__rs_twin" in x for x in p), "dup": any(x.get("__rs_dup") for x in p),
                 "dashes": sorted({x["__rs_dash"] for x in p if x["__rs_dash"]}), "slots": "slots" in style["sources"]}
        print(f"{name:22s} {len(sub):6d} roads  layers {[x['id'] for x in style['layers']]}  {flags}")


if __name__ == "__main__":
    main()
