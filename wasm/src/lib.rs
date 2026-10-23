//! Spike (docs/design/rust-spike.md): the road part of the roadstyle style, from the roads in a GeoParquet file. The same function
//! runs natively (main.rs) and in the browser (`makeStyle`, the wasm32 build, through wasm-bindgen).

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

/// The most roads of one map: the style is made as JSON trees in the module's memory, which a browser keeps under 4 GB (Tartu, 63,965 roads:
/// 1.9 GB; docs/design/rust-spike.md).
pub const MAX_ROADS: usize = 100_000;

/// The style's layers and sources, as JSON text: the roads ("roads", "simple", "slots") and, with `features` (the base-map tables as the
/// worker's SQL writes them), the base map ("ov<i>"). `look`: look.json (the resolved look); `parquet`: the roads as
/// tools/make_spike_fixture.py (or the worker's SQL) writes them.
pub fn make_style(parquet: Vec<u8>, look: &str, features: Option<Vec<u8>>, planner: Option<Vec<u8>>, name: Option<String>) -> Result<String, String> {
    let look_json: Value = serde_json::from_str(look).map_err(|e| format!("look: not JSON: {e}"))?;
    let look = roads::Look::from_json(&look_json)?;
    let mut feats = read::read_roads(parquet, "geometry")?;
    if feats.len() > MAX_ROADS {
        return Err(format!("this area has {} roads, and the plugin draws at most {MAX_ROADS} (the browser's memory): use a smaller area",
                           feats.len()));
    }
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
    let layers = layers::road_layers(&look_json, source_features(&roads), source_features(&simple), source_features(&slots), &mut ovs)?;
    let glyphs = !source_features(&slots).is_empty();
    // The sources become text now, one at a time, and each tree is freed at once: a big area's trees are the most of the memory (Tartu: the
    // peak went from 4.3 GB to 2.5 GB natively). The text has the keys in the order of the style before: roads, ov<i>, simple, slots.
    let mut sources = vec![("roads".to_string(), roads.to_string())];
    drop(roads);
    for (i, ov) in ovs.into_iter().enumerate() {
        let mut s = serde_json::Map::new();
        s.insert("type".into(), "geojson".into());
        s.insert("data".into(), ov.fc);
        s.insert("generateId".into(), true.into());
        sources.push((format!("ov{i}"), Value::Object(s).to_string()));
    }
    sources.push(("simple".into(), simple.to_string()));
    drop(simple);
    sources.push(("slots".into(), slots.to_string()));
    drop(slots);
    images.insert("oneway".into(), look_json["oneway_image"].clone());
    let mut metadata = serde_json::Map::new();
    // the export's page metadata: the road popup's fields, the hover tooltip and the hover delay (roadstyle:interaction)
    let it = &look_json["interaction"];
    metadata.insert("roadstyle:interaction".into(), json!({"popup": it["popup"], "tooltip": look_json["args"]["tooltip"], "hover_delay_ms": it["hover_delay_ms"]}));
    metadata.insert("roadstyle:images".into(), Value::Object(images));
    if let Some(p) = planner {          // export_inline(planner=True): the route planner's graphs, or the number of roads past the limit
        if feats.len() > planner::PLANNER_MAX_EDGES {
            metadata.insert("roadstyle:planner_skipped".into(), json!(feats.len()));
        } else {
            metadata.insert("roadstyle:planner".into(), planner::planner_data(&feats, p)?);
        }
    }
    let (center, zoom) = camera(&feats);
    drop(feats);
    let name = name.map_or_else(|| look_json["args"]["name"].clone(), Value::from);
    let metadata = Value::Object(metadata).to_string();
    let mut out = String::with_capacity(sources.iter().map(|(_, s)| s.len() + 16).sum::<usize>() + metadata.len() + (1 << 20));
    out += &format!(r#"{{"version":8,"name":{name},"center":{center},"zoom":{zoom},"layers":{},"sources":{{"#, Value::Array(layers));
    for (i, (k, s)) in sources.into_iter().enumerate() {
        out += &format!("{}{}:", if i > 0 { "," } else { "" }, Value::from(k));
        out += &s;
    }
    out += r#"},"metadata":"#;
    out += &metadata;
    if glyphs {
        out += &format!(r#","glyphs":{}"#, look_json["theme"]["labels"]["glyphs"]);      // the names' font (roadstyle: when there are labels)
    }
    out.push('}');
    Ok(out)
}

/// roadstyle.export._camera: the centre and the zoom that fit the roads' bounds in a 1024 x 768 px view, as the page's fitBounds.
fn camera(feats: &[read::Feature]) -> (Value, Value) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in feats.iter().filter_map(|f| f.coords.as_ref()).flatten() {
        x0 = x0.min(c[0]);
        y0 = y0.min(c[1]);
        x1 = x1.max(c[0]);
        y1 = y1.max(c[1]);
    }
    let y = |lat: f64| (std::f64::consts::PI / 4.0 + lat.clamp(-85.0, 85.0).to_radians() / 2.0).tan().ln();
    let zx = (1024.0 / 256.0 * 360.0 / (x1 - x0).max(1e-9)).log2();
    let zy = (768.0 / 256.0 * 2.0 * std::f64::consts::PI / (y(y1) - y(y0)).max(1e-9)).log2();
    (json!([(x0 + x1) / 2.0, (y0 + y1) / 2.0]), json!(layers::py_round(zx.min(zy).clamp(0.0, 18.0), 2)))
}

/// A JSON object made by moving its values in. json!({..}) serializes each value by reference: a deep copy of a big tree (a source's features).
pub(crate) fn obj<const N: usize>(kv: [(&str, Value); N]) -> Value {
    Value::Object(kv.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn source_features(source: &Value) -> &[Value] {
    source["data"]["features"].as_array().map_or(&[][..], |v| v.as_slice())
}

/// The allocator in the browser: the standard one (dlmalloc), but when it grows the memory, the memory grows once more by its size (16 to
/// 256 MB) in one step, and the allocator keeps that block free for the next allocations. dlmalloc alone grows the memory 64 KB at a time,
/// thousands of steps for Monaco, and in Edge on Windows each step is slow: the first style took 14 to 21 s there, 3 s with the memory grown first.
#[cfg(target_arch = "wasm32")]
mod grow {
    use core::arch::wasm32::memory_size;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    static PAGES: AtomicUsize = AtomicUsize::new(0); // the memory's size (64 KB pages) after the last step ahead

    fn ahead() {
        let now = memory_size(0);
        if now != PAGES.load(Relaxed) {
            let l = Layout::from_size_align((now << 16).clamp(16 << 20, 256 << 20), 16).unwrap();
            unsafe {
                let p = System.alloc(l); // null when the memory cannot grow: then nothing is taken ahead
                if !p.is_null() {
                    System.dealloc(p, l);
                }
            }
            PAGES.store(memory_size(0), Relaxed);
        }
    }

    struct Ahead;

    unsafe impl GlobalAlloc for Ahead {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            let p = System.alloc(l);
            ahead();
            p
        }
        unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
            let p = System.alloc_zeroed(l);
            ahead();
            p
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
            let q = System.realloc(p, l, size);
            ahead();
            q
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            System.dealloc(p, l)
        }
    }

    #[global_allocator]
    static ALLOC: Ahead = Ahead;
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    /// makeStyle(roads: Uint8Array, look: string, features?: Uint8Array, planner?: Uint8Array, name?: string) -> string; an error is a thrown Error.
    #[wasm_bindgen(js_name = makeStyle)]
    pub fn make_style(parquet: Vec<u8>, look: &str, features: Option<Vec<u8>>, planner: Option<Vec<u8>>, name: Option<String>) -> Result<String, JsError> {
        super::make_style(parquet, look, features, planner, name).map_err(|e| JsError::new(&e))
    }

    /// The size of the module's memory, in bytes.
    #[wasm_bindgen(js_name = memoryBytes)]
    pub fn memory_bytes() -> usize {
        core::arch::wasm32::memory_size(0) * 65536
    }
}
