// Step 3 of the full port (docs/design/rust-spike.md): the base-map tables of a duckOSM file as features.parquet, the input of the Rust module's
// base map. The queries of mapstyle.map.load_layers, one per layer of styles/layers.yaml (the look's features.layers_yaml), with the geometry as WKB:
// DuckDB 1.5's core has no ST_Area, ST_Centroid or ST_AsGeoJSON, so the Rust module computes them. A missing table or a query that fails skips its
// layer, as load_layers does. The rows keep load_layers' order: the layers in layers.yaml's order, each table in its rowid order.

// q(sql) -> rows as plain objects. specs: layers.yaml's layers. Writes features.parquet in duckdb-wasm's file system; false when there is none.
export async function writeFeatures(q, catalog, specs) {
  const have = new Set((await q(`SELECT table_name AS t FROM information_schema.tables WHERE table_catalog = '${catalog}' AND table_schema = 'features'`))
    .map((r) => r.t));
  const parts = [];
  for (const [i, s] of specs.entries()) {
    if (s.show === false || !have.has(s.table)) continue;
    const [levels, height] = s.height ? ["tags['building:levels'][1]", "tags['height'][1]"] : ["NULL", "NULL"];
    const layer = (s.merge_into || s.name).replaceAll("'", "''");
    const sql = `SELECT ${i} AS spec, '${layer}' AS layer, rowid AS rid, CAST(osm_id AS VARCHAR) AS osm_id, kind, COALESCE(name, '') AS name,
        ${s.bearing ? "CAST(bearing AS DOUBLE)" : "CAST(NULL AS DOUBLE)"} AS bearing, CAST(${levels} AS VARCHAR) AS levels, CAST(${height} AS VARCHAR) AS height,
        ST_AsWKB(geom) AS geometry FROM features.${s.table} WHERE geom IS NOT NULL${s.where ? ` AND (${s.where})` : ""}`;
    try {
      await q(`CREATE OR REPLACE TEMP TABLE f${i} AS ${sql}`);
      parts.push(`SELECT * FROM f${i}`);
    } catch (e) {
      console.warn(`base-map layer ${s.name} skipped: ${e.message || e}`);      // load_layers logs it and goes on
    }
  }
  if (!parts.length) return false;
  await q(`COPY (${parts.join(" UNION ALL ")} ORDER BY spec, rid) TO 'features.parquet' (FORMAT parquet)`);
  return true;
}
