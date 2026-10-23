// Step 3 of the full port (docs/design/rust-spike.md): the base-map tables of a duckOSM file as features.parquet, the input of the Rust module's
// base map. The queries of mapstyle.map.load_layers, one per layer of styles/layers.yaml (the look's features.layers_yaml), with the geometry as WKB:
// DuckDB 1.5's core has no ST_Area, ST_Centroid or ST_AsGeoJSON, so the Rust module computes them. A missing table or a query that fails skips its
// layer, as load_layers does. The rows keep load_layers' order: the layers in layers.yaml's order, each table in its rowid order.

// q(sql) -> rows as plain objects. specs: layers.yaml's layers. Writes features.parquet in duckdb-wasm's file system; false when there is none.
export async function writeFeatures(q, catalog, specs) {
  const tables = await q(`SELECT table_schema AS s, table_name AS t FROM information_schema.tables WHERE table_catalog = '${catalog}'`);
  const have = new Set(tables.filter((r) => r.s === "features").map((r) => r.t));
  const main = new Set(tables.filter((r) => r.s === "main").map((r) => r.t));
  // _network_ways: the OSM ids of the ways a mode network holds; a layer of the ways outside them is skipped when there is no network
  const nets = tables.filter((r) => ["driving", "walking", "cycling"].includes(r.s) && ["edges", "private_edges", "edge_id_map"].includes(r.t))
    .map((r) => `SELECT osm_id FROM "${r.s}"."${r.t}"`).join(" UNION ");
  const parts = [];
  for (const [i, s] of specs.entries()) {
    if (s.show === false || !(s.schema === "main" ? main : have).has(s.table)) continue;
    if (s.outside_networks && !nets) continue;
    const layer = (s.merge_into || s.name).replaceAll("'", "''");
    // a table outside features.* (the boundary: the background, the whole boundary, as in the plugin's Pyodide: no ST_Difference without the
    // spatial extension): the kind is the layer's name
    const kind = s.schema ? `'${s.name.replaceAll("'", "''")}'` : "kind";
    const sql = (levels, height) => `SELECT ${i} AS spec, '${layer}' AS layer, rowid AS rid, CAST(osm_id AS VARCHAR) AS osm_id, ${kind} AS kind,
        COALESCE(name, '') AS name, ${s.bearing ? "CAST(bearing AS DOUBLE)" : "CAST(NULL AS DOUBLE)"} AS bearing, CAST(${levels} AS VARCHAR) AS levels,
        CAST(${height} AS VARCHAR) AS height, ST_AsWKB(geom) AS geometry FROM ${s.schema || "features"}.${s.table} WHERE geom IS NOT NULL
        ${s.where ? ` AND (${s.where})` : ""}${s.outside_networks ? ` AND osm_id NOT IN (${nets})` : ""}`;
    try {
      try {
        await q(`CREATE OR REPLACE TEMP TABLE f${i} AS ${s.height ? sql("tags['building:levels'][1]", "tags['height'][1]") : sql("NULL", "NULL")}`);
      } catch (e) {
        if (!s.height) throw e;
        await q(`CREATE OR REPLACE TEMP TABLE f${i} AS ${sql("NULL", "NULL")}`);      // an older file has no tags: the height of the kind
      }
      parts.push(`SELECT * FROM f${i}`);
    } catch (e) {
      console.warn(`base-map layer ${s.name} skipped: ${e.message || e}`);      // load_layers logs it and goes on
    }
  }
  if (!parts.length) return false;
  await q(`COPY (${parts.join(" UNION ALL ")} ORDER BY spec, rid) TO 'features.parquet' (FORMAT parquet)`);
  return true;
}
