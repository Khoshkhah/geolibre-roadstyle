//! The road layers (full port step 2, docs/design/rust-spike.md): the expression builders of roadstyle's render_web, over the road tables of
//! `_load_road_model` (look.json "roads_model" and "theme"). Each function names the Python function it ports.

use serde_json::{json, Map, Value};
use std::collections::HashMap;

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
            offset_frac: num(&split["offset_frac"], "split.offset_frac")?,
            width_frac: num(&split["width_frac"], "split.width_frac")?,
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
