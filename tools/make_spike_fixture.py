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
    finally:
        rs.use_settings(*prev)
    look = {"palette": got["palette"], "settings": settings,
            "args": {k: v for k, v in kw.items() if k != "overlays"},      # name, tooltip, level columns, ... (the overlays are the base map)
            "theme": theme, "config": config}
    (out / "look.json").write_text(json.dumps(look, default=_json, indent=1))

    golden = export_inline(a.db, **ARGS)
    (out / "golden.json").write_text(json.dumps(golden, separators=(",", ":"), default=_json))

    # the layers as Python makes them for this data: they depend on the look and on a few flags of the data (tunnels, twin pairs,
    # dash values, reverse pairs, ...; docs/design/rust-spike.md, S1), so the spike takes them from here
    (out / "layers.json").write_text(json.dumps(golden["layers"], separators=(",", ":"), default=_json))

    (out / "versions.txt").write_text(
        f"roadstyle {version('roadstyle')} {Path(rs.__file__).parent}\n"
        f"mapstyle {version('mapstyle')} {Path(mapstyle.__file__).parent}\n"
        f"database {Path(a.db).resolve()}\nexport_inline arguments {ARGS}\n")
    f = golden["sources"]["roads"]["data"]["features"]
    print(f"{len(roads)} roads, {len(f)} road features, {len(golden['sources']['simple']['data']['features'])} pieces, "
          f"{len(golden['layers'])} layers: {[x['id'] for x in golden['layers']]}")


if __name__ == "__main__":
    main()
