//! Spike and full port (docs/design/rust-spike.md): the road part of the style, natively, with the time of each step.
//!
//!     cargo run --release -- spike/data/look.json spike/data/roads.parquet spike/data/features.parquet spike/data/planner.parquet made.json
//!     (- for no features or no planner)
//!     cargo run --release -- --layers spike/data/look.json spike/data/layers/all/roads.parquet     # the road layers alone, as {"layers": [...]}

use roadstyle_wasm::{layers, read, roads, simple, slots};
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

fn features(source: &Value) -> &[Value] {
    source["data"]["features"].as_array().map_or(&[][..], |v| v.as_slice())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() == 4 && args[1] == "--features" {
        // step 3 check: the base-map feature collections from the worker's features.parquet, as {name: FeatureCollection}
        let data = fs::read(&args[3]).unwrap_or_else(|e| fail(format!("{}: {e}", args[3])));
        let fcs = roadstyle_wasm::features::feature_collections(data, &json_file(&args[2])).unwrap_or_else(|e| fail(e));
        println!("{}", serde_json::to_string(&fcs).unwrap());
        return;
    }
    let layers_only = args.len() == 4 && args[1] == "--layers";
    if !layers_only && args.len() != 6 {
        fail("usage: roadstyle-wasm <look.json> <roads.parquet> <features.parquet | -> <planner.parquet | -> <out.json>  |  \
              roadstyle-wasm --layers <look.json> <roads.parquet>".into());
    }
    let (look_path, roads_path) = if layers_only { (&args[2], &args[3]) } else { (&args[1], &args[2]) };
    let look_json = json_file(look_path);
    let look = roads::Look::from_json(&look_json).unwrap_or_else(|e| fail(e));
    let slot_look = slots::SlotLook::from_json(&look_json).unwrap_or_else(|e| fail(e));
    let data = fs::read(roads_path).unwrap_or_else(|e| fail(format!("{roads_path}: {e}")));
    let opt = |i: usize| if layers_only || args[i] == "-" { None } else { Some(fs::read(&args[i]).unwrap_or_else(|e| fail(format!("{}: {e}", args[i])))) };
    let (fdata, pdata) = (opt(3), opt(4));

    if !layers_only {
        // the whole style, as the worker makes it (make_style), with the total time
        let t = Instant::now();
        let text = roadstyle_wasm::make_style(data, &fs::read_to_string(look_path).unwrap(), fdata, pdata, None).unwrap_or_else(|e| fail(e));
        let dt = t.elapsed();
        fs::write(&args[5], &text).unwrap_or_else(|e| fail(format!("{}: {e}", args[5])));
        eprintln!("make_style {:.0} ms ({:.1} MB)", dt.as_secs_f64() * 1e3, text.len() as f64 / 1e6);
        return;
    }
    // --layers: the road layers alone, for the 12 subsets of tools/make_layer_goldens.py (no base map)
    let mut feats = read::read_roads(data, "geometry").unwrap_or_else(|e| fail(format!("{roads_path}: {e}")));
    roads::mark_roads(&mut feats, &look);
    let roads = roads::roads_source(&feats);
    let simple = simple::simple_source(&feats, look.head_m, look.bridge_shadow).unwrap_or_else(|e| fail(e));
    let slots = slots::slots_source(&feats, &slot_look);
    let layers = layers::road_layers(&look_json, features(&roads), features(&simple), features(&slots), &mut []).unwrap_or_else(|e| fail(e));
    println!("{}", json!({"layers": layers}));
}
