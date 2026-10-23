// Spike S4 (docs/design/rust-spike.md): the Rust style in a module worker, as the plugin's py/worker.js runs Pyodide.
// Message in: {parquet: ArrayBuffer, look: string, runs: number}. Out: the times and the style of the last run.
import init, {makeStyle} from "./pkg/roadstyle_wasm.js";

onmessage = async ({data}) => {
  try {
    const t0 = performance.now();
    await init();                                   // fetch, compile and start the .wasm
    const initMs = performance.now() - t0;
    const runs = [];
    let style = null;
    for (let i = 0; i < data.runs; i++) {
      const t = performance.now();
      style = makeStyle(new Uint8Array(data.parquet), data.look);
      runs.push(performance.now() - t);
    }
    const t = performance.now();
    JSON.parse(style);                              // what the plugin does next with the text
    postMessage({initMs, runs, parseMs: performance.now() - t, style});
  } catch (e) {
    postMessage({error: String(e && (e.message || e))});
  }
};
