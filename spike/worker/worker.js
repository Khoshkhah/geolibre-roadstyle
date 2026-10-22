// Steps 1 to 4 of the full port (docs/design/rust-spike.md): the map of a duckOSM file with no Python. duckdb-wasm reads the file and writes
// the roads table (roads_sql.js), the base-map tables (features_sql.js) and the planner's tables (planner_sql.js); the Rust module (pkg/, the
// spike crate) makes the style from them: the roads, their layers (step 2), the base map (step 3) and the route planner's data (step 4).
// The look still comes from Python at build time (look.json).
// Message in: {db: ArrayBuffer, look: string}. Out: {style, parquet, times} or {error}.
import init, {makeStyle} from "./pkg/roadstyle_spike.js";
import {writeRoads} from "./roads_sql.js";
import {writeFeatures} from "./features_sql.js";
import {writePlanner} from "./planner_sql.js";

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
    const byMode = {};                                // the roads of each mode (the access column), for the check against Python's
    for (const mode of ["all", "walking", "driving", "cycling"]) {
      await db.dropFile("roads.parquet").catch(() => {});   // duckdb-wasm: a second COPY to a kept file name is not seen by copyFileToBuffer
      await timed(times, "roadsSql_" + mode, writeRoads(q, "m", mode));
      byMode[mode] = Array.from(await db.copyFileToBuffer("roads.parquet"));
    }
    await db.dropFile("roads.parquet");
    await timed(times, "roadsSql", writeRoads(q, "m"));
    const parquet = await timed(times, "copy", db.copyFileToBuffer("roads.parquet"));
    const specs = JSON.parse(data.look).features.layers_yaml.layers;
    const hasFeatures = await timed(times, "featuresSql", writeFeatures(q, "m", specs));
    const features = hasFeatures ? await db.copyFileToBuffer("features.parquet") : undefined;
    const hasPlanner = await timed(times, "plannerSql", writePlanner(q, "m"));
    const planner = hasPlanner ? await db.copyFileToBuffer("planner.parquet") : undefined;
    const t = performance.now();
    const style = makeStyle(parquet, data.look, features, planner);
    times.rust = Math.round(performance.now() - t);
    times.total = Math.round(performance.now() - t0);
    await conn.close();
    await db.terminate();
    postMessage({style, parquet, byMode, times}, [parquet.buffer]);
  } catch (e) {
    postMessage({error: String(e && (e.message || e)).slice(0, 2000), times});
  }
};
