//! The "simple" source: simple mode's one road layer, every casing piece and every fill (render_web._casing_parts, _halves, _end_stub,
//! _simple_pieces), from the roads after their stages (roads::mark_roads). The geometry is computed as numpy computes it: a flat
//! projection at the line's first point, sequential sums, `hypot`, and numpy's rounding (`rint(x * 10^d) / 10^d`).
//! Not ported: _mitre_ends (it moves flat ends only for roads with metre widths, `__rs_wm`, and for items; this data has neither).

use crate::read::Feature;
use crate::roads::truthy;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

const SEAM_M: f64 = 0.5; // a seam reaches this far each way from its cut, at most a quarter of either piece
const LAP_M: f64 = 2.0; // a lap (the flat seam below zoom 17) reaches this far each way, at most half of either piece
const PIECES: usize = 24; // the most pieces of one edge (the feature id is edge * PIECES + n)
const TIE: f64 = 1e-8; // the line-sort-key's tie-breaker per edge
const MAX_EDGES: usize = 1_000_000;

pub(crate) type Pt = [f64; 2];

struct Piece {
    props: Map<String, Value>,
    coords: Option<Vec<Pt>>,
}

/// np.round(x, d) for d > 0: rint(x * 10^d) / 10^d.
fn np_round(x: f64, scale: f64) -> f64 {
    (x * scale).round_ties_even() / scale
}

/// A line in metres from its first point (as _casing_parts and _halves project it), with its cumulative lengths.
pub(crate) struct Line {
    pub(crate) xy: Vec<Pt>,
    pub(crate) cum: Vec<f64>,
    pub(crate) lon0: f64,
    pub(crate) lat0: f64,
    pub(crate) kx: f64,
}

pub(crate) const KY: f64 = 111320.0;

impl Line {
    pub(crate) fn new(c: &[Pt]) -> Line {
        let (lon0, lat0) = (c[0][0], c[0][1]);
        let kx = 111320.0 * lat0.to_radians().cos();
        let xy: Vec<Pt> = c.iter().map(|p| [(p[0] - lon0) * kx, (p[1] - lat0) * KY]).collect();
        let mut cum = Vec::with_capacity(xy.len());
        let mut s = 0.0;
        cum.push(0.0);
        for w in xy.windows(2) {
            s += (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            cum.push(s);
        }
        Line { xy, cum, lon0, lat0, kx }
    }

    pub(crate) fn len(&self) -> f64 {
        self.cum[self.cum.len() - 1]
    }

    // np.searchsorted(cum, d, side="right") / side="left"
    fn right(&self, d: f64) -> usize {
        self.cum.partition_point(|&x| x <= d)
    }
    fn left(&self, d: f64) -> usize {
        self.cum.partition_point(|&x| x < d)
    }

    /// _at: the point at the distance d.
    pub(crate) fn at(&self, d: f64) -> Pt {
        let j = (self.right(d) as isize - 1).clamp(0, self.cum.len() as isize - 2) as usize;
        let w = self.cum[j + 1] - self.cum[j];
        let t = if w > 0.0 { (d - self.cum[j]) / w } else { 0.0 };
        let (p, q) = (self.xy[j], self.xy[j + 1]);
        [p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]
    }

    /// _part: the cut points at a and b and the vertices between them.
    pub(crate) fn part(&self, a: f64, b: f64) -> Vec<Pt> {
        let mut out = vec![self.at(a)];
        let (i, j) = (self.right(a), self.left(b));
        if i < j {
            out.extend_from_slice(&self.xy[i..j]);
        }
        out.push(self.at(b));
        out
    }

    /// Back to lon / lat, rounded as numpy rounds (scale 1e7 or 1e8).
    pub(crate) fn lonlat(&self, pts: &[Pt], scale: f64) -> Vec<Pt> {
        pts.iter().map(|p| [np_round(p[0] / self.kx + self.lon0, scale), np_round(p[1] / KY + self.lat0, scale)]).collect()
    }
}

fn int(p: &Map<String, Value>, k: &str) -> i64 {
    p.get(k).and_then(Value::as_i64).unwrap_or(0)
}

fn is_true(v: Option<&Value>) -> bool {
    v == Some(&Value::Bool(true))
}

/// _casing_parts: the casing of every edge as pieces. An edge whose three casing numbers differ (or with two different ends) is cut into
/// its start head, its main part and its end head, with a seam (and a lap, not on a dashed or tunnel casing) at each cut inside it.
fn casing_parts(roads: &[Feature], head_m: f64) -> Vec<Piece> {
    let mut out = Vec::new();
    for (i, f) in roads.iter().enumerate() {
        let p = &f.props;
        if truthy(p.get("__rs_dup")) {
            continue; // one line for a reverse pair: its twin draws it
        }
        let mut base: Map<String, Value> = p.iter()
            .filter(|(k, _)| k.as_str() == "highway" || k.as_str() == "lvl" || (k.starts_with("__rs_") && !k.starts_with("__rs_fill")))
            .map(|(k, v)| (k.clone(), v.clone())).collect();
        let edge = p.get("__rs_edge").and_then(Value::as_i64).unwrap_or(i as i64);
        base.insert("__rs_edge".into(), edge.into());
        if let Some(tw) = p.get("__rs_twin").and_then(Value::as_i64) {
            if tw < edge {
                continue; // a two-way pair's one casing: the first edge's pieces
            }
            base.insert("__rs_pair".into(), true.into());
            base.insert("__rs_edge2".into(), tw.into());
        }
        let (cs, cm, ce) = (int(p, "__rs_cs"), int(p, "__rs_cl"), int(p, "__rs_ce"));
        let split = truthy(p.get("__rs_split"));
        let c = match &f.coords {
            Some(c) if c.len() >= 2 && !(cs == cm && cm == ce && !split) => c,
            _ => {
                out.push(Piece { props: base, coords: f.coords.clone() });
                continue;
            }
        };
        let line = Line::new(c);
        let n = line.len();
        let (h0, h1) = (p.get("__rs_hs").and_then(Value::as_f64).unwrap_or(head_m), p.get("__rs_he").and_then(Value::as_f64).unwrap_or(head_m));
        // k: 0 the start head, 1 the main part, 2 the end head. A short road is cut once, in the heads' ratio (roadstyle after 0.24.2,
        // short_road_heads.md "One cut"): two cuts at h0 and n - h1 are one point only in exact arithmetic
        let cuts: Vec<(f64, f64, i64, usize)> = if h0 + h1 >= n - 0.05 {
            let h0 = n * h0 / (h0 + h1);
            vec![(0.0, h0, cs, 0), (h0, n, ce, 2)]
        } else {
            vec![(0.0, h0, cs, 0), (h0, n - h1, cm, 1), (n - h1, n, ce, 2)]
        };
        for &(a, b, num, k) in &cuts {
            let pts = line.part(a, b);
            if pts.len() < 2 || !pts.windows(2).any(|w| w[0] != w[1]) {
                continue;
            }
            let mut q = base.clone();
            q.insert("__rs_cl".into(), num.into());
            if k == 1 {
                // the main piece ends at two cuts: flat ends
                q.insert("__rs_cap".into(), true.into());
                q.insert("__rs_main".into(), true.into());
            } else if split {
                // a head of an edge with two different ends: that end's cap
                q.insert("__rs_cap".into(), p.get(if k == 0 { "__rs_cap0" } else { "__rs_cap1" }).cloned().unwrap_or(Value::Null));
            }
            if q.get("__rs_cap").map_or(true, Value::is_null) {
                q.shift_remove("__rs_cap");
            }
            out.push(Piece { props: q, coords: Some(line.lonlat(&pts, 1e7)) });
        }
        // a seam at each cut inside the edge, at the lower of the two pieces' numbers; below zoom 17 a flat lap instead
        let mut cut_at: Vec<f64> = cuts[1..].iter().map(|c| c.0).collect();
        cut_at.retain(|&x| x != 0.0 && x != n);
        cut_at.sort_by(f64::total_cmp);
        cut_at.dedup_by(|a, b| a == b);
        for &cut in &cut_at {
            let near: Vec<&(f64, f64, i64, usize)> = cuts.iter().filter(|(a, b, _, _)| b - a > 1e-9 && ((b - cut).abs() < 1e-9 || (a - cut).abs() < 1e-9)).collect();
            let half = near.iter().map(|(a, b, _, _)| (b - a) / 4.0).fold(SEAM_M, f64::min);
            let pts = line.part((cut - half).max(0.0), (cut + half).min(n));
            if near.len() < 2 || pts.len() < 2 {
                continue;
            }
            let mut q = base.clone();
            q.insert("__rs_cl".into(), near.iter().map(|x| x.2).min().unwrap().into());
            q.insert("__rs_seam".into(), true.into());
            q.shift_remove("__rs_cap");
            out.push(Piece { props: q.clone(), coords: Some(line.lonlat(&pts, 1e8)) });
            if truthy(base.get("__rs_dash")) || truthy(base.get("__rs_tunnel")) {
                continue; // a solid lap would fill a dashed casing's gaps
            }
            let half = near.iter().map(|(a, b, _, _)| (b - a) / 2.0).fold(LAP_M, f64::min);
            let pts = line.part((cut - half).max(0.0), (cut + half).min(n));
            q.insert("__rs_cap".into(), true.into());
            q.insert("__rs_lap".into(), true.into());
            out.push(Piece { props: q, coords: Some(line.lonlat(&pts, 1e7)) });
        }
    }
    out
}

/// _halves: the fill of every edge with two different ends (__rs_split) as two halves cut at the middle, each with its end's cap.
fn halves(roads: &[Feature]) -> HashMap<i64, Vec<Piece>> {
    let mut out: HashMap<i64, Vec<Piece>> = HashMap::new();
    for (i, f) in roads.iter().enumerate() {
        let p = &f.props;
        let Some(c) = f.coords.as_ref().filter(|c| c.len() >= 2) else { continue };
        if !truthy(p.get("__rs_split")) || truthy(p.get("__rs_dup")) {
            continue;
        }
        let line = Line::new(c);
        let n = line.len();
        let edge = p.get("__rs_edge").and_then(Value::as_i64).unwrap_or(i as i64);
        for (k, (a, b)) in [(0.0, n / 2.0), (n / 2.0, n)].into_iter().enumerate() {
            let mut q: Map<String, Value> = p.iter().filter(|(kk, _)| !matches!(kk.as_str(), "__rs_cap0" | "__rs_cap1" | "__rs_split"))
                .map(|(kk, v)| (kk.clone(), v.clone())).collect();
            q.insert("__rs_edge".into(), edge.into());
            let cap = p.get(if k == 0 { "__rs_cap0" } else { "__rs_cap1" }).cloned().unwrap_or(Value::Null);
            if !cap.is_null() {
                q.insert("__rs_cap".into(), cap);
            }
            out.entry(edge).or_default().push(Piece { props: q, coords: Some(line.lonlat(&line.part(a, b), 1e7)) });
        }
    }
    out
}

/// _end_stub: a piece with one road end and one cut whose road end is not flat is drawn flat, plus a short stub at the road end that
/// carries the end's shape (0.5 m for a square end, 5 cm for a round one). Any other piece as it is.
fn end_stub(q: Piece, line_of: &[Option<&Vec<Pt>>]) -> Vec<Piece> {
    let edge = q.props.get("__rs_edge").and_then(Value::as_i64).unwrap_or(-1);
    let e = usize::try_from(edge).ok().and_then(|e| line_of.get(e).copied().flatten());
    let at = |a: &Pt, b: &Pt| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6;
    let (Some(c), Some(e)) = (q.coords.as_ref(), e) else { return vec![q] };
    if c.len() < 2 || e.len() < 2 || at(&c[0], &e[0]) == at(&c[c.len() - 1], &e[e.len() - 1]) {
        return vec![q];
    }
    let first = at(&c[0], &e[0]);
    let (end, prev) = if first { (c[0], c[1]) } else { (c[c.len() - 1], c[c.len() - 2]) };
    let m = if q.props.get("__rs_cap") == Some(&Value::from("square")) { 0.5 } else { 0.05 };
    let d = ((prev[0] - end[0]) * 111320.0 * end[1].to_radians().cos()).hypot((prev[1] - end[1]) * 111320.0);
    let k = (m / d.max(1e-6)).min(1.0);
    let inner = [end[0] + (prev[0] - end[0]) * k, end[1] + (prev[1] - end[1]) * k];
    let stub = if first { vec![end, inner] } else { vec![inner, end] };
    let mut flat = q.props.clone();
    flat.insert("__rs_cap".into(), true.into());
    let mut sp = q.props;
    sp.insert("__rs_stub".into(), true.into());
    vec![Piece { props: flat, coords: q.coords }, Piece { props: sp, coords: Some(stub) }]
}

fn with(mut p: Map<String, Value>, k: i64, s: f64) -> Map<String, Value> {
    p.insert("__rs_k".into(), k.into());
    p.insert("__rs_s".into(), s.into());
    p
}

/// The "simple" source: _simple_pieces (casing pieces __rs_k 0, bridge shadows 2, tunnel gap 3 and dashes 4, fills 1), the feature ids
/// (edge * PIECES + n), the tie-breaker per edge and __rs_cls (_mark_cls).
pub fn simple_source(roads: &[Feature], head_m: f64, shadows: bool) -> Result<Value, String> {
    if roads.iter().any(|f| f.props.contains_key("__rs_wm")) {
        return Err("roads with metre widths (__rs_wm): _mitre_ends is not ported".into());
    }
    let line_of: Vec<Option<&Vec<Pt>>> = roads.iter().map(|f| f.coords.as_ref()).collect();
    let mut halves = halves(roads);
    let mut out: Vec<Piece> = Vec::new();
    for q0 in casing_parts(roads, head_m) {
        let stub = !is_true(q0.props.get("__rs_cap")) && !truthy(q0.props.get("__rs_seam"));
        for q in if stub { end_stub(q0, &line_of) } else { vec![q0] } {
            let p = &q.props;
            if truthy(p.get("__rs_dash")) || (truthy(p.get("__rs_seam")) && truthy(p.get("__rs_tunnel"))) {
                continue; // a dashed class has no casing; a tunnel's casing is its dashes alone
            }
            let bridge = truthy(p.get("__rs_bridge"));
            let k = 2.0 * int(p, "__rs_cl") as f64 + if bridge { 0.25 } else { 0.0 };
            let whole = int(p, "__rs_cs") == int(p, "__rs_cl") && int(p, "__rs_cl") == int(p, "__rs_ce");
            if shadows && bridge && (truthy(p.get("__rs_main")) || whole) {
                out.push(Piece { props: with(p.clone(), 2, k - 0.1), coords: q.coords.clone() });
            }
            if truthy(p.get("__rs_tunnel")) && !bridge {
                out.push(Piece { props: with(p.clone(), 3, k), coords: q.coords.clone() });
                out.push(Piece { props: with(q.props, 4, k + 0.1), coords: q.coords });
            } else {
                out.push(Piece { props: with(q.props, 0, k), coords: q.coords });
            }
        }
    }
    for (i, f) in roads.iter().enumerate() {
        let p = &f.props;
        if truthy(p.get("__rs_dup")) {
            continue;
        }
        let edge = p.get("__rs_edge").and_then(Value::as_i64).unwrap_or(i as i64);
        let fills: Vec<Piece> = if truthy(p.get("__rs_split")) {
            halves.remove(&edge).unwrap_or_default().into_iter()
                .flat_map(|h| if is_true(h.props.get("__rs_cap")) { vec![h] } else { end_stub(h, &line_of) }).collect()
        } else {
            vec![Piece { props: p.clone(), coords: f.coords.clone() }]
        };
        for h in fills {
            let props: Map<String, Value> = h.props.into_iter().filter(|(k, _)| k == "highway" || k == "lvl" || k.starts_with("__rs_")).collect();
            let s = 2.0 * int(&props, "__rs_fl") as f64 + if truthy(props.get("__rs_dash")) { -0.5 } else { 1.0 };
            out.push(Piece { props: with(props, 1, s), coords: h.coords });
        }
    }
    let mut n: HashMap<i64, usize> = HashMap::new();
    let mut features = Vec::with_capacity(out.len());
    for mut f in out {
        let e = f.props.get("__rs_edge").and_then(Value::as_i64).unwrap_or(0);
        let c = n.entry(e).or_default();
        if *c == PIECES {
            return Err(format!("edge {e} has more than {PIECES} pieces"));
        }
        if e as usize >= MAX_EDGES {
            return Err(format!("more than {MAX_EDGES} edges"));
        }
        let id = e as usize * PIECES + *c;
        *c += 1;
        let s = f.props["__rs_s"].as_f64().unwrap_or(0.0) + e as f64 * TIE;
        f.props.insert("__rs_s".into(), s.into());
        let cls = roads.get(e as usize).and_then(|r| r.props.get("highway")).cloned().unwrap_or(Value::Null);
        f.props.insert("__rs_cls".into(), cls);
        let geometry = f.coords.map_or(Value::Null, |c| json!({"type": "LineString", "coordinates": c}));
        features.push(crate::obj([("type", "Feature".into()), ("geometry", geometry), ("properties", Value::Object(f.props)), ("id", id.into())]));
    }
    Ok(crate::obj([("type", "geojson".into()), ("data", crate::obj([("type", "FeatureCollection".into()), ("features", Value::Array(features))])),
                   ("tolerance", 0.05.into())]))
}
