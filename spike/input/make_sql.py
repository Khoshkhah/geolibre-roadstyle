"""The SQL of mapstyle.map.load_roads for one database, as the browser probe runs it (docs/design/rust-spike.md, condition 3).

    mapstyle/.venv/bin/python spike/input/make_sql.py monaco.duckdb      # writes spike/input/roads.sql and runs it natively, with the time

The statements are the ones load_roads runs (the union of the modes' edges, one row per edge_id, the geometry as WKB), built with mapstyle's own
_roads_union, so the probe reads the same rows. The last statement writes them as Parquet: the input the Rust module reads."""
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent


def roads_sql(db):
    import duckdb
    import mapstyle.map as mm

    con = duckdb.connect(str(db), read_only=True)
    try:
        union = mm._roads_union(con, db)
    finally:
        con.close()
    first = lambda c: f"first({c} ORDER BY mode IS NULL, mode, pmode)"     # noqa: E731
    return [
        f"""CREATE OR REPLACE TEMP TABLE r AS
SELECT CAST(edge_id AS VARCHAR) AS edge_id, edge_id AS eid, any_value(source) AS s, any_value(target) AS t,
       CAST({first('osm_id')} AS VARCHAR) AS osm_id, {first('highway')} AS highway, COALESCE({first('name')}, '') AS name,
       {first('bridge')} AS bridge, {first('tunnel')} AS tunnel, {first('layer')} AS layer, bool_or(oneway) AS oneway,
       any_value(walk_type) AS walk_type,
       {", ".join(f"{first(c)} AS {c}" for c in mm.OPTIONAL_COLS[1:])},
       {", ".join(f"COALESCE(bool_or(mode = '{m}'), false) AS {m}" for m in mm.MODES)},
       {", ".join(f"max(CASE WHEN pmode = '{m}' THEN access END) AS access_{m}" for m in mm.MODES)},
       {first('geometry')} AS geom
FROM ({union}) GROUP BY edge_id""",
        "COPY (SELECT r.* EXCLUDE (eid, s, t, geom), ST_AsWKB(r.geom) AS geometry FROM r ORDER BY r.eid) TO 'roads.parquet' (FORMAT parquet)",
    ]


def main(db):
    import duckdb
    sql = roads_sql(db)
    (HERE / "roads.sql").write_text(";\n\n".join(sql) + ";\n")
    import os
    import tempfile
    with tempfile.TemporaryDirectory() as tmp:
        os.chdir(tmp)
        con = duckdb.connect()
        con.execute(f"ATTACH '{Path(db).resolve()}' AS monaco (READ_ONLY); USE monaco")
        t = time.perf_counter()
        for s in sql:
            con.execute(s)
        dt = time.perf_counter() - t
        n = con.execute("SELECT count(*) FROM 'roads.parquet'").fetchone()[0]
        print(f"native DuckDB {duckdb.__version__}: {dt * 1000:.0f} ms, {n} rows, roads.parquet {Path('roads.parquet').stat().st_size / 1e6:.2f} MB")


if __name__ == "__main__":
    main(sys.argv[1])
