//! Spike (docs/design/rust-spike.md): the road part of the roadstyle style, from the roads in a GeoParquet file. The same function
//! runs natively (main.rs) and in the browser (`makeStyle`, the wasm32 build, through wasm-bindgen).

pub mod layers;
pub mod read;
pub mod roads;
pub mod simple;
pub mod slots;

use serde_json::{json, Value};

/// The style's road layers and its "roads", "simple" and "slots" sources, as JSON text. `look`: look.json (the resolved look);
/// `parquet`: the roads as tools/make_spike_fixture.py (or the worker's SQL) writes them.
pub fn make_style(parquet: Vec<u8>, look: &str) -> Result<String, String> {
    let look_json: Value = serde_json::from_str(look).map_err(|e| format!("look: not JSON: {e}"))?;
    let look = roads::Look::from_json(&look_json)?;
    let mut feats = read::read_roads(parquet, "geometry")?;
    roads::mark_roads(&mut feats, &look);
    let roads = roads::roads_source(&feats);
    let simple = simple::simple_source(&feats, look.head_m, look.bridge_shadow)?;
    let slots = slots::slots_source(&feats, &slots::SlotLook::from_json(&look_json)?);
    let layers = layers::road_layers(&look_json, features(&roads), features(&simple), features(&slots))?;
    Ok(json!({"layers": layers, "sources": {"roads": roads, "simple": simple, "slots": slots}}).to_string())
}

fn features(source: &Value) -> &[Value] {
    source["data"]["features"].as_array().map_or(&[][..], |v| v.as_slice())
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    /// makeStyle(parquet: Uint8Array, look: string) -> string; an error is a thrown Error with the message.
    #[wasm_bindgen(js_name = makeStyle)]
    pub fn make_style(parquet: Vec<u8>, look: &str) -> Result<String, JsError> {
        super::make_style(parquet, look).map_err(|e| JsError::new(&e))
    }
}
