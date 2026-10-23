# Spike: the roads of the style in Rust WebAssembly

Status: steps 1 to 4 of the full port are done; the 3D bridge decks (step 5) are not in the port until a new version of them exists (2026-10-22).

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

## S6: the names and arrows (condition 2), plan (2026-10-22)

**Question.** Can `_annotation_slots` (render_web.py, about 190 lines) be ported to Rust with the same slots, and how fast is it? It is the largest stage
in the browser (5.8 s of the 12.5 s first map in Pyodide). If the slots cannot be made the same, the full port stops here.

**What it makes (Monaco).** The source `slots`: 2,245 slots in 1,450 chains, each a LineString with `slot`, `chain`, `rank`, `name`, `highway`, `oneway`,
`lvl`, `fl`, `__rs_edge`, `__rs_edge2`, `__rs_tunnel`; then the export adds `__rs_arw` (the number of an arrow slot in its one-way chain) and
`_mark_cls` adds `__rs_cls`. The layers `roads-arrows` and `roads-labels` read it. With names and arrows on, the `roads` and `simple` sources do not change.

**The two halves of the function.**

| Part | What it does | Exact in Rust? |
|---|---|---|
| Graph | one representative per two-way pair; groups by name, level, one-way, class, fill number and tunnel; chains through degree-2 nodes, one-way chains only forward; the chain number | Yes: the same keys (`round(x, 6)`) and the same order (Python's dict order is insertion order) |
| Geometry | the crossing roads of each chain (shapely STRtree, `intersection`, `project`), the free stretches between them, the slots (`slot_m` long, rounded to 6 decimals), the edge under each slot's middle (`STRtree.nearest`) | No: GEOS computes intersection points and projections with its own arithmetic. Rust agrees to a tolerance. The 6-decimal rounding (about 11 cm) hides most of it, but a rounding step or a slot-count threshold (`length % slot_m > 0.3 * slot_m`, `b - a < 0.2 * slot_m`) can flip |

**Pass rule.**
1. Every chain has the same slots in the same order, with the same properties, on at least 99 % of the chains; every difference is explained (a rounding step, or a threshold within 1 mm of its limit).
2. Slot coordinates within 1e-6 deg (one rounding step of 6 decimals, about 11 cm).
3. In wasm, the slots take less than 1 s for Monaco (Pyodide: 5.8 s).

**One new crate.** The cargo cache has `rstar` 0.12.2 (an R-tree, pure Rust, builds for wasm), not `geo`. `rstar` replaces STRtree; segment intersection,
point projection and point-to-line distance are written by hand, a few dozen lines each. This also keeps the wasm module small.

**Steps.**

| Step | Work | Check |
|---|---|---|
| S6.0 | Fixture: `golden_slots.json` (export with names and arrows on); the slot look data in `look.json` (class widths at zoom 20, the `z_order` table, `slot_m`) | Python gives the same slots twice |
| S6.1 | Graph: representatives, groups, chains, chain numbers | The set of (chain, name, highway, oneway, lvl, fl) equals the golden one |
| S6.2 | Geometry: R-tree index, crossings, free stretches, slots, nearest edge, `__rs_arw`, `__rs_cls` | The pass rule above, with a per-chain compare script |
| S6.3 | Time natively and in wasm (the S4 harness) | Pass rule 3 |

## Result of S6: the names and arrows (2026-10-22)

**The slots in Rust equal Python's exactly.** All 2,245 slots of Monaco, in 1,450 chains: the same order, the same properties (`slot`, `chain`, `rank`,
`name`, `highway`, `oneway`, `lvl`, `fl`, `__rs_edge`, `__rs_edge2`, `__rs_tunnel`, `__rs_arw`, `__rs_cls`) and the same coordinates, with tolerance 0,
natively and in wasm. `spike/test_compare.py` checks it (6 tests).

What `spike/rust/src/slots.rs` ports (about 400 lines): the representatives of the two-way pairs, the groups in Python's dict order (`indexmap`), the
chains through degree-2 nodes, the chain numbers, the crossings (an `rstar` R-tree for shapely's STRtree; segment intersection, GEOS's
`project` and point-to-segment distance by hand), the free stretches, the slots (`slot_m` 100 m; Python's float `divmod`), the nearest edge of each
slot, the export's `__rs_arw` and `_mark_cls`.

The intersection points are not computed with GEOS's arithmetic, as the plan expected. The slot coordinates are rounded to 6 decimals (about 11 cm), and
that hid every difference on Monaco: no slot moved by one rounding step. On a larger area, a rare slot can move by one step (pass rule 2 allows it).

| | Rust natively | Rust wasm (Chromium) | Python natively | Pyodide |
|---|---|---|---|---|
| The slots | 129 ms | about 0.2 s (1.46 s with the roads and the pieces, first call) | 2.9 s | 5.8 s |
| Roads + pieces + slots | 0.9 s | **1.46 s** first call, 1.1 s warm | 4.5 s | **10.2 s** |

The wasm module is 3.78 MB, 0.78 MB with gzip.

**Pass rule of S6:** 1. the same slots: yes, all of them. 2. coordinates within 1e-6 deg: yes, equal. 3. under 1 s in wasm: yes, about 0.2 s.

**Condition 2 is met.** The largest stage in the browser ports exactly and is about 25 times faster in wasm than in Pyodide.

## What is left for the full port (2026-10-22)

| Part | State |
|---|---|
| Roads, pieces, names and arrows | Done in the spike: exact natively, within one rounding step in the browser |
| The knife edge of the short roads | Fixed on roadstyle branch `fix/short-road-one-cut` (pushed, no PR) |
| **The input (condition 3)** | Open: duckdb-wasm in the worker, or a Parquet export from `duckosm`. The next step |
| The layers | Taken from Python. About 25 builders and the data flags (S1): JSON only, no geometry |
| Base-map layers, planner data | Not started: mostly SQL and JSON |
| 3D decks | Not started: a polygon buffer |
| The levels solver | Stays offline (`duckosm levels`) |
| The plugin's worker | Not started: `py/worker.js` calls Pyodide; the Rust module goes in its place when all of the above is done |

## Condition 3: the input (2026-10-22)

**Question.** The browser gets a duckOSM `.duckdb` file. Can duckdb-wasm read it and give the Rust module its tables, or must `duckosm` export Parquet?

**The probe** (`spike/input/`): `make_sql.py` takes mapstyle's own roads SQL (`_roads_union` and the statement of `load_roads`, with `ST_AsWKB`) and ends it with
`COPY ... TO 'roads.parquet'`. `run_browser.py` runs it natively and, in headless Chromium, with duckdb-wasm from jsdelivr (a new browser for each version:
a cold cache), and compares the rows with a checksum (count and md5 of the rows in `edge_id` order).

| duckdb-wasm | DuckDB inside | Result | Module (on the wire / decoded) | Start, cold | Attach | Roads SQL + Parquet |
|---|---|---|---|---|---|---|
| 1.32.0 (the last stable release, 2025-12) | 1.4.3 | **Fails**: `ST_AsWKB` is not in the core of 1.4 (the file's `GEOMETRY` type is core from DuckDB 1.5) | — | — | — | — |
| 1.33.1-dev57.0 (2026-06, the `latest` tag) | 1.5.4 | **The same 13,298 rows as native** | 7.1 MB / 35.9 MB | 0.75 s | 56 ms | 708 ms |
| 1.33.1-dev65.0 (2026-09) | 1.5.6 | **The same rows** | 6.2 MB / 36.0 MB | 2.3 s | 51 ms | 747 ms |
| (native DuckDB 1.5.4) | | | | | | 41 ms |

The file was written by DuckDB 1.5.3. The Parquet file from the browser is 1.08 MB, and the Rust module reads Parquet already (S2), so duckdb-wasm hands it
the tables through Parquet bytes with no new Rust code.

**The two routes compared.**

| | duckdb-wasm in the worker | Parquet from `duckosm` (offline) |
|---|---|---|
| The input | The `.duckdb` file, as today | A new file (or files) next to the `.duckdb` |
| Download | 6 to 7 MB once (cached after) | nothing |
| Time in the browser | 0.75 to 2.3 s start + 0.05 s attach + the SQL (0.7 s for the roads; the other tables are not measured) | the read only (42 ms for the roads, S2) |
| Risk | Only dev builds of duckdb-wasm carry DuckDB 1.5: pin one (1.33.1-dev57.0 or later) and test it | A second file to keep in step with the database; every table (features, planner, levels) needs an export |
| Work | The SQL builders of mapstyle (`_roads_union`, the planner and features queries) move to JavaScript or Rust | An export command in `duckosm` or `mapstyle` |

**Decision: duckdb-wasm.** It keeps what the plugin promises today (open a duckOSM `.duckdb` in the browser, no server, no extra step), and its cost is small
next to Pyodide's (5.4 s and tens of MB before the first line of Python). A Parquet export can stay as the fast path, as the style file is today.

**An estimate of the first map with the full port** (Monaco, not measured as a whole): duckdb-wasm start 0.75 s + attach 0.05 s + SQL about 1 to 1.5 s (the
roads 0.7 s, the other tables not measured) + Rust 1.5 s (S6) + the transfer and MapLibre. That is about 4 s, against about 18 s today (the guide).

## Where the spike ends (2026-10-22)

All three conditions of the decision are answered:

1. **The knife edge:** fixed on roadstyle branch `fix/short-road-one-cut` (pushed, no PR, not released).
2. **The names and arrows:** ported exactly (S6), about 0.2 s in wasm against 5.8 s in Pyodide.
3. **The input:** duckdb-wasm 1.33.1-dev (DuckDB 1.5) reads the file and gives the same rows as native.

**The full port, by step** (each with the same method: a golden file from Python, a port in the Python order, a compare with tolerance 0 natively):

| Step | Work | Size (Python) |
|---|---|---|
| 1 | The worker: duckdb-wasm and the Rust module in place of Pyodide, the roads input from SQL (the level columns, `access`, `is_directed`, `has_arrows`, which `render_map` adds) | the SQL of `load_roads` and `stored_levels`, about 100 lines of `render_map` |
| 2 | The layers in Rust: the expression builders and the data flags (S1) | about 25 builders |
| 3 | The base-map layers (`features.*`, `feature_overlays`, `_restyle`) | about 400 lines |
| 4 | The planner data | about 80 lines of SQL |
| 5 | The 3D decks (a polygon buffer: a tolerance, not exact) | about 150 lines |
| 6 | Rust tuning (a struct per road and per piece), and a test on a larger area than Monaco | — |

Open, not decided by the spike: who keeps the Rust copy equal to roadstyle and mapstyle after each release (the golden test finds a difference, someone
must port it), and which duckdb-wasm dev build to pin.

## Full port, step 1: the worker, plan (2026-10-22)

**Goal.** A worker that takes the `.duckdb` file and gives the style, with no Python: duckdb-wasm reads the file and builds the roads table in SQL,
the Rust module makes the style from it. The layers and the look still come from Python at build time (`layers.json`, `look.json`), as in the spike.
The base-map layers, the planner data and the 3D decks are later steps. The worker stays in `spike/` until the port can replace `py/worker.js`.

**The roads table in SQL.** What Python does between the database and roadstyle, as SQL that a JavaScript builder writes for the file it opens:

| Python (mapstyle, roadstyle) | In SQL |
|---|---|
| `_roads_union`: every mode's `edges` and `private_edges`, the optional columns of each (it reads `information_schema`) | the same builder in JavaScript (`roads_sql.js`), on the attached catalog |
| `load_roads`: one row per `edge_id`, then the undirected edges first (stable) | the same statement, then `ORDER BY NOT und, eid` |
| `stored_levels` (`load_area_levels`): `visualization.edge_levels` joined on `edge_id`; an error if the levels are not from a level area or were solved for other edges | `LEFT JOIN`, and the checks: `edge_levels_meta.source = 'area'`, the count, every edge has a row (the hash of the ids: later) |
| `_access` (mode `all`), `_is_directed`, `has_arrows` | expressions on the joined row |

**Checks.**
1. The Parquet file the browser writes equals `spike/data/roads.parquet` (from Python): the same rows in the same order, compared natively in DuckDB.
2. The style of the worker equals the golden files: layers and `roads` with tolerance 0, `simple` and `slots` within one rounding step.
3. The time of each stage in the worker (start of duckdb-wasm, attach, SQL, Rust) against today's Pyodide numbers.

duckdb-wasm is pinned to `1.33.1-dev57.0` (DuckDB 1.5.4), the build the input probe checked.

## Full port, step 1: result (2026-10-22)

**The worker makes the map of the `.duckdb` file with no Python.** `spike/worker/`: `roads_sql.js` (the roads table in SQL, about 80 lines),
`worker.js` (duckdb-wasm and the Rust module), `index.html` and `run_browser.py` (the check in headless Chromium).

| Check | Result |
|---|---|
| 1. The roads table from SQL in the browser against `roads.parquet` from Python | **Equal**: 13,298 rows, 27 columns, the same order |
| 2. Layers and `roads` against `golden.json` | **Equal**, tolerance 0 |
| 2. `simple` against `golden.json` | **Equal** within 2e-7 (one rounding step) |
| 2. `slots` against `golden_slots.json` | **Equal** within 1e-6 (one rounding step) |

The level checks of `load_area_levels` are in the SQL builder too: the stored levels must be from a level area, and the number and the hash of the edge ids
must be the ones they were solved for (SHA-256 with WebCrypto, as `_hash`). A file without them stops with a message: run `duckosm levels`.

Times in the worker (Chromium, Monaco, two runs; duckdb-wasm from jsdelivr with a cold cache):

| Stage | ms |
|---|---|
| Start duckdb-wasm (at the same time as the Rust module, 26 ms) | 510 to 525 |
| Attach the file | 35 to 42 |
| The roads table in SQL, with the checks, to Parquet | 636 to 645 |
| The Rust module: roads, pieces, names and arrows, JSON | 1,437 to 1,641 |
| **Total in the worker** | **2,687 to 2,896** |
| The page has the style | 3,010 to 3,182 |

Today the plugin needs about 18 s for the first open of Monaco (the guide): 6 s to start Python and 10 s to make the map, with the names and arrows 10 s later.
The worker's 3 s do not yet include the base-map layers, the planner data and the 3D decks (steps 3 to 5), or the layers in Rust (step 2: they come from
`layers.json` here).

## Full port, step 2: the layers in Rust, plan (2026-10-22)

**Goal.** The Rust module makes the road layers itself, so `layers.json` from Python is no longer needed. The base-map layers (step 3) and the 3D decks
(step 5) are not in this step.

**What makes the road layers** (roadstyle `render_web.render` with `simple=True` and the level columns, then the export, then mapstyle's export):

| Part | Python | Size |
|---|---|---|
| The road tables | `_load_road_model`: widths per group and zoom, growth rates, casing ratios, class groups, links, zoom stops, z order | from the settings |
| Five layers | `roads-simple`, `roads-fill` (invisible, for clicks), `roads-highlight`, `roads-arrows`, `roads-labels` | the expression builders: `_width_expr`, `_offset_expr`, `_offset_match`, `_pair_width`, `_plus_px`, `_by_feature`, `_simple_color`, `_simple_dasharray`, `_simple_cap`, `_seam_filter`, `_minzoom_filter`, `_sort_key`, `_label_size_expr`, `_label_readable_filter`, the tunnel colours (`_tun_*`, `_fill_expr`, `_gs`, `_painted`): about 25 functions of 5 to 30 lines |
| Seven passes on the list | the reverse-pair filter; `_drop_empty_layers` with its evaluator (`_ev`, `_keys`, about 70 lines); the fill-opacity switch (`rs_fill_on`); `_state_expressions`; `_tunnel_look`; `_bake_state`; the export's arrow filter and placement | about 200 lines |
| Two export steps | roadstyle `_tag_groups` (`roadstyle:group`), mapstyle `_restyle` (no base-map layers here: only its metadata) | small |

**The data flags.** Golden layers for 12 subsets of the Monaco roads (`tools/make_layer_goldens.py`, `spike/data/layers/<subset>/`) give 8 different layer
lists. They cover: tunnels or none, twin pairs, reverse pairs drawn as one line, the two dash values, one or none, no one-way slot (no arrow layer:
`_drop_empty_layers` removes it) and no name (no name layer).

**Check.** For each subset, the layers the Rust module makes from that subset's `roads.parquet` equal its `golden.json` with tolerance 0. Then the worker
(step 1) runs without `layers.json`.

**Order.** 1. The road tables in `look.json` (`_settings.roads()`) and the width, offset and sort builders, checked on the `roads-fill` and
`roads-highlight` layers of `all`. 2. `roads-simple`. 3. Arrows and labels. 4. The passes, subset by subset until all 12 are equal.

## Full port, step 2: progress (2026-10-22)

- `look.json` has the road tables (`roads_model`: `_settings.roads()`), and `tools/make_layer_goldens.py` writes the 12 subsets (`spike/data/layers/`).
- `spike/rust/src/layers.rs` ports the road tables (`_load_road_model`), `_gwidth`, `_width_expr`, `_offset_match`, `_offset_expr`, `class_order` and
  `_sort_key`. Against the golden layers, with tolerance 0: `roads-fill`'s line width and line offset (with twin pairs in `all`, without in `first100`),
  `roads-highlight`'s line width and the arrows' sort key are equal. `test_rust_layer_builders_equal_golden` checks it (7 tests in all).
- Next: `roads-simple` (`_by_feature`, `_plus_px`, `_pair_width`, `_simple_color`, `_simple_dasharray`, `_simple_cap`, `_seam_filter`), then the arrows and
  labels, then the seven passes.

The goldens of step 2 are made with the roadstyle fix branch on `PYTHONPATH` (a `git archive` of `fix/short-road-one-cut`), as the rest of `spike/data/`.

## Full port, step 2: result (2026-10-22)

**The Rust module makes the road layers itself.** `spike/rust/src/layers.rs` (about 750 lines) ports, for simple mode with stored levels and inline data:

- the road tables (`_load_road_model`) and the expression builders (`_gwidth`, `_width_expr`, `_offset_match`, `_offset_expr`, `_pair_width`, `_plus_px`,
  `_by_feature`, `class_order`, `_sort_key`, `_seam_filter`, `_simple_cap`, `_simple_dasharray`, `_simple_color` with the glow of lean mode, the tunnel colours,
  `class_width_px`, `_label_px`, `_label_size_expr`, `_label_readable_filter`);
- the five layers (`roads-simple`, the invisible `roads-fill`, `roads-arrows`, `roads-labels`, `roads-highlight`);
- the passes, in Python's order: the reverse-pair filter, `_drop_empty_layers` with its three-valued evaluator (`_ev`), the fill-opacity switch,
  `_state_expressions`, `_tunnel_look`, `_bake_state`, the export's arrow rework, and the group tag (`_tag_groups`).

**Check.** For each of the 12 subsets (8 different layer lists), the layers equal the golden ones with tolerance 0
(`test_rust_layers_equal_golden_for_every_subset`; 7 tests in all). The worker in Chromium makes the whole style with no `layers.json`:
the layers equal `layers/all/golden.json` with tolerance 0, and the sources equal theirs as before.

`makeStyle` now takes `(parquet, look)`. `layers.json` is gone from the fixture, the harnesses and the worker. The Parquet reader also reads a column that is
null in every row (Arrow's `Null` type: a subset can have one).

**Time.** Natively the layers take about 55 ms. The first version copied every feature's properties and the three feature lists (about 42 MB) and took
0.4 to 0.5 s; borrowing them fixed it. In Chromium the worker takes 2.7 s from the file to the style (duckdb-wasm start 0.49 s, attach 0.04 s, the roads
SQL 0.61 to 0.64 s, Rust 1.47 s), the page has it after 2.9 to 3.0 s. The wasm module is 0.83 MB with gzip.

**What is still from Python** at build time: `look.json` (the resolved look). The base-map layers (step 3), the planner data (step 4) and the 3D decks
(step 5) are not in the worker yet.

## Full port, step 3: the base-map layers, plan (2026-10-22)

**Goal.** The worker makes the base map too: the `features.*` tables of the duckOSM file as GeoJSON sources and their layers, as
`mapstyle.export.export_inline` makes them. With it, the style is the whole map except the planner data (step 4) and the 3D bridge decks (step 5).

**What it is** (Monaco, `spike/data/golden_full.json`: `export_inline(db, view_3d=False)`, 48.6 MB): 14 base-map sources (2,435 features: an ocean
MultiPolygon, landcover, water, waterways, buildings, railways, parking, platforms, and points: parking signs, traffic signals, crossings, bus and train
stations, bicycle) and 55 layers. The crossings and traffic signals sit on a street: their layers are made once per fill number (8 numbers here) and go
between the road layer and the arrows, so a point is drawn at its street's height.

| Part | Python | In the port |
|---|---|---|
| The tables | `mapstyle.map.load_layers`: one query per layer of `styles/layers.yaml` (the table, a `where`, the columns, `ST_Centroid` for some, `ST_Area` to sort the polygons largest first, `ST_AsGeoJSON`), the 3D height of a building (`building_height`) | The queries in the worker (JavaScript, as `roads_sql.js`), with `ST_AsWKB`: DuckDB 1.5's core has no `ST_Area`, `ST_Centroid` or `ST_AsGeoJSON` (checked; Pyodide uses shapely for them today). Rust makes the GeoJSON, the area and the centroid (GEOS's algorithm) from the WKB |
| On the street | `mapstyle.map.edge_attach`: the street a crossing or signal stands on (STRtree, `dwithin` 1 m, then 3 m, then the nearest road; the street drawn on top wins) | Rust, with the R-tree of S6 |
| The overlays | `feature_overlays` (colours, outlines, opacities, zoom ranges, interaction, which layers start hidden) | The look data at build time: `layers.yaml`, the `features` style, the pattern PNGs and icon SVGs as data URLs (`roadstyle:images`) |
| The layers | roadstyle `_overlay_layers`, `_build_overlays`, `_edge_overlay` (the street's `__rs_edge`, `__rs_fl`, ... on each point), the placement (under the roads, over them, between the road layer and the arrows), the overlay colours in `_state_expressions` and `_tunnel_look`, `_tag_groups` (the layer's group is its overlay's label) | Rust, next to the road layers of step 2 |
| The restyle | mapstyle `_restyle`: colour and outline by kind, dashes, textures, icons, 3D buildings (`fill-extrusion`, placed under the arrows), zoom ranges | Rust |

**Checks.** 1. Each base-map source equals `golden_full.json`'s: the properties with tolerance 0, the coordinates within 1e-12 (a centroid computed
in another order of sums). 2. The 55 layers equal the golden ones with tolerance 0. 3. The worker in Chromium, as in steps 1 and 2.

**Order.** 1. The queries and the GeoJSON, area, centroid and height, checked source by source (the point layers on a street without their
`edge_id`). 2. `edge_attach`. 3. The overlays and their layers, the placement and the restyle.

## Full port, step 3: progress (2026-10-22)

**Part 1 done: the base-map features.** The fixture writes `spike/data/features.parquet` with the SQL the worker will run (one row per feature: the layer's
index in `layers.yaml`, the layer's name or the one it is merged into, `osm_id`, `kind`, `name`, `bearing`, the height tags, the geometry as WKB, in
`rowid` order, as `load_layers` reads them) and `golden_full.json` (the whole map). In Rust:

- `spike/rust/src/geom.rs`: a WKB reader for points, lines, polygons and their multi forms; GeoJSON; GEOS's area (`Area::ofRing`) and centroid (the
  area-weighted triangles from the area base point; then the lines; then the points).
- `spike/rust/src/features.rs`: `load_layers` (the props, `building_height`, the centroid of a `centroid` layer, the merged layers, the polygons largest
  first with a stable sort).

Against `golden_full.json`, the 14 base-map sources: 13 are equal with tolerance 0 (all polygons in the same order, all heights); `parking_p` (the
centroids of the parking areas) differs in the last bit of a few coordinates and is equal within 1e-12. The golden file uses DuckDB spatial's
`ST_Centroid`; the plugin today uses shapely's. `test_rust_base_map_sources_equal_golden` checks it (8 tests in all).

**Next:** the worker's SQL for the features (`features_sql.js`, the same statements as the fixture's `features_parquet`), `edge_attach`, then the
overlays and their layers, the placement and mapstyle's `_restyle`.

## Full port, step 3: result (2026-10-22)

**The worker makes the base map too.** On Monaco, in Chromium, the style the worker makes from the `.duckdb` file equals the whole map
(`golden_full.json`, `mapstyle.export.export_inline(db, view_3d=False)`): all 55 layers and the images with tolerance 0, and the 14 base-map sources
within 1e-12 (with the street of each crossing and signal). Natively too (`test_rust_style_equals_golden`; 8 tests in all).

What was added:

- `spike/worker/features_sql.js`: the queries of `load_layers` (one per layer of `layers.yaml`, the geometry as WKB), as `features.parquet`; the fixture
  makes the same file in Python for the native tests.
- `spike/rust/src/overlays.rs` (about 300 lines): `edge_attach` (the street drawn on top within 1 m, else the nearest street within 3 m, else the nearest
  road; an R-tree), `feature_overlays` (each layer's look from the theme's `features` style), `_overlay_layers` and `_edge_overlay`.
- `spike/rust/src/layers.rs`: the placement (under the roads; on a street, one copy per fill number between the road layer and the arrows; over the
  roads), the overlays' colours in the state (`rs_ov_<i>`) and in the tunnel look, the group of each layer (its overlay's label), and mapstyle's
  `_restyle` (colour and outline by kind, dashes, the landcover textures, the icons, the 3D buildings under the arrows, zoom ranges).
- The look data from the build (`look.json`): the theme's `features` style, the images (the textures as PNG and the icons as SVG data URLs, made
  by mapstyle's own `feature_overlays`), and the arrow image of roadstyle's export.

`makeStyle` takes `(roads, look, features?)` and returns the layers, the sources and `metadata["roadstyle:images"]`.

**Time** (Chromium, Monaco): duckdb-wasm start 0.48 s, attach 0.04 s, the roads SQL 0.61 s, the base-map SQL 0.05 s, Rust 1.55 s: 2.78 s in the worker,
3.04 s until the page has the style. Natively the base map adds 22 ms. The Rust code is 2,897 lines; the wasm module is 0.90 MB with gzip.

**Not in the style yet** (compared with mapstyle's export): the planner data (`metadata["roadstyle:planner"]`, step 4), the 3D bridge decks (step 5),
and the page metadata that is not drawn (`roadstyle:interaction`, `center`, `zoom`, `name`): small, from the look and the bounds.

## The golden files come from the released versions (2026-10-22)

**What was wrong.** Until step 3, the golden files came from the local mapstyle source, which was at 0.7.0 (its checkout's `main`), and from the
roadstyle fix branch. The plugin bundles mapstyle 0.12.0 (`py/pkgs.zip`). The road part did not change between them; the base map did.

**Now.** The fixture runs with roadstyle 0.24.3 (the release with the short-road fix: tag `v0.24.3`, on PyPI) and mapstyle 0.12.0 (the PyPI
wheel), on `PYTHONPATH`. It also runs mapstyle as it runs in the plugin: `mapstyle.map._spatial` is replaced by its Pyodide branch (no spatial
extension; ST_AsGeoJSON, ST_Area and ST_Centroid are shapely's). So the golden files are what the plugin draws.

**What mapstyle 0.12.0 changed in the base map, and the port of it:**

| Change | In the port |
|---|---|
| `background`: the land of `main.boundary`, merged into landcover. Without the spatial extension (the plugin) it is the whole boundary, not the boundary without the sea (`ST_Difference`) | The SQL reads `main.boundary` with the layer's name as the kind, the whole geometry |
| `paved` areas (squares, pedestrian areas, piers) | Data only |
| `raceway` and `other_ways`: the ways no mode network holds (`outside_networks`, `_network_ways`) | `osm_id NOT IN` the OSM ids of every `<mode>.edges`, `private_edges` and `edge_id_map`; the layer is skipped when there is no network |
| `width_zoom`: a line as wide as a road (the raceway) | In `feature_overlays` and the restyle |
| Building heights: `12,5`, feet (`40 ft`), at most `max_height` (200 m); an older file without `tags` | In `building_height`, and the SQL tries without `tags` |
| `zoo` in the institutional kinds, four landcover colours | Data only |

**Result.** Natively and in Chromium, the whole map of Monaco equals the golden one: 59 layers and the images with tolerance 0, and the 17 base-map
sources now with tolerance 0 too (the parking centroids are shapely's GEOS centroid, the algorithm Rust ports). The roads, the pieces, the names
and arrows and the 12 road-layer subsets did not change. 8 tests pass; the page has the style after 3.1 s.

## Full port, step 4: the planner data, plan (2026-10-22)

**Goal.** The style has the route planner's data, as mapstyle 0.12.0's `export_inline(planner=True)` puts it in `metadata["roadstyle:planner"]`
(`planner_data`; a database with more than 150,000 roads gets `roadstyle:planner_skipped` instead). The plugin's planner (`planner/planner.js`, from
duckOSM) reads it.

**What it is** (Monaco: 0.55 s natively in Python, 1.98 MB of JSON): per road of `load_roads` (in its order) its `edge_id`, its two nodes as small
numbers, its name, length and class, and the roundabouts; per mode (driving, walking, cycling) its turn graph (`<mode>.edges` by `edge_id`: cost,
length, and the next edges from `<mode>.edge_graph`, a mode's own rows where it has `uses`); walk + drive from `mm.edges` and `mm.transfers`; and the
start and end of the first route (`_planner_ends`: the road of the first mode whose centroid is nearest to 30 % and 70 % along the diagonal of the
roads' box, at half its length).

**In the port.** The worker's SQL writes the tables as one `planner.parquet` (a `part` column: the nodes of each edge, a mode's edges, its turns,
the walk + drive edges and transfers; in the order Python reads them). Rust builds the data: the node numbers, the rounding of Python (`round(x, 1)`,
`2`, `3`), the first row of an edge that is in several tables, and `_planner_ends` with GEOS's line centroid and `interpolate` (LengthIndexedLine).

**Check.** `metadata["roadstyle:planner"]` equals the golden one (`golden_full.json`, now with `planner=True`) with tolerance 0, natively and in the
worker.

## Full port, step 4: result (2026-10-22)

**The style has the route planner's data.** `spike/worker/planner_sql.js` writes the planner's tables as `planner.parquet` (the fixture has the same
SQL in Python); `spike/rust/src/planner.rs` (about 170 lines) builds `metadata["roadstyle:planner"]`: the node numbers, the roads' arrays, the turn
graph of each mode, walk + drive, and `_planner_ends` (GEOS's line centroid and `interpolate`). Past 150,000 roads it writes
`roadstyle:planner_skipped`, as `export_inline` does.

**Check.** The planner data equals the golden one (`export_inline(planner=True)`, mapstyle 0.12.0) with tolerance 0, natively and in Chromium: every
array, the three turn graphs, walk + drive, and the start and end of the first route. `test_rust_style_equals_golden` checks it (8 tests).

Two details on the way: `mm.edges` is a view (no `rowid`), so the SQL keeps a table's or view's reading order with `row_number() OVER ()`; and the line
centroid sums `length * midpoint` as GEOS does (the order of the operations changes the last bit).

**Time** (Chromium, Monaco): duckdb-wasm start 0.47 s, attach 0.04 s, the roads SQL 0.63 s, the base-map SQL 0.07 s, the planner SQL 0.16 s, Rust 1.82 s:
3.24 s in the worker, 3.51 s until the page has the style. Natively the planner data takes 0.33 s in Rust (0.55 s in Python): most of it is reading
150,000 rows as generic maps, which a typed reader would make faster. The Rust code is 3,091 lines; the wasm module is 0.92 MB with gzip.

**What is left** before the worker can replace `py/worker.js`: the 3D bridge decks (step 5, `view_3d`), the page metadata that is not drawn
(`roadstyle:interaction`, `center`, `zoom`, `name`, `glyphs`, `version`), the other choices of the plugin's dialog (the mode, the colours, the path
style: today the look is one `look.json` for mode all, `osm` and theme `osm`; the build would make one look per choice), the `split` of the first
picture (no names and arrows first), and the tuning of the Rust code (a struct per road and piece).

## Full port, step 5: the 3D bridge decks, plan (2026-10-22)

**Why it is needed.** The plugin's style always has the decks: roadstyle's `export_inline` sets `view_3d=True`, and the worker does not change it. The
dialog's "3D bridge decks" choice only acts in `index.js`: off (the default) hides the deck layers and rewrites the flat layers' deck split
(`["<", ["zoom"], 16]`) so the flat bridges draw at every zoom. So the golden files of steps 1 to 4 (`view_3d=False`) are not quite what the plugin
gets: `spike/data/golden_3d.json` is (`export_inline(db, layers=True, planner=True)`, the defaults).

**What the decks change** (Monaco): a `decks` source (1,311 polygons: 517 deck ribbons and 794 casing slabs; 1,116 of them have 5 points, a rectangle),
two layers after the highlight (`roads-deck-casing`, `roads-bridge-decks`), the deck split in the filters of `roads-simple` and `roads-fill`, and the
highlight's filter. Every other source is the same.

**`_bridge_decks`** (render_web.py, 216 lines):

| Part | Exact in Rust? |
|---|---|
| The bridge edges, their twins, the chains through degree-2 nodes, the spans of each edge in its chain, the true ground ends | Yes (the graph code of S6) |
| The ramp: the cuts every `step_m` near a ground end, at the span boundaries; the smoothstep height; the widths (the class width at `match_zoom`, `width_scale`) | Yes |
| `shapely.ops.substring` (a part of the chain between two cuts) | Yes: shapely's Python and GEOS's interpolate |
| The properties of each ribbon (its edge's, `__rs_base`, `__rs_height`, `__rs_chain`, `__rs_edges`) and of each slab | Yes |
| The polygons: `buffer` (flat caps, mitre joins), and for a two-way bridge and the slabs `offset_curve` (round joins, 8 segments a quarter) first | A straight part (two points) is a rectangle: exact up to where GEOS starts the ring. A bent part needs GEOS's buffer and offset-curve builders: not exact |

**Pass rule.** 1. The layers equal `golden_3d.json` with tolerance 0. 2. The deck features: the same number, the same properties in the same order.
3. The polygon of a straight part equals GEOS's within 1e-6 deg (one rounding step), the ring compared from its lowest point and in one orientation.
4. A bent part's polygon: within 0.1 m of GEOS's (the largest distance between their outlines), and the number of such parts reported.

## Full port, step 5: progress (2026-10-22)

`spike/rust/src/decks.rs` (about 330 lines) ports `_bridge_decks`: the bridge chains and twins, the ramp cuts, `shapely.ops.substring`, the widths and
heights, the properties, and the polygons: GEOS's offset arithmetic (`computeOffsetSegment`) and ring order (from the end's left point, clockwise) for a
straight part; mitre joins (buffer) and round joins (`offset_curve`) computed here for a bent part. `layers.rs` adds what the decks change: the deck split
in the filters of `roads-simple` and `roads-fill`, the highlight's filter, and the two deck layers. `make_style` always makes the decks, as the plugin's
export does. `spike/compare_decks.py` compares two deck sources; `test_rust_decks_against_golden_3d` checks them (9 tests in all).

Against `golden_3d.json` (the style exactly as the plugin gets it), natively and in Chromium:

| Check | Result |
|---|---|
| All 61 layers | **Equal**, tolerance 0 |
| The deck features | 1,311 and 1,312: 1,309 align (99.8 %); 2 only in GEOS's, 3 only in Rust's: slabs on the inner side of a sharp bend, where GEOS makes none or two |
| The properties of the aligned features | **All equal**, tolerance 0 |
| The straight parts (four corners on both sides) | **1,104 of 1,104 equal** within 1e-6 deg |
| The bent parts (205) | The outline distance: median 0 m, 90 % 0.29 m, largest 4.2 m |

**Not exact yet: the bent parts.** Two GEOS behaviours are not ported: `BufferInputLineSimplifier` (before a buffer, GEOS drops the vertices that are
nearly on a straight line, within 1 % of the distance: 11 parts that GEOS makes into rectangles have 6 to 8 corners here) and the collapse of an offset
curve on the inner side of a sharp bend (the slabs that GEOS leaves out or splits). Porting these two is the next part of step 5.

How much it matters: the plugin hides the decks unless the user chooses them ("3D bridge decks: off" is the default), and a deck draws only from zoom
16. A slab up to 4 m off at a sharp bend of a ramp would show in a tilted 3D view.

**Time** (Chromium): the worker 3.31 s, the page has the style after 3.58 s. Natively the decks take about 15 ms.

## Decision: no 3D bridge decks in the port, for now (2026-10-22)

Kaveh: the 3D bridges have issues; do not use them until a new version of them exists.

**Done.** The decks are out of the port: the Rust module makes the style with no decks (as `view_3d=False`), and the tests and the worker compare it
with the golden files without decks (`golden_full.json`). The port of the decks is kept in the history: commit `38d10ad` (`spike/rust/src/decks.rs`,
`spike/compare_decks.py`, the deck layers in `layers.rs`); the next commit removed it.

**What the user sees.** The same map as the plugin's default today. With "3D bridge decks: off" (the dialog's default), the plugin hides the deck layers
and replaces the deck split in the filters by `true`, so the flat bridges draw at every zoom: a style without decks draws exactly that. The dialog's
"3D bridge decks: on" has no decks to show in the port.

**For the new version.** The record (`38d10ad`) shows what an exact port needs: GEOS's `BufferInputLineSimplifier` and the collapse of an offset curve on
the inner side of a sharp bend. A new design could avoid both, for example decks made from the casing pieces of S3 (already exact) instead of buffers.

## The whole style in Rust, and a preview plugin (2026-10-22)

**The whole style.** `makeStyle` (`spike/rust/src/lib.rs`) now makes the complete style that the page draws: `version`, `name`, `center` and
`zoom` (a port of `_camera`), the layers, the sources, `glyphs`, and the metadata `roadstyle:interaction`, `roadstyle:images` and `roadstyle:planner`.
It is equal to `golden_full.json` as one object, with tolerance 0 (natively). The 8 tests in `spike/test_compare.py` pass.

**The preview plugin.** `spike/plugin/build_preview.py` makes `dist/roadstyle-rust-preview.zip` (1.5 MB). It is the plugin's own `index.js` with its
own id (`roadstyle-rust`), name ("roadstyle (Rust preview)"), menu ("roadstyle (Rust)") and style cache, so it can be installed next to the roadstyle
plugin. Its worker (`spike/plugin/worker.js`) sends the same messages as `py/worker.js`, so `index.js` does not change. Limits:

1. Only the dialog's default choices: all modes, OpenStreetMap colours, Google footpaths. Other choices give an error message.
2. No 3D bridge decks.
3. The look data (`look.json`) comes from the fixture, so the build needs `spike/data/` (`tools/make_spike_fixture.py`).

**Test in the real GeoLibre** (`spike/plugin/e2e_preview.py`, headless Chromium, https://web.geolibre.app, `monaco.duckdb`). It installs the zip, opens
the database with the default choices, and checks the map. Result:

| | First map | Second map (warm) |
|---|---|---|
| Start DuckDB and the Rust module | 0.7 s | — |
| Roads, base map, planner (SQL) | 0.8 s | 0.5 s |
| Make the style (Rust) and send it | 2.8 s | 2.6 s |
| Draw (MapLibre) | 2.3 s | 1.4 s |
| **Click on Open to idle map** | **6.6 s** | **4.5 s** |

The map has 76 roadstyle layers (52 base-map layers), the names, the arrows and the casing pieces. The route planner shows its panel, its two pins
and a route (66 pieces drawn). No page errors. The plugin with Python takes about 18 s for the first map.

## The first style was slow in Edge on Windows: the memory's growth (2026-10-22)

**The problem.** In Kaveh's Edge 155 on Windows, the preview's step "making the map (Rust)" took more than 12 s for Monaco. In headless Chromium
(Linux, the same computer) it took 2 s. A test page with only the Rust call (a new worker for each test) showed that only the **first** call was
slow. Plain JavaScript and the later calls ran at normal speed:

| Edge, Monaco | First call | Second call |
|---|---|---|
| The memory grows in small steps (as before) | 14.1 to 20.9 s | 3.5 to 3.9 s |
| 1.2 GB taken first, in 1 MB blocks (the taking: 1.9 s) | 3.05 s | 2.17 s |
| The new allocator | **2.20 to 2.84 s** | 2.03 to 2.10 s |

**The cause.** Rust's standard allocator in wasm (dlmalloc) grows the module's memory 64 KB at a time: thousands of `memory.grow` steps up to
the 738 MB that Monaco needs. In Edge on Windows each step is slow (about 1.6 ms), and on Linux it is cheap. The later calls use the memory that is
already there. V8's two compilers are not the cause: with V8's simple compiler only (`--liftoff-only`), the call takes 3.8 s.

**The fix** (`grow` in `spike/rust/src/lib.rs`, the wasm build only): an allocator in front of dlmalloc. When dlmalloc grows the memory, the
allocator takes one more block of the memory's size (16 to 256 MB) and frees it at once, so the memory grows in about 15 large steps. dlmalloc uses
the freed block for the next allocations (tested: the memory stays at the reserved size during the call). The cost is more memory: 857 MB, not
738 MB, for Monaco. Released as the preview `0.20.0-rust.3`.

**For later.** 738 MB for a 46 MB style is much. The style is built as `serde_json::Value` trees and copied when it is put together. Typed structs
and fewer copies would use less memory and time in every browser.

## Towards the release: the dialog's choices and the kept maps (2026-10-22)

Kaveh agreed to the plan: (1) the dialog's choices and the kept maps in the preview, (2) a bigger area and the memory, (3) then the Rust version
replaces the Python one as roadstyle 0.21.0, with the same plugin id, (4) then the registration in GeoLibre.

**Step 1 is done.**

- **The looks.** `tools/make_spike_fixture.py --choices` makes the look of each of the 64 choices (4 modes × 4 themes × 4 path styles) with
  `look_of(db, mode, theme, paths)`, and writes them as `looks.json`: each part once, and per choice an index into each part (1.4 MB; the
  `config` part has 9 values, `styles` 28, `features` 3). The worker puts a choice's look together from it. A choice that is not in the pack
  gives an error.
- **The mode in the SQL.** `writeRoads(q, catalog, mode)`: in one mode, `access` is that mode's restriction where the mode cannot use the road
  (`_access`). For all four modes the browser's roads are equal to Python's (`spike/worker/run_browser.py`). In duckdb-wasm, a second `COPY` to
  a file name that is kept is not seen by `copyFileToBuffer`: drop the file first.
- **The mode without lanes.** For walking and cycling, mapstyle passes `offset_frac=0, width_frac=1` (one centred line per road). The Rust
  `Model` now takes them from the look's `args`, over the theme's split.
- **The test.** 16 choices in which each pair of values occurs once (an orthogonal array OA(16, 3, 4, 2)): the golden files
  (`spike/data/choices/*.json.gz`, 48 MB) against the Rust style made from the look pack and the roads of the mode. All 16 are equal with
  tolerance 0 (`test_rust_style_equals_golden_for_16_choices`).
- **The kept maps.** The preview keeps each style in the browser's storage (IndexedDB), with a patch of `index.js` in `build_preview.py`. The
  real plugin keeps only the style with the names and arrows, because its Python worker sends a first style without them.

**In GeoLibre** (preview `0.21.0-rust.5`, Monaco, `spike/plugin/e2e_preview.py`): the first map 6.7 s; the same file and choices again 2.7 s (from
the browser's storage); walking, grey and Komoot 4.6 s.

## Step 2: a bigger area and the memory (2026-10-22)

**The areas.** Only Monaco (13,298 roads) and Sundbyberg (15,425) have stored levels. On copies, `duckosm levels` made them for Tartu (63,965
roads in the map, 16 s) and Vancouver (356,433 roads, 5 min).

**Before** (Tartu, Chromium): the Rust step 9.8 s and 3.4 GB of WebAssembly memory, near the browser's 4 GB; a 226 MB style. Natively the peak
was 4.3 GB. Where (a counting allocator in a native build, one stage at a time):

| Stage | In use after it | Peak in it |
|---|---|---|
| Read the roads | 473 MB | 511 MB |
| Roads source | 971 MB | 1,489 MB |
| Casing pieces | 2,021 MB | 3,140 MB |
| Planner | 2,251 MB | 3,575 MB |
| Assemble the style | 4,048 MB | 4,048 MB |
| Write the text | 4,317 MB | 4,317 MB |

**The cause:** serde_json's `json!` macro serializes its values by reference, so `json!({"data": {"features": features}})` makes a deep copy of
a big tree while the tree is still in use; the style was assembled the same way.

**The changes:**
1. `obj([...])` builds a JSON object by moving its values in. It replaces `json!` wherever a big tree goes into an object (the four sources,
   the base-map collections, each feature).
2. After the layers, each source becomes text at once and its tree is freed; the style is joined from the texts in the order of before.
3. `MAX_ROADS = 100,000`: a bigger area gets the message "this area has N roads, and the plugin draws at most 100000 (the browser's memory):
   use a smaller area", not a crash.

**After** (Tartu): natively a 2.47 GB peak and 7.0 s (from 4.3 GB and 14.8 s); in Chromium 1.9 GB and 7.1 s. In GeoLibre (preview, Tartu): the
first map 22.4 s (Rust 7.4 s, sending the style 3.9 s, MapLibre 8.3 s), the same again 11.2 s, other choices 17.5 s; the route planner works.
All golden tests are equal as before.

**The Python plugin cannot draw Tartu:** its map fails after 38 s with `Unexpected token 'N', ...""footway",NaN,"servi"... is not valid JSON`
(the Python export writes NaN in a road property). The Rust version writes valid JSON for the same file.

**Vancouver** (356,433 roads) would need about 13 GB and a 1.2 GB style: too big for a style with the data inside, in Rust or in Python. Such an
area needs another design (vector tiles).

**What is left in memory** (for later, if areas between 64,000 and 100,000 roads matter): the source trees while the layers are made (about
2.4 GB natively for Tartu), and the planner's rows as JSON maps (1.3 GB for a short time). Typed pieces and a typed planner reader would
make both smaller.

## Step 3 plan: Rust replaces Python in the real plugin, as roadstyle 0.21.0 (2026-10-22)

**Goal.** The plugin `roadstyle` (the same id) makes a database's map with DuckDB (duckdb-wasm) and the Rust module, not Python (Pyodide).
0.20.1 stays on GitHub for a user who needs the Python version.

**What does not change.** The plugin id, the menu, the dialog's colour choices, the Layers rows and folders, the filters, the route planner,
"Add a roadstyle layer" (a style file or a URL: no Python, no change), the project state, the zip install.

**The work:**

1. **Move the code out of `spike/`.** The Rust crate to `wasm/` (crate `roadstyle-wasm`), the worker and its SQL (`worker.js`, `roads_sql.js`,
   `features_sql.js`, `planner_sql.js`) to `worker/`. `spike/` keeps the checks (golden files, `compare.py`, the browser harness, the preview
   tools), pointed at the new places.
2. **`build.py`** makes, in place of `py/pkgs.zip` and `py/worker.js`:
   - `looks.json`: the looks of the 64 choices (`look_of`, from `tools/make_spike_fixture.py`, moved into `build.py`). It needs roadstyle and
     mapstyle at build time, as now, and a small database (`--db`, default `../duckOSM/monaco.duckdb`): the look does not depend on the
     database (Monaco and Tartu give the same looks, except the name, which the worker gives).
   - the wasm module (cargo, wasm-bindgen) and the worker bundle (esbuild); the zip carries them as assets, as now.
   - The Python versions the looks come from are pinned in `build.py` and checked: roadstyle 0.24.3, mapstyle 0.12.0.
3. **`index.js`:**
   - The worker's messages stay the same, without the second message (the names and arrows are in the first style). The style is kept in the
     browser's storage when it comes (now only with the names and arrows).
   - The texts: no "loading Python", "the Python worker failed" and so on; "starting DuckDB and the Rust module" instead.
   - **The 3D bridge decks option leaves the database dialog** (Kaveh: no decks until a new version of them). A style file with decks
     (`mapstyle --inline`, "Add a roadstyle layer") is drawn as now.
4. **Remove** `py/worker.js` and `py/pkgs.zip` (the history keeps them).
5. **Tests:** `tests/test_plugin.py` (the asset names, the failed-start test, the dialog without the decks field), `tests/e2e_geolibre.py`,
   `tests/e2e_inputs.py`, `tests/probe_timing.py` (the texts); the spike's golden tests and the browser harness at the new places.
6. **Docs:** `README.md`, `docs/guide.md` (how it works, the times, the network, the limits: 100,000 roads, no decks from a database) and
   `docs/img/how-it-works.svg` (DuckDB and Rust in place of Python).
7. **Checks before the release**, all in the real GeoLibre: `tests/e2e_geolibre.py` and `tests/e2e_inputs.py` (Monaco), the preview's test
   (Monaco and Tartu, three opens each), the 16-choice golden test, `tests/test_plugin.py`. Then the zip, version 0.21.0.
8. **The release** only after Kaveh has seen the results: GitHub release 0.21.0 as Latest. Then step 4 (the registration in GeoLibre).

**Size.** The zip grows from 0.43 MB to about 1.8 MB (the wasm module and the 64 looks). At run time the plugin loads duckdb-wasm (about 7 MB,
once, then from the browser's cache) from `cdn.jsdelivr.net`, in place of Pyodide and its packages.

## Step 3: result (2026-10-22)

The work of the plan is done; the release waits for Kaveh.

- **The code.** The crate is `wasm/` (`roadstyle-wasm`), the worker and its SQL are `worker/`. `build.py` makes `map/worker.js` (esbuild) and
  `map/pkgs.bin` (the 64 looks and the wasm module) in about 30 s, with roadstyle 0.24.3 and mapstyle 0.12.0 (it refuses other versions); `map/`
  is not in git, the zip carries it. The looks the build makes are equal to the fixture's, so the golden tests cover the plugin's build.
  `py/` (the Python worker and its packages) and the preview's build are gone.
- **`index.js`.** No second message (the names and arrows are in the first style), no Python texts, no 3D bridge decks choice. The style is
  kept in the browser's storage as a Blob: Chrome 156 refuses an IndexedDB value over 133,169,152 bytes, and Tartu's style is 226 MB.
- **The checks** (all pass): `tests/test_plugin.py` with a real database (20 tests, 17 s; with Python it took 1 to 2 minutes),
  `tests/e2e_geolibre.py` and `tests/e2e_inputs.py` in the real GeoLibre, `tests/e2e_maps.py` (Monaco and Tartu), `spike/test_compare.py`
  (9 tests, the 16 choices among them), `spike/worker/run_browser.py` (the SQL roads of the four modes, and the style).
- **Two test fixes.** The failed-start test cuts the dialog's warm-up request too (the worker recovers within one open otherwise). The
  database test waits for the progress box to close before it counts the layers (the Rust map came while the count was taken).

| In GeoLibre, click on Open to the finished map | Monaco (13,298 roads) | Tartu (63,965 roads) |
|---|---|---|
| First open | 6.7 s | 23.7 s |
| The same again (from the browser's storage) | 2.7 s | 12.2 s |
| Other choices | 4.0 s | 20.0 s |
| roadstyle 0.20.1 (Python), first open | about 18 s | fails (NaN in the JSON) |

## Vector tiles for duckosm-rust's page, and a faster slots step (2026-10-23)

duckosm-rust's page (one map for a build in the browser) gave MapLibre the whole style as one text with inline GeoJSON:
Tartu was 150 MB of text, the page parsed it, and MapLibre cut all of it into tiles again. Kaveh asked for vector tiles.

**What is new in `wasm/`:**
- `make_parts` makes the layers, the sources, the metadata and the camera. `make_style` (the plugin's `makeStyle`) writes
  them as one text, as before: its output is byte for byte the same on Monaco and Tartu.
- `make_tiled` and the wasm class `Tiler` (`tiles.rs`): the same layers, each with `source-layer`, and each source as
  `{"type": "vector", "tiles": ["<prefix><source>/{z}/{x}/{y}"]}`. `tile(source, z, x, y)` makes a vector tile (MVT 2.1)
  when the map asks for it: an R-tree finds the features, the lines and rings are cut to the tile and a buffer of
  64 px, and the points are rounded to the 4096 units of the tile. Zoom 16 is the last zoom of the tiles.
- The tiles keep each feature's id (`id`, or its position for `generateId`) and only the properties that something
  reads: the texts in the layers, the popup and tooltip fields of the metadata, `name` and `kind`, and the caller's keys.
  A null value is not in a tile; MapLibre reads a missing value as null.
- `props(source, id)`: the properties of one feature. The style hides the layers of the source `roads` (the roads are
  drawn from `simple`), and MapLibre does not load a source whose layers are all hidden. A click on a road finds the
  drawn piece, and its `__rs_edge` is the id for `props("roads", id)`.
- `main.rs --tiles look.json roads.parquet features.parquet`: the times and the sizes of the tiles.

**slots was slow** (Tartu 2.28 s of 6.0 s). Two parts grew with the square of the size: every road in a chain's
box was checked against every segment of the chain, and each slot measured its distance to every road of its
group (one group holds, for example, all unnamed residential streets). Now an R-tree of the chain's segments
and an R-tree of the group's roads find the candidates. The result is the same (the first of equal distances
still wins), and `makeStyle`'s text is byte for byte the same. slots: 2.28 s to 0.70 s; the whole style on Tartu,
natively: 5.2 s to 3.6–3.9 s.

**In duckosm-rust's page** (headless Chromium with software rendering, from the click to an idle map):

| Area | GeoJSON in the style | Vector tiles | Page memory (JS heap) |
|---|---|---|---|
| Monaco | 3.1–3.4 s | 2.0 s | 215–242 MiB to 51–54 MiB |
| Tartu (63,965 roads) | 15.7 s | 7.4 s | 803 MiB to 104 MiB |

The pixels of the maps are the same except for the edges of lines and one street name that MapLibre places at another
point of its street (the tiles cut the line). The module's own memory (1,387 MiB on Tartu) does not change: the
sources are still made as JSON trees first. `MAX_ROADS` stays 100,000 for that reason.

## The engine moves to roadstyle (2026-10-23)

Kaveh's decision: one Rust engine for roadstyle, in the roadstyle repository (`rust/`: the library `roadstyle-rs` and the
browser front-end `roadstyle-wasm`). The plugin, duckosm-rust and later Python roadstyle use that one copy.

- The crate `wasm/` of this repository is deleted. `build.py` has `ROADSTYLE_COMMIT` (now `9175c7f`); `engine()` clones
  roadstyle into `engine/roadstyle` (not in git) at that commit, and `build_wasm()` builds `roadstyle-wasm` there into
  `engine/pkg`. `spike/test_compare.py` and `spike/wasm/run_browser.py` use the same clone (the native program is now
  `roadstyle-rs`).
- A change of the style's Rust code goes into the roadstyle repository, then `ROADSTYLE_COMMIT` moves to it.
- At `9175c7f` the spike tests pass except the building heights: the golden files have the rule of `0806ac2`, which is
  not in the engine yet. The next pin (Kaveh's rule C for the heights) comes from the building session.
- Now `900b43e`: Kaveh's rule C for the building heights (`fba3192`) and the typed sources `roads` and `simple` (half
  the memory, the same output). All 9 spike tests pass. The plugin's looks come from mapstyle 0.12.0, which has
  `m_per_level` 3; `build.py` sets 3.66 (see below).

## Fixes after the pin 900b43e (2026-10-23)

With the pin `900b43e` (above), three fixes:
- `worker/worker.js` imported the deleted `wasm/target/pkg`; it now imports `engine/pkg`.
- The looks in `map/pkgs.bin` had `m_per_level` 3 (mapstyle 0.12.0), and the golden files have 3.66 (mapstyle's
  repository). `build.py` sets `M_PER_LEVEL = 3.66` in both places of the look.
- The new test `test_build_looks_equal_fixture_looks` makes sure that the build's looks are the fixture's looks.

All tests pass: `tests/` (20, with Monaco) and `spike/` (10).
