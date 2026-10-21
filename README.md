# roadstyle for GeoLibre

A GeoLibre plugin that adds a roadstyle export (`rs.export_style`: `style.json`, tiles, sprite) as one layer over GeoLibre's
own base map, with roadstyle's look and the roads' data. One plain ES module, no build step.

- **Install:** GeoLibre › Settings › Manage Plugins › Settings › Manifest URLs: the URL of this folder's `plugin.json`, then Add.
- **Open an export:** the menu **roadstyle** › Add a roadstyle layer, or `?roadstyle=<URL of style.json>` in the GeoLibre link.
- **Open a duckOSM database:** the menu **roadstyle** › Open a duckOSM database asks for a `.duckdb` file on your disk.
  roadstyle and mapstyle make its map in the browser with their own Python code (Pyodide in a worker, `py/worker.js`;
  `mapstyle.export.export_inline`), so it is the map of mapstyle's page: one row per layer, the base map's layers in a
  "features" folder. No server and no export. The first time it loads Python and the packages (about 30 s).
- **Install from a file:** `pip install roadstyle mapstyle`, then `python build.py`. It makes the plugin's generated files (the
  look, the popup files, `py/pkgs.zip` with roadstyle's and mapstyle's Python) and `out/roadstyle-geolibre.zip` for
  GeoLibre › Settings › Manage Plugins › Settings › Install from file. The `py/pkgs.zip` in this repo is from the time of
  plugin 0.11.0 (an old roadstyle): run `python build.py --files` to refresh it, and increase `version` in `plugin.json`.
- **Style a data layer:** the menu **roadstyle** › Style a layer with roadstyle puts a roadstyle look (`rs.export_look`;
  the default is `looks/default.json`) on a GeoLibre data layer that has duckOSM's columns, for example a DuckDB or
  GeoParquet layer. GeoLibre's own line of the layer becomes transparent; Identify still shows the data.
- **Not in the menu?** Plugins › Installed › roadstyle makes the plugin active.
- **After a change to the plugin:** GeoLibre trusts a plugin by the hash of its files. When the files change, it does not load
  the plugin (the console says "changed since you last trusted it"). Remove the manifest URL in Manage Plugins › Settings,
  add it again and accept the review. Increase `version` in `plugin.json` with each change.
- **Hover and popup of a look:** each look file has a `hover` layer and the `popup` columns, made from the config files (roadstyle's
  selection colour; roadstyle's default road fields; mapstyle's `layers.yaml` `popup`). The plugin only sets which feature the
  hover layer shows and fills the popup, in MapLibre's own popup look. A GeoLibre tool (Identify) has the click first.
- **What it does:** adds the style's sources and layers to GeoLibre's map, registers them as one row of the Layers panel
  (`registerExternalNativeLayer`), shows a road's data on click (not while a GeoLibre tool such as Identify is on), and
  highlights the road under the cursor. The names take a font of GeoLibre's base map, because a map has one glyph source.
- **What it does not do yet:** roadstyle's page arrows (one per one-way street; the export has fixed rules instead), the
  colour options, `rsColor` and the other `rs*` functions.

Design: `docs/design/style_export.md` in the [roadstyle repo](https://github.com/Khoshkhah/roadstyle), version 2. This repo was the `geolibre-plugin/` folder of roadstyle's `geolibre` branch; its history is kept.
