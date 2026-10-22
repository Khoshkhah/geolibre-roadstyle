// Steps 1 to 3 of the full port (docs/design/rust-spike.md): the map of a duckOSM file with no Python. duckdb-wasm reads the file and writes
// the roads table (roads_sql.js) and the base-map tables (features_sql.js), the Rust module (pkg/, the spike crate) makes the style from them:
// the roads, their layers (step 2) and the base map (step 3). The look still comes from Python at build time (look.json).
// Message in: {db: ArrayBuffer, look: string}. Out: {style, parquet, times} or {error}.
import init, {makeStyle} from "./pkg/roadstyle_spike.js";
import {writeRoads} from "./roads_sql.js";
import {writeFeatures} from "./features_sql.js";

const DUCKDB = "https://cdn.jsdelivr.net/npm/@duckdb/duckdb-wasm@1.33.1-dev57.0/+esm";      // DuckDB 1.5.4: the file's GEOMETRY type is core

async function startDuckDB() {
  const duckdb = await import(DUCKDB);
  const bundle = await duckdb.selectBundle(duckdb.getJsDelivrBundles());
  const url = URL.createObjectURL(new Blob([`importScripts("${bundle.mainWorker}");`], {type: "text/javascript"}));
  const db = new duckdb.AsyncDuckDB(new duckdb.VoidLogger(), new Worker(url));
  await db.instantiate(bundle.mainModule, bundle.pthreadWorker);
  URL.revokeObjectURL(url);
  return db;
}

const timed = async (times, key, p) => { const t = performance.now(); const v = await p; times[key] = Math.round(performance.now() - t); return v; };

onmessage = async ({data}) => {
  const times = {};
  const t0 = performance.now();
  try {
    const [db] = await Promise.all([timed(times, "duckdbStart", startDuckDB()), timed(times, "rustStart", init())]);
    const conn = await db.connect();
    const q = async (sql) => (await conn.query(sql)).toArray().map((r) => r.toJSON());
    await timed(times, "attach", (async () => {
      await db.registerFileBuffer("map.duckdb", new Uint8Array(data.db));
      await conn.query("ATTACH 'map.duckdb' AS m (READ_ONLY)");
      await conn.query("USE m");
    })());
    await timed(times, "roadsSql", writeRoads(q, "m"));
    const parquet = await timed(times, "copy", db.copyFileToBuffer("roads.parquet"));
    const specs = JSON.parse(data.look).features.layers_yaml.layers;
    const hasFeatures = await timed(times, "featuresSql", writeFeatures(q, "m", specs));
    const features = hasFeatures ? await db.copyFileToBuffer("features.parquet") : undefined;
    const t = performance.now();
    const style = makeStyle(parquet, data.look, features);
    times.rust = Math.round(performance.now() - t);
    times.total = Math.round(performance.now() - t0);
    await conn.close();
    await db.terminate();
    postMessage({style, parquet, times}, [parquet.buffer]);
  } catch (e) {
    postMessage({error: String(e && (e.message || e)).slice(0, 2000), times});
  }
};
