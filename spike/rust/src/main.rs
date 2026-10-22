//! Spike (docs/design/rust-spike.md): the road part of the style, natively, with the time of each step.
//!
//!     cargo run --release -- spike/data/look.json spike/data/layers.json spike/data/roads.parquet made.json

use roadstyle_spike::{read, roads, simple, slots};
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
    if args.len() == 3 && args[1] == "--exprs" {
        // step 2 check: the builders on their own, for tools that compare them with the golden layers
        let m = roadstyle_spike::layers::Model::from_look(&json_file(&args[2])).unwrap_or_else(|e| fail(e));
        println!("{}", json!({"fw": m.width_expr("highway", false, 1.0), "cw": m.width_expr("highway", true, 1.0),
                              "off": m.offset_expr("highway", false), "off_pairs": m.offset_expr("highway", true), "sort": m.sort_key("highway")}));
        return;
    }
    if args.len() != 5 {
        fail("usage: roadstyle-spike <look.json> <layers.json> <roads.parquet> <out.json>".into());
    }
    let look_json = json_file(&args[1]);
    let look = roads::Look::from_json(&look_json).unwrap_or_else(|e| fail(e));
    let slot_look = slots::SlotLook::from_json(&look_json).unwrap_or_else(|e| fail(e));
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
    let slots = slots::slots_source(&feats, &slot_look);
    let t_slots = t.elapsed();
    let text = json!({"layers": layers, "sources": {"roads": roads, "simple": simple, "slots": slots}}).to_string();
    let t_json = t.elapsed();
    fs::write(&args[4], &text).unwrap_or_else(|e| fail(format!("{}: {e}", args[4])));
    eprintln!("read {:.0} ms, roads {:.0} ms, simple {:.0} ms, slots {:.0} ms, JSON {:.0} ms ({:.1} MB)", ms(Duration::ZERO, t_read),
              ms(t_read, t_roads), ms(t_roads, t_simple), ms(t_simple, t_slots), ms(t_slots, t_json), text.len() as f64 / 1e6);
}
