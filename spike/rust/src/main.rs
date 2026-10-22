//! Spike (docs/design/rust-spike.md): the road part of the style, natively, with the time of each step.
//!
//!     cargo run --release -- spike/data/look.json spike/data/layers.json spike/data/roads.parquet made.json

use roadstyle_spike::{read, roads, simple};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
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

    let ms = |a: Duration, b: Duration| (b - a).as_secs_f64() * 1e3;
    let t = Instant::now();
    let mut feats = read::read_roads(data, "geometry").unwrap_or_else(|e| fail(format!("{}: {e}", args[3])));
    let t_read = t.elapsed();
    roads::mark_roads(&mut feats, &look);
    let roads = roads::roads_source(&feats);
    let t_roads = t.elapsed();
    let simple = simple::simple_source(&feats, look.head_m, look.bridge_shadow).unwrap_or_else(|e| fail(e));
    let t_simple = t.elapsed();
    let text = json!({"layers": layers, "sources": {"roads": roads, "simple": simple}}).to_string();
    let t_json = t.elapsed();
    fs::write(&args[4], &text).unwrap_or_else(|e| fail(format!("{}: {e}", args[4])));
    eprintln!("read {:.0} ms, roads {:.0} ms, simple {:.0} ms, JSON {:.0} ms ({:.1} MB)", ms(Duration::ZERO, t_read),
              ms(t_read, t_roads), ms(t_roads, t_simple), ms(t_simple, t_json), text.len() as f64 / 1e6);
}
