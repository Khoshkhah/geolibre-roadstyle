// The plugin's Pyodide setup (py/worker.js: the same CDN version, the same packages, pkgs.zip), with rs_profile.py run inside.
const PYODIDE = "https://cdn.jsdelivr.net/pyodide/v314.0.7/full/";
const PACKAGES = ["numpy", "pandas", "shapely", "pyproj", "geopandas", "pyyaml", "duckdb", "jinja2", "pillow"];

onmessage = async ({data}) => {
  try {
    const t0 = performance.now();
    const {loadPyodide} = await import(PYODIDE + "pyodide.mjs");
    const py = await loadPyodide({indexURL: PYODIDE});
    const t1 = performance.now();
    await py.loadPackage(PACKAGES);
    const t2 = performance.now();
    py.unpackArchive(data.pkgs, "zip", {extractDir: "/home/pyodide/pkgs"});
    py.FS.writeFile("/home/pyodide/db.duckdb", new Uint8Array(data.db));
    py.FS.writeFile("/home/pyodide/rs_profile.py", data.profile);
    await py.runPythonAsync(`import sys\nsys.path.insert(0, "/home/pyodide/pkgs")\nsys.path.insert(0, "/home/pyodide")\nimport rs_profile`);
    const t3 = performance.now();
    const runs = [];
    for (let i = 0; i < 2; i++) {       // twice: the second run has every module imported and warm
      const out = await py.runPythonAsync(`import json\njson.dumps(rs_profile.profile("/home/pyodide/db.duckdb", planner=True))`);
      runs.push(JSON.parse(out));
    }
    postMessage({pyodide_start_s: (t1 - t0) / 1000, packages_s: (t2 - t1) / 1000, import_s: (t3 - t2) / 1000, runs});
  } catch (e) {
    postMessage({error: String(e && (e.message || e)).slice(-4000)});
  }
};
