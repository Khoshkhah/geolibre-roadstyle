//! The "roads" source: the stages of roadstyle's render_web.render that bake the `__rs_*` properties of each road, in the same
//! order. Each function names the Python function it ports; the columns are the ones mapstyle passes (docs/design/rust-spike.md, S2).

use crate::read::Feature;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

/// The look data the stages read (look.json: the resolved class styles and the render's defaults).
pub struct Look {
    classes: HashMap<String, [Value; 2]>,
    fallback: [Value; 2],
    single_line: HashSet<String>,
    twin_one: bool,
    /// the heads' default length (render's head_m)
    pub head_m: f64,
    /// config bridge_shadow: a bridge's main casing piece casts a shadow (simple source)
    pub bridge_shadow: bool,
}

impl Look {
    pub fn from_json(look: &Value) -> Result<Look, String> {
        let pair = |v: &Value| -> Result<[Value; 2], String> {
            match v.as_array().map(|a| a.as_slice()) {
                Some([a, b]) => Ok([a.clone(), b.clone()]),
                _ => Err("look.json: a class style is not [plain, link]".into()),
            }
        };
        let styles = &look["styles"];
        let classes = styles["classes"].as_object().ok_or("look.json: no styles.classes")?;
        Ok(Look {
            classes: classes.iter().map(|(k, v)| Ok((k.clone(), pair(v)?))).collect::<Result<_, String>>()?,
            fallback: pair(&styles["fallback"])?,
            single_line: look["config"]["single_line_classes"].as_array().into_iter().flatten()
                .filter_map(|v| v.as_str().map(String::from)).collect(),
            twin_one: look["config"]["twin_casing"] == "one",
            head_m: look["head_m"].as_f64().ok_or("look.json: no head_m")?,
            bridge_shadow: truthy(look["config"].get("bridge_shadow")),
        })
    }
}

// the columns of the roads, as mapstyle names them (render_map's kw)
const HIGHWAY: &str = "highway";
const DIRECTED: &str = "is_directed";
const DRIVING: &str = "has_arrows";
const LAYER: &str = "layer";
const BRIDGE: &str = "bridge";
const TUNNEL: &str = "tunnel";
const CAP_START: &str = "cap_start";
const CAP_END: &str = "cap_end";
const JS_MAX_SAFE_INT: u64 = (1 << 53) - 1;

/// The stages of render_web.render on the roads, in its order; each road gets `__rs_edge`, its position.
pub fn mark_roads(fs: &mut [Feature], look: &Look) {
    bake_props(fs, look);
    mark_twoway(fs);
    if !look.single_line.is_empty() {
        mark_single_line(fs, &look.single_line);
    }
    mark_lvl(fs);
    mark_caps(fs);
    mark_heads(fs);
    mark_levels(fs);
    if look.twin_one && mark_twin_casing(fs) {
        mark_twin_dead_ends(fs);
    }
    stringify_unsafe_ints(fs);
    for (i, f) in fs.iter_mut().enumerate() {
        f.props.insert("__rs_edge".into(), i.into());
    }
}

/// The "roads" source of the style, from the marked roads (with __rs_cls, _mark_cls: filter_col is highway_col).
pub fn roads_source(fs: &[Feature]) -> Value {
    let features: Vec<Value> = fs.iter().enumerate().map(|(i, f)| {
        let mut props = f.props.clone();
        props.insert("__rs_cls".into(), f.props.get(HIGHWAY).cloned().unwrap_or(Value::Null));
        let geometry = f.coords.as_ref().map_or(Value::Null, |c| json!({"type": "LineString", "coordinates": c}));
        crate::obj([("id", i.into()), ("type", "Feature".into()), ("properties", Value::Object(props)), ("geometry", geometry)])
    }).collect();
    crate::obj([("type", "geojson".into()), ("data", crate::obj([("type", "FeatureCollection".into()), ("features", Value::Array(features))])),
                ("tolerance", 0.05.into())])
}

// --- helpers: Python's values ---

fn missing(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null))
}

/// render_web._truthy: not missing and not "", "no", "false", "0", 0 or False.
pub(crate) fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !matches!(s.as_str(), "" | "no" | "false" | "0"),
        Some(_) => true,
    }
}

/// Python's float(v): a number, or a text that is one; None where Python raises.
fn py_float(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(*b as u8 as f64),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// round(x, digits) as a key: the text of the correctly rounded decimal is the same as Python's round (a tie cannot occur at 6 or 7
/// places: half of 1e-6 has no exact binary value), and -0 is 0.
fn round_key(x: f64, digits: usize) -> String {
    let s = format!("{x:.digits$}");
    if s.starts_with('-') && s[1..].bytes().all(|c| c == b'0' || c == b'.') { s[1..].to_string() } else { s }
}

fn point_key(c: &[f64; 2]) -> (String, String) {
    (round_key(c[0], 6), round_key(c[1], 6))
}

/// _class_key: a missing class is "".
fn class_key(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

/// The feature's two end points, for a LineString of two points or more.
fn ends(f: &Feature) -> Option<([f64; 2], [f64; 2])> {
    let c = f.coords.as_ref()?;
    (c.len() >= 2).then(|| (c[0], c[c.len() - 1]))
}

// --- the stages ---

/// stylers.ClassStyler.resolve_frame + bake_props: the class's style (style.normalize_highway: lower case, "_link" cut off; unknown: the fallback).
fn bake_props(fs: &mut [Feature], look: &Look) {
    for f in fs {
        let h = match f.props.get(HIGHWAY) {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.trim().to_lowercase(),
            Some(v) => v.to_string().trim().to_lowercase(),
        };
        let (base, link) = match h.strip_suffix("_link") { Some(b) => (b.to_string(), 1), None => (h, 0) };
        let s = &look.classes.get(&base).unwrap_or(&look.fallback)[link];
        for (k, src) in [("__rs_fill", "fill"), ("__rs_w", "width"), ("__rs_op", "opacity"), ("__rs_dash", "dash"),
                         ("__rs_casing", "casing"), ("__rs_cw", "casing_width"), ("__rs_cop", "casing_opacity")] {
            f.props.insert(k.into(), s[src].clone());
        }
        f.props.insert("__rs_class".into(), base.into());
    }
}

type EndKey = Option<((String, String), (String, String), String)>;

/// _mark_twoway: __rs_twoway (a reverse twin of the same class, both directed) and __rs_oneway (the arrows).
fn mark_twoway(fs: &mut [Feature]) {
    let keys: Vec<EndKey> = fs.iter().map(|f| ends(f).map(|(a, z)| (point_key(&a), point_key(&z), class_key(f.props.get(HIGHWAY))))).collect();
    let rev = |k: &EndKey| k.as_ref().map(|(a, z, c)| (z.clone(), a.clone(), c.clone()));
    let mut by: HashMap<EndKey, Vec<usize>> = HashMap::new();
    for (i, k) in keys.iter().enumerate() {
        if k.is_some() {        // not a line: a key of its own (Python: id(ft)), never a twin
            by.entry(k.clone()).or_default().push(i);
        }
    }
    let count = |k: &EndKey| if k.is_some() { by.get(k).map_or(0, |v| v.len()) } else { 0 };
    for (i, k) in keys.iter().enumerate() {
        let r = rev(k);
        let n = count(&r);
        let two = if k.is_some() && r == *k { n >= 2 } else { n >= 1 };
        let p = &mut fs[i].props;
        p.insert("__rs_twoway".into(), two.into());
        let ow = if missing(p.get("oneway")) { !two } else { truthy(p.get("oneway")) };
        p.insert("__rs_oneway".into(), ow.into());
    }
    // directed_col: a pair is two lanes only when neither edge is undirected (null = directed)
    let und = |f: &Feature| !missing(f.props.get(DIRECTED)) && !truthy(f.props.get(DIRECTED));
    let undirected: Vec<bool> = fs.iter().map(und).collect();
    for (i, k) in keys.iter().enumerate() {
        if fs[i].props["__rs_twoway"] != true {
            continue;
        }
        let others_und = by.get(&rev(k)).map_or(true, |v| v.iter().filter(|&&t| t != i).all(|&t| undirected[t]));
        if undirected[i] || others_und {
            let p = &mut fs[i].props;
            p.insert("__rs_twoway".into(), false.into());
            if missing(p.get("oneway")) {
                p.insert("__rs_oneway".into(), (!undirected[i]).into());
            }
        }
    }
    for f in fs.iter_mut() {        // driving_col: no arrows where cars may not drive (null = driving)
        if !missing(f.props.get(DRIVING)) && !truthy(f.props.get(DRIVING)) {
            f.props.insert("__rs_oneway".into(), false.into());
        }
    }
}

/// _mark_single_line: a reverse pair of a single-line class is one line; the later edge is __rs_dup, both name the other in __rs_edge2.
fn mark_single_line(fs: &mut [Feature], classes: &HashSet<String>) {
    let mut first: HashMap<((String, String), (String, String), String), usize> = HashMap::new();
    for i in 0..fs.len() {
        let kind = match fs[i].props.get(HIGHWAY) { Some(Value::String(s)) if classes.contains(s) => s.clone(), _ => continue };
        let Some((a, z)) = ends(&fs[i]) else { continue };
        let (a, z) = (point_key(&a), point_key(&z));
        match first.remove(&(z.clone(), a.clone(), kind.clone())) {
            None => {
                first.entry((a, z, kind)).or_insert(i);
            }
            Some(j) => {
                fs[i].props.insert("__rs_dup".into(), true.into());
                fs[i].props.insert("__rs_edge2".into(), j.into());
                fs[j].props.insert("__rs_edge2".into(), i.into());
                for r in [i, j] {
                    fs[r].props.insert("__rs_twoway".into(), false.into());
                    fs[r].props.insert("__rs_oneway".into(), false.into());
                }
            }
        }
    }
}

/// _mark_lvl: lvl from the OSM layer (else 1 for a bridge, -1 for a tunnel), and the bridge and tunnel looks.
fn mark_lvl(fs: &mut [Feature]) {
    for f in fs {
        let p = &mut f.props;
        let ly = p.get(LAYER).and_then(py_float).filter(|v| v.is_finite()).map_or(0, |v| v.trunc() as i64);
        let (br, tu) = (truthy(p.get(BRIDGE)), truthy(p.get(TUNNEL)));
        let lvl = if ly != 0 { ly } else if br { 1 } else if tu { -1 } else { 0 };
        p.insert("lvl".into(), lvl.into());
        p.insert("__rs_bridge".into(), br.into());
        p.insert("__rs_tunnel".into(), tu.into());
    }
}

/// _cap_value: "square", true (flat at the end point) or null (round).
fn cap_value(v: &Value) -> Value {
    if let Value::String(s) = v {
        match s.trim().to_lowercase().as_str() {
            "square" => return "square".into(),
            "round" => return Value::Null,
            _ => {}
        }
    }
    if truthy(Some(v)) { true.into() } else { Value::Null }
}

/// _mark_caps (no cap_col; cap_start_col / cap_end_col): __rs_cap when the two ends agree, else __rs_cap0 / __rs_cap1 / __rs_split.
fn mark_caps(fs: &mut [Feature]) {
    for f in fs {
        let p = &mut f.props;
        let end = |col: &str| p.get(col).filter(|v| !v.is_null()).map_or(Value::Null, cap_value);
        let (s0, s1) = (end(CAP_START), end(CAP_END));
        if s0 == s1 {
            if !s0.is_null() {
                p.insert("__rs_cap".into(), s0);
            }
        } else {
            p.insert("__rs_cap0".into(), s0);
            p.insert("__rs_cap1".into(), s1);
            p.insert("__rs_split".into(), true.into());
        }
    }
}

/// The head lengths of each edge (head_start_m_col / head_end_m_col): __rs_hs / __rs_he where the value is a number above 0.
fn mark_heads(fs: &mut [Feature]) {
    for f in fs {
        for (col, k) in [("head_start_m", "__rs_hs"), ("head_end_m", "__rs_he")] {
            if let Some(v) = f.props.get(col).and_then(py_float).filter(|v| *v > 0.0) {
                f.props.insert(k.into(), v.into());
            }
        }
    }
}

/// _mark_levels: __rs_cl / __rs_fl (casing at most the fill), the heads' casing numbers __rs_cs / __rs_ce, band 0.
fn mark_levels(fs: &mut [Feature]) {
    let num = |v: Option<&Value>| v.and_then(py_float).filter(|x| !x.is_nan()).map_or(0, |x| x.round_ties_even() as i64);
    for f in fs {
        let p = &mut f.props;
        let (c, fl) = (num(p.get("casing_level")), num(p.get("fill_level")));
        let (c, fl) = (c.min(fl), c.max(fl));
        let cs = num(p.get("casing_start")).min(fl);
        let ce = num(p.get("casing_end")).min(fl);
        for (k, v) in [("__rs_cl", c), ("__rs_fl", fl), ("__rs_cs", cs), ("__rs_ce", ce), ("__rs_band", 0)] {
            p.insert(k.into(), v.into());
        }
    }
}

/// _twin_pairs + _mark_twin_casing: a two-way edge and a reverse edge of its class on the same line backwards share one casing
/// (__rs_twin: the other's position). Returns whether any pair was found. (Python also warns when the two disagree; not here.)
fn mark_twin_casing(fs: &mut [Feature]) -> bool {
    let mut first: HashMap<(String, String), usize> = HashMap::new();
    let mut pairs = Vec::new();
    for i in 0..fs.len() {
        if !truthy(fs[i].props.get("__rs_twoway")) {
            continue;
        }
        let Some(c) = fs[i].coords.as_ref().filter(|c| c.len() >= 2) else { continue };
        let pts: Vec<String> = c.iter().map(|x| format!("{},{}", round_key(x[0], 7), round_key(x[1], 7))).collect();
        let line = pts.join(";");
        let back = pts.iter().rev().cloned().collect::<Vec<_>>().join(";");
        let ck = class_key(fs[i].props.get(HIGHWAY));
        match first.remove(&(back, ck.clone())) {
            None => {
                first.entry((line, ck)).or_insert(i);
            }
            Some(j) => pairs.push((j, i)),
        }
    }
    for &(j, i) in &pairs {
        fs[i].props.insert("__rs_twin".into(), j.into());
        fs[j].props.insert("__rs_twin".into(), i.into());
    }
    !pairs.is_empty()
}

/// _mark_twin_dead_ends: a twin pair's dead end (no other edge ends there) without a given cap is square.
fn mark_twin_dead_ends(fs: &mut [Feature]) {
    let mut count: HashMap<(String, String), usize> = HashMap::new();
    let cs: Vec<Option<((String, String), (String, String))>> = fs.iter().map(|f| ends(f).map(|(a, z)| (point_key(&a), point_key(&z)))).collect();
    for (a, z) in cs.iter().flatten() {
        *count.entry(a.clone()).or_default() += 1;
        *count.entry(z.clone()).or_default() += 1;
    }
    for (i, f) in fs.iter_mut().enumerate() {
        let p = &mut f.props;
        let Some((a, z)) = &cs[i] else { continue };
        if missing(p.get("__rs_twin")) {
            continue;
        }
        let get = |k: &str, d: &Value| p.get(k).cloned().unwrap_or_else(|| d.clone());
        let whole = p.get("__rs_cap").cloned().unwrap_or(Value::Null);
        let split = truthy(p.get("__rs_split"));
        let (s0, s1) = if split { (get("__rs_cap0", &whole), get("__rs_cap1", &whole)) } else { (whole.clone(), whole.clone()) };
        let mut new = [s0.clone(), s1.clone()];
        for (k, (col, pnt)) in [(CAP_START, a), (CAP_END, z)].into_iter().enumerate() {
            if a != z && count.get(pnt) == Some(&2) && missing(p.get(col)) {
                new[k] = "square".into();
            }
        }
        if new[0] == new[1] {
            for k in ["__rs_cap0", "__rs_cap1", "__rs_split"] {
                p.shift_remove(k);
            }
            if !new[0].is_null() {
                p.insert("__rs_cap".into(), new[0].clone());
            }
        } else if new != [s0, s1] || split {
            p.shift_remove("__rs_cap");
            let [n0, n1] = new;
            p.insert("__rs_cap0".into(), n0);
            p.insert("__rs_cap1".into(), n1);
            p.insert("__rs_split".into(), true.into());
        }
    }
}

/// _stringify_unsafe_ints: an integer past 2**53 - 1 as text (JavaScript would round it).
fn stringify_unsafe_ints(fs: &mut [Feature]) {
    for f in fs {
        let p: &mut Map<String, Value> = &mut f.props;
        for v in p.values_mut() {
            if let Value::Number(n) = v {
                let big = n.as_u64().map(|x| x > JS_MAX_SAFE_INT).or_else(|| n.as_i64().map(|x| x.unsigned_abs() > JS_MAX_SAFE_INT));
                if big == Some(true) {
                    *v = n.to_string().into();
                }
            }
        }
    }
}
