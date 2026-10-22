// The Rust preview of the plugin's worker (docs/design/rust-spike.md): the map of a duckOSM file with no Python. The same messages as
// py/worker.js, so index.js does not change: in {warm, pkgs} or {db, name, opts, pkgs}; out {step}, {ready}, {style: JSON text}, {error}.
// pkgs: the look (look.json, made at build time with Python) and the wasm module, as one blob: [length of the look, 4 bytes LE][look][wasm].
// duckdb-wasm reads the file and writes the roads, the base-map tables and the planner's tables as Parquet (../worker/*_sql.js); the Rust
// module makes the style from them. The names and arrows are in the first style (no "extra": in Rust they take about 0.2 s).
// The preview makes only the dialog's default choices (all modes, OpenStreetMap colours, Google footpaths), and no 3D bridge decks.
import init, {makeStyle} from "../rust/target/pkg/roadstyle_spike.js";
import {writeRoads} from "../worker/roads_sql.js";
import {writeFeatures} from "../worker/features_sql.js";
import {writePlanner} from "../worker/planner_sql.js";

const DUCKDB = "https://cdn.jsdelivr.net/npm/@duckdb/duckdb-wasm@1.33.1-dev57.0/+esm";      // DuckDB 1.5.4: the file's GEOMETRY type is core
const DEFAULTS = {mode: "all", theme: "osm", paths: "google"};
const NAMES = {mode: "Roads in front: all modes", theme: "Colours: OpenStreetMap", paths: "Footpaths and cycleways: Google"};
let db = null, look = null;

async function startDuckDB() {
  const duckdb = await import(DUCKDB);
  const bundle = await duckdb.selectBundle(duckdb.getJsDelivrBundles());
  const url = URL.createObjectURL(new Blob([`importScripts("${bundle.mainWorker}");`], {type: "text/javascript"}));
  const d = new duckdb.AsyncDuckDB(new duckdb.VoidLogger(), new Worker(url));
  await d.instantiate(bundle.mainModule, bundle.pthreadWorker);
  URL.revokeObjectURL(url);
  return d;
}

async function start(pkgs) {
  if (!pkgs) throw new Error("the Rust module (pkgs) was not sent");
  const n = new DataView(pkgs).getUint32(0, true);
  const text = new TextDecoder().decode(new Uint8Array(pkgs, 4, n));
  const [d] = await Promise.all([startDuckDB(), init({module_or_path: new Uint8Array(pkgs, 4 + n)})]);
  db = d;
  look = text;                              // set only when everything started: a failed start is tried again from the start
}

let queue = Promise.resolve();               // one message at a time: a warm-up and a map message may come close together
onmessage = ({data}) => { queue = queue.then(() => handle(data)); };

async function handle(data) {
  try {
    if (!look) {
      postMessage({step: "starting DuckDB and the Rust module"});
      await start(data.pkgs);
    }
    postMessage({ready: true});
    if (data.warm) return;                  // a warm-up (the plugin sends it when its dialog opens)
    const o = data.opts || {};
    const other = Object.keys(DEFAULTS).filter((k) => o[k] && o[k] !== DEFAULTS[k]);
    if (other.length)
      throw new Error(`the Rust preview makes only the default choices (${Object.values(NAMES).join(", ")}); choose them, or use the roadstyle plugin`);
    postMessage({step: "reading " + data.name});
    await db.registerFileBuffer("map.duckdb", new Uint8Array(data.db));
    const conn = await db.connect();
    const q = async (sql) => (await conn.query(sql)).toArray().map((r) => r.toJSON());
    try {
      await conn.query("ATTACH 'map.duckdb' AS m (READ_ONLY)");
      await conn.query("USE m");
      postMessage({step: "the roads (SQL)"});
      await writeRoads(q, "m");
      const roads = await db.copyFileToBuffer("roads.parquet");
      postMessage({step: "the base map and the route planner (SQL)"});
      const features = (await writeFeatures(q, "m", JSON.parse(look).features.layers_yaml.layers)) ? await db.copyFileToBuffer("features.parquet") : undefined;
      const planner = o.planner && (await writePlanner(q, "m")) ? await db.copyFileToBuffer("planner.parquet") : undefined;
      postMessage({step: "making the map (Rust)"});
      const t = performance.now(), style = makeStyle(roads, look, features, planner, data.name);
      const s = ((performance.now() - t) / 1000).toFixed(1);       // shown in the box, so a slow browser can be told from a slow page
      console.info(`roadstyle (Rust): the style in ${s} s, ${(style.length / 1e6).toFixed(1)} MB`);
      postMessage({step: `sending the map (Rust took ${s} s)`});
      postMessage({style});
    } finally {                             // also after an error: the database is not kept in the worker's memory
      await conn.query("USE memory").catch(() => {});
      await conn.query("DETACH m").catch(() => {});
      await conn.close();
      for (const f of ["map.duckdb", "roads.parquet", "features.parquet", "planner.parquet"]) await db.dropFile(f).catch(() => {});
    }
  } catch (e) {
    postMessage({error: String(e && (e.message || e)).slice(-3000)});
  }
}
