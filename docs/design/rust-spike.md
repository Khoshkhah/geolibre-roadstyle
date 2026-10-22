# Spike: the roads of the style in Rust WebAssembly

Status: S0 to S4 are done, and condition 1 of the decision (the knife edge) is fixed on a roadstyle branch (2026-10-22).

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

## Result of S3 (2026-10-22)

**The "simple" source in Rust equals Python's exactly.** All 47,530 pieces of Monaco are equal with tolerance 0: every coordinate, every sort key, every property and every id. The compare script finds a coordinate changed by 1e-8, a sort key changed by 1e-6 and one missing lap. The test (`spike/test_compare.py`) now compares the layers, `roads` and `simple` with tolerance 0.

The pieces: 34,280 casings (13,096 seams, 6,548 laps, 10,211 stubs, 2,908 main pieces), 10,112 fills, 1,430 tunnel gap and 1,430 tunnel dash pieces, 278 bridge shadows.

What `spike/rust/src/simple.rs` ports (314 lines): `_casing_parts` (the cuts at the heads, short roads cut in the heads' ratio, the seams and the laps), `_halves`, `_end_stub`, `_simple_pieces` (the piece kinds, the sort keys, the shadows, the tunnel pieces), the feature ids and `_mark_cls`.

How the numbers stay exact:
- The same flat projection at the line's first point, the same sequential sums and `hypot`, and numpy's `searchsorted` as `partition_point`.
- numpy's `round(x, 7)` is `rint(x * 1e7) / 1e7` (checked on one million values), so Rust uses `(x * 1e7).round_ties_even() / 1e7`.
- In wasm, Rust's `cos` and `hypot` come from its own math library, not from glibc. A last-bit difference can change a rounded coordinate in rare cases. S4 must compare the browser output with tolerance 0 too, and accept 1e-7 if it fails only there.

Not ported: `_mitre_ends`. It moves flat ends only for roads with metre widths (`__rs_wm`) and for items, and this data has neither. The Rust code stops with an error if a road has `__rs_wm`, so it cannot give a wrong answer without a warning.

Times on this computer, native, Monaco, three runs:

| Step | Rust | Python (native, the same stages) |
|---|---|---|
| Read the roads | 42 ms (Parquet) | 0.2 s (DuckDB + GeoDataFrame) |
| Roads stages | 270 ms | 0.4 s (`fc_dict` + marks) |
| Simple pieces | 460 ms | 1.0 s |
| JSON text (42 MB) | 340 ms | 0.4 s |

Natively, Rust is only about 1.7 times faster. The Rust code is not tuned: every piece is a map with about 40 text keys, copied from its road. A struct per road and per piece would remove most of the 2 million key copies. In the browser the gap is larger, because Pyodide is about 2.2 times slower than native and wasm usually about 1.5 times.

## Go / no-go for the full port (2026-10-22)

**Go on the main technical question.** The hardest geometry in the style (heads, seams, laps, stubs, halves, shadows, tunnel pieces, twin pairs) ports exactly, in about 830 lines of Rust, in one working day. The method works: a golden file, a compare script with tolerance 0, and a port function by function in the Python order.

**What is still open for the full port**, by size and risk:

| Part | Python | Risk |
|---|---|---|
| Names and arrows (`_annotation_slots`) | about 250 lines; shapely STRtree | The largest stage (5.8 s in the browser). Needs a spatial index in Rust (`rstar`). Exact equality is harder: shapely's distances and projections. |
| The layers | about 25 builders and 3 passes | Data flags (S1). Exact JSON, no geometry: medium work, low risk. |
| Base-map layers (`features.*`) | about 400 lines; SQL and `_restyle` | Mostly data and JSON. |
| Planner data | about 80 lines of SQL | Low. |
| 3D decks | about 150 lines; a polygon buffer | Needs a buffer in Rust (`geo`), exact equality is unlikely: accept a tolerance. |
| Reading the `.duckdb` file | — | duckdb-wasm in JavaScript, or Parquet written by `duckosm`. A decision, not code. |
| The levels solver | about 1,100 lines | Stays offline (`duckosm levels`): a file without levels opens without the browser solver. |

**Recommendation:** finish S4 (the wasm module in a worker, measured in headless Chromium against the Pyodide numbers). Then decide the full port with real browser numbers. The next unknown after S4 is the names and arrows, because they are the largest stage and depend on shapely.

## Result of S4 (2026-10-22)

**The wasm module runs in a worker in headless Chromium** (`spike/wasm/`: `makeStyle(parquet, look, layers)` through wasm-bindgen 0.2.129; `run_browser.py` builds it, runs it, compares and times it).

| | Rust wasm | Pyodide (the same stages, `spike/profile/`) |
|---|---|---|
| Start | 24 to 30 ms to compile and start the module (local server) | 5.4 s (Python and its packages) |
| Download | 3.66 MB, **0.74 MB with gzip** | Pyodide and numpy, pandas, shapely, geopandas, pyproj, DuckDB: tens of MB |
| The roads and the simple pieces, first map | **1.24 to 1.30 s** | 4.40 s |
| The same, warm | 0.92 s | 3.3 s (run 2) |
| The page has the style (start, the run, `JSON.parse`, the transfer) | 1.5 s | — |

The wasm module is about 3.4 times faster than Pyodide for the same work, and it needs no Python start for it. It is 1.4 times slower than the same Rust natively.

### Equality: the knife edge of the short roads

The layers and the `roads` source are **equal with tolerance 0** to the style that Pyodide makes. The `simple` source is not, and it cannot be, on any two platforms:

- A road shorter than its two heads (for example 3.6 m with two 5 m heads) is cut at `h0` and `n - h1`, which are the same point in exact arithmetic. In floating point the middle piece is one last bit long, zero or negative. Where it survives, it is under 2 cm after rounding, and its cut gets a second seam and lap. A last bit of `cos` or `hypot` decides which. glibc (native Python), Pyodide's math library and Rust's wasm math library give three different answers.
- **The plugin itself is affected today**: the style from Pyodide differs from the native golden file in 78 of 8,561 edges.
- `spike/compare_pieces.py` compares per edge. It forgives exactly this case (a main piece under 2 cm, and a seam or lap repeated within 1e-6 deg) and one rounding step (2e-7 deg, about 1 cm), and nothing else. A test checks that it still finds a moved piece and a missing fill.

| Pair | Edges that differ | Knife edge | One rounding step | Other |
|---|---|---|---|---|
| Native Python and Pyodide | 78 | 76 | 2 | 0 |
| **Pyodide and Rust wasm** | 58 | 57 | 1 | **0** |
| Native Python and Rust wasm | 27 | 26 | 1 | 0 |

**Fix to propose upstream (roadstyle `_casing_parts`):** when `h0 + h1 >= n - 0.05`, cut once at `h0` (two pieces), not at `h0` and `n - h1` (three pieces with an empty middle). This removes the knife edge on every platform. Only rounding steps remain, and a tolerance of 2e-7 deg covers them.

### The pass rule

1. **Equal to the golden file: yes, with the knife edge.** Natively Rust equals Python with tolerance 0. In the browser it equals Pyodide, which is what the plugin draws, except for the knife edge. The knife edge is a property of the Python algorithm, not of the port.
2. **1 s or less in the browser: not on the first call.** The first call takes 1.3 s and later calls take 0.92 s. The code is not tuned: every piece is a map with about 40 text keys. A struct per road and per piece would very likely bring the first call under 1 s. That is not measured.
3. **1.5 MB or less with gzip: yes**, 0.74 MB, without `wasm-opt`.

## Decision (2026-10-22)

**The spike says go for the full port, with three conditions.**

- **Why go.** The hardest code in the style ports exactly. The method finds every difference. The module is small (0.74 MB) and 3.4 times faster than Pyodide for the same work. The full port removes the Python start (5.4 s, and the first download of tens of MB), which no hybrid can do.
- **Condition 1: the knife edge is fixed upstream first.** Fix `_casing_parts` in roadstyle, so that the golden files can be compared on every platform with a small tolerance.
- **Condition 2: the next step is the names and arrows.** They are the largest stage (5.8 s in Pyodide) and the only one that depends on shapely's spatial index. If they cannot be ported to the same result with `rstar` and our own distance code, the full port stops there.
- **Condition 3: a decision on the input.** Either duckdb-wasm (JavaScript) reads the `.duckdb` file in the worker and hands Arrow tables to Rust, or `duckosm` writes the tables as Parquet. The levels solver stays offline (`duckosm levels`) in both cases.

Not decided by the spike: who keeps the Rust copy equal to roadstyle after each release. The golden test finds a difference, but someone must port the change.

## Condition 1 done: a short road is cut once (2026-10-22)

**The fix in roadstyle.** Branch `fix/short-road-one-cut` of the roadstyle repository (commit `9d93fcf`, from `v0.24.2`, not pushed and not released).
`_casing_parts` cuts a road shorter than its two heads once, at `h0`: two pieces, one seam and one lap, as `docs/design/short_road_heads.md` already said.
The new test `test_short_road_is_cut_once` uses three Monaco roads (edges 305, 84 and 4021) that had the extra pieces natively. It fails without the fix.
All 439 roadstyle tests pass (`tests/test_studio_samples.py` not run: it needs `streamlit`). CHANGELOG: under "Unreleased".

**The same rule in Rust** (`spike/rust/src/simple.rs`).

**The result.** The three styles were made again with the fix: native (`PYTHONPATH=<the branch>/src tools/make_spike_fixture.py`), Pyodide
(`spike/profile/run_browser.py --golden ... --pkgs <a pkgs.zip of the branch>`) and wasm (`spike/wasm/run_browser.py`).

| Pair | Pieces | Knife edge | One rounding step (about 1 cm) | Plain compare, `--tol 2e-7` |
|---|---|---|---|---|
| Native Python and Pyodide | 46,876 and 46,876 | 0 | 14 edges | equal |
| Pyodide and Rust wasm | 46,876 and 46,876 | 0 | 9 edges | equal |
| Native Python and Rust wasm | 46,876 and 46,876 | 0 | 5 edges | equal |

Native Rust still equals native Python with tolerance 0 (`spike/test_compare.py`, 6 tests). The style has 654 pieces fewer than before (47,530):
many short roads had a doubled seam and lap on every platform, not only on the 78 edges that differed. The map looks the same.

Since the fix, `spike/compare.py --tol 2e-7` is enough for the browser outputs. `spike/compare_pieces.py` stays as the record of the old case.

**Note on the fixture.** `spike/data/` is now made with the roadstyle branch, not the installed 0.24.2. Until a release has the fix, make it with
`PYTHONPATH=<the roadstyle branch>/src`.
