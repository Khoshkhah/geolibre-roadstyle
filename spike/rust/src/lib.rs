//! Spike (docs/design/rust-spike.md): the road part of the roadstyle style, from the roads in a GeoParquet file. The same function
//! runs natively (main.rs) and in the browser (`makeStyle`, the wasm32 build, through wasm-bindgen).

pub mod decks;
pub mod features;
pub mod geom;
pub mod layers;
pub mod overlays;
pub mod planner;
pub mod read;
pub mod roads;
pub mod simple;
pub mod slots;

use serde_json::{json, Value};

/// The style's layers and sources, as JSON text: the roads ("roads", "simple", "slots") and, with `features` (the base-map tables as the
/// worker's SQL writes them), the base map ("ov<i>"). `look`: look.json (the resolved look); `parquet`: the roads as
/// tools/make_spike_fixture.py (or the worker's SQL) writes them.
pub fn make_style(parquet: Vec<u8>, look: &str, features: Option<Vec<u8>>, planner: Option<Vec<u8>>) -> Result<String, String> {
    let look_json: Value = serde_json::from_str(look).map_err(|e| format!("look: not JSON: {e}"))?;
    let look = roads::Look::from_json(&look_json)?;
    let mut feats = read::read_roads(parquet, "geometry")?;
    roads::mark_roads(&mut feats, &look);
    let roads = roads::roads_source(&feats);
    let simple = simple::simple_source(&feats, look.head_m, look.bridge_shadow)?;
    let slots = slots::slots_source(&feats, &slots::SlotLook::from_json(&look_json)?);
    let mut ovs = Vec::new();
    let mut images = serde_json::Map::new();
    if let Some(f) = features {
        let mut fcs = features::feature_collections(f, &look_json)?;
        let attached = overlays::edge_attach(&mut fcs, &feats);
        (ovs, images) = overlays::feature_overlays(fcs, &look_json, &attached);
    }
    // the 3D bridge decks: always, as the plugin's export makes them (roadstyle's export_inline: view_3d=True); the plugin hides them unless chosen
    let model = layers::Model::from_look(&look_json)?;
    let decks = json!({"type": "geojson", "data": decks::bridge_decks(&feats, &look_json, &model).0, "generateId": true});
    let deck_features = source_features(&decks).to_vec();
    let layers = layers::road_layers(&look_json, source_features(&roads), source_features(&simple), source_features(&slots), &mut ovs, &deck_features)?;
    let mut sources = serde_json::Map::new();
    sources.insert("roads".into(), roads);
    if !deck_features.is_empty() {
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
    if let Some(p) = planner {          // export_inline(planner=True): the route planner's graphs, or the number of roads past the limit
        if feats.len() > planner::PLANNER_MAX_EDGES {
            metadata.insert("roadstyle:planner_skipped".into(), json!(feats.len()));
        } else {
            metadata.insert("roadstyle:planner".into(), planner::planner_data(&feats, p)?);
        }
    }
    Ok(json!({"layers": layers, "sources": sources, "metadata": metadata}).to_string())
}

fn source_features(source: &Value) -> &[Value] {
    source["data"]["features"].as_array().map_or(&[][..], |v| v.as_slice())
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    /// makeStyle(roads: Uint8Array, look: string, features?: Uint8Array, planner?: Uint8Array) -> string; an error is a thrown Error.
    #[wasm_bindgen(js_name = makeStyle)]
    pub fn make_style(parquet: Vec<u8>, look: &str, features: Option<Vec<u8>>, planner: Option<Vec<u8>>) -> Result<String, JsError> {
        super::make_style(parquet, look, features, planner).map_err(|e| JsError::new(&e))
    }
}
