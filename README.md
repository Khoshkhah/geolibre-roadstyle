# roadstyle for GeoLibre

A GeoLibre plugin that adds a roadstyle export (`rs.export_style`: `style.json`, tiles, sprite) as one layer over GeoLibre's
own base map, with roadstyle's look and the roads' data. One plain ES module, no build step.

- **Install:** GeoLibre › Settings › Manage Plugins › Settings › Manifest URLs: the URL of this folder's `plugin.json`, then Add.
- **Open an export:** the menu **roadstyle** › Add a roadstyle layer, or `?roadstyle=<URL of style.json>` in the GeoLibre link.
- **Style a data layer:** the menu **roadstyle** › Style a layer with roadstyle puts a roadstyle look (`rs.export_look`;
  the default is `looks/default.json`) on a GeoLibre data layer that has duckOSM's columns, for example a DuckDB or
  GeoParquet layer. GeoLibre's own line of the layer becomes transparent; Identify still shows the data.
- **Not in the menu?** Plugins › Installed › roadstyle makes the plugin active.
- **What it does:** adds the style's sources and layers to GeoLibre's map, registers them as one row of the Layers panel
  (`registerExternalNativeLayer`), shows a road's data on click (not while a GeoLibre tool such as Identify is on), and
  highlights the road under the cursor. The names take a font of GeoLibre's base map, because a map has one glyph source.
- **What it does not do yet:** roadstyle's page arrows (one per one-way street; the export has fixed rules instead), the
  colour options, `rsColor` and the other `rs*` functions.

Design: `docs/design/style_export.md`, version 2.
