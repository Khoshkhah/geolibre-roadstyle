//! The base-map layers' features (full port step 3): mapstyle.map.load_layers from the rows the worker's SQL writes (features.parquet: one row
//! per feature with its layer, osm_id, kind, name, bearing, the height tags and the WKB), as `{layer: FeatureCollection}` in draw order.

use crate::geom::{from_wkb, Geom};
use crate::read::read_table;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};

/// _number: the number at the start of a tag value ("12 m" -> 12.0, "12,5" -> 12.5) and the rest of the text.
fn number(text: Option<&str>) -> Option<(f64, &str)> {
    let t = text?.trim_start_matches(|c: char| c.is_whitespace());
    let int = t.bytes().take_while(u8::is_ascii_digit).count();
    if int == 0 {
        return None;
    }
    let mut end = int;
    if matches!(t.as_bytes().get(int), Some(b'.' | b',')) {
        let frac = t[int + 1..].bytes().take_while(u8::is_ascii_digit).count();
        if frac > 0 {
            end = int + 1 + frac;
        }
    }
    Some((t[..end].replace(',', ".").parse().ok()?, &t[end..]))
}

/// _metres: a height tag in metres: the number, times 0.3048 when its unit is feet ("40 ft", "40'").
fn metres(text: Option<&str>) -> Option<f64> {
    let (h, rest) = number(text)?;
    let unit = rest.trim_start().to_lowercase();
    let feet = unit.starts_with("ft") || unit.starts_with("feet") || unit.starts_with("foot") || unit.starts_with('\'');
    Some(if feet { h * 0.3048 } else { h })
}

/// building_height: the height tag from min_height, else (building:levels + roof:levels) x m_per_level, else the height of the kind (the
/// order of OpenMapTiles); at most max_height.
fn building_height(height: Option<&str>, levels: Option<&str>, roof_levels: Option<&str>, kind: Option<&str>, cfg: &Value) -> Value {
    let top = cfg.get("max_height").and_then(Value::as_f64).unwrap_or(200.0);
    let cap = |v: f64| if v <= top { json!(v) } else { cfg.get("max_height").cloned().unwrap_or(json!(200)) };
    if let Some(h) = metres(height).filter(|h| *h >= cfg["min_height"].as_f64().unwrap()) {
        return cap(h);
    }
    if let Some(l) = number(levels).map(|n| n.0).filter(|l| *l != 0.0) {
        return cap((l + number(roof_levels).map_or(0.0, |n| n.0)) * cfg["m_per_level"].as_f64().unwrap());
    }
    let hs = &cfg["heights"];
    kind.and_then(|k| hs.get(k)).unwrap_or(&hs["default"]).clone()
}

/// building_base: the bottom of a building (part) for fill-extrusion-base: the min_height tag, else building:min_level x m_per_level, else 0.
fn building_base(min_height: Option<&str>, min_level: Option<&str>, cfg: &Value) -> Value {
    match metres(min_height).or_else(|| number(min_level).map(|n| n.0 * cfg["m_per_level"].as_f64().unwrap())) {
        Some(b) if b > 0.0 => json!(b.min(cfg.get("max_height").and_then(Value::as_f64).unwrap_or(200.0))),
        _ => json!(0),
    }
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
            props.insert("height".into(), building_height(r["height"].as_str(), r["levels"].as_str(), r["roof_levels"].as_str(), r["kind"].as_str(), b3d));
            props.insert("base".into(), building_base(r["min_height"].as_str(), r["min_level"].as_str(), b3d));
        }
        let name = r["layer"].as_str().unwrap_or_default().to_string();
        out.entry(name).or_default().push((area, crate::obj([("type", "Feature".into()), ("properties", Value::Object(props)), ("geometry", g.geojson())])));
    }
    Ok(out.into_iter().map(|(name, mut fs)| {
        if kind_of(&name) == "polygon" {
            fs.sort_by(|a, b| b.0.total_cmp(&a.0)); // stable: largest first, ties in reading order
        }
        let features: Vec<Value> = fs.into_iter().map(|(_, f)| f).collect();
        (name, crate::obj([("type", "FeatureCollection".into()), ("features", Value::Array(features))]))
    }).collect())
}
