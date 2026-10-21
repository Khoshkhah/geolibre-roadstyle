// roadstyle and mapstyle in the browser (Pyodide), for the roadstyle GeoLibre plugin (roadstyle › Open a duckOSM database): the map
// of a duckOSM .duckdb made by their own Python code (mapstyle.export.export_inline), so it is the page's map. A module worker: a
// large WebAssembly file (DuckDB) cannot be compiled on the page's main thread, and GeoLibre stays responsive.
// Messages in: {db: ArrayBuffer, name, opts: {mode, theme, paths}, pkgs: ArrayBuffer (roadstyle + mapstyle, until {ready} came back)}; out: {step}, {ready}, {style: JSON}, {error}.

const PYODIDE = "https://cdn.jsdelivr.net/pyodide/v314.0.7/full/";   // Python 3.14, DuckDB 1.5.1 (checked 2026-10-20)
const PACKAGES = ["numpy", "pandas", "shapely", "pyproj", "geopandas", "pyyaml", "duckdb", "scipy", "jinja2", "requests", "xyzservices", "pillow"];
const PYPI = ["folium==0.20.0", "branca==0.8.2"];                       // roadstyle imports them; pure Python, from PyPI
let py = null;

async function start(pkgs) {
  if (!pkgs) throw new Error("the Python packages (pkgs.zip) were not sent");
  const {loadPyodide} = await import(PYODIDE + "pyodide.mjs");
  const p = await loadPyodide({indexURL: PYODIDE});
  await p.loadPackage(PACKAGES);
  await p.loadPackage("micropip");
  await p.runPythonAsync(`import micropip\nawait micropip.install(${JSON.stringify(PYPI)}, deps=False)`);
  p.unpackArchive(pkgs, "zip", {extractDir: "/home/pyodide/pkgs"});
  await p.runPythonAsync(`import sys\nsys.path.insert(0, "/home/pyodide/pkgs")`);
  py = p;                                   // set only when everything loaded: a failed start is tried again from the start
}

onmessage = async ({data}) => {
  try {
    if (!py) {
      postMessage({step: "loading Python, roadstyle and mapstyle (the first time about 30 s)"});
      await start(data.pkgs);
    }
    postMessage({ready: true});             // Python is up: the next map needs no packages
    postMessage({step: "making the map of " + data.name});
    py.FS.writeFile("/home/pyodide/db.duckdb", new Uint8Array(data.db));
    py.globals.set("rs_name", data.name);
    py.globals.set("rs_opts", JSON.stringify(data.opts || {}));
    let style;
    try {
      style = await py.runPythonAsync(`import json
from mapstyle.export import export_inline
opts = {k: v for k, v in json.loads(rs_opts).items() if k in ("mode", "theme", "paths") and isinstance(v, str)}
json.dumps(export_inline("/home/pyodide/db.duckdb", name=rs_name, **opts), separators=(",", ":"))`);
    } finally {
      py.FS.unlink("/home/pyodide/db.duckdb");     // also after an error: the database is not kept in the worker's memory
    }
    postMessage({style});
  } catch (e) {
    postMessage({error: String(e && e.message || e).slice(-3000)});
  }
};
