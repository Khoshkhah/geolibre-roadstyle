//! Spike and full port (docs/design/rust-spike.md): the road part of the style, natively, with the time of each step.
//!
//!     cargo run --release -- spike/data/look.json spike/data/roads.parquet made.json
//!     cargo run --release -- --layers spike/data/look.json spike/data/layers/all/roads.parquet     # the road layers alone, as {"layers": [...]}

use roadstyle_spike::{layers, read, roads, simple, slots};
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

fn features(source: &Value) -> &[Value] {
    source["data"]["features"].as_array().map_or(&[][..], |v| v.as_slice())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let layers_only = args.len() == 4 && args[1] == "--layers";
    if !layers_only && args.len() != 4 {
        fail("usage: roadstyle-spike <look.json> <roads.parquet> <out.json>  |  roadstyle-spike --layers <look.json> <roads.parquet>".into());
    }
    let (look_path, roads_path) = if layers_only { (&args[2], &args[3]) } else { (&args[1], &args[2]) };
    let look_json = json_file(look_path);
    let look = roads::Look::from_json(&look_json).unwrap_or_else(|e| fail(e));
    let slot_look = slots::SlotLook::from_json(&look_json).unwrap_or_else(|e| fail(e));
    let data = fs::read(roads_path).unwrap_or_else(|e| fail(format!("{roads_path}: {e}")));

    let ms = |a: Duration, b: Duration| (b - a).as_secs_f64() * 1e3;
    let t = Instant::now();
    let mut feats = read::read_roads(data, "geometry").unwrap_or_else(|e| fail(format!("{roads_path}: {e}")));
    let t_read = t.elapsed();
    roads::mark_roads(&mut feats, &look);
    let roads = roads::roads_source(&feats);
    let t_roads = t.elapsed();
    let simple = simple::simple_source(&feats, look.head_m, look.bridge_shadow).unwrap_or_else(|e| fail(e));
    let t_simple = t.elapsed();
    let slots = slots::slots_source(&feats, &slot_look);
    let t_slots = t.elapsed();
    let layers = layers::road_layers(&look_json, features(&roads), features(&simple), features(&slots)).unwrap_or_else(|e| fail(e));
    let t_layers = t.elapsed();
    if layers_only {
        println!("{}", json!({"layers": layers}));
        return;
    }
    let text = json!({"layers": layers, "sources": {"roads": roads, "simple": simple, "slots": slots}}).to_string();
    let t_json = t.elapsed();
    fs::write(&args[3], &text).unwrap_or_else(|e| fail(format!("{}: {e}", args[3])));
    eprintln!("read {:.0} ms, roads {:.0} ms, simple {:.0} ms, slots {:.0} ms, layers {:.0} ms, JSON {:.0} ms ({:.1} MB)", ms(Duration::ZERO, t_read),
              ms(t_read, t_roads), ms(t_roads, t_simple), ms(t_simple, t_slots), ms(t_slots, t_layers), ms(t_layers, t_json), text.len() as f64 / 1e6);
}
