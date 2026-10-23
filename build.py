"""Make the generated files of the plugin, and the .zip for GeoLibre's "Install from file".

    python build.py            # the files of this folder, then dist/roadstyle-geolibre.zip
    python build.py --files    # only the files (the look, popup.css, popup.js, map/worker.js, map/pkgs.bin)
    python build.py --zip      # only the .zip
    python build.py --db area.duckdb     # the database the looks are made with (default: ../duckOSM/monaco.duckdb)

Needs roadstyle 0.24.3 and mapstyle 0.12.0 (``pip install roadstyle==0.24.3 mapstyle==0.12.0``: the Rust code follows them, see
spike/test_compare.py), cargo with the wasm32-unknown-unknown target, the wasm-bindgen program (0.2.129) and npx (esbuild). The browser
runs no Python: the map of a database is made by DuckDB (duckdb-wasm) and the Rust module: roadstyle's own Rust engine (roadstyle repository,
rust/, the crate roadstyle-wasm) at ROADSTYLE_COMMIT, cloned into engine/ (docs/design/rust-spike.md, "The engine moves to roadstyle").
"""
import argparse
import base64
import dataclasses
import inspect
import json
import re
import struct
import subprocess
import zipfile
from importlib.resources import files
from pathlib import Path

HERE = Path(__file__).resolve().parent
VERSIONS = {"roadstyle": "0.24.3", "mapstyle": "0.12.0"}     # the looks and the Rust code follow these releases (the golden files of spike/)
# roadstyle's Rust engine (roadstyle repository, rust/): the style, the tiles and the levels, one copy for the plugin, duckosm-rust and roadstyle
ROADSTYLE_REPO, ROADSTYLE_COMMIT = "https://github.com/Khoshkhah/roadstyle.git", "9175c7f"
DB = HERE.parent / "duckOSM" / "monaco.duckdb"                 # any duckOSM database: the looks do not depend on it (only the name, which the worker gives)
# the dialog's choices (render_map's mode, theme and paths), and the arguments a look is read with
MODES, THEMES, PATHS = ("all", "driving", "walking", "cycling"), ("osm", "google", "grey", "roadstyle"), ("google", "osm", "komoot", "cyclosm")
LOOK_ARGS = dict(layers=False, arrows=False, labels=False, view_3d=False)


def _plugin_css():
    """roadstyle's popup.css for the plugin: each rule only under .rs-plugin, so an app's own popups keep their look."""
    css = re.sub(r"/\*.*?\*/", "", files("roadstyle").joinpath("static/popup.css").read_text(), flags=re.S)
    css = re.sub(r"(^|})\s*([^{}]+?)\s*{", lambda m: m.group(1) + "\n" + ",".join(".rs-plugin " + x.strip() for x in m.group(2).split(",")) + "{", css)
    return css.strip() + "\n"


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

def look_of(db, mode="all", theme="osm", paths="google"):
    """The resolved look of one choice of the plugin's dialog (render_map's mode, theme and paths), and the roads as roadstyle gets them."""
    import roadstyle as rs
    from mapstyle.map import render_map
    from mapstyle.style import load_style
    from roadstyle import _settings
    from roadstyle import render_web as rw

    got = {}

    def grab(roads, palette, settings, kw, config):    # what mapstyle gives to rs.export_inline
        got.update(roads=roads, palette=palette, settings=settings, kw=kw)
        return None

    render_map(db, mode, _export=grab, theme=theme, paths=paths, **LOOK_ARGS)
    settings, kw = got["settings"], got["kw"]
    prev = list(_settings._EXTRA)                      # the look as render_edges applies it: the settings over the ones in force
    rs.use_settings(*prev, settings)
    try:
        theme_rs, config = _settings.theme(), dataclasses.asdict(rw.CONFIG)
        styles = _class_styles(got["palette"], rw.CONFIG)
        head_m = inspect.signature(rw.render).parameters["head_m"].default
        # S6: a slot's rank (the z_order table) and how far a name keeps from a crossing road (its drawn width at zoom 20, _crossing_half_m)
        slots_look = {"z_order": dict(rw.ROAD_Z), "width_z20": {c: rw.class_width_px(c, 20) for c in rw._CLASSES},
                      "width_z20_fallback": rw.class_width_px(None, 20)}
        roads_model = _settings.roads()        # step 2 of the full port: the road tables the layer builders read (render_web._load_road_model)
        # the page metadata of the export: roadstyle:interaction's road popup fields and hover delay (render's defaults)
        interaction = {"popup": list(rw.DEFAULT_ROAD_POPUP), "hover_delay_ms": inspect.signature(rw.render).parameters["hover_delay_ms"].default}
        oneway = _oneway_image()
    finally:
        rs.use_settings(*prev)
    look = {"palette": got["palette"], "settings": settings,
            "args": {k: v for k, v in kw.items() if k != "overlays"},      # name, tooltip, level columns, ... (the overlays are the base map)
            "theme": theme_rs, "config": config,
            # what the Rust code reads (S2): the class styles as the styler resolves them, and the render's defaults
            "styles": styles, "head_m": head_m, "slots": slots_look, "roads_model": roads_model,
            # step 3 of the full port: the base-map layers (mapstyle's styles/layers.yaml and the 3D heights of load_layers), the look of
            # feature_overlays (the theme's "features" style) and its images, and the arrow image of roadstyle's export
            "features": {"layers_yaml": load_style("layers"), "buildings_3d": load_style()["features"]["buildings_3d"],
                         "style": _theme_features(theme), "images": _feature_images(theme)},
            "oneway_image": oneway, "interaction": interaction}
    return json.loads(json.dumps(look, default=_json)), got["roads"]

def looks(db=DB):
    """The looks of the dialog's 64 choices, each part once: {"parts": {key: [values]}, "choices": {"mode|theme|paths": {key: index}}}
    (1.4 MB, where the 64 looks are 4 MB). The worker puts a choice's look together from it."""
    parts, choices = {}, {}
    for m in MODES:
        for t in THEMES:
            for p in PATHS:
                idx = {}
                for k, v in look_of(db, m, t, p)[0].items():
                    vals = parts.setdefault(k, [])
                    if v not in vals:
                        vals.append(v)
                    idx[k] = vals.index(v)
                choices[f"{m}|{t}|{p}"] = idx
    return {"parts": parts, "choices": choices}


def engine():
    """The rust/ folder of roadstyle's Rust engine at ROADSTYLE_COMMIT: a clone in engine/roadstyle (made the first time, then fetched)."""
    repo = HERE / "engine" / "roadstyle"
    if not repo.exists():
        subprocess.run(["git", "clone", "-q", ROADSTYLE_REPO, str(repo)], check=True)
    if subprocess.run(["git", "-C", str(repo), "cat-file", "-e", ROADSTYLE_COMMIT + "^{commit}"], capture_output=True).returncode:
        subprocess.run(["git", "-C", str(repo), "fetch", "-q", "origin"], check=True)
    subprocess.run(["git", "-C", str(repo), "checkout", "-q", ROADSTYLE_COMMIT], check=True)
    return repo / "rust"


def build_wasm():
    """The Rust module (the engine's crate roadstyle-wasm) as engine/pkg: roadstyle_wasm_bg.wasm and its JavaScript glue."""
    rust, pkg = engine(), HERE / "engine" / "pkg"
    subprocess.run(["cargo", "build", "--release", "--target", "wasm32-unknown-unknown", "-p", "roadstyle-wasm", "-q"], cwd=rust, check=True)
    subprocess.run(["wasm-bindgen", "--target", "web", "--out-dir", str(pkg),
                    str(rust / "target" / "wasm32-unknown-unknown" / "release" / "roadstyle_wasm.wasm")], check=True)
    return pkg


def write_map_files(folder=HERE, db=DB):
    """map/worker.js: the worker that makes a database's map (worker/worker.js and its SQL, one file, by esbuild); map/pkgs.bin: the looks
    and the Rust module, as one blob: [length of the looks, 4 bytes LE][looks][wasm]."""
    from importlib.metadata import version
    bad = {k: version(k) for k, v in VERSIONS.items() if version(k) != v}
    if bad:
        raise SystemExit(f"build.py needs {VERSIONS}, not {bad}: pip install " + " ".join(f"{k}=={v}" for k, v in VERSIONS.items()))
    if not Path(db).exists():
        raise SystemExit(f"no database {db} for the looks: pass --db with any duckOSM .duckdb (the looks do not depend on it)")
    pkg = build_wasm()
    pack = json.dumps(looks(db), separators=(",", ":")).encode()
    d = Path(folder) / "map"
    d.mkdir(exist_ok=True)
    (d / "pkgs.bin").write_bytes(struct.pack("<I", len(pack)) + pack + (pkg / "roadstyle_wasm_bg.wasm").read_bytes())
    r = subprocess.run(["npx", "--yes", "esbuild@0.25.5", str(HERE / "worker" / "worker.js"), "--bundle", "--format=esm", "--minify", "--charset=utf8",
                        "--log-level=warning"], capture_output=True, text=True, cwd=HERE)
    if r.returncode or not r.stdout.strip():
        raise SystemExit("esbuild failed: " + r.stderr[-800:])
    (d / "worker.js").write_text(r.stdout)


def write_plugin_files(folder=HERE, db=DB):
    """The generated files: the default look and the page's popup files (from roadstyle), and map/ (write_map_files)."""
    from roadstyle.export import export_look

    d = Path(folder)
    export_look(d / "looks" / "default.json")
    (d / "popup.css").write_text(_plugin_css())
    (d / "popup.js").write_text(files("roadstyle").joinpath("static/popup.js").read_text())
    write_map_files(d, db)


def _minify(js):
    """The plugin's code minified (GeoLibre's registry asks for a minified bundle); the readable source is in this repository. esbuild, through npx."""
    import subprocess
    r = subprocess.run(["npx", "--yes", "esbuild@0.25.5", "--minify", "--format=esm", "--charset=utf8", "--log-level=warning"], input=js, capture_output=True, text=True)
    if r.returncode or not r.stdout.strip():
        raise SystemExit("esbuild failed: " + r.stderr[-500:])
    return r.stdout


def write_plugin_zip(folder, out_zip):
    """The plugin in ``folder`` as one .zip for GeoLibre's Install from file. GeoLibre gives a zip plugin no URL for its
    other files, so the zip's index.js carries them (its ASSETS): the look, map/worker.js, map/pkgs.bin (as base64), popup.css, popup.js and the route planner's files (planner/)."""
    d = Path(folder)
    files_ = [d / "popup.css", d / "popup.js", d / "map" / "worker.js", d / "map" / "pkgs.bin", *sorted((d / "planner").glob("*")), *sorted((d / "looks").glob("*.json"))]
    assets = {p.relative_to(d).as_posix(): (base64.b64encode(p.read_bytes()).decode() if p.suffix == ".bin" else p.read_text())
              for p in files_ if p.is_file()}      # only the plugin's files (not tests/ or out/)
    index = (d / "index.js").read_text()
    if "/*__ASSETS__*/null" not in index:
        raise ValueError(f"{d / 'index.js'} has no /*__ASSETS__*/null for the files")
    Path(out_zip).parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out_zip, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("plugin.json", (d / "plugin.json").read_text())
        z.writestr("index.js", _minify(index.replace("/*__ASSETS__*/null", json.dumps(assets))))
        if (d / "README.md").exists():
            z.writestr("README.md", (d / "README.md").read_text())
    return Path(out_zip)


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--files", action="store_true", help="only the generated files")
    ap.add_argument("--zip", action="store_true", help="only the .zip")
    ap.add_argument("--db", default=str(DB), help="the duckOSM database the looks are made with (any: they do not depend on it)")
    a = ap.parse_args()
    if not a.zip:
        write_plugin_files(HERE, a.db)
    if not a.files:
        print(write_plugin_zip(HERE, HERE / "dist" / "roadstyle-geolibre.zip"))
