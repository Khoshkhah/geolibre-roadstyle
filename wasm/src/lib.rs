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
pub mod tiles;

use serde_json::{json, Value};

/// The most roads of one map: the style is made as JSON trees in the module's memory, which a browser keeps under 4 GB (Tartu, 63,965 roads:
/// 1.9 GB; docs/design/rust-spike.md).
pub const MAX_ROADS: usize = 100_000;

/// The parts of a style: its layers, its sources in their order (roads, ov<i>, simple, slots), its metadata and its camera.
pub struct Parts {
    pub name: Value,
    pub center: Value,
    pub zoom: Value,
    pub layers: Vec<Value>,
    pub sources: Vec<(String, Value)>,
    pub metadata: Value,
    /// The names' font, when there are labels.
    pub glyphs: Option<Value>,
}

/// The style's layers and sources: the roads ("roads", "simple", "slots") and, with `features` (the base-map tables as the worker's SQL
/// writes them), the base map ("ov<i>"). `look`: look.json (the resolved look); `parquet`: the roads as tools/make_spike_fixture.py (or
/// the worker's SQL) writes them.
pub fn make_parts(parquet: Vec<u8>, look: &str, features: Option<Vec<u8>>, planner: Option<Vec<u8>>, name: Option<String>) -> Result<Parts, String> {
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
    let glyphs = (!source_features(&slots).is_empty()).then(|| look_json["theme"]["labels"]["glyphs"].clone());
    let mut sources = vec![("roads".to_string(), roads)];
    for (i, ov) in ovs.into_iter().enumerate() {
        let mut s = serde_json::Map::new();
        s.insert("type".into(), "geojson".into());
        s.insert("data".into(), ov.fc);
        s.insert("generateId".into(), true.into());
        sources.push((format!("ov{i}"), Value::Object(s)));
    }
    sources.push(("simple".into(), simple));
    sources.push(("slots".into(), slots));
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
    let name = name.map_or_else(|| look_json["args"]["name"].clone(), Value::from);
    Ok(Parts { name, center, zoom, layers, sources, metadata: Value::Object(metadata), glyphs })
}

/// The style as one JSON text, with each source's GeoJSON in it (roadstyle's export_inline).
pub fn make_style(parquet: Vec<u8>, look: &str, features: Option<Vec<u8>>, planner: Option<Vec<u8>>, name: Option<String>) -> Result<String, String> {
    let Parts { name, center, zoom, layers, sources, metadata, glyphs } = make_parts(parquet, look, features, planner, name)?;
    // The sources become text one at a time, and each tree is freed at once: a big area's trees are the most of the memory (Tartu: the
    // peak went from 4.3 GB to 2.5 GB natively).
    let metadata = metadata.to_string();
    let mut out = format!(r#"{{"version":8,"name":{name},"center":{center},"zoom":{zoom},"layers":{},"sources":{{"#, Value::Array(layers));
    for (i, (k, s)) in sources.into_iter().enumerate() {
        out += &format!("{}{}:", if i > 0 { "," } else { "" }, Value::from(k));
        out += &s.to_string();
    }
    out += r#"},"metadata":"#;
    out += &metadata;
    if let Some(g) = glyphs {
        out += &format!(r#","glyphs":{g}"#);
    }
    out.push('}');
    Ok(out)
}

/// The style with vector-tile sources (`<prefix><source>/{z}/{x}/{y}`), and the tiles to answer them with (tiles.rs). The layers,
/// the metadata and the camera are those of `make_style`; each layer gets `source-layer`, the name of its source. The tiles keep only
/// the properties that are read: the texts in the layers (all their keys, and more), the popup and tooltip fields of the metadata, the
/// overlays' popup fields (name, kind) and `keep` (the caller's own, for example its popup's).
pub fn make_tiled(parquet: Vec<u8>, look: &str, features: Option<Vec<u8>>, name: Option<String>, prefix: &str, keep: &[String])
                  -> Result<(String, tiles::Tiles), String> {
    let Parts { name, center, zoom, mut layers, sources, metadata, glyphs } = make_parts(parquet, look, features, None, name)?;
    let mut read: std::collections::HashSet<String> = keep.iter().cloned().chain(["name".to_string(), "kind".to_string()]).collect();
    fn texts(v: &Value, out: &mut std::collections::HashSet<String>) {
        match v {
            Value::String(s) => {
                out.insert(s.clone());
            }
            Value::Array(a) => a.iter().for_each(|x| texts(x, out)),
            Value::Object(o) => o.values().for_each(|x| texts(x, out)),
            _ => {}
        }
    }
    layers.iter().for_each(|l| texts(l, &mut read));
    texts(&metadata["roadstyle:interaction"], &mut read);
    let mut tiled = tiles::Tiles::default();
    let mut vector = serde_json::Map::new();
    for (k, s) in sources {
        tiled.add(&k, s, &read); // frees the source's tree
        vector.insert(k.clone(), json!({"type": "vector", "tiles": [format!("{prefix}{k}/{{z}}/{{x}}/{{y}}")], "minzoom": 0,
                                        "maxzoom": tiles::MAX_ZOOM}));
    }
    for l in layers.iter_mut() {
        if let Some(src) = l.get("source").cloned() {
            l["source-layer"] = src;
        }
    }
    let mut style = json!({"version": 8, "name": name, "center": center, "zoom": zoom, "layers": layers, "sources": vector, "metadata": metadata});
    if let Some(g) = glyphs {
        style["glyphs"] = g;
    }
    Ok((style.to_string(), tiled))
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

    /// A map with vector tiles: `new Tiler(roads, look, features?, name?, prefix, keep)`, then `style()` (the style's JSON text) and
    /// `tile(source, z, x, y)` (a vector tile, empty when nothing is in it) for each request of the map, and `props(source, id)`.
    #[wasm_bindgen]
    pub struct Tiler {
        style: String,
        tiles: crate::tiles::Tiles,
    }

    #[wasm_bindgen]
    impl Tiler {
        #[wasm_bindgen(constructor)]
        pub fn new(parquet: Vec<u8>, look: &str, features: Option<Vec<u8>>, name: Option<String>, prefix: &str, keep: Vec<String>)
                   -> Result<Tiler, JsError> {
            let (style, tiles) = super::make_tiled(parquet, look, features, name, prefix, &keep).map_err(|e| JsError::new(&e))?;
            Ok(Tiler { style, tiles })
        }

        pub fn style(&self) -> String {
            self.style.clone()
        }

        pub fn tile(&self, source: &str, z: u32, x: u32, y: u32) -> Vec<u8> {
            self.tiles.tile(source, z, x, y)
        }

        /// The properties of the feature `id` of a source, as JSON (`null` when there is none).
        pub fn props(&self, source: &str, id: u32) -> String {
            self.tiles.props(source, id as u64)
        }
    }

    /// The size of the module's memory, in bytes.
    #[wasm_bindgen(js_name = memoryBytes)]
    pub fn memory_bytes() -> usize {
        core::arch::wasm32::memory_size(0) * 65536
    }
}
