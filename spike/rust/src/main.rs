//! Spike and full port (docs/design/rust-spike.md): the road part of the style, natively, with the time of each step.
//!
//!     cargo run --release -- spike/data/look.json spike/data/roads.parquet spike/data/features.parquet spike/data/planner.parquet made.json
//!     (- for no features or no planner)
//!     cargo run --release -- --layers spike/data/look.json spike/data/layers/all/roads.parquet     # the road layers alone, as {"layers": [...]}

use roadstyle_spike::{decks, layers, overlays, planner, read, roads, simple, slots};
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
    if args.len() == 4 && args[1] == "--features" {
        // step 3 check: the base-map feature collections from the worker's features.parquet, as {name: FeatureCollection}
        let data = fs::read(&args[3]).unwrap_or_else(|e| fail(format!("{}: {e}", args[3])));
        let fcs = roadstyle_spike::features::feature_collections(data, &json_file(&args[2])).unwrap_or_else(|e| fail(e));
        println!("{}", serde_json::to_string(&fcs).unwrap());
        return;
    }
    let layers_only = args.len() == 4 && args[1] == "--layers";
    if !layers_only && args.len() != 6 {
        fail("usage: roadstyle-spike <look.json> <roads.parquet> <features.parquet | -> <planner.parquet | -> <out.json>  |  \
              roadstyle-spike --layers <look.json> <roads.parquet>".into());
    }
    let (look_path, roads_path) = if layers_only { (&args[2], &args[3]) } else { (&args[1], &args[2]) };
    let look_json = json_file(look_path);
    let look = roads::Look::from_json(&look_json).unwrap_or_else(|e| fail(e));
    let slot_look = slots::SlotLook::from_json(&look_json).unwrap_or_else(|e| fail(e));
    let data = fs::read(roads_path).unwrap_or_else(|e| fail(format!("{roads_path}: {e}")));
    let opt = |i: usize| if layers_only || args[i] == "-" { None } else { Some(fs::read(&args[i]).unwrap_or_else(|e| fail(format!("{}: {e}", args[i])))) };
    let (fdata, pdata) = (opt(3), opt(4));

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
    let (mut ovs, mut images) = (Vec::new(), serde_json::Map::new());
    if let Some(f) = fdata {
        let mut fcs = roadstyle_spike::features::feature_collections(f, &look_json).unwrap_or_else(|e| fail(e));
        let attached = overlays::edge_attach(&mut fcs, &feats);
        (ovs, images) = overlays::feature_overlays(fcs, &look_json, &attached);
    }
    let t_base = t.elapsed();
    // the decks: as the plugin's export (view_3d=True); not in --layers, whose golden layers are made with view_3d=False
    let model = layers::Model::from_look(&look_json).unwrap_or_else(|e| fail(e));
    let (fc, bent) = if layers_only { (json!({"type": "FeatureCollection", "features": []}), 0) } else { decks::bridge_decks(&feats, &look_json, &model) };
    let decks = json!({"type": "geojson", "data": fc, "generateId": true});
    let t_decks = t.elapsed();
    let layers = layers::road_layers(&look_json, features(&roads), features(&simple), features(&slots), &mut ovs, features(&decks)).unwrap_or_else(|e| fail(e));
    let t_layers = t.elapsed();
    if layers_only {
        println!("{}", json!({"layers": layers}));
        return;
    }
    let n_decks = features(&decks).len();
    let mut sources = serde_json::Map::new();
    sources.insert("roads".into(), roads);
    if n_decks > 0 {
        sources.insert("decks".into(), decks);
    }
    for (i, ov) in ovs.into_iter().enumerate() {
        sources.insert(format!("ov{i}"), json!({"type": "geojson", "data": ov.fc, "generateId": true}));
    }
    sources.insert("simple".into(), simple);
    sources.insert("slots".into(), slots);
    images.insert("oneway".into(), look_json["oneway_image"].clone());
    let mut metadata = serde_json::Map::new();
    metadata.insert("roadstyle:images".into(), Value::Object(images));
    if let Some(p) = pdata {
        if feats.len() > planner::PLANNER_MAX_EDGES {
            metadata.insert("roadstyle:planner_skipped".into(), json!(feats.len()));
        } else {
            metadata.insert("roadstyle:planner".into(), planner::planner_data(&feats, p).unwrap_or_else(|e| fail(e)));
        }
    }
    let t_planner = t.elapsed();
    let text = json!({"layers": layers, "sources": sources, "metadata": metadata}).to_string();
    let t_json = t.elapsed();
    fs::write(&args[5], &text).unwrap_or_else(|e| fail(format!("{}: {e}", args[5])));
    eprintln!("read {:.0} ms, roads {:.0} ms, simple {:.0} ms, slots {:.0} ms, base map {:.0} ms, decks {:.0} ms ({n_decks}, {bent} bent parts), layers {:.0} ms, \
               planner {:.0} ms, JSON {:.0} ms ({:.1} MB)", ms(Duration::ZERO, t_read), ms(t_read, t_roads), ms(t_roads, t_simple), ms(t_simple, t_slots),
              ms(t_slots, t_base), ms(t_base, t_decks), ms(t_decks, t_layers), ms(t_layers, t_planner), ms(t_planner, t_json), text.len() as f64 / 1e6);
}
