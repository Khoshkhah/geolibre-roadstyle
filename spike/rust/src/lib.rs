//! Spike (docs/design/rust-spike.md): the road part of the roadstyle style, from the roads in a GeoParquet file. The same function
//! runs natively (main.rs) and in the browser (`makeStyle`, the wasm32 build, through wasm-bindgen).

pub mod read;
pub mod roads;
pub mod simple;
pub mod slots;

use serde_json::{json, Value};

/// The style's layers and its "roads" and "simple" sources, as JSON text. `look`: look.json (the resolved look); `layers`: the layers
/// from Python (S1: they depend on flags of the data); `parquet`: the roads as tools/make_spike_fixture.py writes them.
pub fn make_style(parquet: Vec<u8>, look: &str, layers: &str) -> Result<String, String> {
    let look_json: Value = serde_json::from_str(look).map_err(|e| format!("look: not JSON: {e}"))?;
    let look = roads::Look::from_json(&look_json)?;
    let layers: Value = serde_json::from_str(layers).map_err(|e| format!("layers: not JSON: {e}"))?;
    let mut feats = read::read_roads(parquet, "geometry")?;
    roads::mark_roads(&mut feats, &look);
    let roads = roads::roads_source(&feats);
    let simple = simple::simple_source(&feats, look.head_m, look.bridge_shadow)?;
    let slots = slots::slots_source(&feats, &slots::SlotLook::from_json(&look_json)?);
    Ok(json!({"layers": layers, "sources": {"roads": roads, "simple": simple, "slots": slots}}).to_string())
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    /// makeStyle(parquet: Uint8Array, look: string, layers: string) -> string; an error is a thrown Error with the message.
    #[wasm_bindgen(js_name = makeStyle)]
    pub fn make_style(parquet: Vec<u8>, look: &str, layers: &str) -> Result<String, JsError> {
        super::make_style(parquet, look, layers).map_err(|e| JsError::new(&e))
    }
}
