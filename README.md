# roadstyle for GeoLibre

A GeoLibre plugin that adds a roadstyle export (`rs.export_style`: `style.json`, tiles, sprite) as one layer over GeoLibre's
own base map, with roadstyle's look and the roads' data. One plain ES module, no build step.

- **Install:** GeoLibre › Settings › Manage Plugins › Settings › Manifest URLs: the URL of this folder's `plugin.json`, then Add.
- **Open an export:** the menu **roadstyle** › Add a roadstyle layer, or `?roadstyle=<URL of style.json>` in the GeoLibre link.
- **Open a duckOSM database:** the menu **roadstyle** › Open a duckOSM database asks for a `.duckdb` file on your disk. The
  plugin reads it in the browser with DuckDB-wasm (`duckosm.js`; no server, no export), and draws the roads of every mode
  with roadstyle's look (`looks/default.json`; each casing cut into its heads at the file's drawing positions), the
  `features.*` base map with mapstyle's look (`looks/mapstyle.json`) and the boundary: one row per layer, in one folder.
  A file with no `visualization.edge_levels` draws every road at one position, and the plugin says so (`duckosm levels`).
  Make the plugin's files again after a change of the config: `python -c "import roadstyle.export as e; e.write_plugin_files('geolibre-plugin')"`
  and mapstyle's `export_look('geolibre-plugin/looks/mapstyle.json')`.
- **Open a duckOSM folder:** the menu **roadstyle** › Open a duckOSM folder takes the URL of a folder from
  `duckosm export-gis --format geoparquet` (it has `layers.json`). The plugin opens GeoLibre again with every file of the
  folder as a data layer (`?data=…&data=…&roadstyle_look=…&mapstyle_look=…`), but not the nodes and not a features table
  that mapstyle does not draw. It puts roadstyle's look (`looks/default.json`, from `rs.export_look`) on the edges and the
  boundary, and mapstyle's look (`looks/mapstyle.json`, from `mapstyle.look.export_look`) on each `features_<table>` layer.
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

Design: `docs/design/style_export.md`, version 2.
