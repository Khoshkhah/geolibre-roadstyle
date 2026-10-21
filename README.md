# roadstyle for GeoLibre

A [GeoLibre](https://web.geolibre.app) plugin that draws roads in roadstyle's look (casings, drawing positions, tunnels,
bridge decks), with the roads' data, over GeoLibre's own base map. One plain ES module, no build step.

## The menu

The plugin adds the menu **roadstyle**:

| Item | Does |
|---|---|
| Open a duckOSM database… | Asks for a `.duckdb` file on your disk. roadstyle and mapstyle make its map in the browser with their own Python code (Pyodide in a worker, `py/worker.js`). No server and no export. The first time, it loads Python and the packages (about 30 s). A box shows the progress, and Cancel stops it. The result is one row for each layer, with the base map's layers in a "features" folder. |
| Add a roadstyle layer… | Asks for the URL of a roadstyle export (`style.json`, from `rs.export_style`). The link parameter `?roadstyle=<URL>` does the same. |
| Style a layer with roadstyle… | Puts a roadstyle look (`rs.export_look`) on a GeoLibre data layer that has duckOSM's columns, for example a DuckDB or GeoParquet layer. Without a look URL, it uses `looks/default.json`. GeoLibre's own line of the layer becomes transparent, and Identify still shows the data. |
| Remove roadstyle layers | Removes all that the plugin added. |

A click on a road opens a popup with its data (not while a GeoLibre tool such as Identify is on). Resting the mouse on a road
lights it. The popup shows every name and value as text.

## Install

**Download the plugin:** [`dist/roadstyle-geolibre.zip`](dist/roadstyle-geolibre.zip) in this repo (open it, then click the download
button). The same file is on the [Releases](https://github.com/Khoshkhah/geolibre-roadstyle/releases) page. In GeoLibre: Settings › Manage Plugins ›
Settings › Install from file, and choose the `.zip`. Then use the menu **roadstyle**.

Two other ways:

1. **By URL.** GeoLibre › Settings › Manage Plugins › Settings › Manifest URLs: the URL of `plugin.json`, then Add. The host must
   serve `.js` files as `text/javascript`. GitHub Pages does. `raw.githubusercontent.com` does not, so GeoLibre cannot load the
   plugin from there.
2. **Build the file yourself.** `pip install roadstyle mapstyle`, then `python build.py`. It makes the generated files (the look, the popup
   files, `py/pkgs.zip` with roadstyle's and mapstyle's Python) and `dist/roadstyle-geolibre.zip` for GeoLibre › Settings ›
   Manage Plugins › Settings › Install from file.

**Not in the menu?** Plugins › Installed › roadstyle makes the plugin active.

**After a change to the plugin:** GeoLibre trusts a plugin by the hash of its files. When the files change, it does not load the
plugin (the console says "changed since you last trusted it"). Remove the manifest URL, add it again and accept the review.
Increase `version` in `plugin.json` and in `index.js` (`VERSION`) with each change. A test checks that the two are equal.

## What the plugin needs

- **mapstyle 0.6.0 or later, and roadstyle 0.24.0 or later.** "Open a duckOSM database" calls `mapstyle.export.export_inline`.
  `py/pkgs.zip` in this repo holds roadstyle 0.24.0 and mapstyle 0.6.0. To refresh it, install newer releases and run
  `python build.py --files`, then increase the version.
- **Network.** The worker loads Pyodide from `cdn.jsdelivr.net` and two small packages from PyPI (`folium`, `branca`).
- **Memory.** The `.duckdb` file is read into memory twice (the browser and the worker). A large file needs a large amount of memory.

## What it does not do yet

roadstyle's page arrows (one per one-way street; the export has fixed rules instead), the colour options, `rsColor` and the other
`rs*` functions.

## Tests

The tests run the plugin in a stand-in for GeoLibre (`tests/host.html`: a MapLibre map and the host calls that the plugin uses).
They need `pytest`, `playwright` with Chromium, `node` and the network (MapLibre is loaded from a CDN).

```bash
python -m pytest tests -q                                              # a few seconds
RS_TEST_DB=/path/to/monaco.duckdb python -m pytest tests -q -m slow    # also makes a map of a database with Pyodide (minutes)
```

The tests do not run GeoLibre itself. After a change, also open the plugin in https://web.geolibre.app.

Design: `docs/design/style_export.md` in the [roadstyle repo](https://github.com/Khoshkhah/roadstyle), version 2. This repo was the
`geolibre-plugin/` folder of roadstyle's `geolibre` branch; its history is kept.
