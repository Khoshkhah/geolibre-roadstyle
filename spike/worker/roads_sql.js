// Step 1 of the full port (docs/design/rust-spike.md): the roads table that the Rust module reads, made by duckdb-wasm from a duckOSM file.
// The SQL of mapstyle.map (_roads_union, load_roads), the checks of roadstyle's load_area_levels and the columns that render_map adds
// (the stored levels, access, is_directed, has_arrows), for the map of a mode (render_map's mode: "all" or one network). The result is
// written as roads.parquet.

const MODES = ["driving", "walking", "cycling"];
const OPTIONAL_COLS = ["walk_type", "junction", "edge_ref", "lanes"];
const PATH_CLASSES = ["footway", "path", "cycleway", "steps", "pedestrian", "bridleway", "corridor"];
const COLS = "edge_id, source, target, osm_id, highway, name, bridge, tunnel, layer, oneway, geometry";

// _roads_union: every mode's edges and private_edges, NULL for an optional column a file does not have
async function roadsUnion(q, catalog) {
  const tables = await q(`SELECT table_schema AS s, table_name AS t FROM information_schema.tables WHERE table_catalog = '${catalog}'`);
  const has = new Set((await q(`SELECT table_schema AS s, table_name AS t, column_name AS c FROM information_schema.columns WHERE table_catalog = '${catalog}'`))
    .map((r) => `${r.s}.${r.t}.${r.c}`));
  const edges = new Set(tables.filter((r) => r.t === "edges").map((r) => r.s));
  const modes = MODES.filter((m) => edges.has(m));
  if (!modes.length) throw new Error(`no <mode>.edges table (modes: ${MODES.join(", ")})`);
  const priv = [...new Set(tables.filter((r) => r.t === "private_edges" && modes.includes(r.s)).map((r) => r.s))].sort();
  const optional = (m, t, skip = []) => OPTIONAL_COLS.map((c) =>
    `${has.has(`${m}.${t}.${c}`) && !skip.includes(c) ? c : (c === "edge_ref" ? "CAST(NULL AS VARCHAR)" : "NULL")} AS ${c}`).join(", ");
  return [
    ...modes.map((m) => `SELECT ${COLS}, ${optional(m, "edges")}, '${m}' AS mode, NULL AS pmode, NULL AS access FROM ${m}.edges`),
    ...priv.map((m) => `SELECT ${COLS}, ${optional(m, "private_edges", ["walk_type"])}, NULL AS mode, '${m}' AS pmode, access FROM ${m}.private_edges`),
  ].join(" UNION ALL ");
}

// load_area_levels' checks: the stored levels are from a level area and were solved for these edges (their number and the hash of their ids)
async function checkLevels(q) {
  let meta;
  try {
    meta = (await q("SELECT source, n_edges, edge_hash FROM visualization.edge_levels_meta"))[0];
  } catch (e) {
    throw new Error("no visualization.edge_levels_meta in this file: make and solve its level area (duckosm levels)");
  }
  if (!meta || meta.source !== "area") throw new Error("the stored levels are not from a level area: make and solve its level area again");
  const ids = (await q("SELECT eid FROM r ORDER BY eid")).map((r) => BigInt(r.eid));
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new BigInt64Array(ids).buffer));     // _hash: sha256 of the sorted int64 ids
  const hash = [...digest].map((b) => b.toString(16).padStart(2, "0")).join("").slice(0, 32);
  if (ids.length !== Number(meta.n_edges) || hash !== meta.edge_hash)
    throw new Error(`the edges are not the edges the stored levels were solved for (${ids.length} given, ${Number(meta.n_edges)} stored, or other edge_id values): run duckosm levels again`);
}

// q(sql) -> rows as plain objects; mode: "all", "driving", "walking" or "cycling". Writes roads.parquet in duckdb-wasm's file system.
export async function writeRoads(q, catalog, mode = "all") {
  if (mode !== "all" && !MODES.includes(mode)) throw new Error(`unknown mode ${mode}`);
  const union = await roadsUnion(q, catalog);
  const first = (c) => `first(${c} ORDER BY mode IS NULL, mode, pmode)`;
  await q(`CREATE OR REPLACE TEMP TABLE r AS
    SELECT CAST(edge_id AS VARCHAR) AS edge_id, edge_id AS eid, any_value(source) AS s, any_value(target) AS t,
           CAST(${first("osm_id")} AS VARCHAR) AS osm_id, ${first("highway")} AS highway, COALESCE(${first("name")}, '') AS name,
           ${first("bridge")} AS bridge, ${first("tunnel")} AS tunnel, ${first("layer")} AS layer, bool_or(oneway) AS oneway,
           any_value(walk_type) AS walk_type,
           ${OPTIONAL_COLS.slice(1).map((c) => `${first(c)} AS ${c}`).join(", ")},
           ${MODES.map((m) => `COALESCE(bool_or(mode = '${m}'), false) AS ${m}`).join(", ")},
           ${MODES.map((m) => `max(CASE WHEN pmode = '${m}' THEN access END) AS access_${m}`).join(", ")},
           ${first("geometry")} AS geom
    FROM (${union}) GROUP BY edge_id`);
  await checkLevels(q);
  const path = `coalesce(highway IN (${PATH_CLASSES.map((c) => `'${c}'`).join(", ")}), false)`;
  const restricted = "coalesce(access IN ('private', 'bus'), false)";
  await q(`COPY (
    WITH x AS (SELECT r.* EXCLUDE (s, t, geom), ST_AsWKB(r.geom) AS geometry FROM r),
    -- load_roads: an undirected edge (a one-way street's walking-only reverse) on the street's own directed edge goes first
    u AS (SELECT *, NOT driving AND NOT cycling AND NOT ${path} AND osm_id IN (SELECT osm_id FROM x WHERE driving OR cycling) AS und FROM x),
    -- _access: in one mode, its restriction where that mode can't use the road; mode "all": bus for a bus road or lane for cars, else the
    -- first restriction where no mode can use the road
    a AS (SELECT u.*, ${mode !== "all" ? `CASE WHEN NOT ${mode} THEN access_${mode} END` : `CASE WHEN access_driving = 'bus' THEN access_driving
                           WHEN NOT (driving OR walking OR cycling) AND (access_driving IS NOT NULL OR access_walking IS NOT NULL OR access_cycling IS NOT NULL)
                           THEN coalesce(access_driving, access_walking, access_cycling) END`} AS access FROM u)
    SELECT a.* EXCLUDE (eid, und, access_driving, access_walking, access_cycling),
           lv.casing_start, lv.casing_level, lv.casing_end, lv.fill_level, lv.head_start_m, lv.head_end_m, lv.cap_start, lv.cap_end,
           (driving OR cycling OR ${restricted}) AND NOT ${path} AS is_directed,     -- _is_directed
           driving OR ${restricted} AS has_arrows
    FROM a LEFT JOIN visualization.edge_levels lv ON lv.edge_id = a.eid
    ORDER BY NOT und, a.eid) TO 'roads.parquet' (FORMAT parquet)`);
}
