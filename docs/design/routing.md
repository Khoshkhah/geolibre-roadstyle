# Design: routing in the roadstyle plugin

**Status:** proposal, 2026-10-22. Nothing is coded. It waits for sign-off.

## Goal

Kaveh opens a duckOSM database in GeoLibre (menu **roadstyle › Open a duckOSM database…**). He then drops a start and an end on the map and sees the
route, with time, length and turn-by-turn steps. Routing runs in the browser. No server.

## What exists

- **mapstyle's route planner** (`mapstyle/planner.html`, about 36 KB of JavaScript, from `duckosm route-map`). It has Dijkstra in the page (a heap), snapping
  of a click to the nearest road, the modes Drive, Walk, Cycle and Walk + drive, fastest or shortest, the route as coloured legs, and turn-by-turn
  directions (about 150 lines). In the page it gives the same answers as duckOSM's `route()` (8 of 8 trips on Monaco). The data it needs are the
  graphs of the modes (`mapstyle.map.planner_data(db, roads)`, in the page as `RM`).
- **The plugin's worker** already runs mapstyle in Python in the browser (Pyodide), so it can call `planner_data` on the opened database.
- **GeoLibre gives plugins panels:** `registerFloatingPanel`, `openFloatingPanel`, `registerRightPanel`. The planner needs no hand-made box.

## Proposal

1. **Data.** The worker returns the planner data with the style, when the user chooses "Route planner: on" in the dialog (default off).
   `mapstyle.export.export_inline(db, planner=True)` puts the graphs in the style's metadata (`roadstyle:planner`). Monaco: about 2 MB.
2. **Code.** The routing code of `planner.html` moves into one JavaScript file, `planner-core.js`, in mapstyle: the heap, `routeOne`, `routeMulti`,
   snapping, `directions`. It has no DOM and no map calls. `planner.html` and the plugin both load it, so there is one copy of the algorithms and one
   set of tests. This is mapstyle 0.11.
3. **Plugin.** A new menu item **Route planner** opens a floating panel: the mode, fastest or shortest, the start and the end (two markers that you drag),
   the summary (time, length), and the steps (a click on a step zooms to it). The route is drawn as a GeoJSON line (casing and fill, coloured by leg)
   in a row "route" of the Features folder, so it is over the roads. The row can be switched off.
4. **No clash with GeoLibre's tools.** The markers use their own mouse handling and stop while a GeoLibre tool (Identify, …) is on, as the road popup does.

## Choice: where does the routing run?

| | A. JavaScript in the page (recommended) | B. Python in the worker |
|---|---|---|
| Code | The code of `planner.html`, reused as it is | New: `scipy.sparse.csgraph` in Pyodide; the turn-by-turn rules must be ported to Python |
| Speed on drag | Instant (no message to the worker) | A message for each move: tens of milliseconds |
| Size limit | The graphs go into the page: fine up to about 100,000 edges | The graphs stay in the worker: larger areas possible |
| Test | The 8 of 8 trips check of duckOSM carries over | New checks |

Recommendation: **A** now. B only if someone needs a county. For a larger database the plugin says "too large for the planner" and does not load the data.

## Not in the first version

Lane advice, exit numbers, spoken directions, saved routes, a route from a typed address. A button "Download the route as GeoJSON" is cheap and can come with it.

## Tests

- `planner-core.js` has unit tests in mapstyle (Node): the same trips as duckOSM's `route()`.
- The plugin: the stand-in host test opens a style with planner data, drops two points, checks the route layer. The real GeoLibre e2e (`tests/e2e_geolibre.py`) does the same with Monaco.

## Steps

1. mapstyle 0.11: `planner-core.js`, `export_inline(planner=True)`, the page uses the core.
2. Plugin 0.19: the dialog choice, the floating panel, the route row, the tests, the README section.
3. Real GeoLibre test, then release (mapstyle first, then the plugin).

## Decisions for Kaveh

1. A or B? (Recommendation: A.)
2. The panel: a floating panel (recommended; the user can move it) or the right panel?
3. Modes in the first version: all four (as the page has them), or driving and walking only?
