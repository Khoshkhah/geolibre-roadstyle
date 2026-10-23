// Step 4 of the full port (docs/design/rust-spike.md): the tables of the route planner's data (mapstyle.map.planner_data) as planner.parquet, the
// input of the Rust module's planner. One row per record with its part and the order Python reads it in (grp, ord: a table's or view's reading
// order): `mode` (a mode with edges and edge_graph), `end` (an edge's nodes, length and junction: edges then private_edges, each mode in order),
// `graph` (a mode's edges by edge_id), `next` (a mode's turns: its own rows where edge_graph has `uses`), `mm` (walk + drive is there), `mme` and
// `mmt` (mm.edges, mm.transfers). The fixture writes the same file in Python (tools/make_spike_fixture.py, planner_parquet).

const MODES = ["driving", "walking", "cycling"];
const OWN = {driving: "car", walking: "walk", cycling: "bike"};

// q(sql) -> rows as plain objects. Writes planner.parquet in duckdb-wasm's file system; false when no mode can be routed on.
export async function writePlanner(q, catalog) {
  const have = new Set((await q(`SELECT table_schema AS s, table_name AS t FROM information_schema.tables WHERE table_catalog = '${catalog}'`))
    .map((r) => `${r.s}.${r.t}`));
  const cols = new Set((await q(`SELECT table_schema AS s, table_name AS t, column_name AS c FROM information_schema.columns WHERE table_catalog = '${catalog}'`))
    .map((r) => `${r.s}.${r.t}.${r.c}`));
  const routable = (m) => have.has(`${m}.edges`) && have.has(`${m}.edge_graph`);
  const parts = [];
  MODES.forEach((m, i) => { if (routable(m)) parts.push(`SELECT 'mode' AS part, ${i} AS grp, 0 AS ord, '${m}' AS mode`); });
  if (!parts.length) return false;
  ["edges", "private_edges"].forEach((tb, ti) => MODES.forEach((m, i) => {
    if (!have.has(`${m}.${tb}`)) return;
    const jn = cols.has(`${m}.${tb}.junction`) ? "junction" : "CAST(NULL AS VARCHAR)";
    parts.push(`SELECT 'end' AS part, ${ti * 10 + i} AS grp, row_number() OVER () AS ord, * FROM (SELECT edge_id AS e, source AS s, target AS t,
      CAST(length_m AS DOUBLE) AS len, ${jn} AS junction FROM ${m}.${tb})`);
  }));
  MODES.forEach((m, i) => {
    if (!routable(m)) return;
    parts.push(`SELECT 'graph' AS part, ${i} AS grp, row_number() OVER (ORDER BY edge_id) AS ord, '${m}' AS mode, edge_id AS e,
      CAST(cost_s AS DOUBLE) AS cost, CAST(length_m AS DOUBLE) AS len FROM ${m}.edges`);
    const uses = cols.has(`${m}.edge_graph.uses`) ? ` WHERE uses = '${OWN[m]}'` : "";
    parts.push(`SELECT 'next' AS part, ${i} AS grp, row_number() OVER () AS ord, * FROM (SELECT '${m}' AS mode, from_edge AS e, to_edge AS e2
      FROM ${m}.edge_graph${uses})`);
  });
  if (have.has("mm.edges") && have.has("mm.transfers")) {
    parts.push("SELECT 'mm' AS part, 0 AS grp, 0 AS ord");
    parts.push(`SELECT 'mme' AS part, 0 AS grp, row_number() OVER () AS ord, * FROM (SELECT mode, source AS s, target AS t, edge_id AS e,
      CAST(cost_s AS DOUBLE) AS cost FROM mm.edges)`);
    parts.push(`SELECT 'mmt' AS part, 0 AS grp, row_number() OVER () AS ord, * FROM (SELECT node_id AS s, from_mode AS fm, to_mode AS tm,
      CAST(cost_s AS DOUBLE) AS cost FROM mm.transfers)`);
  }
  await q(`COPY (${parts.join(" UNION ALL BY NAME ")} ORDER BY part, grp, ord) TO 'planner.parquet' (FORMAT parquet)`);
  return true;
}
