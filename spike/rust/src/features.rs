//! The base-map layers' features (full port step 3): mapstyle.map.load_layers from the rows the worker's SQL writes (features.parquet: one row
//! per feature with its layer, osm_id, kind, name, bearing, the height tags and the WKB), as `{layer: FeatureCollection}` in draw order.

use crate::geom::{from_wkb, Geom};
use crate::read::read_table;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};

/// _number: the number at the start of a tag value ("12 m" -> 12.0).
fn number(text: Option<&str>) -> Option<f64> {
    let t = text?.trim_start();
    let int = t.bytes().take_while(u8::is_ascii_digit).count();
    if int == 0 {
        return None;
    }
    let mut end = int;
    if t.as_bytes().get(int) == Some(&b'.') {
        let frac = t[int + 1..].bytes().take_while(u8::is_ascii_digit).count();
        if frac > 0 {
            end = int + 1 + frac;
        }
    }
    t[..end].parse().ok()
}

/// building_height: building:levels x m_per_level, else the height tag from min_height, else the height of the kind.
fn building_height(levels: Option<&str>, height: Option<&str>, kind: Option<&str>, cfg: &Value) -> Value {
    let lv = number(levels);
    if let Some(l) = lv.filter(|l| *l != 0.0) {
        return json!(l * cfg["m_per_level"].as_f64().unwrap());
    }
    if let Some(h) = number(height).filter(|h| *h >= cfg["min_height"].as_f64().unwrap()) {
        return json!(h);
    }
    let hs = &cfg["heights"];
    kind.and_then(|k| hs.get(k)).unwrap_or(&hs["default"]).clone()
}

/// load_layers: `{name: FeatureCollection}`, in the order of the first row of each layer; a polygon layer's features largest first.
pub fn feature_collections(parquet: Vec<u8>, look: &Value) -> Result<Map<String, Value>, String> {
    let rows = read_table(parquet, "geometry")?;
    let specs = look["features"]["layers_yaml"]["layers"].as_array().ok_or("look.json: no features.layers_yaml.layers")?;
    let kind_of = |name: &str| specs.iter().find(|s| s["name"] == name).and_then(|s| s["kind"].as_str()).unwrap_or("");
    let b3d = &look["features"]["buildings_3d"];
    let mut out: IndexMap<String, Vec<(f64, Value)>> = IndexMap::new();
    for (r, wkb) in rows {
        let spec = &specs[r["spec"].as_u64().ok_or("features.parquet: no spec")? as usize];
        let Some(g) = wkb.as_deref().and_then(from_wkb) else { return Err(format!("features.parquet: a geometry of {} is not read", r["layer"])) };
        let area = g.area();
        let g = if spec["centroid"] == true { Geom::Point(g.centroid()) } else { g };
        let mut props = Map::new();
        props.insert("osm_id".into(), r["osm_id"].clone());
        props.insert("kind".into(), r["kind"].clone());
        props.insert("name".into(), r["name"].clone());
        if !r["bearing"].is_null() {
            props.insert("bearing".into(), r["bearing"].clone());
        }
        if spec["height"] == true {
            props.insert("height".into(), building_height(r["levels"].as_str(), r["height"].as_str(), r["kind"].as_str(), b3d));
        }
        let name = r["layer"].as_str().unwrap_or_default().to_string();
        out.entry(name).or_default().push((area, json!({"type": "Feature", "properties": props, "geometry": g.geojson()})));
    }
    Ok(out.into_iter().map(|(name, mut fs)| {
        if kind_of(&name) == "polygon" {
            fs.sort_by(|a, b| b.0.total_cmp(&a.0)); // stable: largest first, ties in reading order
        }
        let features: Vec<Value> = fs.into_iter().map(|(_, f)| f).collect();
        (name, json!({"type": "FeatureCollection", "features": features}))
    }).collect())
}
