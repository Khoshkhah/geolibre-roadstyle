# Design: routing in the roadstyle plugin

**Status:** proposal, 2026-10-22 (changed the same day: the panel is duckOSM's own route-planner panel). Nothing is coded. It waits for sign-off.

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

The plugin shows **the same panel as `duckosm route-map`** (Kaveh's request), not a new one. The panel of the page is designed in duckOSM's
`docs/design/route_planner_panel.md`: a header, the mode as a segmented control (Walk, Cycle, Drive, and the combined modes), Fastest or Shortest, a card for
the start (violet, A) and the end (amber, B) with the name of the nearest road and a swap button, a summary card (time in large type, the distance, a bar
of the legs in the colour of each mode), a timeline of steps (a click flies to the step), and a closed "Options" block (radius, walking speed, `edge_id`).

1. **Data.** The worker returns the planner data with the style, when the user chooses "Route planner: on" in the dialog (default off).
   `mapstyle.export.export_inline(db, planner=True)` puts the graphs in the style's metadata (`roadstyle:planner`). Monaco: about 2 MB.
2. **Code, in three parts, in mapstyle (0.11).** `planner.html` is split so that the page and the plugin share everything:
   - `planner-core.js`: the heap, `routeOne`, `routeMulti`, snapping, `directions`. No DOM, no map calls. Unit tests in Node.
   - `planner-panel.html` and `planner-panel.css`: the markup and the CSS of the panel, as they are in the page now (the ids `rm-*` stay).
   - `planner-ui.js`: the wiring of the panel, the markers and the map (the code that `planner.html` has after the algorithms).
   The page assembles the three files, so `duckosm route-map` looks and works as today (its tests stay). The plugin loads the same three files as assets,
   as it does with `popup.css` and `popup.js`.
3. **Plugin (0.19).** A menu item **Route planner** opens the panel in GeoLibre's **right panel** (`registerRightPanel`), which is where the page has its side
   panel. The panel follows GeoLibre's light or dark theme (the page's own theme button is hidden). On a narrow screen GeoLibre's own layout applies. The
   route is drawn as GeoJSON lines (casing and fill, coloured by leg) in a row "route" of the Features folder, over the roads, so it can be switched off.
   The two markers are the page's pins (A violet, B amber).
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
2. The panel in GeoLibre's right panel (recommended, as the page's side panel) or a floating panel?
3. Modes in the first version: all four (as the page has them), or driving and walking only?
