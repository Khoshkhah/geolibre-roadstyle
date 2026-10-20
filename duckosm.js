// A duckOSM database (.duckdb) read in the browser with DuckDB-wasm, for the roadstyle GeoLibre plugin (roadstyle › Open a
// duckOSM database): no server, no export. readDuckOSM(file) returns the roads of every mode (one row per edge_id, with the
// file's drawing positions from visualization.edge_levels), each features.* table and the boundary, as GeoJSON. casingPieces
// cuts a road's casing into its start head, main part and end head, as roadstyle draws it (casing_start_col / head_start_m_col).

const DUCKDB = "https://cdn.jsdelivr.net/npm/@duckdb/duckdb-wasm@1.32.0/+esm";   // reads files of DuckDB 1.5 (checked 2026-10-20)
let engine = null;

function duck() {
  if (!engine) engine = (async () => {
    const duckdb = await import(DUCKDB);
    const b = await duckdb.selectBundle(duckdb.getJsDelivrBundles());
    const worker = new Worker(URL.createObjectURL(new Blob([`importScripts("${b.mainWorker}");`], {type: "text/javascript"})));
    const db = new duckdb.AsyncDuckDB(new duckdb.ConsoleLogger(duckdb.LogLevel.WARNING), worker);
    await db.instantiate(b.mainModule, b.pthreadWorker);
    const c = await db.connect();
    await c.query("INSTALL spatial; LOAD spatial;");
    return {duckdb, db, c};
  })();
  return engine;
}

// a BIGINT comes as a BigInt (1n): a number when it fits one exactly, else text (an id past 2**53)
const plain = (v) => (typeof v === "bigint" ? (Number.isSafeInteger(Number(v)) ? Number(v) : String(v)) : v);
const rowsOf = (table) => table.toArray().map((r) => Object.fromEntries(Object.entries(r.toJSON()).map(([k, v]) => [k, plain(v)])));
const fc = (rows) => ({type: "FeatureCollection", features: rows.map(({g, ...p}) => ({type: "Feature", properties: p, geometry: JSON.parse(g)}))});

export async function readDuckOSM(file, onStep = () => {}) {
  onStep("loading DuckDB in the browser");
  const {duckdb, db, c} = await duck();
  const name = `rs_${Date.now()}.duckdb`, a = `rs_${Date.now()}`;
  await db.registerFileHandle(name, file, duckdb.DuckDBDataProtocol.BROWSER_FILEREADER, true);
  await c.query(`ATTACH '${name}' AS ${a} (READ_ONLY)`);
  try {
    const q = async (sql) => rowsOf(await c.query(sql));
    const tables = await q(`SELECT table_schema s, table_name t FROM information_schema.tables WHERE table_catalog = '${a}'`);
    const has = (s, t) => tables.some((x) => x.s === s && x.t === t);
    const cols = async (s, t) => new Set((await q(`SELECT column_name c FROM information_schema.columns
      WHERE table_catalog = '${a}' AND table_schema = '${s}' AND table_name = '${t}'`)).map((r) => r.c));
    const modes = ["driving", "walking", "cycling"].filter((m) => has(m, "edges"));
    if (!modes.length) throw new Error("roadstyle: " + file.name + " is not a duckOSM database (no driving, walking or cycling edges table)");

    onStep("reading the roads");
    // one row per edge_id over the modes: the attributes of the first mode that has it (driving, walking, cycling, as duckosm
    // levels' load_roads), one-way as the driving network says, the modes that use it
    const parts = [];
    for (const [i, m] of modes.entries()) {
      const cs = await cols(m, "edges"), o = (k) => (cs.has(k) ? k : "NULL");
      parts.push(`SELECT edge_id, ${o("edge_ref")} AS edge_ref, osm_id, highway, name, ${o("oneway")} AS oneway, ${o("lanes")} AS lanes,
        ${o("layer")} AS layer, bridge, tunnel, ${o("junction")} AS junction, geometry, '${m}' AS mode, ${i} AS rank FROM ${a}.${m}.edges`);
    }
    const first = (k) => `first(${k} ORDER BY rank) AS ${k}`;
    const lv = has("visualization", "edge_levels");
    const roads = await q(`WITH u AS (${parts.join(" UNION ALL ")}),
      r AS (SELECT edge_id, ${["edge_ref", "osm_id", "highway", "name", "lanes", "layer", "bridge", "tunnel", "junction", "geometry"].map(first).join(", ")},
                   bool_or(oneway) FILTER (WHERE mode = 'driving') AS oneway, string_agg(DISTINCT mode, ' + ' ORDER BY mode) AS modes
            FROM u GROUP BY edge_id)
      SELECT r.* EXCLUDE (edge_id, osm_id, geometry), CAST(r.edge_id AS VARCHAR) AS edge_id, CAST(r.osm_id AS VARCHAR) AS osm_id
             ${lv ? ", v.* EXCLUDE (edge_id)" : ""}, ST_AsGeoJSON(r.geometry) AS g
      FROM r ${lv ? `LEFT JOIN ${a}.visualization.edge_levels v ON r.edge_id = v.edge_id` : ""}`);

    onStep("reading the base map");
    const features = {};
    for (const {t} of tables.filter((x) => x.s === "features")) {
      const cs = await cols("features", t);
      if (!cs.has("geom")) continue;
      features[t] = fc(await q(`SELECT CAST(osm_id AS VARCHAR) AS osm_id, ${cs.has("kind") ? "kind" : "NULL AS kind"}, ${cs.has("name") ? "name" : "NULL AS name"}
        ${cs.has("bearing") ? ", bearing" : ""}, ST_AsGeoJSON(geom) AS g FROM ${a}.features.${t} WHERE geom IS NOT NULL`));
    }
    const boundary = has("main", "boundary") ? fc(await q(`SELECT name, ST_AsGeoJSON(geom) AS g FROM ${a}.main.boundary`)) : null;
    return {roads: fc(roads), features, boundary, levels: lv};
  } finally {
    await c.query(`DETACH ${a}`);
    await db.dropFile(name);
  }
}

// metres between two lon/lat points (an equirectangular step: exact enough along a road)
const R = 6371008.8, RAD = Math.PI / 180;
const metres = (p, q) => Math.hypot((q[0] - p[0]) * RAD * Math.cos((p[1] + q[1]) / 2 * RAD), (q[1] - p[1]) * RAD) * R;

function slice(cs, d0, d1) {           // the part of a line from d0 to d1 metres along it
  const out = [];
  let at = 0;
  for (let i = 0; i < cs.length - 1; i++) {
    const p = cs[i], q = cs[i + 1], len = metres(p, q);
    if (len > 0 && at + len >= d0 && at <= d1) {
      const pt = (t) => [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
      if (!out.length) out.push(pt(Math.max(0, (d0 - at) / len)));
      out.push(pt(Math.min(1, (d1 - at) / len)));
    }
    at += len;
  }
  return out;
}

// A road's casing in pieces, each with its own drawing position in casing_level: one piece when its three numbers are the same,
// else the start head, the main part and the end head. A head is head_start_m / head_end_m metres (empty: headM); a road shorter
// than its two heads is cut in their ratio (roadstyle's rule).
export function casingPieces(roads, headM) {
  const out = [];
  for (const f of roads.features) {
    const p = f.properties, cs = f.geometry && f.geometry.type === "LineString" ? f.geometry.coordinates : null;
    const cl = p.casing_level ?? 0, c0 = p.casing_start ?? cl, c1 = p.casing_end ?? cl;
    if (!cs || (c0 === cl && c1 === cl)) { out.push({...f, properties: {...p, casing_level: cl}}); continue; }
    let len = 0;
    for (let i = 0; i < cs.length - 1; i++) len += metres(cs[i], cs[i + 1]);
    let hs = p.head_start_m ?? headM, he = p.head_end_m ?? headM;
    if (hs + he > len) { const k = len / (hs + he); hs *= k; he *= k; }
    for (const [d0, d1, pos] of [[0, hs, c0], [hs, len - he, cl], [len - he, len, c1]])
      if (d1 > d0) out.push({type: "Feature", properties: {...p, casing_level: pos}, geometry: {type: "LineString", coordinates: slice(cs, d0, d1)}});
  }
  return {type: "FeatureCollection", features: out};
}
