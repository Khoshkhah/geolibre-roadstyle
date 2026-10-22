//! Spike (docs/design/rust-spike.md): the road part of the style, from the roads in a GeoParquet file.
//!
//!     cargo run --release -- spike/data/look.json spike/data/layers.json spike/data/roads.parquet made.json
//!
//! S1: the layers come from Python (`layers.json`): they depend on the look and on flags of the data. S2: the "roads" source.

mod read;
mod roads;

use serde_json::{json, Value};
use std::time::Instant;
use std::{env, fs, process};

fn fail(msg: String) -> ! {
    eprintln!("{msg}");
    process::exit(1)
}

fn json_file(path: &str) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| fail(format!("{path}: {e}")));
    serde_json::from_str(&text).unwrap_or_else(|e| fail(format!("{path}: not JSON: {e}")))
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 {
        fail("usage: roadstyle-spike <look.json> <layers.json> <roads.parquet> <out.json>".into());
    }
    let look = roads::Look::from_json(&json_file(&args[1])).unwrap_or_else(|e| fail(e));
    let layers = json_file(&args[2]);
    let data = fs::read(&args[3]).unwrap_or_else(|e| fail(format!("{}: {e}", args[3])));

    let t = Instant::now();
    let feats = read::read_roads(data, "geometry").unwrap_or_else(|e| fail(format!("{}: {e}", args[3])));
    let t_read = t.elapsed();
    let source = roads::roads_source(feats, &look);
    let t_roads = t.elapsed();
    let text = json!({"layers": layers, "sources": {"roads": source}}).to_string();
    let t_json = t.elapsed();
    fs::write(&args[4], &text).unwrap_or_else(|e| fail(format!("{}: {e}", args[4])));
    eprintln!("read {:.0} ms, roads {:.0} ms, JSON {:.0} ms ({:.1} MB)", t_read.as_secs_f64() * 1e3,
              (t_roads - t_read).as_secs_f64() * 1e3, (t_json - t_roads).as_secs_f64() * 1e3, text.len() as f64 / 1e6);
}
