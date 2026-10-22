# roadstyle plugin: the guide

The details of the plugin. The short version is in the [README](../README.md).

## Input data

| Menu item | Input | How to make it |
|---|---|---|
| **Open a duckOSM database…** | a `.duckdb` file | `pip install duckosm`, then `duckosm build --config <area>.yaml`. Or use the Monaco file above. [duckOSM](https://github.com/Khoshkhah/duckOSM) |
| **Add a roadstyle layer…** | a `style.json` file from your disk (the fast way) or its URL | see below |
| **Style a layer with roadstyle…** | a data layer in GeoLibre and a look (optional) | see below |

The link `https://web.geolibre.app/?roadstyle=<URL of a style.json>` draws a style when GeoLibre starts (the plugin must be active).

**The fast way: make the style file once, with Python.**

```bash
pip install mapstyle
mapstyle monaco.duckdb --inline monaco.json --planner      # 6 seconds; --planner adds the data of the route planner
```

In GeoLibre: **roadstyle › Add a roadstyle layer…**, choose `monaco.json`. The map opens in about **1 second**, also for a big area. Opening the `.duckdb` file
itself makes the same file in the browser, where Python is about 3 times slower: the map is on the screen after about 18 seconds for Monaco, with the street names and arrows added about 10 seconds later (the next time you open the same file, the browser has kept the result).

**A `style.json` with the data inside** (the same file, from Python):

```python
import json
from mapstyle.export import export_inline          # pip install mapstyle
json.dump(export_inline("monaco.duckdb"), open("style.json", "w"))
```

**A `style.json` with vector tiles** (large areas):

```python
import mapstyle
from roadstyle.export import export_style          # pip install roadstyle mapbox-vector-tile
export_style(mapstyle.load_roads("monaco.duckdb"), "out", base_url="https://example.com/out")
```

Put the files on a server with **https** and **CORS** (GitHub Pages works). GeoLibre cannot read from `http://localhost`.

**A look for a data layer:** `rs.export_look("look.json")` writes a look. The layer needs the columns `highway`, `oneway`, `tunnel`, `bridge`,
`casing_level` and `fill_level` (duckOSM's `duckosm export-gis --format geoparquet` writes them).

## The route planner in detail

- **Drag the two pins** (A start, B end). The route changes at once. **Walk, Cycle, Drive, Walk + drive**, and **Fastest** or **Shortest**.
- The summary shows the time and the length, with a bar for each leg. The steps are turn by turn: a click on a step zooms the map to it.
- Click a road to keep its `edge_id` in the panel. The route is a row **route** in the Layers panel: switch it off there.
- **Back to the normal map.** Close the panel (the × in its header): the pins and the route go, and the map stays as it was. The menu item **roadstyle › Route planner**
  opens the panel again, and the route comes back. **Remove roadstyle layers** removes the whole map.
- **Full screen.** GeoLibre's full screen shows only the map, so the panel becomes a box over the map. The tab "Route planner" hides and shows the box (hiding it also
  leaves routing mode). When you leave full screen, the panel goes back to the side panel.
- The graphs are in the page, so the planner works for areas up to 150,000 roads. A bigger database opens without it.
- The panel and its code are made from duckOSM's `templates/planner.html` by `tools/make_planner.py`.

## The Layers panel

Each map has three folders. The first is on top, and it is drawn last.

| Folder | Rows |
|---|---|
| **Features** | buildings 3D, crossings, traffic signals, bus and train stations, parking signs, bicycle |
| **Roads** | one row for each road type (bridges, tunnels, motorway … footway), then raceway and other ways |
| **Background** | paved areas, buildings, railways, parking, platforms, waterways, water, land cover |

- The check box of a folder switches all its rows. The **Opacity** slider fades them. The slider of a row fades that row.
- A click on a road, a raceway or another way opens its popup.
- Crossings and traffic signals show from zoom 16.

## The dialog of "Open a duckOSM database"

| Choice | Values |
|---|---|
| Roads in front | all modes, driving, walking, cycling |
| Colours | OpenStreetMap (default), Google, Grey, roadstyle default |
| Footpaths and cycleways | Google, OpenStreetMap, Komoot, CyclOSM |
| Ocean polygon | off (default: GeoLibre's own map shows the sea), on |
| 3D bridge decks | off (default), on |
| Route planner | on (default), off |

## Needs

- A browser with enough memory: the `.duckdb` file is read into memory twice.
- The network: the plugin loads Pyodide and its packages (numpy, pandas, shapely, geopandas, DuckDB and a few more) from `cdn.jsdelivr.net`, a pinned version (Pyodide 314.0.7).
  Nothing is installed from PyPI at run time: the roadstyle, mapstyle and branca code is inside the plugin (roadstyle 0.24.2, mapstyle 0.12.0, branca 0.8.2). The code of the zip is
  minified; the readable source is in this repository.

## Build and test

```bash
pip install roadstyle mapstyle
python build.py                                    # makes dist/roadstyle-geolibre.zip
python -m pytest tests -q -m "not slow"            # a few seconds, in a stand-in for GeoLibre
python tests/e2e_geolibre.py monaco.duckdb         # the real web.geolibre.app, with a database (minutes)
python tests/e2e_inputs.py monaco.duckdb           # the real web.geolibre.app, with a style URL and a tiles export
```

Change `version` in `plugin.json` and in `index.js` together (a test checks that they are equal). GeoLibre trusts a plugin by the hash of
its files: after a change, remove the plugin and install the new `.zip`.

Design: `docs/design/style_export.md` in the [roadstyle repo](https://github.com/Khoshkhah/roadstyle).

The GeoLibre logo is the icon [docs/assets/geolibre-icon.png](https://github.com/opengeos/GeoLibre/blob/9cf193094e6575bfd152ec020345a8e69b75290c/docs/assets/geolibre-icon.png) of [GeoLibre](https://github.com/opengeos/geolibre) (MIT license).
