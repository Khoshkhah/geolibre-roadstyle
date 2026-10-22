# Spike: the roads of the style in Rust WebAssembly

Status: S0, S1 (smaller form) and S2 are done. The browser profile is done (see "Where the browser time goes"). S3 waits for a decision (2026-10-22).

## Question

Can a Rust WebAssembly module read the roads from a Parquet file and make the road part of the style, with the same result as `export_inline`, and in much less time than Pyodide?

If yes, the plugin can drop Pyodide later, step by step. If no, the plugin keeps the Python worker, and this spike costs about one week.

## What the Python code makes today

The numbers are from `export_inline("duckOSM/monaco.duckdb", layers=False, arrows=False, labels=False)`, run natively on 2026-10-22. It took about 2.2 s.

| Part | Size for Monaco | Made by |
|---|---|---|
| Source `roads` | 13,298 LineStrings, 48 properties each, 15 MB of JSON | `render_web._edge_features` |
| Source `simple` | 47,530 LineStrings (the casing pieces), 31 MB of JSON | `render_web._casing_parts`, `_simple_pieces` |
| Source `decks` | 1,311 Polygons (3D bridge decks), 0.9 MB | `render_web` (needs a polygon buffer) |
| Layers | `roads-simple`, `roads-fill`, `roads-highlight`, 2 deck layers | `render_web` expression builders |

Monaco has the stored drawing order in `visualization.edge_levels` (13,298 rows). The columns are `casing_start`, `casing_level`, `casing_end`, `fill_level`, `head_start_m`, `head_end_m`, `cap_start` and `cap_end`. Python reads them with `mapstyle.map.stored_levels`. The spike uses them as they are. It does not solve the levels.

## Scope

**In:**
- The default look: mode `all`, path style `osm`, theme `osm`.
- The sources `roads` and `simple`.
- The layers `roads-simple`, `roads-fill` and `roads-highlight`.
- The twin-lane and seam logic (`__rs_dup`, `__rs_seam`), because it is inside these sources.

**Out** (each one is a later step if the spike passes):
- The 3D decks: the layers `roads-deck-casing` and `roads-bridge-decks`, and the source `decks`. Run Python with `view_3d=False` for the golden file.
- The one-way arrows and the street names.
- The base-map layers (`features.*`) and the route-planner data.
- The solver for the levels (`roadstyle.levels`, about 1,100 lines with level_area). A file without `edge_levels` needs it.
- Reading the `.duckdb` file, and the other modes, path styles and themes.

## The look is resolved at build time

`mapstyle.map.mode_settings` turns the YAML files into one palette and one settings object. The spike does not port this. The Python fixture script resolves the look once and writes it as `look.json`. Rust reads `look.json` as data.

The same method can work for the real port: the plugin build can pre-compute the look for each mode, path style and theme. Then Rust never reads YAML.

## Input: the fixture

The fixture script is `tools/make_spike_fixture.py`. It uses the mapstyle environment (`mapstyle/.venv`) and writes to `spike/data/`:

| File | Content |
|---|---|
| `roads.parquet` | The roads as `mapstyle.map.render_map` gives them to roadstyle, after `stored_levels`. That is: `edge_id`, `osm_id`, `highway`, `name`, `bridge`, `tunnel`, `layer`, `oneway`, `walk_type`, `junction`, `edge_ref`, `lanes`, `driving`, `walking`, `cycling`, `access`, the level columns, `is_directed`, `has_arrows`, and `geometry` as WKB. `edge_id` and `osm_id` are strings, as in Python (the hashes can pass 2**53). |
| `look.json` | The resolved palette, the `roads` settings and the `config` from `mode_settings("all", "osm", "osm")`, and the roadstyle `CONFIG` values that the road layers read. |
| `golden.json` | The result of `export_inline(..., layers=False, arrows=False, labels=False, view_3d=False)`. Only `sources.roads`, `sources.simple` and the three layers are compared. |
| `versions.txt` | The versions of roadstyle, mapstyle and the Python packages. The golden file belongs to these versions. |

**Gap to say clearly:** this Parquet file is not what `duckosm-rust` writes today. `duckosm-rust` writes edges and nodes for one mode. It has no level columns, no mode flags and no `is_directed`. The spike tests the style code, not the input contract. Step S5 lists the gap.

## Steps

Each step has a check. Do not start the next step until the check passes.

| Step | Work | Check |
|---|---|---|
| S0 (0.5 day) | The fixture script and the compare script (see "The compare script"). | The compare script says "equal" when it compares `golden.json` with itself, and shows a diff when one number changes. |
| S1 (1 day) | A Rust crate in `spike/rust/`. It reads `look.json` and writes the three layers. The expression builders are ports of `_width_expr`, `_offset_match`, `_sort_key` and the others in `render_web.py`. | The layers are equal to the golden layers. |
| S2 (1.5 days) | Read `roads.parquet` and write the source `roads`: the `__rs_*` properties, `__rs_dup` and the twin pairs, and the feature ids. | The 13,298 features are equal to the golden ones. |
| S3 (2 days) | The source `simple`: `_casing_parts`, `_end_stub`, `_simple_pieces`, the keys `__rs_s`, `__rs_k` and the ids. This is the hardest step. | The 47,530 features are equal to the golden ones. |
| S4 (1 day) | A `wasm-bindgen` function `make_style(parquet: Uint8Array, look: string) -> string`. Run it in a worker in headless Chromium, as `duckosm-rust` already does for its own wasm. | The browser result is equal to the golden file. The time and the sizes are measured (see "Pass rule"). |
| S5 (0.5 day) | Write the result in this file: the numbers, the lines of Rust against the lines of Python, and the list of what remains. The list includes the gap between this Parquet file and `duckosm-rust`'s output. | A go or no-go for the next stage. |

Total: about 6.5 working days.

## The compare script

The script is `spike/compare.py`. It compares two style files as data, not as text:
- Numbers are equal when they differ by less than 1e-9. The text of a float can differ between Python and Rust.
- The order of the keys does not matter. The order of the features and the layers does.
- It prints the first 20 differences as paths, for example `sources.simple.data.features[812].properties.__rs_s`.

## Pass rule

The spike passes when all of these are true on Monaco:
1. The three layers and the features of `roads` and `simple` are equal to the golden file (S1 to S4).
2. `make_style` takes 1 s or less in headless Chromium, without the download of the `.wasm` file. (Today: about 2.2 s natively. Pyodide is about three times slower, so about 6 s or more. Measure Pyodide with `tests/probe_timing.py` in step S5 for a fair number.)
3. The `.wasm` file is 1.5 MB or less with gzip. (Reference: `duckosm-wasm` is 0.81 MB with gzip.)

If check 1 fails because of a rule that is hard to port, write which rule and stop. That is a result too.

## Risks

| Risk | Effect | What to do |
|---|---|---|
| The seam and twin-lane code (`_mark_twin_casing`, `_casing_parts`) has many special cases. | S3 takes longer. | Port function by function. Compare after each one with a small Python test on a few edges before the full Monaco run. |
| The output is 42 MB of JSON (the golden file, without the decks). | Building the string is slow in wasm, and the copy to the page doubles the memory. | Measure it in S4. Write with `serde_json` straight to a byte buffer. |
| Rust and Python round floats in a different way. | False differences. | The compare script uses a tolerance. |
| The real input is `.duckdb`, not Parquet. | The spike result does not cover the whole path. | S5 names the options: duckdb-wasm in the worker, or a `duckosm` export to Parquet. |
| Two copies of the drawing logic (Python and Rust). | They can drift. | Keep `golden.json` in the tests. Run it on each roadstyle release. |

## Files this spike adds

```text
docs/design/rust-spike.md     this file
tools/make_spike_fixture.py   S0: roads.parquet, look.json, golden.json, versions.txt
spike/compare.py              S0: compare two styles as data
spike/rust/                   S1 to S4: the crate (Cargo.toml, src/)
spike/data/                   generated files (in .gitignore)
```

The spike makes no change to `index.js`, `py/worker.js` or `plugin.json`.

## Result of S0 (2026-10-22)

- `mapstyle/.venv/bin/python tools/make_spike_fixture.py` writes the four files in `spike/data/` in about 5 s. It uses roadstyle 0.24.2 and the local mapstyle source.
- Monaco gives 13,298 roads, 13,298 road features, 47,530 pieces and 3 layers (`roads-simple`, `roads-fill`, `roads-highlight`). `golden.json` is 42 MB.
- `roads.parquet` has these columns: `edge_id`, `osm_id`, `highway`, `name`, `bridge`, `tunnel`, `layer`, `oneway`, `walk_type`, `junction`, `edge_ref`, `lanes`, `driving`, `walking`, `cycling`, `geometry` (WKB), `access`, `casing_start`, `casing_level`, `casing_end`, `fill_level`, `head_start_m`, `head_end_m`, `cap_start`, `cap_end`, `is_directed`, `has_arrows`. It is 1.1 MB.
- **The Parquet file loses nothing.** `roadstyle.export_inline`, with only `roads.parquet` and `look.json` as input, gives a style that `spike/compare.py` calls equal to `golden.json`. So the Rust code needs no other input for this look.
- `python -m pytest spike/test_compare.py -q`: 4 tests pass. The compare script says "equal" for the golden file against itself, and shows `sources.simple.data.features[812].properties.__rs_s` when that number changes.
- `look.json` holds the resolved `theme` and `config` (31 KB). Step S1 finds out which of their values the layers read. It can then cut the file down.

## Result of S1 (2026-10-22)

**Finding: the layers are not a function of the look alone.** The same look gives different layers for different road data. I made the style in Python for subsets of the Monaco roads and compared the layers. At least these properties of the data change the layers:

| Flag of the data | What it changes |
|---|---|
| A tunnel is present | `roads-simple`: the colour (`_simple_color`), the width cases, the dash array |
| The set of dash values (`__rs_dash`) | `roads-simple`: the entries of the `line-dasharray` match |
| A two-way pair with one casing (`__rs_pair`) | `line-offset` of all three layers, `line-width` of `roads-simple` |
| A reverse pair drawn as one line (`__rs_dup`) | An extra filter on the layers of the `roads` source |

Two subsets with the same four flags still gave different layers in one test. Thus more flags can exist. After the layers are built, three more passes change them: `_drop_empty_layers`, `_state_expressions` and `_tunnel_look`. I did not read these passes yet.

**What a full port of the layers needs.** About 25 expression builders in `render_web.py` (`_width_expr`, `_offset_expr`, `_offset_match`, `_pair_width`, `_plus_px`, `_by_feature`, `_simple_color`, `_simple_dasharray`, `_simple_cap`, `_seam_filter`, `_minzoom_filter`, the tunnel colour helpers, ...), the road tables of `_load_road_model`, the flags above, and the three passes. That is much more than one day.

**What S1 did instead.** The fixture writes the golden layers to `spike/data/layers.json`. The Rust crate (`spike/rust/`, only `serde_json`) reads them and writes them into the style. The compare script says "equal" for the layers. The crate builds offline, for the native target and for `wasm32-unknown-unknown`.

**Why this is still a valid spike.** The layers are about 100 KB of JSON, and Python makes them once per map. The time and the 46 MB of output come from the features (S2 and S3). The speed question of the spike is in S2 and S3. The layers are a separate question: port them to Rust, or make them in Python at build time for each flag set. That question needs the full list of flags first.

**The estimate changes.** S2 and S3 also go through many stages before the features are complete: `bake_props` (the palette colours), `_mark_twoway`, `_mark_single_line`, `_mark_lvl`, `_mark_caps`, `_mark_levels`, `_mark_twin_casing`, `_stringify_unsafe_ints`, `_casing_parts`, `_end_stub`, `_simple_pieces` and `_mark_cls`. I estimate about 2,000 lines of Python for the features and the layers together. This is two to three weeks of work, not 6.5 days.

## Result of S2 (2026-10-22)

**The "roads" source in Rust equals Python's.** All 13,298 features of Monaco are equal: the 26 columns of the Parquet file, the 23 computed `__rs_*` properties, the ids and the geometry. The compare script found no difference, and it does find a deliberate change in one property, one coordinate or one missing key.

What the crate does (`spike/rust/src/`, 500 lines with `main.rs`):
- `read.rs`: reads the GeoParquet file (crates `parquet` and `arrow`, version 60, the same as `duckosm-rust`, feature `snap`) and decodes the WKB LineStrings.
- `roads.rs`: ports the stages of `render_web.render` in their order: `bake_props` (the class styles), `_mark_twoway`, `_mark_single_line`, `_mark_lvl`, `_mark_caps`, the head lengths, `_mark_levels`, `_mark_twin_casing`, `_mark_twin_dead_ends`, `_stringify_unsafe_ints`, the ids and `_mark_cls`.

Decisions:
- **The class styles are look data.** `ClassStyler` gives each class one style, plain or as a link, so the fixture resolves the 22 classes and the fallback once (`styles` in `look.json`). Rust does not port the palette code.
- **Python's `round(x, 6)` as a key** is the text of `format!("{x:.6}")`. Both round the exact binary value correctly, and a tie cannot occur at 6 or 7 places.
- **The web renderer has its own `_truthy`**: everything is true except null, `""`, `"no"`, `"false"`, `"0"`, `0` and `False`. For example, `tunnel=building_passage` is a tunnel.
- **Not ported:** the warning of `_mark_twin_casing` when the two directions of a pair disagree.

Times on this computer, native, Monaco: read the Parquet file 44 ms, the stages 230 ms, the JSON 110 ms (13.9 MB). The stages are not tuned yet: they use text keys and a map per feature.

Check: `python -m pytest spike/test_compare.py -q` (5 tests). The test `test_rust_style_equals_golden` builds the crate and compares `layers` and `sources.roads` with the golden file.

## Where the browser time goes (2026-10-22)

Measured with `spike/profile/` : the same Python profile (`rs_profile.py`, a timer around each stage function) natively and in Pyodide in headless Chromium (`run_browser.py`, the plugin's own setup: the CDN version, the packages, `py/pkgs.zip`). Monaco, `export_inline(planner=True)`, the full export. Run 1 is the first map of the session, run 2 the second (warm).

| Stage | Native | Browser, run 1 | Browser, run 2 |
|---|---|---|---|
| Pyodide start + packages (once per session, CDN files in the cache) | — | 5.4 s | — |
| **Names and arrows** (`_annotation_slots`) | 2.9 s | **5.8 s** | 5.0 s |
| Roads: read from DuckDB (`load_roads`) | 0.2 s | 1.3 s | 0.8 s |
| Roads: GeoDataFrame to GeoJSON (`fc_dict`; no DuckDB spatial in wasm, so the slow path) | 0.2 s | 1.2 s | 1.1 s |
| Roads: the marks (`_mark_*`) | 0.2 s | 0.3 s | 0.2 s |
| Simple: casing parts | 0.7 s | 1.2 s | 1.1 s |
| Simple: pieces, stubs, mitres | 0.3 s | 0.4 s | 0.2 s |
| Planner data (SQL) | 0.2 s | 0.7 s | 0.7 s |
| Features layers: read + overlays + restyle | 0.4 s | 0.6 s | 0.4 s |
| Decks (3D) | 0.1 s | 0.2 s | 0.2 s |
| Stored levels | 0.1 s | 0.2 s | 0.1 s |
| Everything else | 0.5 s | 0.7 s | 0.6 s |
| **Total `export_inline`** | **5.6 s** | **12.5 s** | **10.3 s** |
| `json.dumps` of the style (47 MB) | 0.4 s | 0.6 s | 0.5 s |

Pyodide is about 2.2 times slower than native overall; DuckDB reads are 4 to 6 times slower (one thread, no spatial extension).

**What this means for the spike.**
- **What the spike ports (roads + simple) is 4.4 s of the 12.5 s** of the first map (2.8 s roads, 1.6 s simple). S2's Rust does the roads part in 0.4 s natively. A Rust module for both saves about 3 s of the time to the first picture, which is Python start (5.4 s) + everything but the names and arrows (6.7 s) + the transfer and MapLibre: about 25 %.
- **The names and arrows are the biggest single stage (5.8 s, 46 %)**, but in the plugin they run after the first picture (the `split` mode of `py/worker.js`), so they delay the names, not the map.
- **The Python start (5.4 s here, 6 s in the guide, 30 s with the first download) does not change** until Pyodide is gone. That needs the full port: roads, simple, names and arrows, features layers, planner data, decks, and a reader of the `.duckdb` file (duckdb-wasm in JavaScript, or Parquet written by `duckosm`). The levels solver can stay offline (`duckosm levels`).

**Decision rule, as set before the measurement:** roads + simple at 4 s or more of the map time means S3 and S4 go on. It is 4.4 s. But the honest aim changes: the spike is a go / no-go for the **full port** (the only way to remove the Python start and the first download), not for a hybrid. A hybrid (Rust next to Pyodide) gains about 3 s and adds a second copy of the drawing logic; it is not worth shipping. S3 is the step that shows whether the hardest geometry (seams, laps, stubs, halves) can be ported exactly, which is the main unknown of the full port.

Cheap wins in Python, seen on the way (not part of the spike): `fc_dict` takes the GeoPandas path in the browser (1.2 s) because DuckDB wasm has no spatial extension; the planner data is 0.7 s of `fetchall` loops that could be one SQL.

Rerun: `mapstyle/.venv/bin/python spike/profile/rs_profile.py monaco.duckdb` (native, 6 s) and `... spike/profile/run_browser.py monaco.duckdb` (browser, about 40 s).
