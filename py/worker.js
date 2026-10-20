// roadstyle and mapstyle in the browser (Pyodide), for the roadstyle GeoLibre plugin (roadstyle › Open a duckOSM database): the map
// of a duckOSM .duckdb made by their own Python code (mapstyle.export.export_inline), so it is the page's map. A module worker: a
// large WebAssembly file (DuckDB) cannot be compiled on the page's main thread, and GeoLibre stays responsive.
// Messages in: {db: ArrayBuffer, name, pkgs: ArrayBuffer (roadstyle + mapstyle, the first time)}; out: {step}, {style: JSON}, {error}.

const PYODIDE = "https://cdn.jsdelivr.net/pyodide/v314.0.7/full/";   // Python 3.14, DuckDB 1.5.1 (checked 2026-10-20)
const PACKAGES = ["numpy", "pandas", "shapely", "pyproj", "geopandas", "pyyaml", "duckdb", "scipy", "jinja2", "requests", "xyzservices", "pillow"];
const PYPI = ["folium==0.20.0", "branca==0.8.2"];                       // roadstyle imports them; pure Python, from PyPI
let py = null;

async function start(pkgs) {
  const {loadPyodide} = await import(PYODIDE + "pyodide.mjs");
  py = await loadPyodide({indexURL: PYODIDE});
  await py.loadPackage(PACKAGES);
  await py.loadPackage("micropip");
  await py.runPythonAsync(`import micropip\nawait micropip.install(${JSON.stringify(PYPI)}, deps=False)`);
  py.unpackArchive(pkgs, "zip", {extractDir: "/home/pyodide/pkgs"});
  await py.runPythonAsync(`import sys\nsys.path.insert(0, "/home/pyodide/pkgs")`);
}

onmessage = async ({data}) => {
  try {
    if (!py) {
      postMessage({step: "loading Python, roadstyle and mapstyle (the first time about 30 s)"});
      await start(data.pkgs);
    }
    postMessage({step: "making the map of " + data.name});
    py.FS.writeFile("/home/pyodide/db.duckdb", new Uint8Array(data.db));
    py.globals.set("rs_name", data.name);
    const style = await py.runPythonAsync(`import json
from mapstyle.export import export_inline
json.dumps(export_inline("/home/pyodide/db.duckdb", name=rs_name), separators=(",", ":"))`);
    py.FS.unlink("/home/pyodide/db.duckdb");
    postMessage({style});
  } catch (e) {
    postMessage({error: String(e && e.message || e).slice(-3000)});
  }
};
