// roadstyle and mapstyle in the browser (Pyodide), for the roadstyle GeoLibre plugin (roadstyle › Open a duckOSM database): the map
// of a duckOSM .duckdb made by their own Python code (mapstyle.export.export_inline), so it is the page's map. A module worker: a
// large WebAssembly file (DuckDB) cannot be compiled on the page's main thread, and GeoLibre stays responsive.
// Messages in: {warm: true, pkgs} (only start Python), or {db: ArrayBuffer, name, opts: {mode, theme, paths, planner, split}, pkgs: ArrayBuffer (roadstyle + mapstyle, until {ready} came back)}; out: {step}, {ready}, {style: JSON}, {error}.

const PYODIDE = "https://cdn.jsdelivr.net/pyodide/v314.0.7/full/";   // Python 3.14, DuckDB 1.5.1 (checked 2026-10-20)
const PACKAGES = ["numpy", "pandas", "shapely", "pyproj", "geopandas", "pyyaml", "duckdb", "jinja2", "pillow"];   // what a map imports (checked 2026-10-22: no scipy, requests, xyzservices, folium)
// no PyPI at run time: branca (pure Python, which roadstyle imports) is inside pkgs.zip, with roadstyle and mapstyle
let py = null;

async function start(pkgs) {
  if (!pkgs) throw new Error("the Python packages (pkgs.zip) were not sent");
  const {loadPyodide} = await import(PYODIDE + "pyodide.mjs");
  const p = await loadPyodide({indexURL: PYODIDE});
  await p.loadPackage(PACKAGES);
  p.unpackArchive(pkgs, "zip", {extractDir: "/home/pyodide/pkgs"});
  await p.runPythonAsync(`import sys\nsys.path.insert(0, "/home/pyodide/pkgs")`);
  py = p;                                   // set only when everything loaded: a failed start is tried again from the start
}

let queue = Promise.resolve();               // one message at a time: a warm-up and a map message may come close together
onmessage = ({data}) => { queue = queue.then(() => handle(data)); };

async function handle(data) {
  try {
    if (!py) {
      postMessage({step: "loading Python, roadstyle and mapstyle (the first time about 30 s)"});
      await start(data.pkgs);
    }
    postMessage({ready: true});             // Python is up: the next map needs no packages
    if (data.warm) return;                  // a warm-up (the plugin sends it when its dialog opens): Python loads while the user chooses the file
    postMessage({step: "making the map of " + data.name});
    py.FS.writeFile("/home/pyodide/db.duckdb", new Uint8Array(data.db));
    py.globals.set("rs_name", data.name);
    py.globals.set("rs_opts", JSON.stringify(data.opts || {}));
    const split = !!(data.opts && data.opts.split);
    try {
      // The street names and the one-way arrows are two thirds of the work of a map. With `split`, the first style has none (the plugin draws it at once),
      // and the second pass makes the whole style again and sends what the first one lacks (their source and layers, and where to put them).
      const quick = await py.runPythonAsync(`import json
from mapstyle.export import export_inline
given = json.loads(rs_opts)
rs_args = {k: v for k, v in given.items() if k in ("mode", "theme", "paths") and isinstance(v, str)}
if given.get("planner") is True:
    rs_args["planner"] = True               # the route planner's graphs go into the style (mapstyle 0.11)
rs_quick = export_inline("/home/pyodide/db.duckdb", name=rs_name, **(dict(rs_args, arrows=False, labels=False) if given.get("split") else rs_args))
json.dumps(rs_quick, separators=(",", ":"))`);
      postMessage({style: quick});
      if (split) {
        try {
          const extra = await py.runPythonAsync(`full = export_inline("/home/pyodide/db.duckdb", name=rs_name, **rs_args)
have = {l["id"] for l in rs_quick["layers"]}
ids = [l["id"] for l in full["layers"]]
new = [l for l in full["layers"] if l["id"] not in have]
before = {l["id"]: next((x for x in ids[ids.index(l["id"]) + 1:] if x in have), None) for l in new}
sources = {k: v for k, v in full["sources"].items() if k not in rs_quick["sources"]}
json.dumps({"layers": new, "sources": sources, "before": before, "images": full.get("metadata", {}).get("roadstyle:images", {})}, separators=(",", ":"))`);
          postMessage({extra, full: await py.runPythonAsync(`json.dumps(full, separators=(",", ":"))`)});
        } catch (e) { postMessage({extraError: String(e && e.message || e).slice(-1500)}); }
      }
    } finally {
      py.FS.unlink("/home/pyodide/db.duckdb");     // also after an error: the database is not kept in the worker's memory
    }
  } catch (e) {
    postMessage({error: String(e && e.message || e).slice(-3000)});
  }
}
