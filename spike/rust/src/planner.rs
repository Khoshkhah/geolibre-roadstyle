//! The route planner's data (full port step 4): mapstyle.map.planner_data and _planner_ends, from the roads (in load_roads' order) and the
//! tables the worker's SQL writes (planner.parquet: a `part` column, in the order Python reads them). The style's metadata
//! "roadstyle:planner"; mapstyle.export.export_inline puts "roadstyle:planner_skipped" (the number of roads) there instead past 150,000 roads.

use crate::layers::py_round;
use crate::read::{read_table, Feature};
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};

const MODES: [&str; 3] = ["driving", "walking", "cycling"];
pub const PLANNER_MAX_EDGES: usize = 150_000;

fn i64_of(r: &Map<String, Value>, k: &str) -> Option<i64> {
    r.get(k).and_then(Value::as_i64)
}

fn f64_or0(r: &Map<String, Value>, k: &str) -> f64 {
    r.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

/// GEOS's length-indexed point of a line at `d` along it (LengthIndexedLine::extractPoint), as shapely's interpolate.
fn point_at(c: &[[f64; 2]], d: f64) -> [f64; 2] {
    if d <= 0.0 {
        return c[0];
    }
    let mut total = 0.0;
    for w in c.windows(2) {
        let len = ((w[1][0] - w[0][0]) * (w[1][0] - w[0][0]) + (w[1][1] - w[0][1]) * (w[1][1] - w[0][1])).sqrt();
        if total + len > d {
            let f = (d - total) / len;
            return [w[0][0] + f * (w[1][0] - w[0][0]), w[0][1] + f * (w[1][1] - w[0][1])];
        }
        total += len;
    }
    c[c.len() - 1]
}

/// _planner_ends: the first route's two ends: the road of `mode` whose centroid is nearest to 30 % and 70 % along the diagonal of the box of
/// those roads, at half its length (interpolate(0.5, normalized=True)).
fn planner_ends(roads: &[Feature], mode: &str) -> Vec<Value> {
    let lines: Vec<&Vec<[f64; 2]>> = {
        let of_mode: Vec<&Vec<[f64; 2]>> = roads.iter().filter(|r| crate::roads::truthy(r.props.get(mode))).filter_map(|r| r.coords.as_ref()).collect();
        if of_mode.is_empty() { roads.iter().filter_map(|r| r.coords.as_ref()).collect() } else { of_mode }
    };
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in &lines {
        for p in c.iter() {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
    }
    let cen: Vec<[f64; 2]> = lines.iter().map(|c| crate::geom::Geom::LineString(c.to_vec()).centroid()).collect();
    [0.3, 0.7].iter().map(|f| {
        let (tx, ty) = (x0 + (x1 - x0) * f, y0 + (y1 - y0) * f);
        let mut best = (f64::INFINITY, 0);
        for (i, c) in cen.iter().enumerate() {
            let (dx, dy) = (c[0] - tx, c[1] - ty);
            let d = dx * dx + dy * dy;
            if d < best.0 {
                best = (d, i);
            }
        }
        let c = lines[best.1];
        let len: f64 = c.windows(2).map(|w| ((w[1][0] - w[0][0]) * (w[1][0] - w[0][0]) + (w[1][1] - w[0][1]) * (w[1][1] - w[0][1])).sqrt()).sum();
        json!(point_at(c, 0.5 * len))
    }).collect()
}

/// planner_data: the route planner's graphs over the roads' indices (the page's RM).
pub fn planner_data(roads: &[Feature], planner: Vec<u8>) -> Result<Value, String> {
    let rows: Vec<Map<String, Value>> = read_table(planner, "")?.into_iter().map(|(r, _)| r).collect();
    let mut by_part: HashMap<String, Vec<&Map<String, Value>>> = HashMap::new();
    for r in &rows {
        by_part.entry(r["part"].as_str().unwrap_or("").to_string()).or_default().push(r);
    }
    let empty = Vec::new();
    let part = |p: &str| by_part.get(p).unwrap_or(&empty).iter().copied();
    let modes: Vec<String> = MODES.iter().filter(|m| part("mode").any(|r| r["mode"] == **m)).map(|m| m.to_string()).collect();
    if modes.is_empty() {
        return Err("no mode with edges + edge_graph to route on".into());
    }
    let eid = |r: &Feature| -> i64 { match r.props.get("edge_id") { Some(Value::String(s)) => s.parse().unwrap_or(0), Some(v) => v.as_i64().unwrap_or(0), None => 0 } };
    let mut k_of: HashMap<i64, usize> = HashMap::new(); // a road cut into pieces has several rows: the first one is the road
    for (k, r) in roads.iter().enumerate() {
        k_of.entry(eid(r)).or_insert(k);
    }
    let mut ends: HashMap<i64, (i64, i64, f64, Value)> = HashMap::new(); // edge_id -> (source, target, length_m, junction): the first row
    for r in part("end") {
        if let Some(e) = i64_of(r, "e") {
            ends.entry(e).or_insert((i64_of(r, "s").unwrap_or(0), i64_of(r, "t").unwrap_or(0), f64_or0(r, "len"), r.get("junction").cloned().unwrap_or(Value::Null)));
        }
    }
    let node_ids: BTreeSet<i64> = ends.values().flat_map(|(s, t, ..)| [*s, *t]).collect();
    let n_of: HashMap<i64, usize> = node_ids.iter().enumerate().map(|(i, n)| (*n, i)).collect();
    let mut row = Vec::with_capacity(roads.len());
    for r in roads {
        row.push(ends.get(&eid(r)).ok_or_else(|| format!("edge {} has no nodes in the mode tables", eid(r)))?);
    }
    let mut data = Map::new();
    data.insert("modes".into(), json!(modes));
    data.insert("n".into(), json!(roads.len()));
    data.insert("eid".into(), json!(roads.iter().map(|r| eid(r).to_string()).collect::<Vec<_>>()));
    data.insert("src".into(), json!(row.iter().map(|r| n_of[&r.0]).collect::<Vec<_>>()));
    data.insert("tgt".into(), json!(row.iter().map(|r| n_of[&r.1]).collect::<Vec<_>>()));
    data.insert("name".into(), json!(roads.iter().map(|r| r.props.get("name").cloned().unwrap_or(Value::Null)).collect::<Vec<_>>()));
    data.insert("len".into(), json!(row.iter().map(|r| py_round(r.2, 1)).collect::<Vec<_>>()));
    data.insert("hw".into(), json!(roads.iter().map(|r| r.props.get("highway").and_then(Value::as_str).unwrap_or("").to_string()).collect::<Vec<_>>()));
    data.insert("rb".into(), json!(row.iter().enumerate().filter(|(_, r)| matches!(r.3.as_str(), Some("roundabout" | "circular"))).map(|(k, _)| k).collect::<Vec<_>>()));
    let mut graphs = Map::new();
    for (mi, m) in MODES.iter().enumerate().filter(|(_, m)| modes.iter().any(|x| x == **m)) {
        let mut nxt: HashMap<usize, Vec<usize>> = HashMap::new();
        for r in part("next").filter(|r| r["grp"].as_i64() == Some(mi as i64)) {
            if let (Some(f), Some(t)) = (i64_of(r, "e").and_then(|e| k_of.get(&e)), i64_of(r, "e2").and_then(|e| k_of.get(&e))) {
                nxt.entry(*f).or_default().push(*t);
            }
        }
        let es: Vec<&Map<String, Value>> = part("graph").filter(|r| r["grp"].as_i64() == Some(mi as i64)).collect();
        let ks: Vec<usize> = es.iter().map(|r| k_of.get(&i64_of(r, "e").unwrap_or(0)).copied().ok_or("a mode's edge is not a road")).collect::<Result<_, _>>()?;
        graphs.insert(m.to_string(), json!({"k": ks, "cost": es.iter().map(|r| py_round(f64_or0(r, "cost"), 3)).collect::<Vec<_>>(),
                                             "len": es.iter().map(|r| py_round(f64_or0(r, "len"), 2)).collect::<Vec<_>>(),
                                             "next": ks.iter().map(|k| nxt.get(k).cloned().unwrap_or_default()).collect::<Vec<_>>()}));
    }
    data.insert("graphs".into(), Value::Object(graphs));
    // walk + drive over duckOSM's intermodal graph, when it is there
    let mut mm = Value::Null;
    if modes.iter().any(|m| m == "walking") && modes.iter().any(|m| m == "driving") && part("mm").next().is_some() {
        let mi: HashMap<&str, usize> = modes.iter().enumerate().filter(|(_, m)| matches!(m.as_str(), "walking" | "driving")).map(|(i, m)| (m.as_str(), i)).collect();
        let mode_of = |r: &Map<String, Value>, k: &str| r.get(k).and_then(Value::as_str).and_then(|m| mi.get(m)).copied();
        let edges: Vec<Value> = part("mme").filter_map(|r| {
            let (md, s, t, e) = (mode_of(r, "mode")?, n_of.get(&i64_of(r, "s")?)?, n_of.get(&i64_of(r, "t")?)?, k_of.get(&i64_of(r, "e")?)?);
            Some(json!([md, s, t, e, py_round(f64_or0(r, "cost"), 3)]))
        }).collect();
        let transfers: Vec<Value> = part("mmt").filter_map(|r| {
            let (fm, tm, n) = (mode_of(r, "fm")?, mode_of(r, "tm")?, n_of.get(&i64_of(r, "s")?)?);
            Some(json!([n, fm, tm, py_round(f64_or0(r, "cost"), 3)]))
        }).collect();
        mm = json!({"edges": edges, "transfers": transfers, "nodes": node_ids.len()});
    }
    data.insert("mm".into(), mm);
    let se = planner_ends(roads, &modes[0]);
    data.insert("start".into(), se[0].clone());
    data.insert("end".into(), se[1].clone());
    Ok(Value::Object(data))
}
