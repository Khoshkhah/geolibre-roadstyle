"""Make the generated files of the plugin, and the .zip for GeoLibre's "Install from file".

    python build.py            # the files of this folder, then dist/roadstyle-geolibre.zip
    python build.py --files    # only the files (the look, popup.css, popup.js, py/pkgs.zip)
    python build.py --zip      # only the .zip

Needs roadstyle 0.24.0 or later and mapstyle (``pip install roadstyle mapstyle``). The two functions come
from roadstyle's old ``geolibre`` branch (``roadstyle.export``), which no longer exists.
"""
import argparse
import base64
import json
import re
import zipfile
from importlib.resources import files
from pathlib import Path

HERE = Path(__file__).parent


def _plugin_css():
    """roadstyle's popup.css for the plugin: each rule only under .rs-plugin, so an app's own popups keep their look."""
    css = re.sub(r"/\*.*?\*/", "", files("roadstyle").joinpath("static/popup.css").read_text(), flags=re.S)
    css = re.sub(r"(^|})\s*([^{}]+?)\s*{", lambda m: m.group(1) + "\n" + ",".join(".rs-plugin " + x.strip() for x in m.group(2).split(",")) + "{", css)
    return css.strip() + "\n"


def write_plugin_files(folder=HERE, mapstyle_src=None):
    """The files that roadstyle makes: the default look, the page's popup files, and py/pkgs.zip: roadstyle's and
    mapstyle's Python, which the plugin runs in the browser (Pyodide) to make a .duckdb's map.
    ``mapstyle_src``: the folder of the mapstyle package (default: the installed one)."""
    import roadstyle
    from roadstyle.export import export_look

    d = Path(folder)
    export_look(d / "looks" / "default.json")
    (d / "popup.css").write_text(_plugin_css())
    (d / "popup.js").write_text(files("roadstyle").joinpath("static/popup.js").read_text())
    if mapstyle_src is None:
        try:
            import mapstyle
        except ImportError:
            raise SystemExit("mapstyle is not installed: pip install mapstyle (or pass mapstyle_src)")
        mapstyle_src = Path(next(iter(mapstyle.__path__)))
    (d / "py").mkdir(exist_ok=True)
    with zipfile.ZipFile(d / "py" / "pkgs.zip", "w", zipfile.ZIP_DEFLATED) as z:
        for pkg in (Path(next(iter(roadstyle.__path__))), Path(mapstyle_src)):
            for f in sorted(pkg.rglob("*")):     # not the page's vendored MapLibre and the studio: the browser makes no page
                if f.is_file() and not ({"__pycache__", "vendor", "studio"} & set(f.relative_to(pkg).parts)):
                    z.write(f, (Path(pkg.name) / f.relative_to(pkg)).as_posix())


def write_plugin_zip(folder, out_zip):
    """The plugin in ``folder`` as one .zip for GeoLibre's Install from file. GeoLibre gives a zip plugin no URL for its
    other files, so the zip's index.js carries them (its ASSETS): the look, py/worker.js, py/pkgs.zip (as base64), popup.css, popup.js and the route planner's files (planner/)."""
    d = Path(folder)
    files_ = [d / "popup.css", d / "popup.js", d / "py" / "worker.js", d / "py" / "pkgs.zip", *sorted((d / "planner").glob("*")), *sorted((d / "looks").glob("*.json"))]
    assets = {p.relative_to(d).as_posix(): (base64.b64encode(p.read_bytes()).decode() if p.suffix == ".zip" else p.read_text())
              for p in files_ if p.is_file()}      # only the plugin's files (not tests/ or out/)
    index = (d / "index.js").read_text()
    if "/*__ASSETS__*/null" not in index:
        raise ValueError(f"{d / 'index.js'} has no /*__ASSETS__*/null for the files")
    Path(out_zip).parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out_zip, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("plugin.json", (d / "plugin.json").read_text())
        z.writestr("index.js", index.replace("/*__ASSETS__*/null", json.dumps(assets)))
        if (d / "README.md").exists():
            z.writestr("README.md", (d / "README.md").read_text())
    return Path(out_zip)


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--files", action="store_true", help="only the generated files")
    ap.add_argument("--zip", action="store_true", help="only the .zip")
    a = ap.parse_args()
    if not a.zip:
        write_plugin_files()
    if not a.files:
        print(write_plugin_zip(HERE, HERE / "dist" / "roadstyle-geolibre.zip"))
