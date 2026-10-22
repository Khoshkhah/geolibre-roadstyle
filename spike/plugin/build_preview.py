"""The Rust preview of the plugin as a .zip for GeoLibre's "Install from file" (docs/design/rust-spike.md).

    mapstyle/.venv/bin/python spike/plugin/build_preview.py            # dist/roadstyle-rust-preview.zip

It is the plugin's own index.js with its own id ("roadstyle-rust", menu "roadstyle (Rust)", its own style cache), so it can be installed next to
the roadstyle plugin (activate one of the two at a time). Behind the names index.js already uses: py/worker.js is the Rust worker
(spike/plugin/worker.js, bundled with esbuild), py/pkgs.zip the looks of the dialog's 64 choices (spike/data/looks.json) and the wasm module.
The preview keeps each map in the browser's storage (a patch of index.js: its Python worker keeps only the map with the names and arrows).
Needs cargo (wasm32), wasm-bindgen, npx (esbuild) and the fixture's looks.json (tools/make_spike_fixture.py --choices)."""
import base64
import importlib.util
import json
import struct
import subprocess
import sys
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(ROOT))
import build  # noqa: E402  the plugin's own build (its minifier)

_spec = importlib.util.spec_from_file_location("s4_wasm", HERE.parent / "wasm" / "run_browser.py")
_s4 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_s4)

ID, NAME, MENU, VERSION = "roadstyle-rust", "roadstyle (Rust preview)", "roadstyle (Rust)", "0.21.0-rust.5"
OUT = ROOT / "dist" / "roadstyle-rust-preview.zip"
PATCHES = [         # (the text in index.js, the preview's): each must occur exactly once
    ('const ID = "roadstyle";', f'const ID = "{ID}";'),
    ('const VERSION = "0.20.1";', f'const VERSION = "{VERSION}";'),
    ('  name: "roadstyle",', f'  name: "{NAME}",'),
    ('urlParameterNames: ["roadstyle"]', f'urlParameterNames: ["{ID}"]'),
    ('params.getAll("roadstyle")', f'params.getAll("{ID}")'),
    ('id: "roadstyle-menu", label: "roadstyle"', f'id: "{ID}-menu", label: "{MENU}"'),
    ('indexedDB.open("roadstyle-cache", 1)', f'indexedDB.open("{ID}-cache", 1)'),
    ('box.say("loading Python, roadstyle and mapstyle (the first time about 30 s)")', 'box.say("starting DuckDB and the Rust module")'),
    ('        else done({style: data.style, extra});', '        else { IDB.put(key, data.style).catch(() => false); done({style: data.style, extra}); }'),
]


def bundle_worker():
    r = subprocess.run(["npx", "--yes", "esbuild@0.25.5", str(HERE / "worker.js"), "--bundle", "--format=esm", "--minify", "--charset=utf8",
                        "--log-level=warning"], capture_output=True, text=True, cwd=ROOT)
    if r.returncode or not r.stdout.strip():
        raise SystemExit("esbuild failed: " + r.stderr[-800:])
    return r.stdout


def pkgs():
    looks = (ROOT / "spike" / "data" / "looks.json").read_bytes()
    wasm = (_s4.PKG / "roadstyle_spike_bg.wasm").read_bytes()
    return struct.pack("<I", len(looks)) + looks + wasm


def main():
    _s4.build()                                         # the wasm module and its glue (spike/rust/target/pkg)
    index = (ROOT / "index.js").read_text()
    for a, b in PATCHES:
        if index.count(a) != 1:
            raise SystemExit(f"index.js: {a!r} occurs {index.count(a)} times, not once: update PATCHES")
        index = index.replace(a, b)
    files = [ROOT / "popup.css", ROOT / "popup.js", *sorted((ROOT / "planner").glob("*")), *sorted((ROOT / "looks").glob("*.json"))]
    assets = {p.relative_to(ROOT).as_posix(): p.read_text() for p in files if p.is_file()}
    assets["py/worker.js"] = bundle_worker()
    assets["py/pkgs.zip"] = base64.b64encode(pkgs()).decode()           # index.js decodes a .zip asset from base64
    manifest = json.loads((ROOT / "plugin.json").read_text())
    manifest.update(id=ID, name=NAME, version=VERSION,
                    description="Preview: the roadstyle plugin with the map made in Rust (WebAssembly) and DuckDB in the browser, not Python. "
                                "No 3D bridge decks.")
    OUT.parent.mkdir(exist_ok=True)
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("plugin.json", json.dumps(manifest, indent=2))
        z.writestr("index.js", build._minify(index.replace("/*__ASSETS__*/null", json.dumps(assets))))
    print(f"{OUT} ({OUT.stat().st_size / 1e6:.2f} MB)")


if __name__ == "__main__":
    main()
