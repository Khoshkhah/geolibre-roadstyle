//! The road layers (full port step 2, docs/design/rust-spike.md): the expression builders of roadstyle's render_web, over the road tables of
//! `_load_road_model` (look.json "roads_model" and "theme"). Each function names the Python function it ports.

use crate::overlays::{edge_overlay, overlay_layers, Overlay};
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

/// Python's round(x, n) on a float: the nearest double to the correctly rounded decimal.
pub(crate) fn py_round(x: f64, n: usize) -> f64 {
    let v: f64 = format!("{x:.n$}").parse().unwrap();
    if v == 0.0 { 0.0 } else { v }
}

/// render_web's road tables (_load_road_model) and the theme values the builders read.
pub struct Model {
    width: HashMap<String, Vec<(f64, f64)>>, // group -> (zoom, px), by zoom
    hi_rate: HashMap<String, f64>,
    casing_ratio: HashMap<String, f64>,
    group: Vec<(String, String)>, // class -> width group, in the table's order
    classes: Vec<String>,         // the groups' classes, then the links (_CLASSES)
    zstops: Vec<Value>,
    z_order: Map<String, Value>,
    link_w: f64,
    offset_frac: f64,
    width_frac: f64,
    offset_zoom: f64,
    ramp_zooms: f64,
    fb_group: String,
    fb_order: Value,
    fb_link_order: f64,
}

fn obj<'a>(v: &'a Value, what: &str) -> Result<&'a Map<String, Value>, String> {
    v.as_object().ok_or_else(|| format!("look.json: no {what}"))
}

fn num(v: &Value, what: &str) -> Result<f64, String> {
    v.as_f64().ok_or_else(|| format!("look.json: {what} is not a number"))
}

impl Model {
    pub fn from_look(look: &Value) -> Result<Model, String> {
        let r = &look["roads_model"];
        let th = &look["theme"]["roads"];
        let mut width = HashMap::new();
        for (g, t) in obj(&r["width"], "roads_model.width")? {
            let mut stops: Vec<(f64, f64)> = obj(t, "a width table")?.iter()
                .map(|(z, w)| Ok((z.parse::<f64>().map_err(|e| e.to_string())?, num(w, "a width")?))).collect::<Result<_, String>>()?;
            stops.sort_by(|a, b| a.0.total_cmp(&b.0));
            width.insert(g.clone(), stops);
        }
        let floats = |v: &Value, what: &str| -> Result<HashMap<String, f64>, String> {
            obj(v, what)?.iter().map(|(k, x)| Ok((k.clone(), num(x, what)?))).collect()
        };
        let group: Vec<(String, String)> = obj(&r["group"], "roads_model.group")?.iter()
            .map(|(c, g)| (c.clone(), g.as_str().unwrap_or_default().to_string())).collect();
        let links: Vec<String> = r["links"].as_array().ok_or("look.json: no roads_model.links")?.iter()
            .filter_map(|v| v.as_str().map(String::from)).collect();
        let classes = group.iter().map(|(c, _)| c.clone()).chain(links.iter().cloned()).collect();
        let split = &th["split"];
        let fb = &th["fallback"];
        Ok(Model {
            width,
            hi_rate: floats(&r["width_zoom_rate"], "roads_model.width_zoom_rate")?,
            casing_ratio: floats(&r["casing_ratio"], "roads_model.casing_ratio")?,
            group,
            classes,
            zstops: r["zoom_stops"].as_array().ok_or("look.json: no roads_model.zoom_stops")?.clone(),
            z_order: obj(&r["z_order"], "roads_model.z_order")?.clone(),
            link_w: num(&th["links"]["width"], "theme roads.links.width")?,
            // render(offset_frac=, width_frac=) over the theme's split: mapstyle passes 0 and 1 for a mode without lanes (one centred line per road)
            offset_frac: look["args"]["offset_frac"].as_f64().map_or_else(|| num(&split["offset_frac"], "split.offset_frac"), Ok)?,
            width_frac: look["args"]["width_frac"].as_f64().map_or_else(|| num(&split["width_frac"], "split.width_frac"), Ok)?,
            offset_zoom: num(&split["offset_zoom"], "split.offset_zoom")?,
            ramp_zooms: num(&split["ramp_zooms"], "split.ramp_zooms")?,
            fb_group: fb["group"].as_str().ok_or("look.json: no fallback group")?.to_string(),
            fb_order: fb["order"].clone(),
            fb_link_order: num(&fb["link_order"], "fallback.link_order")?,
        })
    }

    /// _base: a link's main class.
    fn base(c: &str) -> (&str, bool) {
        match c.strip_suffix("_link") { Some(b) => (b, true), None => (c, false) }
    }

    fn group_of(&self, base: &str) -> &str {
        self.group.iter().find(|(c, _)| c == base).map_or(self.fb_group.as_str(), |(_, g)| g.as_str())
    }

    /// _gwidth: a group's width at zoom z (linear between its stops, growing by its rate past the last).
    fn gwidth(&self, g: &str, z: f64) -> f64 {
        let t = &self.width[g];
        let (first, last) = (t[0], t[t.len() - 1]);
        if z <= first.0 {
            return first.1;
        }
        if z >= last.0 {
            return last.1 * self.hi_rate[g].powf(z - last.0);
        }
        for w in t.windows(2) {
            if z <= w[1].0 {
                let f = (z - w[0].0) / (w[1].0 - w[0].0);
                return w[0].1 + (w[1].1 - w[0].1) * f;
            }
        }
        last.1
    }

    fn stops(&self) -> impl Iterator<Item = (f64, &Value)> {
        self.zstops.iter().map(|z| (z.as_f64().unwrap(), z))
    }

    /// _width_expr: interpolate(zoom) of match(class -> width); two-way lanes narrow to width_frac from offset_zoom (over 2 zooms).
    pub fn width_expr(&self, col: &str, casing: bool, scale: f64) -> Value {
        let (split_zoom, split_frac) = (self.offset_zoom, self.width_frac);
        let mut e = vec![json!("interpolate"), json!(["linear"]), json!(["zoom"])];
        for (z, zv) in self.stops() {
            let w_of = |g: &str, lk: bool| {
                let mut w = self.gwidth(g, z) * if casing { self.casing_ratio[g] } else { 1.0 } * scale;
                if lk {
                    w *= self.link_w;
                }
                py_round(w, 2)
            };
            let mut m = vec![json!("match"), json!(["get", col])];
            for c in &self.classes {
                let (b, lk) = Self::base(c);
                m.push(json!(c));
                m.push(json!(w_of(self.group_of(b), lk)));
            }
            m.push(json!(w_of(&self.fb_group, false)));
            let f = if z <= split_zoom { 1.0 } else if z >= split_zoom + 2.0 { split_frac } else { 1.0 - (1.0 - split_frac) * (z - split_zoom) / 2.0 };
            let m = if f < 1.0 { json!(["*", m, ["case", ["to-boolean", ["get", "__rs_twoway"]], py_round(f, 3), 1]]) } else { Value::Array(m) };
            e.push(zv.clone());
            e.push(m);
        }
        Value::Array(e)
    }

    /// _offset_match: a two-way direction's offset at zoom z, per class.
    pub fn offset_match(&self, col: &str, z: f64) -> Value {
        let ramp = ((z - self.offset_zoom) / self.ramp_zooms).clamp(0.0, 1.0);
        let mut m = vec![json!("match"), json!(["get", col])];
        for c in &self.classes {
            let (b, lk) = Self::base(c);
            let w = self.gwidth(self.group_of(b), z) * if lk { self.link_w } else { 1.0 };
            m.push(json!(c));
            m.push(json!(py_round(w * self.offset_frac * ramp, 3)));
        }
        m.push(json!(py_round(self.gwidth(&self.fb_group, z) * self.offset_frac * ramp, 3)));
        Value::Array(m)
    }

    /// _offset_expr (no metre widths): line-offset of a two-way edge; with `pairs`, a pair's one casing (__rs_pair) is not shifted.
    pub fn offset_expr(&self, col: &str, pairs: bool) -> Value {
        let two = json!(["to-boolean", ["get", "__rs_twoway"]]);
        let two = if pairs { json!(["all", two, ["!", ["to-boolean", ["get", "__rs_pair"]]]]) } else { two };
        let mut e = vec![json!("interpolate"), json!(["linear"]), json!(["zoom"])];
        for (z, zv) in self.stops() {
            e.push(zv.clone());
            e.push(json!(["case", two, self.offset_match(col, z), 0]));
        }
        Value::Array(e)
    }

    /// class_order: the table's order, else a link just under its main class.
    fn class_order(&self, c: &str) -> Value {
        if let Some(v) = self.z_order.get(c) {
            return v.clone();
        }
        let (b, lk) = Self::base(c);
        let base = self.z_order.get(b).unwrap_or(&self.fb_order);
        if lk { json!(base.as_f64().unwrap() + self.fb_link_order) } else { base.clone() }
    }

    /// _sort_key: the grade (lvl * 1000), then the class.
    pub fn sort_key(&self, col: &str) -> Value {
        let mut m = vec![json!("match"), json!(["get", col])];
        for c in &self.classes {
            m.push(json!(c));
            m.push(self.class_order(c));
        }
        m.push(self.fb_order.clone());
        json!(["+", ["*", ["coalesce", ["get", "lvl"], 0], 1000], m])
    }
}

// --- step 2: the road layers of simple mode with stored levels (render_web.render, simple=True, inline data), the passes and the export ---

fn gs(name: &str) -> Value {
    json!(["global-state", name])
}

/// _painted: the colour rsColor (or rsColorBase: state "rs_base") gave the road the property `key` names, else null.
fn painted(key: &str, state: &str) -> Value {
    json!(["get", ["to-string", ["get", key]], gs(state)])
}

/// _fill_expr with one colour option (_fill_base: the baked __rs_fill).
fn fill_expr(fb_fill: &Value) -> Value {
    json!(["coalesce", painted("__rs_edge", "rs_colors"), painted("__rs_edge2", "rs_colors"), painted("__rs_edge", "rs_base"),
           painted("__rs_edge2", "rs_base"), ["coalesce", ["get", "__rs_fill"], fb_fill]])
}

/// _tun_mix: a tunnel feature's colour moved toward the tunnel colour by the strength, keeping its own alpha.
fn tun_mix(expr: &Value) -> Value {
    let mixed = json!(["to-rgba", ["interpolate", ["linear"], gs("rs_tun_strength"), 0, expr, 100, gs("rs_tun_toward")]]);
    let keep = json!(["rgba", ["at", 0, ["var", "m"]], ["at", 1, ["var", "m"]], ["at", 2, ["var", "m"]], ["at", 3, ["to-rgba", expr]]]);
    json!(["case", ["to-boolean", ["get", "__rs_tunnel"]], ["let", "m", mixed, keep], expr])
}

fn tun_gap() -> Value {
    json!(["case", gs("rs_tun_two"), ["interpolate", ["linear"], gs("rs_tun_strength"), 0, gs("rs_tun_c1"), 100, gs("rs_tun_toward")], "rgba(0,0,0,0)"])
}

fn tun_dash() -> Value {
    json!(["interpolate", ["linear"], gs("rs_tun_strength"), 0, gs("rs_tun_c0"), 100, gs("rs_tun_toward")])
}

/// _plus_px: a zoom curve `px(zoom)` pixels wider at each stop (only where `when` holds, if given).
fn plus_px(expr: &Value, px: impl Fn(&Value) -> Value, when: Option<&Value>) -> Value {
    let e = expr.as_array().unwrap();
    let first = if e[0] == "interpolate" { 4 } else { 2 };
    let mut out: Vec<Value> = e[..first].to_vec();
    for i in first..e.len() {
        let v = &e[i];
        out.push(if (i - first) % 2 == 0 {
            let p = px(&e[i - 1]);
            match when {
                None => json!(["+", v, p]),
                Some(w) => json!(["case", w, ["+", v, p], v]),
            }
        } else {
            v.clone()
        });
    }
    Value::Array(out)
}

/// _by_feature: one zoom curve that picks a curve per feature, the choice inside each stop.
fn by_feature(cases: &[(Value, Value)], default: &Value) -> Value {
    let d = default.as_array().unwrap();
    let mut out: Vec<Value> = d[..3].to_vec();
    let mut i = 0;
    while 3 + 2 * i < d.len() {
        let mut c = vec![json!("case")];
        for (cond, curve) in cases {
            c.push(cond.clone());
            c.push(curve[4 + 2 * i].clone());
        }
        c.push(d[4 + 2 * i].clone());
        out.push(d[3 + 2 * i].clone());
        out.push(Value::Array(c));
        i += 1;
    }
    Value::Array(out)
}

fn seam_filter() -> Value {
    let (seam, lap) = (json!(["to-boolean", ["get", "__rs_seam"]]), json!(["to-boolean", ["get", "__rs_lap"]]));
    json!(["any", ["!", seam], ["all", ["!", lap], [">=", ["zoom"], 17]], ["all", lap, ["<", ["zoom"], 17]]])
}

fn simple_cap() -> Value {
    json!(["case", ["to-boolean", ["get", "__rs_dash"]], "butt", ["==", ["get", "__rs_k"], 4], "butt", ["==", ["get", "__rs_k"], 5], "butt",
           ["==", ["get", "__rs_cap"], "square"], "square", ["to-boolean", ["get", "__rs_cap"]], "butt", "round"])
}

fn simple_dasharray(dashes: &[String]) -> Value {
    let solid = json!(["literal", [1, 0]]);
    let by = if dashes.is_empty() { solid } else {
        let mut m = vec![json!("match"), json!(["to-string", ["coalesce", ["get", "__rs_dash"], ""]])];
        for d in dashes {
            m.push(json!(d));
            m.push(json!(["literal", d.split(',').map(|v| v.trim().parse::<f64>().unwrap()).collect::<Vec<_>>()]));
        }
        m.push(solid);
        Value::Array(m)
    };
    json!(["case", ["==", ["get", "__rs_k"], 4], gs("rs_tun_ratio"), by])
}

/// Python's truthiness of a JSON value (bool(v)).
fn py_truth(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// _ev's three values: a value, or one only the page can tell (the zoom, an operator it does not read).
enum Ev {
    V(Value),
    Maybe,
}

/// _same: MapLibre's ==, a boolean never equals a number.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(_), Value::Bool(_)) => a == b,
        (Value::Bool(_), _) | (_, Value::Bool(_)) => false,
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

fn as_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(*b as u8 as f64),
        _ => None,
    }
}

/// _ev: a MapLibre expression on a feature's properties, or Maybe where only the page can tell.
fn ev(e: &Value, p: &Map<String, Value>) -> Ev {
    let Some(arr) = e.as_array().filter(|a| !a.is_empty()) else { return Ev::V(e.clone()) };
    let (op, a) = (arr[0].as_str().unwrap_or(""), &arr[1..]);
    match op {
        "literal" => Ev::V(a[0].clone()),
        "get" if a.len() == 1 => Ev::V(a[0].as_str().and_then(|k| p.get(k)).cloned().unwrap_or(Value::Null)),
        "has" if a.len() == 1 => Ev::V(Value::Bool(a[0].as_str().is_some_and(|k| p.contains_key(k)))),
        "all" | "any" => {
            let all = op == "all";
            let mut out = Ev::V(Value::Bool(all));
            for x in a {
                match ev(x, p) {
                    Ev::Maybe => out = Ev::Maybe,
                    Ev::V(v) if py_truth(&v) != all => return Ev::V(Value::Bool(!all)),
                    _ => {}
                }
            }
            out
        }
        "!" | "to-boolean" | "==" | "!=" | "<" | ">" | "<=" | ">=" | "%" | "*" | "+" | "-" => {
            let mut vals = Vec::new();
            for x in a {
                match ev(x, p) {
                    Ev::Maybe => return Ev::Maybe,
                    Ev::V(v) => vals.push(v),
                }
            }
            match op {
                "!" => Ev::V(Value::Bool(!py_truth(&vals[0]))),
                "to-boolean" => Ev::V(Value::Bool(crate::roads::truthy(Some(&vals[0])))),
                "==" => Ev::V(Value::Bool(same(&vals[0], &vals[1]))),
                "!=" => Ev::V(Value::Bool(!same(&vals[0], &vals[1]))),
                _ => {
                    if let (Value::String(x), Value::String(y)) = (&vals[0], &vals[1]) {
                        return match op {
                            "<" => Ev::V((x < y).into()), ">" => Ev::V((x > y).into()), "<=" => Ev::V((x <= y).into()), ">=" => Ev::V((x >= y).into()),
                            "+" => Ev::V(format!("{x}{y}").into()),
                            _ => Ev::Maybe,
                        };
                    }
                    let (Some(x), Some(y)) = (as_num(&vals[0]), as_num(&vals[1])) else { return Ev::Maybe };
                    match op {
                        "<" => Ev::V((x < y).into()), ">" => Ev::V((x > y).into()), "<=" => Ev::V((x <= y).into()), ">=" => Ev::V((x >= y).into()),
                        "%" if y == 0.0 => Ev::Maybe,
                        "%" => Ev::V(json!(x - y * (x / y).floor())),
                        "*" => Ev::V(json!(x * y)), "+" => Ev::V(json!(x + y)), "-" => Ev::V(json!(x - y)),
                        _ => Ev::Maybe,
                    }
                }
            }
        }
        "coalesce" => {
            for x in a {
                match ev(x, p) {
                    Ev::Maybe => return Ev::Maybe,
                    Ev::V(Value::Null) => {}
                    v => return v,
                }
            }
            Ev::V(Value::Null)
        }
        "case" => {
            let mut i = 0;
            while i + 1 < a.len() {
                match ev(&a[i], p) {
                    Ev::Maybe => return Ev::Maybe,
                    Ev::V(v) if py_truth(&v) => return ev(&a[i + 1], p),
                    _ => {}
                }
                i += 2;
            }
            ev(&a[a.len() - 1], p)
        }
        "match" => {
            let v = match ev(&a[0], p) { Ev::Maybe => return Ev::Maybe, Ev::V(v) => v };
            let mut i = 1;
            while i + 1 < a.len() {
                let hit = match &a[i] { Value::Array(labels) => labels.iter().any(|l| same(&v, l)), l => same(&v, l) };
                if hit {
                    return ev(&a[i + 1], p);
                }
                i += 2;
            }
            ev(&a[a.len() - 1], p)
        }
        _ => Ev::Maybe,
    }
}

/// _drop_empty_layers: a road layer stays when its filter holds, or may hold, for some feature of its source; roads-fill always stays.
fn drop_empty_layers(layers: Vec<Value>, feats: &HashMap<&str, Vec<&Map<String, Value>>>) -> Vec<Value> {
    layers.into_iter().filter(|l| {
        let id = l["id"].as_str().unwrap_or("");
        if !id.starts_with("roads-") || id == "roads-fill" {
            return true;
        }
        let (Some(f), Some(fs)) = (l.get("filter"), l["source"].as_str().and_then(|s| feats.get(s))) else { return true };
        fs.iter().any(|p| match ev(f, p) { Ev::Maybe => true, Ev::V(v) => py_truth(&v) })
    }).collect()
}

/// _swap: `v` with every part equal to `old` replaced by `new`.
fn swap(v: &Value, old: &Value, new: &Value) -> Value {
    if v == old {
        return new.clone();
    }
    match v { Value::Array(a) => Value::Array(a.iter().map(|x| swap(x, old, new)).collect()), _ => v.clone() }
}

/// _bake_state: every ["global-state", k] its start value, and the tunnel mix without `let`.
fn bake(e: &Value, state: &Map<String, Value>) -> Value {
    match e {
        Value::Array(a) => {
            if a.len() == 2 && a[0] == "global-state" {
                let v = &state[a[1].as_str().unwrap()];
                return if v.is_array() || v.is_object() { json!(["literal", v]) } else { v.clone() };
            }
            if a.len() == 4 && a[0] == "let" && a[1] == "m" && a[2].as_array().is_some_and(|x| x.first() == Some(&json!("to-rgba"))) {
                return bake(&a[2][1], state);
            }
            Value::Array(a.iter().map(|x| bake(x, state)).collect())
        }
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), bake(v, state))).collect()),
        _ => e.clone(),
    }
}

fn merged(a: &Value, b: &Value) -> Map<String, Value> {
    let mut m = a.as_object().cloned().unwrap_or_default();
    if let Some(o) = b.as_object() {
        for (k, v) in o {
            m.insert(k.clone(), v.clone());
        }
    }
    m
}

impl Model {
    /// class_width_px (casing as given): the class's width at `zoom` read off the width curve (linear between its stops, the ends outside).
    fn class_width_px(&self, cls: Option<&str>, zoom: f64, casing: bool) -> f64 {
        let e = self.width_expr("highway", casing, 1.0);
        let e = e.as_array().unwrap();
        let zs: Vec<f64> = e[3..].iter().step_by(2).map(|z| z.as_f64().unwrap()).collect();
        let ms: Vec<&Value> = e[4..].iter().step_by(2).collect();
        let at = |m: &Value| -> f64 {
            let m = if m[0] == "*" { &m[1] } else { m };
            let a = m.as_array().unwrap();
            let mut i = 2;
            while i + 1 < a.len() {
                if cls.is_some_and(|c| a[i] == c) {
                    return a[i + 1].as_f64().unwrap();
                }
                i += 2;
            }
            a[a.len() - 1].as_f64().unwrap()
        };
        if zoom <= zs[0] {
            return at(ms[0]);
        }
        if zoom >= zs[zs.len() - 1] {
            return at(ms[ms.len() - 1]);
        }
        let i = (0..zs.len()).filter(|&k| zs[k] <= zoom).max().unwrap();
        let f = (zoom - zs[i]) / (zs[i + 1] - zs[i]);
        at(ms[i]) + (at(ms[i + 1]) - at(ms[i])) * f
    }

    /// _pair_width: a two-way pair's one casing as wide as its two directions (no metre widths).
    fn pair_width(&self, expr: &Value) -> Value {
        let (z0, z1) = (self.zstops[0].as_f64().unwrap(), self.zstops[self.zstops.len() - 1].as_f64().unwrap());
        plus_px(expr, |z| json!(["*", 2, self.offset_match("highway", z.as_f64().unwrap().clamp(z0, z1))]),
                Some(&json!(["to-boolean", ["get", "__rs_pair"]])))
    }
}

fn props_of<'a>(fs: &'a [Value], empty: &'a Map<String, Value>) -> Vec<&'a Map<String, Value>> {
    fs.iter().map(|f| f["properties"].as_object().unwrap_or(empty)).collect()
}

/// The road layers of the style: what render_web.render, the export and mapstyle's export make for simple mode with stored levels and
/// inline data (no base-map layers, no 3D decks, no metre widths, no items). `roads`, `simple`, `slots`: the features of the three sources.
pub fn road_layers(look: &Value, roads: &[Value], simple: &[Value], slots: &[Value], ovs: &mut [Overlay]) -> Result<Vec<Value>, String> {
    let m = Model::from_look(look)?;
    let (th, cfg) = (&look["theme"], &look["config"]);
    let empty = Map::new();
    let (rp, sp, lp) = (props_of(roads, &empty), props_of(simple, &empty), props_of(slots, &empty));
    let fb_fill = &th["roads"]["fallback"]["fill"];
    let fb_casing = &th["roads"]["fallback"]["casing"];
    let sel = &th["selection"];
    let (select, hover) = (&sel["select"], &sel["hover"]);
    let any_tunnel = rp.iter().any(|p| p.get("__rs_tunnel").is_some_and(py_truth));
    let pairs = rp.iter().any(|p| p.contains_key("__rs_twin"));
    let any_dup = rp.iter().any(|p| p.get("__rs_dup").is_some_and(py_truth));
    let mut dashes: Vec<String> = rp.iter().filter_map(|p| p.get("__rs_dash").and_then(Value::as_str)).filter(|s| !s.is_empty()).map(String::from).collect();
    dashes.sort();
    dashes.dedup();
    let minor: Vec<Value> = th["roads"]["classes"].as_object().into_iter().flatten()
        .filter(|(_, e)| e.get("minor").is_some_and(py_truth)).map(|(c, _)| json!(c)).collect();
    let arw = merged(&th["arrows"], &cfg["arrows"]);
    let lbl = merged(&th["labels"], &cfg["labels"]);

    let lay = json!({"line-cap": th["roads"]["cap"], "line-join": th["roads"]["join"], "line-sort-key": 0});
    let off = m.offset_expr("highway", pairs);
    let cw = m.width_expr("highway", true, 1.0);
    let fw = m.width_expr("highway", false, 1.0);
    let bcw = m.width_expr("highway", true, th["bridges"]["wing_scale"].as_f64().unwrap_or(1.0));
    let lean_layout = |l: &Value| { let mut o = l.as_object().unwrap().clone(); o.insert("visibility".into(), "none".into()); Value::Object(o) };

    // roads-simple and the invisible roads-fill (simple mode)
    let flt = json!(["all", seam_filter()]);
    let k = |n: i64| json!(["==", ["get", "__rs_k"], n]);
    let (is_c, is_sh, is_b) = (k(0), k(2), json!(["to-boolean", ["get", "__rs_bridge"]]));
    let blur = cfg["bridge_shadow_blur"].as_f64().unwrap();
    let extra = cfg["bridge_casing_extra"].as_f64().unwrap();
    let (r0v, r1v) = (&th["bridges"]["ramp"][0], &th["bridges"]["ramp"][1]);
    let (r0, r1) = (r0v.as_f64().unwrap(), r1v.as_f64().unwrap());
    let ramp = |z: &Value| ((z.as_f64().unwrap() - r0) / (r1 - r0)).clamp(0.0, 1.0);
    let bwide = plus_px(&bcw, |z| json!(2.0 * extra * ramp(z)), None);
    let mut wide = vec![(is_sh.clone(), plus_px(&bcw, |z| json!(2.0 * (extra + blur) * ramp(z)), None)), (json!(["all", is_c, is_b]), bwide)];
    if any_tunnel {
        let px = th["tunnels"]["dash_extra_px"].clone();
        wide.push((json!(["any", k(3), k(4)]), plus_px(&cw, |_| px.clone(), None)));
    }
    wide.push((is_c.clone(), cw.clone()));
    let op = json!(["case", ["all", ["!", gs("rs_fill_on")], k(1)], 0, 1]);
    // _simple_color, with the glow of lean mode
    let mut fill = fill_expr(fb_fill);
    let mut item = json!(["coalesce", painted("__rs_edge", "rs_colors"), painted("__rs_edge2", "rs_colors"), painted("__rs_edge", "rs_base"),
                          painted("__rs_edge2", "rs_base"), ["coalesce", ["get", "__rs_ic"], fb_fill]]);
    let mut cases = vec![k(2), cfg["bridge_shadow_color"].clone(), is_c.clone(),
                         json!(["case", is_b, cfg["bridge_casing_color"], ["coalesce", ["get", "__rs_casing"], fb_casing]])];
    if any_tunnel {
        fill = tun_mix(&fill);
        item = tun_mix(&item);
        cases.extend([k(3), tun_gap(), k(4), tun_dash()]);
    }
    let (s_on, h_on) = (json!(["boolean", ["feature-state", "select"], false]), json!(["boolean", ["feature-state", "hover"], false]));
    item = json!(["case", s_on, select, h_on, hover, item]);
    let mix = |c: &Value, f: &Value| json!(["interpolate", ["linear"], sel["opacity"], 0, f, 1, c]);
    fill = json!(["case", s_on, mix(select, &fill), h_on, mix(hover, &fill), fill]);
    let mut color = vec![json!("case")];
    color.extend(cases);
    color.extend([k(5), item, fill]);
    let mut paint = Map::new();
    if !dashes.is_empty() || any_tunnel {
        paint.insert("line-dasharray".into(), simple_dasharray(&dashes));
    }
    paint.insert("line-color".into(), Value::Array(color));
    let width = by_feature(&wide, &fw);
    paint.insert("line-width".into(), if pairs { m.pair_width(&width) } else { width });
    paint.insert("line-offset".into(), off.clone());
    paint.insert("line-blur".into(), json!(["interpolate", ["linear"], ["zoom"], r0v, 0, r1v, ["case", is_sh, blur, 0]]));
    paint.insert("line-opacity".into(), json!(["interpolate", ["linear"], ["zoom"], r0v, ["case", is_sh, 0, op], r1v, op]));
    let mut simple_layout = lay.as_object().unwrap().clone();
    simple_layout.insert("line-cap".into(), simple_cap());
    simple_layout.insert("line-sort-key".into(), json!(["get", "__rs_s"]));
    let road = json!({"id": "roads-simple", "type": "line", "source": "simple", "layout": simple_layout, "filter": flt, "paint": paint});
    let pick = json!({"id": "roads-fill", "type": "line", "source": "roads", "layout": lean_layout(&lay), "filter": flt,
                      "paint": {"line-color": ["coalesce", ["get", "__rs_fill"], fb_fill], "line-width": fw, "line-offset": off, "line-opacity": 0}});
    let highlight = json!({"id": "roads-highlight", "type": "line", "source": "roads", "layout": lean_layout(&lay),
                           "paint": {"line-color": ["case", s_on, select, hover],
                                     "line-opacity": ["case", ["any", h_on, s_on], sel["opacity"], 0], "line-width": fw, "line-offset": off}});
    // the base map's overlays (_build_overlays): under the roads, over them, or on a street (_edge_overlay: one copy per fill number)
    let by_id: HashMap<String, &Map<String, Value>> = rp.iter()
        .filter_map(|p| match p.get("edge_id") { Some(Value::String(s)) => Some((s.clone(), *p)), _ => None }).collect();
    let (mut under, mut over, mut edge, mut ov_ids) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (i, ov) in ovs.iter_mut().enumerate() {
        let sid = format!("ov{i}");
        let mut ids: Vec<String> = Vec::new();
        if ov.edge {
            for (pos, order) in edge_overlay(&mut ov.fc, &by_id)? {
                let flt = json!(["all", ["==", ["get", "__rs_fl"], pos], ["==", ["get", "__rs_ord"], order]]);
                let mine: Vec<Value> = overlay_layers(&sid, ov, look).into_iter().map(|mut l| {
                    l["id"] = format!("{}-lv{pos}-o{order}", l["id"].as_str().unwrap()).into();
                    l["filter"] = flt.clone();
                    l
                }).collect();
                ids.extend(mine.iter().map(|l| l["id"].as_str().unwrap().to_string()));
                edge.push((pos, order, i, mine));
            }
        } else {
            let ls = overlay_layers(&sid, ov, look);
            ids.extend(ls.iter().map(|l| l["id"].as_str().unwrap().to_string()));
            if ov.under { under.extend(ls) } else { over.extend(ls) }
        }
        ov_ids.push(ids);
    }
    let mut layers = under;
    layers.extend([road, pick]);

    // the arrows and the names (simple mode: one layer each, above all roads, under the highlight)
    let sort = json!(["*", -1, m.sort_key("highway")]);
    if !slots.is_empty() {
        let minor_or = |mz: &Value| json!(["any", ["!", ["match", ["get", "highway"], minor, true, false]], [">=", ["zoom"], mz]]);
        let size = &arw["size"];
        let mut icon = vec![json!("interpolate"), json!(["exponential", size["base"]]), json!(["zoom"])];
        for zb in size["stops"].as_array().into_iter().flatten() {
            icon.push(zb[0].clone());
            icon.push(json!(["*", zb[1], ["match", ["get", "highway"], minor, size["minor"], 1.0]]));
        }
        let mut spacing = vec![json!("interpolate"), json!(["linear"]), json!(["zoom"])];
        for zp in arw["spacing"].as_array().into_iter().flatten() {
            spacing.extend(zp.as_array().unwrap().iter().cloned());
        }
        layers.push(json!({"id": "roads-arrows", "type": "symbol", "source": "slots", "minzoom": arw["minzoom"],
                           "filter": ["all", ["==", ["get", "oneway"], 1], true, minor_or(&arw["minor_minzoom"])],
                           "layout": {"symbol-placement": "line", "symbol-spacing": spacing, "icon-image": "oneway", "icon-rotation-alignment": "map",
                                      "symbol-sort-key": sort, "icon-size": icon},
                           "paint": {"icon-opacity": arw["opacity"]}}));
        let mut lf = vec![json!("all"), json!(["==", ["%", ["get", "slot"], 2], 0]), json!(["to-boolean", ["get", "name"]]), minor_or(&lbl["minor_minzoom"])];
        let fixed = lbl.get("size").filter(|v| py_truth(v));
        if fixed.is_none() {
            lf.push(label_readable_filter(&m, th));
        }
        let mut lpaint = json!({"text-color": lbl["color"], "text-opacity": lbl.get("opacity").unwrap_or(&arw["opacity"])});
        if lbl.get("halo_color").is_some_and(py_truth) && lbl.get("halo_width").is_some_and(py_truth) {
            lpaint["text-halo-color"] = lbl["halo_color"].clone();
            lpaint["text-halo-width"] = lbl["halo_width"].clone();
        }
        layers.push(json!({"id": "roads-labels", "type": "symbol", "source": "slots", "minzoom": lbl["minzoom"], "filter": lf,
                           "layout": {"symbol-placement": "line-center", "text-field": ["get", "name"], "text-font": lbl["font"],
                                      "text-size": label_size_expr(&m, th, fixed), "text-max-angle": lbl["max_angle"], "text-padding": lbl["padding"],
                                      "symbol-sort-key": sort},
                           "paint": lpaint}));
    }
    layers.push(highlight);

    // the passes: a reverse pair's second edge draws nowhere; the layers no feature can draw go; the fill's opacity switch
    if any_dup {
        let nd = json!(["!", ["to-boolean", ["get", "__rs_dup"]]]);
        for l in layers.iter_mut().filter(|l| l["source"] == "roads") {
            l["filter"] = match l.get("filter") { Some(f) => json!(["all", f, nd]), None => nd.clone() };
        }
    }
    let feats: HashMap<&str, Vec<&Map<String, Value>>> = [("roads", rp.clone()), ("slots", lp.clone()), ("arrows", lp.clone()), ("simple", sp.clone())].into();
    let mut layers = drop_empty_layers(layers, &feats);
    for l in layers.iter_mut().filter(|l| l["id"].as_str().is_some_and(|i| i.starts_with("roads-fill") && !i.ends_with("-pat"))) {
        let v = l["paint"].get("line-opacity").cloned().unwrap_or(json!(1));
        l["paint"]["line-opacity"] = json!(["case", gs("rs_fill_on"), if v.is_null() { json!(1) } else { v }, 0]);
    }
    // the overlays on a street: between the road layer and the arrows, by fill number, order and overlay; the overlays over the roads: on top
    edge.sort_by_key(|e| (e.0, e.1, e.2));
    let edge_ids: HashSet<String> = edge.iter().flat_map(|e| e.3.iter().map(|l| l["id"].as_str().unwrap().to_string())).collect();
    if !edge.is_empty() {
        let at = layers.iter().position(|l| matches!(l["id"].as_str(), Some("roads-arrows" | "roads-labels" | "roads-highlight"))).unwrap_or(layers.len());
        let items: Vec<Value> = edge.into_iter().flat_map(|e| e.3).collect();
        layers.splice(at..at, items);
    }
    layers.extend(over);
    // _state_expressions: a road fill reads the colours of rsColor; a painted road sorts on top
    let base = json!(["coalesce", ["get", "__rs_fill"], fb_fill]);
    let has = |key: &str| json!(["has", ["to-string", ["get", key]], gs("rs_colors")]);
    for l in layers.iter_mut() {
        if l["id"] == "roads-fill" {
            l["paint"]["line-color"] = swap(&l["paint"]["line-color"], &base, &fill_expr(fb_fill));
        }
        if l["type"] == "line" && l["layout"].get("line-sort-key").is_some_and(|v| !v.is_null()) {
            let key = l["layout"]["line-sort-key"].clone();
            if l["source"] == "roads" {
                l["layout"]["line-sort-key"] = json!(["+", key, ["case", ["any", has("__rs_edge"), has("__rs_edge2")], 500, 0]]);
            } else if l["id"] == "roads-simple" {
                l["layout"]["line-sort-key"] = json!(["+", key, ["case", ["all", ["any", k(1), k(5)], ["any", has("__rs_edge"), has("__rs_edge2")]], 0.5, 0]]);
            }
        }
    }
    // an overlay's colours set by rsColor (state rs_ov_<i>)
    for (i, ids) in ov_ids.iter().enumerate() {
        for l in layers.iter_mut().filter(|l| l["id"].as_str().is_some_and(|x| ids.iter().any(|y| y == x))) {
            let t = l["type"].as_str().unwrap_or("").to_string();
            if matches!(t.as_str(), "line" | "fill" | "circle") {
                let k = format!("{t}-color");
                if let Some(c) = l["paint"].get(&k).cloned() {
                    l["paint"][&k] = json!(["coalesce", ["get", ["to-string", ["id"]], gs(&format!("rs_ov_{i}"))], c]);
                }
            }
        }
    }
    // the page's state (its start values, baked below)
    let towards = cfg["tunnel_towards"].as_object().cloned().unwrap_or_default();
    let toward = &cfg["tunnel_toward"];
    let colour = toward.as_str().and_then(|t| towards.get(t)).cloned().unwrap_or_else(|| toward.clone());
    let pair = cfg["tunnel_palette"].as_str().and_then(|p| cfg["tunnel_palettes"].get(p)).filter(|v| !v.is_null()).cloned();
    let dash = &th["tunnels"]["to"]["dash"];
    let ratio: Vec<f64> = cfg["tunnel_casing_dash"].as_array().filter(|a| !a.is_empty()).map(|a| a.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_else(|| vec![1.0, 1.0]);
    let state: Map<String, Value> = json!({"rs_co": 0, "rs_colors": {}, "rs_base": {}, "rs_fill_on": true,
        "rs_tun_strength": cfg["tunnel_strength"].as_f64().unwrap_or(0.0), "rs_tun_toward": colour, "rs_tun_two": pair.is_some(),
        "rs_tun_c0": pair.as_ref().map_or(dash, |p| &p[0]), "rs_tun_c1": pair.as_ref().map_or(dash, |p| &p[1]), "rs_tun_ratio": ratio})
        .as_object().unwrap().clone();
    let mut state = state;
    for i in 0..ovs.len() {
        state.insert(format!("rs_ov_{i}"), json!({}));
    }
    // _tunnel_look: everything on a tunnel moves toward the tunnel colour
    if any_tunnel {
        for l in layers.iter_mut().filter(|l| matches!(l["source"].as_str(), Some("roads" | "casings" | "halves" | "slots" | "arrows"))
                                            || l["id"].as_str().is_some_and(|i| edge_ids.contains(i))) {
            let id = l["id"].as_str().unwrap_or("").to_string();
            if id.starts_with("roads-fill") && !id.ends_with("-pat") {
                l["paint"]["line-color"] = tun_mix(&l["paint"]["line-color"]);
            } else if id.starts_with("roads-labels") {
                l["paint"]["text-color"] = tun_mix(&l["paint"]["text-color"]);
            } else if id.starts_with("roads-arrows") {
                l["paint"]["icon-color"] = tun_mix(&arw["color"]);
            } else if edge_ids.contains(&id) {
                for k in ["fill-color", "line-color", "circle-color", "text-color"] {
                    if let Some(c) = l["paint"].get(k).cloned() {
                        l["paint"][k] = tun_mix(&c);
                    }
                }
            }
        }
    }
    // the export: the state baked; fewer arrows, one in the middle of its slot; each layer's group
    let main: Vec<String> = arw["main"].as_array().into_iter().flatten().filter_map(Value::as_str)
        .flat_map(|h| [h.to_string(), format!("{h}_link")]).collect();
    let mut out = Vec::new();
    for l in layers {
        let mut l = l.as_object().unwrap().clone();
        for k in ["paint", "layout", "filter"] {
            if let Some(v) = l.get(k) {
                let b = bake(v, &state);
                l.insert(k.into(), b);
            }
        }
        if l["id"].as_str().is_some_and(|i| i.starts_with("roads-arrows")) {
            if l["paint"].get("icon-color").is_none() {
                l["paint"]["icon-color"] = arw["color"].clone();
            }
            l["filter"] = json!(["all", l["filter"], ["has", "__rs_arw"], ["any", ["match", ["get", "highway"], main, true, false], [">=", ["zoom"], arw["main_below"]]],
                                 ["==", ["%", ["get", "__rs_arw"], ["step", ["zoom"], 4, 16, 2, 17, 1]], 0]]);
            let lay = l["layout"].as_object_mut().unwrap();
            lay.shift_remove("symbol-spacing");
            lay.insert("symbol-placement".into(), "line-center".into());
        }
        // _tag_groups: the group of a layer is its overlay's label, else "roads"
        let group = l["source"].as_str().and_then(|s| s.strip_prefix("ov")).and_then(|n| n.parse::<usize>().ok())
            .and_then(|i| ovs.get(i)).map_or("roads".to_string(), |ov| ov.name.clone());
        let meta = l.entry("metadata").or_insert_with(|| json!({}));
        meta["roadstyle:group"] = group.into();
        out.push(Value::Object(l));
    }
    Ok(if ovs.is_empty() { out } else { restyle(out, ovs) })
}

fn kind_match(colors: &Value, default: &Value) -> Value {
    match colors.as_object().filter(|m| !m.is_empty()) {
        None => default.clone(),
        Some(m) => {
            let mut e = vec![json!("match"), json!(["get", "kind"])];
            for (k, c) in m {
                e.push(json!(k));
                e.push(c.clone());
            }
            e.push(default.clone());
            Value::Array(e)
        }
    }
}

/// mapstyle.export._swap: a hover / select `case` kept around a new base colour.
fn ms_swap(expr: Option<&Value>, value: Value) -> Value {
    match expr.and_then(Value::as_array) {
        Some(a) if a.first() == Some(&json!("case")) => {
            let mut a = a.clone();
            a.pop();
            a.push(value);
            Value::Array(a)
        }
        _ => value,
    }
}

/// mapstyle.export._restyle: the base-map layers styled as layers.js styles them on the page (colour and outline by kind, dashes, textures,
/// icons, 3D buildings, zoom ranges). A 3D layer goes under the arrows, names and highlight; the others right after the layer they belong to.
fn restyle(layers: Vec<Value>, ovs: &[Overlay]) -> Vec<Value> {
    let mut layers = layers;
    let mut by_ov: IndexMap<usize, Vec<usize>> = IndexMap::new();
    for (k, l) in layers.iter().enumerate() {
        if let Some(i) = l["source"].as_str().and_then(|s| s.strip_prefix("ov")).and_then(|n| n.parse::<usize>().ok()) {
            by_ov.entry(i).or_default().push(k);
        }
    }
    let mut extra: IndexMap<String, Vec<Value>> = IndexMap::new();
    for (i, lys) in &by_ov {
        let ll = &ovs[*i].l;
        let first = layers[lys[0]].clone();
        let body_type = first["type"].as_str().unwrap_or("").to_string();
        let bodies: Vec<usize> = lys.iter().copied().filter(|&k| layers[k]["type"] == body_type.as_str()).collect();
        let meta = first.get("metadata").cloned().unwrap_or(json!({}));
        let vis = first["layout"].get("visibility").cloned().unwrap_or(json!("visible"));
        let p = match body_type.as_str() { "fill" => Some("fill-color"), "line" => Some("line-color"), "circle" => Some("circle-color"), _ => None };
        for &b in &bodies {
            if let (Some(cb), Some(p)) = (ll.get("color_by").filter(|v| py_truth(v)), p) {
                let v = ms_swap(layers[b]["paint"].get(p), kind_match(&cb["map"], &cb["default"]));
                layers[b]["paint"][p] = v;
            }
            if ll.get("dash").is_some_and(py_truth) && body_type == "line" {
                layers[b]["paint"]["line-dasharray"] = ll["dash"].clone();
            }
            if let Some(wz) = ll.get("width_zoom").and_then(Value::as_array).filter(|a| !a.is_empty()) {
                if body_type == "line" {
                    let mut e = vec![json!("interpolate"), json!(["linear"]), json!(["zoom"])];
                    e.extend(wz.iter().flat_map(|s| s.as_array().cloned().unwrap_or_default()));
                    layers[b]["paint"]["line-width"] = Value::Array(e);
                }
            }
        }
        if let Some(ob) = ll.get("outline_by").filter(|v| py_truth(v)) {
            for &k in lys {
                if layers[k]["id"].as_str().is_some_and(|x| x.ends_with("-outline")) {
                    layers[k]["paint"]["line-color"] = kind_match(&ob["map"], &ob["default"]);
                }
            }
        }
        let mut news: Vec<(String, Value)> = Vec::new(); // (after which layer, the spec)
        let source = first["source"].clone();
        let new = |spec: Value| -> Value {
            let mut s = spec.as_object().unwrap().clone();
            s.insert("source".into(), source.clone());
            let mut m = meta.as_object().cloned().unwrap_or_default();
            for (k, v) in spec.get("metadata").and_then(Value::as_object).into_iter().flatten() {
                m.insert(k.clone(), v.clone());
            }
            s.insert("metadata".into(), Value::Object(m));
            let mut lay = spec.get("layout").and_then(Value::as_object).cloned().unwrap_or_default();
            if !lay.contains_key("visibility") {
                lay.insert("visibility".into(), vis.clone());
            }
            s.insert("layout".into(), Value::Object(lay));
            Value::Object(s)
        };
        if let Some(pats) = ll.get("patterns").and_then(Value::as_object).filter(|m| !m.is_empty()) {
            let keys: Vec<Value> = pats.keys().map(|k| json!(k)).collect();
            let mut fp = vec![json!("match"), json!(["get", "kind"])];
            for (k, v) in pats {
                fp.push(json!(k));
                fp.push(v.clone());
            }
            fp.push(pats.values().next().unwrap().clone());
            news.push((layers[bodies[0]]["id"].as_str().unwrap().to_string(),
                       new(json!({"id": format!("ov{i}-pattern"), "type": "fill", "filter": ["match", ["get", "kind"], keys, true, false],
                                  "paint": {"fill-pattern": fp}}))));
        }
        if let Some(ic) = ll.get("icon").filter(|v| py_truth(v)) {
            let pairs = ic["size"].as_array().cloned().unwrap_or_default();
            let curve = |f: f64| -> Value {
                let mut e = vec![json!("interpolate"), json!(["linear"]), json!(["zoom"])];
                for zs in &pairs {
                    e.push(zs[0].clone());
                    e.push(json!(zs[1].as_f64().unwrap() * f));
                }
                Value::Array(e)
            };
            let on_edge = bodies.iter().any(|&b| layers[b].get("filter").is_some_and(|f| f.to_string().contains("__rs_fl")));
            for (k, &b) in bodies.iter().enumerate() {
                let mut spec = json!({"id": format!("ov{i}-icon{}", if k == 0 { String::new() } else { k.to_string() }), "type": "symbol",
                                      "layout": {"icon-image": ic["image"], "icon-allow-overlap": true, "icon-size": curve(1.0),
                                                 "icon-rotate": ["coalesce", ["get", "bearing"], 0], "icon-rotation-alignment": "map"},
                                      "paint": {"icon-opacity": ic["opacity"]}});
                if on_edge {
                    if let Some(f) = layers[b].get("filter") {
                        spec["filter"] = f.clone();
                    }
                }
                news.push((layers[b]["id"].as_str().unwrap().to_string(), new(spec)));
                layers[b]["paint"]["circle-opacity"] = json!(0);
                layers[b]["paint"]["circle-stroke-width"] = json!(0);
                layers[b]["paint"]["circle-radius"] = curve(24.0);
            }
        }
        if let Some(ex) = ll.get("extrude").filter(|v| py_truth(v)) {
            let last = news.last().map(|n| n.1["id"].as_str().unwrap().to_string()).unwrap_or_else(|| layers[*lys.last().unwrap()]["id"].as_str().unwrap().to_string());
            news.push((last, new(json!({"id": format!("ov{i}-3d"), "type": "fill-extrusion", "metadata": {"roadstyle:group": "buildings 3D", "roadstyle:over": true},
                                        "layout": {"visibility": "none"},
                                        "paint": {"fill-extrusion-color": ex["color"], "fill-extrusion-height": ["get", "height"], "fill-extrusion-opacity": ex["opacity"]}}))));
        }
        if let Some(mz) = ll.get("min_zoom").filter(|v| py_truth(v)) {
            for &k in lys {
                layers[k]["minzoom"] = mz.clone();
            }
            for n in news.iter_mut() {
                n.1["minzoom"] = mz.clone();
            }
        }
        for (after, spec) in news {
            extra.entry(after).or_default().push(spec);
        }
    }
    let is_over = |x: &Value| x["metadata"].get("roadstyle:over").is_some_and(py_truth);
    let mut out = Vec::new();
    for l in layers {
        let id = l["id"].as_str().unwrap_or("").to_string();
        out.push(l);
        if let Some(xs) = extra.get(&id) {
            out.extend(xs.iter().filter(|x| !is_over(x)).cloned());
        }
    }
    let over: Vec<Value> = extra.values().flatten().filter(|x| is_over(x)).cloned().collect();
    if !over.is_empty() {
        let at = out.iter().position(|x| matches!(x["id"].as_str(), Some("roads-arrows" | "roads-labels" | "roads-highlight"))).unwrap_or(out.len());
        out.splice(at..at, over);
    }
    out
}

/// _label_base_px: the size every name had (labels.base_size: px at two zooms).
fn label_base_px(th: &Value, z: f64) -> f64 {
    let b = &th["labels"]["base_size"];
    let (z0, p0, z1, p1) = (b[0][0].as_f64().unwrap(), b[0][1].as_f64().unwrap(), b[1][0].as_f64().unwrap(), b[1][1].as_f64().unwrap());
    p0 + (p1 - p0) * ((z - z0) / (z1 - z0)).clamp(0.0, 1.0)
}

/// _label_px: a name's size: at most the base size, labels.fraction of its road's fill width.
fn label_px(m: &Model, th: &Value, cls: Option<&str>, z: f64) -> f64 {
    label_base_px(th, z).min(th["labels"]["fraction"].as_f64().unwrap() * m.class_width_px(cls, z, false))
}

/// _label_size_expr: text-size per class (or labels.size's own stops).
fn label_size_expr(m: &Model, th: &Value, fixed: Option<&Value>) -> Value {
    if let Some(stops) = fixed {
        let mut s: Vec<Vec<f64>> = stops.as_array().unwrap().iter().map(|zp| zp.as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect()).collect();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut e = vec![json!("interpolate"), json!(["linear"]), json!(["zoom"])];
        e.extend(s.into_iter().flatten().map(|v| json!(v)));
        return Value::Array(e);
    }
    let mut zs: Vec<i64> = m.zstops.iter().filter_map(Value::as_f64).filter(|z| *z >= 14.0).map(|z| z as i64).collect();
    zs.extend([14, 18]);
    zs.sort();
    zs.dedup();
    let mut e = vec![json!("interpolate"), json!(["linear"]), json!(["zoom"])];
    for z in zs {
        let mut mm = vec![json!("match"), json!(["get", "highway"])];
        for c in &m.classes {
            mm.push(json!(c));
            mm.push(json!(py_round(label_px(m, th, Some(c), z as f64), 2)));
        }
        mm.push(json!(py_round(label_px(m, th, None, z as f64), 2)));
        e.push(json!(z));
        e.push(Value::Array(mm));
    }
    Value::Array(e)
}

/// _label_readable_filter: the first whole zoom from which a class's name is at least labels.min_px tall.
fn label_readable_filter(m: &Model, th: &Value) -> Value {
    let min = th["labels"]["min_px"].as_f64().unwrap();
    let first = |c: Option<&str>| (0..23).find(|&z| label_px(m, th, c, z as f64) >= min).unwrap_or(99);
    let mut mm = vec![json!("match"), json!(["get", "highway"])];
    for c in &m.classes {
        mm.push(json!(c));
        mm.push(json!(first(Some(c))));
    }
    mm.push(json!(first(None)));
    json!([">=", ["zoom"], mm])
}
