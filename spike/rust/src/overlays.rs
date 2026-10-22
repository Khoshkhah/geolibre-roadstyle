//! The base map's overlays (full port step 3): mapstyle.map.edge_attach and feature_overlays, then roadstyle's _edge_overlay and
//! _overlay_layers: the overlay sources and their layers before the placement and the passes (layers.rs).

use crate::read::Feature;
use crate::roads::truthy;
use crate::slots::{bbox, point_line_dist};
use rstar::{primitives::GeomWithData, primitives::Rectangle, RTree, AABB};
use serde_json::{json, Map, Value};

const EDGE_ATTACHED: [&str; 2] = ["crossings", "traffic_signals"];
const PATH_CLASSES: [&str; 7] = ["footway", "path", "cycleway", "steps", "pedestrian", "bridleway", "corridor"];
const CLASS_RANK: [&str; 9] = ["motorway", "trunk", "primary", "secondary", "tertiary", "unclassified", "residential", "living_street", "service"];

/// One overlay, as mapstyle's feature_overlays gives it to roadstyle (rs.Overlay), and its entry of the restyle's config (`l`).
pub struct Overlay {
    pub name: String,
    pub kind: &'static str, // fill | line | circle
    pub under: bool,
    pub edge: bool,
    pub color: Value,
    pub outline: Value,
    pub opacity: Value,
    pub width: Value,
    pub radius: Value,
    pub visible: bool,
    pub fc: Value,
    pub l: Value,
}

/// _band: below (-1 ...), on (0) or over (1 ...) the ground: the OSM layer if it is a number, else 1 for a bridge, -1 for a tunnel.
fn band(p: &Map<String, Value>) -> f64 {
    let layer = match p.get("layer") {
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    }
    .filter(|v| v.is_finite())
    .map_or(0.0, f64::trunc);
    let yes = |k: &str| match p.get(k) {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !matches!(s.as_str(), "" | "no" | "None" | "nan"),
        Some(_) => true,
    };
    if layer != 0.0 { layer } else if yes("bridge") { 1.0 } else if yes("tunnel") { -1.0 } else { 0.0 }
}

/// edge_attach: `edge_id` on every crossing and traffic signal: of the streets within 1e-5 deg, the one drawn on top (the highest fill
/// number, then band, then class); else the nearest street within 3e-5 deg (within 2e-6 of the nearest); else the nearest road.
/// Returns the names of the layers that got one.
pub fn edge_attach(fcs: &mut Map<String, Value>, roads: &[Feature]) -> Vec<String> {
    let n = roads.len();
    let klass: Vec<f64> = roads.iter().map(|r| {
        let h = r.props.get("highway").and_then(Value::as_str).unwrap_or("");
        let h = h.strip_suffix("_link").unwrap_or(h);
        CLASS_RANK.iter().position(|c| *c == h).map_or(99.0, |i| (i + 1) as f64)
    }).collect();
    let fill: Vec<f64> = roads.iter().map(|r| r.props.get("fill_level").and_then(Value::as_f64).unwrap_or(0.0)).collect();
    let bands: Vec<f64> = roads.iter().map(|r| band(&r.props)).collect();
    // np.lexsort((-klass, band, fill))[::-1]: by fill, then band, then -klass, ascending and stable; reversed
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| fill[a].total_cmp(&fill[b]).then(bands[a].total_cmp(&bands[b])).then((-klass[a]).total_cmp(&-klass[b])).then(a.cmp(&b)));
    let mut rank = vec![0usize; n];
    for (k, &i) in order.iter().rev().enumerate() {
        rank[i] = k;
    }
    let street: Vec<bool> = roads.iter().map(|r| !r.props.get("highway").and_then(Value::as_str).is_some_and(|h| PATH_CLASSES.contains(&h))).collect();
    let tree: RTree<GeomWithData<Rectangle<[f64; 2]>, usize>> = RTree::bulk_load(roads.iter().enumerate()
        .filter_map(|(i, r)| r.coords.as_ref().filter(|c| !c.is_empty()).map(|c| { let (lo, hi) = bbox(c); GeomWithData::new(Rectangle::from_corners(lo, hi), i) }))
        .collect());
    let dist = |p: [f64; 2], i: usize| roads[i].coords.as_ref().map_or(f64::INFINITY, |c| point_line_dist(p, c));
    let within = |p: [f64; 2], d: f64| -> Vec<usize> {
        let mut v: Vec<usize> = tree.locate_in_envelope_intersecting(&AABB::from_corners([p[0] - d, p[1] - d], [p[0] + d, p[1] + d]))
            .map(|g| g.data).filter(|&i| dist(p, i) <= d).collect();
        v.sort();
        v
    };
    let mut done = Vec::new();
    for name in EDGE_ATTACHED {
        let Some(feats) = fcs.get_mut(name).and_then(|fc| fc["features"].as_array_mut()).filter(|f| !f.is_empty()) else { continue };
        for f in feats.iter_mut() {
            let c = &f["geometry"]["coordinates"];
            let p = [c[0].as_f64().unwrap_or(0.0), c[1].as_f64().unwrap_or(0.0)];
            let mut hits: Vec<usize> = within(p, 1e-5).into_iter().filter(|&i| street[i]).collect();
            if hits.is_empty() {
                let near = within(p, 3e-5);
                hits = if near.iter().any(|&i| street[i]) { near.into_iter().filter(|&i| street[i]).collect() } else { near };
                if !hits.is_empty() {
                    let d: Vec<f64> = hits.iter().map(|&i| dist(p, i)).collect();
                    let min = d.iter().copied().fold(f64::INFINITY, f64::min);
                    hits = hits.into_iter().zip(d).filter(|(_, d)| *d <= min + 2e-6).map(|(i, _)| i).collect();
                }
            }
            let j = if hits.is_empty() {
                (0..n).min_by(|&a, &b| dist(p, a).total_cmp(&dist(p, b))).unwrap_or(0) // query_nearest
            } else {
                *hits.iter().min_by_key(|&&i| rank[i]).unwrap()
            };
            let id = match roads[j].props.get("edge_id") { Some(Value::String(s)) => s.clone(), Some(v) => v.to_string(), None => String::new() };
            f["properties"]["edge_id"] = id.into();
        }
        done.push(name.to_string());
    }
    done
}

/// A table of the style keyed by zoom (YAML numbers, text in JSON) as [[zoom, value], ...], the zooms as numbers.
fn zoom_pairs(t: &Value, f: impl Fn(f64) -> f64) -> Value {
    Value::Array(t.as_object().into_iter().flatten().map(|(z, v)| {
        let zf: f64 = z.parse().unwrap_or(0.0);
        let z = if zf.fract() == 0.0 { json!(zf as i64) } else { json!(zf) };
        json!([z, f(v.as_f64().unwrap_or(0.0))])
    }).collect())
}

/// feature_overlays: one overlay per layer (in the collections' order), its look from the theme's "features" style, and what the restyle adds
/// (colour by kind, outlines, textures, icons, zoom ranges, 3D); and the images of the layers there are.
pub fn feature_overlays(fcs: Map<String, Value>, look: &Value, edge_attached: &[String]) -> (Vec<Overlay>, Map<String, Value>) {
    let st = &look["features"]["style"];
    let specs = look["features"]["layers_yaml"]["layers"].as_array().cloned().unwrap_or_default();
    let all_images = look["features"]["images"].as_object().cloned().unwrap_or_default();
    let (areas, lines, points, icon) = (&st["areas"], &st["lines"], &st["points"], &st["icon"]);
    let mut out = Vec::new();
    let mut images = Map::new();
    for (name, fc) in fcs {
        let spec = specs.iter().find(|s| s["name"] == name.as_str()).cloned().unwrap_or(Value::Null);
        let kind = spec["kind"].as_str().unwrap_or("");
        let ov_kind = match kind { "polygon" => "fill", "line" => "line", _ => "circle" };
        let ticked = spec.get("ticked").map_or(true, |v| v != false);
        let mut l = json!({"label": name, "kind": ov_kind, "ticked": ticked});
        let mut ov = Overlay { name: name.clone(), kind: ov_kind, under: kind != "point", edge: edge_attached.contains(&name), color: Value::Null,
                               outline: Value::Null, opacity: Value::Null, width: Value::Null, radius: Value::Null, visible: ticked, fc: Value::Null, l: Value::Null };
        if name == "landcover" {
            let mut fills = areas["landcover"].as_object().cloned().unwrap_or_default();
            let default = fills.shift_remove("default").unwrap_or(Value::Null);
            ov.color = default.clone();
            ov.opacity = json!(1.0);
            ov.width = json!(1);
            l["color_by"] = json!({"map": fills, "default": default});
            l["outline_by"] = json!({"map": st["landcover_outline"], "default": "rgba(0,0,0,0)"});
            let mut pats = Map::new();
            for k in st["landcover_pattern"].as_object().into_iter().flatten().map(|(k, _)| k) {
                let id = format!("ms-pat-{k}");
                if let Some(img) = all_images.get(&id) {
                    images.insert(id.clone(), img.clone());
                }
                pats.insert(k.clone(), id.into());
            }
            l["patterns"] = Value::Object(pats);
        } else if kind == "polygon" {
            let a = areas.get(&name).cloned().unwrap_or(json!({}));
            ov.color = a.get("fill").cloned().unwrap_or(Value::Null);
            ov.outline = a.get("outline").cloned().unwrap_or(Value::Null);
            ov.opacity = a.get("opacity").cloned().unwrap_or(json!(1.0));
            ov.width = json!(0.5);
            l["min_zoom"] = a.get("min_zoom").cloned().unwrap_or(Value::Null);
            if fc["features"].get(0).is_some_and(|f| f["properties"].get("height").is_some()) {
                l["extrude"] = json!({"color": a.get("fill").cloned().unwrap_or(Value::Null), "opacity": st["buildings_3d"]["opacity"]});
            }
        } else if kind == "line" {
            let ln = lines.get(&name).cloned().unwrap_or(json!({}));
            ov.color = ln.get("color").cloned().unwrap_or(Value::Null);
            ov.width = ln.get("width").cloned().unwrap_or(Value::Null);
            l["dash"] = ln.get("dash").cloned().unwrap_or(Value::Null);
            // width_zoom: px by zoom (a road's width), the zooms in order
            l["width_zoom"] = match ln.get("width_zoom").and_then(Value::as_object).filter(|m| !m.is_empty()) {
                None => Value::Null,
                Some(m) => {
                    let mut pairs: Vec<(f64, &str, &Value)> = m.iter().map(|(z, w)| (z.parse::<f64>().unwrap_or(0.0), z.as_str(), w)).collect();
                    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
                    Value::Array(pairs.into_iter().map(|(z, _, w)| json!([if z.fract() == 0.0 { json!(z as i64) } else { json!(z) }, w])).collect())
                }
            };
        } else {
            let mut p = points["default"].as_object().cloned().unwrap_or_default();
            for (k, v) in points.get(&name).and_then(Value::as_object).into_iter().flatten() {
                p.insert(k.clone(), v.clone());
            }
            ov.color = p.get("color").cloned().unwrap_or(Value::Null);
            ov.radius = json!(4);
            l["min_zoom"] = p.get("min_zoom").cloned().unwrap_or(Value::Null);
            if p.get("icon").is_some_and(|v| truthy(Some(v))) {
                let id = format!("ms-icon-{name}");
                if let Some(img) = all_images.get(&id) {
                    images.insert(id.clone(), img.clone());
                }
                let size = p.get("size").and_then(Value::as_f64).unwrap_or(1.0);
                l["icon"] = json!({"image": id, "opacity": icon["opacity"], "size": zoom_pairs(&icon["size"], |px| px * size / 48.0)});
            }
        }
        ov.fc = fc;
        ov.l = l;
        out.push(ov);
    }
    (out, images)
}

/// _ov_hl: an interactive overlay's colour lights up on hover and select (feature-state).
fn ov_hl(base: &Value, hover: &Value, select: &Value) -> Value {
    json!(["case", ["boolean", ["feature-state", "select"], false], select, ["boolean", ["feature-state", "hover"], false], hover, base])
}

fn or(v: &Value, d: &Value) -> Value {
    if v.is_null() || v.as_str() == Some("") { d.clone() } else { v.clone() }
}

/// _overlay_layers: the layer(s) of one overlay source (a fill and its outline, a circle, or a line), interactive (a popup is set).
pub fn overlay_layers(sid: &str, ov: &Overlay, look: &Value) -> Vec<Value> {
    let (th, cfg) = (&look["theme"], &look["config"]);
    let mut c = th["overlays"].as_object().cloned().unwrap_or_default();
    for (k, v) in cfg["overlays"].as_object().into_iter().flatten() {
        c.insert(k.clone(), v.clone());
    }
    let (hover, select) = (&th["selection"]["hover"], &th["selection"]["select"]);
    let base = or(&ov.color, &c["color"]);
    let width = or(&ov.width, &c["width"]);
    let radius = or(&ov.radius, &c["radius"]);
    let col = ov_hl(&base, hover, select);
    let lay = json!({"visibility": if ov.visible { "visible" } else { "none" }});
    let op = |d: &str| or(&ov.opacity, &c[d]);
    match ov.kind {
        "fill" => vec![
            json!({"id": format!("{sid}-fill"), "type": "fill", "source": sid, "layout": lay, "paint": {"fill-color": col, "fill-opacity": op("fill_opacity")}}),
            json!({"id": format!("{sid}-outline"), "type": "line", "source": sid, "layout": lay,
                   "paint": {"line-color": or(&ov.outline, &base), "line-width": width, "line-opacity": c["outline_opacity"]}}),
        ],
        "circle" => vec![json!({"id": format!("{sid}-circle"), "type": "circle", "source": sid, "layout": lay,
                                "paint": {"circle-radius": radius, "circle-color": col, "circle-opacity": op("circle_opacity"),
                                          "circle-stroke-color": or(&ov.outline, &c["circle_stroke"]), "circle-stroke-width": or(&ov.width, &c["circle_stroke_width"])}})],
        _ => {
            let mut l = lay.as_object().unwrap().clone();
            l.insert("line-cap".into(), th["roads"]["cap"].clone()); // no overlay of the base map has a dash of its own (the restyle adds it)
            vec![json!({"id": format!("{sid}-line"), "type": "line", "source": sid, "layout": l,
                        "paint": {"line-color": col, "line-width": width, "line-opacity": op("line_opacity")}})]
        }
    }
}

/// _edge_overlay: on each feature of an overlay on a street, what it takes from its edge (`roads`: the roads source's properties, by edge id
/// as text); returns the (fill number, order) pairs, sorted.
pub fn edge_overlay(fc: &mut Value, roads: &std::collections::HashMap<String, &Map<String, Value>>) -> Result<Vec<(i64, i64)>, String> {
    let mut orders = std::collections::BTreeSet::new();
    for f in fc["features"].as_array_mut().into_iter().flatten() {
        let p = f["properties"].as_object_mut().unwrap();
        let e = match p.get("edge_id") { Some(Value::String(s)) => s.clone(), Some(v) => v.to_string(), None => String::new() };
        let r = roads.get(&e).ok_or_else(|| format!("an edge_id that is not an edge of the roads: {e}"))?;
        let fl = r.get("__rs_fl").and_then(Value::as_i64).unwrap_or(0);
        p.insert("__rs_edge".into(), r["__rs_edge"].clone());
        p.insert("__rs_fl".into(), fl.into());
        p.insert("__rs_ord".into(), 0.into());
        p.insert("__rs_cls".into(), r.get("highway").cloned().unwrap_or(Value::Null));
        p.insert("__rs_lvl".into(), r.get("lvl").cloned().unwrap_or(json!(0)));
        if r.get("__rs_tunnel").is_some_and(|v| truthy(Some(v))) {
            p.insert("__rs_tunnel".into(), true.into());
        }
        orders.insert((fl, 0));
    }
    Ok(orders.into_iter().collect())
}
