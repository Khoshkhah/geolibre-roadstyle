"""Where the time of mapstyle.export.export_inline goes, by stage. The same code runs natively and in Pyodide (run_browser.py).

    mapstyle/.venv/bin/python spike/profile/rs_profile.py monaco.duckdb [on|off]      # natively; the route planner on or off

    profile(db, planner=True) -> {"stages": {label: seconds}, "total": seconds, "rest": seconds, "json_dumps": seconds, "n_roads": int, "json_mb": float}

The stages are the functions of mapstyle.map and roadstyle.render_web, wrapped with a timer where render_map / render look them up
(module globals), so nothing in the libraries changes. A function is wrapped once, so profile() can run again in the same process."""
import functools
import json
import time

T = {}


def wrap(mod, name, label):
    f = getattr(mod, name)
    if getattr(f, "_rs_wrapped", False):
        return

    @functools.wraps(f)
    def w(*a, **k):
        t = time.perf_counter()
        try:
            return f(*a, **k)
        finally:
            T[label] = T.get(label, 0.0) + time.perf_counter() - t
    w._rs_wrapped = True
    setattr(mod, name, w)


def profile(db, planner=True):
    import mapstyle.export as me
    import mapstyle.map as mm
    import roadstyle.render_web as rw
    T.clear()
    # the DuckDB reads (mapstyle.map)
    wrap(mm, "load_roads", "read: roads (SQL + GeoDataFrame)")
    wrap(mm, "stored_levels", "read: stored levels")
    wrap(mm, "load_layers", "read: features layers")
    wrap(mm, "planner_data", "read: planner data")
    wrap(mm, "feature_overlays", "features: overlays (python)")
    wrap(me, "_restyle", "features: restyle")
    # roadstyle (render_web.render)
    wrap(rw, "fc_dict", "roads: GeoDataFrame -> GeoJSON")
    wrap(rw, "bake_props", "roads: bake class styles")
    for n in ("_mark_twoway", "_mark_single_line", "_mark_lvl", "_mark_caps", "_mark_levels", "_mark_twin_casing", "_mark_twin_dead_ends", "_stringify_unsafe_ints"):
        wrap(rw, n, "roads: marks")
    wrap(rw, "_casing_parts", "simple: casing parts")
    wrap(rw, "_simple_pieces", "simple: pieces (+stubs, mitres)")
    wrap(rw, "_bridge_decks", "decks (3D)")
    wrap(rw, "_annotation_slots", "arrows + labels: slots")
    wrap(rw, "_build_overlays", "features: style layers")
    t = time.perf_counter()
    style = me.export_inline(db, planner=planner)
    total = time.perf_counter() - t
    t = time.perf_counter()
    text = json.dumps(style, separators=(",", ":"))
    dump = time.perf_counter() - t
    n = len(style["sources"]["roads"]["data"]["features"])
    return {"stages": dict(sorted(T.items(), key=lambda kv: -kv[1])), "total": total, "rest": total - sum(T.values()),
            "json_dumps": dump, "n_roads": n, "json_mb": len(text) / 1e6}


def report(r):
    for k, v in r["stages"].items():
        print(f"{v:7.2f} s  {k}")
    print(f"{r['rest']:7.2f} s  (everything else)")
    print(f"{r['total']:7.2f} s  total export_inline   |  json.dumps {r['json_dumps']:.2f} s, {r['json_mb']:.1f} MB, {r['n_roads']} roads")


if __name__ == "__main__":
    import sys
    report(profile(sys.argv[1], planner=(sys.argv[2] if len(sys.argv) > 2 else "on") == "on"))
