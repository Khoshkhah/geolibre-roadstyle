# roadstyle for GeoLibre

> ## Download: [roadstyle-geolibre.zip](https://github.com/Khoshkhah/geolibre-roadstyle/releases/latest/download/roadstyle-geolibre.zip) (version 0.15.1)
>
> In GeoLibre: **Settings › Manage Plugins › Settings › Install from file**, choose the `.zip`, then **Plugins › Installed › roadstyle**.
> The same file is in this repo as [`dist/roadstyle-geolibre.zip`](dist/roadstyle-geolibre.zip). All versions: [Releases](https://github.com/Khoshkhah/geolibre-roadstyle/releases).

A [GeoLibre](https://web.geolibre.app) plugin that draws roads in roadstyle's look (casings, drawing positions, tunnels,
bridge decks), with the roads' data, over GeoLibre's own base map. One plain ES module, no build step.

## The menu

The plugin adds the menu **roadstyle**:

| Item | Does |
|---|---|
| Open a duckOSM database… | A dialog asks for a `.duckdb` file on your disk and for the colours: the roads in front (all modes, driving, walking, cycling), the theme (OpenStreetMap, Google, Grey), the path style (Google, OpenStreetMap, Komoot, CyclOSM) and whether to show all feature layers (crossings, traffic signals, …; the default) or only those that the mapstyle page shows. roadstyle and mapstyle make the map in the browser with their own Python code (Pyodide in a worker, `py/worker.js`). No server and no export. The first time, it loads Python and the packages (about 30 s). A box shows the progress, and Cancel stops it. |
| Redraw the database with other colours… | Makes the map of the last file again with other choices, and replaces the first map. |
| Add a roadstyle layer… | Asks for the URL of a roadstyle export (`style.json`, from `rs.export_style`). The link parameter `?roadstyle=<URL>` does the same. |
| Style a layer with roadstyle… | Puts a roadstyle look (`rs.export_look`) on a GeoLibre data layer that has duckOSM's columns, for example a DuckDB or GeoParquet layer. Without a look URL, it uses `looks/default.json`. GeoLibre's own line of the layer becomes transparent, and Identify still shows the data. |
| Remove roadstyle layers | Removes all that the plugin added. |

**The Layers panel** has three folders for each map (the name of the file is in each title). From the top of the panel to the bottom,
which is the draw order:

| Folder | Rows |
|---|---|
| **Features (monaco)**, over the roads | buildings 3D, crossings, traffic signals, bus stations, train stations, parking signs, bicycle |
| **Roads (monaco)** | roads, then the road types: bridges, tunnels and a row for each highway class in the data (motorway to footway). The check box of a type row hides or shows those roads. |
| **Background (monaco)**, under the roads | buildings, railways, parking, platforms, waterways, water, land cover |

**The slider "Opacity"** under a folder (GeoLibre's own) fades all the layers of the folder, and the slider of a row fades the layers of that row.
The plugin multiplies the opacity that the style has (also an opacity that depends on the zoom). The value 1 puts the style's own values back.

Two layers are **left out unless you choose them** in the open dialog: the **ocean polygon** (the blue sea of the mapstyle page: over GeoLibre's
own map it looks like a blue land background) and the **3D bridge decks** (roadstyle's export draws them at 70%, so the water shows through; the
plugin draws them solid). Each gets a row of its own when chosen. GeoLibre ignores the starting visibility of a row, so a row cannot start switched
off; every other layer, also those that the mapstyle page starts hidden, is shown, and you switch it off in the panel. The flat buildings are
in Background and the 3D buildings in Features, because GeoLibre draws a whole folder at one place.
Crossings and traffic signals show from zoom 16 (the plugin says so when you open a map at a lower zoom). The buildings in 3D need
mapstyle 0.7.0 or later.

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

- **mapstyle 0.7.0 or later (0.7.1 has the fixes for the 3D buildings), and roadstyle 0.24.0 or later.** "Open a duckOSM database" calls `mapstyle.export.export_inline`.
  `py/pkgs.zip` in this repo holds roadstyle 0.24.0 and mapstyle 0.7.1. To refresh it, install newer releases and run
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

The tests use a stand-in for GeoLibre. `python tests/e2e_geolibre.py <path to the .duckdb>` runs the real https://web.geolibre.app
in a headless browser: it installs `dist/roadstyle-geolibre.zip`, activates the plugin, opens the database and switches a road
type off (it needs the network and a few minutes). After a change, run it.

GeoLibre shows the plugin only after Plugins › Installed › roadstyle (the plugin is installed, but not active). Then the menu
**roadstyle** is at the end of the top bar.

Design: `docs/design/style_export.md` in the [roadstyle repo](https://github.com/Khoshkhah/roadstyle), version 2. This repo was the
`geolibre-plugin/` folder of roadstyle's `geolibre` branch; its history is kept.
