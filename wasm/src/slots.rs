//! The "slots" source: the names and the one-way arrows (render_web._annotation_slots, then the export's `__rs_arw` and _mark_cls), from
//! the roads after their stages (roads::mark_roads). Spike S6 (docs/design/rust-spike.md).
//!
//! The graph half (representatives, groups, chains, chain numbers) is the same as Python's. The geometry half replaces shapely: an R-tree
//! (rstar) for STRtree, and segment intersection, projection and point-to-line distance written as GEOS computes them where it matters
//! (the first nearest segment wins a tie; lengths along a line are sums of `sqrt(dx * dx + dy * dy)`). Intersection points are not computed
//! with GEOS's arithmetic, so a slot can differ from Python's by a rounding step of its 6 decimals.

use crate::read::Feature;
use crate::roads::truthy;
use crate::simple::{Line, Pt, KY};
use indexmap::IndexMap;
use rstar::{primitives::GeomWithData, primitives::Rectangle, RTree, AABB};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

const NAME_MARGIN_M: f64 = 2.0; // a name or an arrow stays this far beyond a crossing road's drawn half-width
const ZEBRA_HALF_M: f64 = 2.0; // ... and at least this far from a footway, path, cycleway or steps
const ARROW_MIN_M: f64 = 8.0; // a one-way stretch too short for a name slot still gets an arrow slot from this length

/// The look data the slots read (look.json "slots" and config.annotations).
pub struct SlotLook {
    z_order: Map<String, Value>,
    width_z20: HashMap<String, f64>,
    width_fallback: f64,
    slot_m: f64,
}

impl SlotLook {
    pub fn from_json(look: &Value) -> Result<SlotLook, String> {
        let s = &look["slots"];
        Ok(SlotLook {
            z_order: s["z_order"].as_object().cloned().ok_or("look.json: no slots.z_order")?,
            width_z20: s["width_z20"].as_object().ok_or("look.json: no slots.width_z20")?.iter()
                .filter_map(|(k, v)| v.as_f64().map(|w| (k.clone(), w))).collect(),
            width_fallback: s["width_z20_fallback"].as_f64().ok_or("look.json: no slots.width_z20_fallback")?,
            slot_m: look["config"]["annotations"]["slot_m"].as_f64().unwrap_or(100.0),
        })
    }

    /// _crossing_half_m: half the class's drawn width at zoom 20 in metres at `lat` (a path class: at least a zebra's half-width), plus the margin.
    fn crossing_half_m(&self, cls: Option<&str>, lat: f64) -> f64 {
        let px = cls.and_then(|c| self.width_z20.get(c)).copied().unwrap_or(self.width_fallback);
        let half = 0.5 * px * 40075016.686 * lat.to_radians().cos() / (512.0 * 2f64.powi(20));
        let path = matches!(cls, Some("footway" | "path" | "cycleway" | "steps"));
        (if path { half.max(ZEBRA_HALF_M) } else { half }) + NAME_MARGIN_M
    }
}

/// Python's round(x, 6) as a float: the nearest double to the correctly rounded decimal (no tie can occur at 6 places).
fn round6(x: f64) -> f64 {
    let v: f64 = format!("{x:.6}").parse().unwrap();
    if v == 0.0 { 0.0 } else { v }
}

type Node = (u64, u64); // a rounded point as a key (the bits of the two floats)

fn node(c: &Pt) -> (Node, (f64, f64)) {
    let (x, y) = (round6(c[0]), round6(c[1]));
    ((x.to_bits(), y.to_bits()), (x, y))
}

fn class_key(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

/// Python's divmod for floats (CPython float_divmod): the floor quotient and the modulo, as `length // slot_m` and `length % slot_m` give them.
fn py_divmod(vx: f64, wx: f64) -> (f64, f64) {
    let mut m = vx % wx;
    let mut div = (vx - m) / wx;
    if m != 0.0 {
        if (wx < 0.0) != (m < 0.0) {
            m += wx;
            div -= 1.0;
        }
    } else {
        m = 0f64.copysign(wx);
    }
    let floordiv = if div != 0.0 {
        let f = div.floor();
        if div - f > 0.5 { f + 1.0 } else { f }
    } else {
        0f64.copysign(vx / wx)
    };
    (floordiv, m)
}

// --- geometry (in lon / lat, or in metres for the chain) ---

fn cross(o: Pt, a: Pt, b: Pt) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

fn sign(v: f64) -> i8 {
    if v > 0.0 { 1 } else if v < 0.0 { -1 } else { 0 }
}

fn in_box(p: Pt, a: Pt, b: Pt) -> bool {
    p[0] >= a[0].min(b[0]) && p[0] <= a[0].max(b[0]) && p[1] >= a[1].min(b[1]) && p[1] <= a[1].max(b[1])
}

/// The points where two segments meet: none, one (a crossing or a touch) or the two ends of their common part (collinear).
fn seg_meet(p1: Pt, p2: Pt, q1: Pt, q2: Pt) -> Vec<Pt> {
    if !(p1[0].min(p2[0]) <= q1[0].max(q2[0]) && q1[0].min(q2[0]) <= p1[0].max(p2[0])
        && p1[1].min(p2[1]) <= q1[1].max(q2[1]) && q1[1].min(q2[1]) <= p1[1].max(p2[1])) {
        return vec![];
    }
    let (o1, o2) = (sign(cross(p1, p2, q1)), sign(cross(p1, p2, q2)));
    let (o3, o4) = (sign(cross(q1, q2, p1)), sign(cross(q1, q2, p2)));
    if o1 == 0 && o2 == 0 && o3 == 0 && o4 == 0 {
        // collinear: the common part, along the longer axis of p
        let ax = if (p2[0] - p1[0]).abs() >= (p2[1] - p1[1]).abs() { 0 } else { 1 };
        let mut pts: Vec<Pt> = [p1, p2, q1, q2].into_iter()
            .filter(|&r| in_box(r, p1, p2) && in_box(r, q1, q2)).collect();
        pts.sort_by(|a, b| a[ax].total_cmp(&b[ax]));
        pts.dedup();
        return match pts.len() {
            0 => vec![],
            1 => pts,
            _ => vec![pts[0], pts[pts.len() - 1]],
        };
    }
    if o1 * o2 > 0 || o3 * o4 > 0 {
        return vec![];
    }
    // a touch at an end point: that point exactly
    for (o, r, a, b) in [(o1, q1, p1, p2), (o2, q2, p1, p2), (o3, p1, q1, q2), (o4, p2, q1, q2)] {
        if o == 0 && in_box(r, a, b) {
            return vec![r];
        }
    }
    let d = cross([0.0, 0.0], [p2[0] - p1[0], p2[1] - p1[1]], [q2[0] - q1[0], q2[1] - q1[1]]);
    if d == 0.0 {
        return vec![];
    }
    let t = cross([0.0, 0.0], [q1[0] - p1[0], q1[1] - p1[1]], [q2[0] - q1[0], q2[1] - q1[1]]) / d;
    vec![[p1[0] + t * (p2[0] - p1[0]), p1[1] + t * (p2[1] - p1[1])]]
}

fn dist(a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
    (dx * dx + dy * dy).sqrt()
}

/// GEOS Distance::pointToSegment.
fn point_seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    if a == b {
        return dist(p, a);
    }
    let len2 = (b[0] - a[0]) * (b[0] - a[0]) + (b[1] - a[1]) * (b[1] - a[1]);
    let r = ((p[0] - a[0]) * (b[0] - a[0]) + (p[1] - a[1]) * (b[1] - a[1])) / len2;
    if r <= 0.0 {
        return dist(p, a);
    }
    if r >= 1.0 {
        return dist(p, b);
    }
    let s = ((a[1] - p[1]) * (b[0] - a[0]) - (a[0] - p[0]) * (b[1] - a[1])) / len2;
    s.abs() * len2.sqrt()
}

pub(crate) fn point_line_dist(p: Pt, c: &[Pt]) -> f64 {
    c.windows(2).map(|w| point_seg_dist(p, w[0], w[1])).fold(f64::INFINITY, f64::min)
}

/// GEOS LengthIndexedLine::project (LengthIndexOfPoint): the length along `c` to the point of it nearest `p`; the first nearest segment
/// wins a tie; lengths are sums of segment lengths.
fn project(c: &[Pt], p: Pt) -> f64 {
    let (mut best, mut at, mut start) = (f64::INFINITY, 0.0, 0.0);
    for w in c.windows(2) {
        let len = dist(w[0], w[1]);
        let d = point_seg_dist(p, w[0], w[1]);
        if d < best {
            best = d;
            let r = if w[0] == w[1] { 0.0 } else {    // LineSegment::projectionFactor
                ((p[0] - w[0][0]) * (w[1][0] - w[0][0]) + (p[1] - w[0][1]) * (w[1][1] - w[0][1]))
                    / ((w[1][0] - w[0][0]) * (w[1][0] - w[0][0]) + (w[1][1] - w[0][1]) * (w[1][1] - w[0][1]))
            };
            at = if r <= 0.0 { start } else if r <= 1.0 { start + r * len } else { start + len };
        }
        start += len;
    }
    at
}

pub(crate) fn bbox(c: &[Pt]) -> ([f64; 2], [f64; 2]) {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in c {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

/// One road of a group: its ends (rounded), its line, and the road it belongs to (owner: its position and its twin's).
struct Rep {
    a: (Node, (f64, f64)),
    z: (Node, (f64, f64)),
    c: Vec<Pt>,
    p: usize, // the road's position
    owner: (usize, Option<usize>),
}

/// The "slots" source.
pub fn slots_source(roads: &[Feature], look: &SlotLook) -> Value {
    let hw_of = |i: usize| roads[i].props.get("highway");
    // every road's line: what a name or an arrow keeps away from
    let tree: RTree<GeomWithData<Rectangle<[f64; 2]>, usize>> = RTree::bulk_load(roads.iter().enumerate()
        .filter_map(|(i, f)| f.coords.as_ref().filter(|c| c.len() >= 2).map(|c| {
            let (lo, hi) = bbox(c);
            GeomWithData::new(Rectangle::from_corners(lo, hi), i)
        })).collect());

    // one representative per two-way pair; owner: the road and its twin (or its single-line partner)
    let mut by_ends: HashMap<(Node, Node, String), usize> = HashMap::new();
    let mut lines = Vec::new();
    for (i, f) in roads.iter().enumerate() {
        let Some(c) = f.coords.as_ref().filter(|c| c.len() >= 2) else { continue };
        let (a, z) = (node(&c[0]), node(&c[c.len() - 1]));
        by_ends.insert((a.0, z.0, class_key(hw_of(i))), i);
        lines.push((i, a, z));
    }
    let mut reps: Vec<Rep> = Vec::new();
    for (i, a, z) in lines {
        let p = &roads[i].props;
        if truthy(p.get("__rs_dup")) {
            continue;
        }
        let ck = class_key(hw_of(i));
        let twin = by_ends.get(&(z.0, a.0, ck.clone())).copied();
        if truthy(p.get("__rs_twoway")) {
            let q_one = twin.is_some_and(|j| truthy(roads[j].props.get("__rs_oneway")));
            let mine = (!truthy(p.get("__rs_oneway")), a.1, z.1);
            let other = (!q_one, z.1, a.1);
            if other.partial_cmp(&mine) == Some(std::cmp::Ordering::Less) {
                continue;
            }
        }
        let partner = if truthy(p.get("__rs_twoway")) { twin } else { p.get("__rs_edge2").and_then(Value::as_u64).map(|x| x as usize) };
        reps.push(Rep { a, z, c: roads[i].coords.clone().unwrap(), p: i, owner: (i, partner) });
    }

    // groups by (name, level, one-way, class, fill number, tunnel), in the order Python's dict keeps
    let mut groups: IndexMap<String, (Option<String>, Value, bool, Value, bool, Vec<usize>)> = IndexMap::new();
    for (r, rep) in reps.iter().enumerate() {
        let p = &roads[rep.p].props;
        let name = p.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
        let lvl = p.get("lvl").cloned().unwrap_or(json!(0));
        let one = truthy(p.get("__rs_oneway"));
        let fl = p.get("__rs_fl").cloned().unwrap_or(Value::Null);
        let tun = truthy(p.get("__rs_tunnel"));
        let key = json!([name, lvl, one as u8, class_key(hw_of(rep.p)), fl, tun]).to_string();
        groups.entry(key).or_insert_with(|| (name, lvl, one, fl, tun, Vec::new())).5.push(r);
    }

    let mut feats: Vec<Value> = Vec::new();
    for (_, (name, lvl, oneway, fl, tun, edges)) in &groups {
        let edges: Vec<&Rep> = edges.iter().map(|&r| &reps[r]).collect();
        let n = edges.len();
        let mut used = vec![false; n];
        let mut at: HashMap<Node, Vec<usize>> = HashMap::new();
        for (i, e) in edges.iter().enumerate() {
            at.entry(e.a.0).or_default().push(i);
            at.entry(e.z.0).or_default().push(i);
        }
        // chains through degree-2 nodes; a one-way chain only through directed continuations
        let mut chains: Vec<(Vec<Pt>, Vec<usize>)> = Vec::new();
        for start in 0..n {
            if used[start] {
                continue;
            }
            used[start] = true;
            let mut members = vec![start];
            let mut chain = edges[start].c.clone();
            for prepend in [false, true] {
                let mut nd = if prepend { edges[start].a.0 } else { edges[start].z.0 };
                loop {
                    let here = &at[&nd];
                    let cand: Vec<usize> = here.iter().copied().filter(|&j| !used[j]).collect();
                    if here.len() != 2 || cand.len() != 1 {
                        break;
                    }
                    let j = cand[0];
                    let e = edges[j];
                    let (seg, nxt): (Vec<Pt>, Node) = if prepend {
                        if e.z.0 == nd { (e.c.clone(), e.a.0) } else if !oneway { (e.c.iter().rev().copied().collect(), e.z.0) } else { break }
                    } else if e.a.0 == nd { (e.c.clone(), e.z.0) } else if !oneway { (e.c.iter().rev().copied().collect(), e.a.0) } else { break };
                    used[j] = true;
                    members.push(j);
                    if prepend {
                        let mut c2 = seg[..seg.len() - 1].to_vec();
                        c2.extend_from_slice(&chain);
                        chain = c2;
                    } else {
                        chain.extend_from_slice(&seg[1..]);
                    }
                    nd = nxt;
                }
            }
            chains.push((chain, members));
        }
        // the group's class: the most common (the first of equal counts)
        let mut count: IndexMap<String, (usize, Value)> = IndexMap::new();
        for e in &edges {
            let v = roads[e.p].props.get("highway").cloned().unwrap_or(Value::Null);
            count.entry(v.to_string()).or_insert((0, v)).0 += 1;
        }
        let mut hw = Value::Null;
        let mut best = 0;
        for (_, (k, v)) in &count {
            if *k > best {
                best = *k;
                hw = v.clone();
            }
        }
        let rank = hw.as_str().and_then(|h| look.z_order.get(h)).cloned().unwrap_or(json!(0));
        let near: Option<RTree<GeomWithData<Rectangle<[f64; 2]>, usize>>> = (n > 32).then(|| RTree::bulk_load(edges.iter().enumerate()
            .map(|(g, e)| { let (lo, hi) = bbox(&e.c); GeomWithData::new(Rectangle::from_corners(lo, hi), g) }).collect()));

        for (chain, members) in &chains {
            let cid = 1 + members.iter().map(|&e| edges[e].owner.0).min().unwrap();
            let line = Line::new(chain);
            let total = line.len();
            // the crossings: every other road that meets or crosses the chain takes half its width plus a margin out of it
            let mine: HashSet<usize> = members.iter().flat_map(|&e| [Some(edges[e].owner.0), edges[e].owner.1]).flatten().collect();
            let back: HashSet<Vec<(u64, u64)>> = members.iter()
                .map(|&e| edges[e].c.iter().rev().map(|p| (p[0].to_bits(), p[1].to_bits())).collect()).collect();
            let (lo, hi) = bbox(chain);
            let mut cuts: Vec<(f64, f64)> = Vec::new();
            // a long chain: an R-tree of its segments, else every road in its box is checked against all of them (Tartu: 2 s)
            let segs = (chain.len() > 16).then(|| RTree::bulk_load(chain.windows(2).enumerate()
                .map(|(i, w)| { let (lo, hi) = bbox(w); GeomWithData::new(Rectangle::from_corners(lo, hi), i) }).collect()));
            for hit in tree.locate_in_envelope_intersecting(&AABB::from_corners(lo, hi)) {
                let k = hit.data;
                let kc = roads[k].coords.as_ref().unwrap();
                if mine.contains(&k) || back.contains(&kc.iter().map(|p| (p[0].to_bits(), p[1].to_bits())).collect::<Vec<_>>()) {
                    continue;
                }
                let half = look.crossing_half_m(hw_of(k).and_then(Value::as_str), line.lat0);
                let mut meet = |w: &[Pt], v: &[Pt]| {
                    let pts = seg_meet(w[0], w[1], v[0], v[1]);
                    if !pts.is_empty() {
                        let d: Vec<f64> = pts.iter().map(|p| project(&line.xy, [(p[0] - line.lon0) * line.kx, (p[1] - line.lat0) * KY])).collect();
                        cuts.push((d.iter().copied().fold(f64::INFINITY, f64::min) - half, d.iter().copied().fold(f64::NEG_INFINITY, f64::max) + half));
                    }
                };
                for v in kc.windows(2) {
                    match &segs {
                        // only the chain's segments whose box meets this segment's box can meet it (the cuts are sorted below)
                        Some(t) => {
                            let (lo, hi) = bbox(v);
                            for h in t.locate_in_envelope_intersecting(&AABB::from_corners(lo, hi)) {
                                meet(&chain[h.data..h.data + 2], v);
                            }
                        }
                        None => chain.windows(2).for_each(|w| meet(w, v)),
                    }
                }
            }
            cuts.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut free = Vec::new();
            let mut at_ = 0.0f64;
            for &(lo, hi) in &cuts {
                if lo > at_ {
                    free.push((at_, lo));
                }
                at_ = at_.max(hi);
            }
            if at_ < total {
                free.push((at_, total));
            }
            // each stretch divided into slots on its own, starting with a name slot (an even number)
            let slot_m = look.slot_m;
            let mut nxt: i64 = 0;
            for &(lo, hi) in &free {
                nxt += nxt % 2;
                let length = hi - lo;
                let (q, r) = py_divmod(length, slot_m);
                let pieces = (q as i64 + if r > slot_m * 0.3 { 1 } else { 0 }).max(1);
                for j in 0..pieces {
                    let (a, b) = (lo + j as f64 * slot_m, lo + ((j + 1) as f64 * slot_m).min(length));
                    if b - a < slot_m * 0.2 {
                        if !(*oneway && b - a >= ARROW_MIN_M) {
                            continue;
                        }
                        nxt += 1 - nxt % 2; // too short for a name, long enough for an arrow: an odd slot
                    }
                    let i = nxt;
                    nxt += 1;
                    let coords = line.lonlat(&line.part(a, b), 1e6);
                    let (road, twin) = if n == 1 { edges[0].owner } else {
                        let m = line.at((a + b) / 2.0);
                        let pt = [m[0] / line.kx + line.lon0, m[1] / KY + line.lat0];
                        // the nearest road of the group (the first of equal distances): a big group looks only at the roads whose box
                        // is as near as the nearest road of this chain
                        let mut best = (f64::INFINITY, 0);
                        let try_road = |g: usize, best: &mut (f64, usize)| {
                            let d = point_line_dist(pt, &edges[g].c);
                            if d < best.0 || (d == best.0 && g < best.1) {
                                *best = (d, g);
                            }
                        };
                        match &near {
                            Some(t) => {
                                members.iter().for_each(|&g| try_road(g, &mut best));
                                let r = best.0;
                                for h in t.locate_in_envelope_intersecting(&AABB::from_corners([pt[0] - r, pt[1] - r], [pt[0] + r, pt[1] + r])) {
                                    try_road(h.data, &mut best);
                                }
                            }
                            None => (0..n).for_each(|g| try_road(g, &mut best)),
                        }
                        edges[best.1].owner
                    };
                    let mut p = Map::new();
                    p.insert("slot".into(), i.into());
                    p.insert("chain".into(), cid.into());
                    p.insert("rank".into(), rank.clone());
                    p.insert("name".into(), name.clone().map_or(Value::Null, Value::from));
                    p.insert("highway".into(), hw.clone());
                    p.insert("oneway".into(), (*oneway as u8).into());
                    p.insert("lvl".into(), lvl.clone());
                    p.insert("__rs_edge".into(), road.into());
                    if let Some(t) = twin {
                        p.insert("__rs_edge2".into(), t.into());
                    }
                    if !fl.is_null() {
                        p.insert("fl".into(), fl.clone());
                    }
                    if *tun {
                        p.insert("__rs_tunnel".into(), true.into());
                    }
                    feats.push(crate::obj([("type", "Feature".into()), ("properties", Value::Object(p)),
                                           ("geometry", crate::obj([("type", "LineString".into()), ("coordinates", json!(coords))]))]));
                }
            }
        }
    }
    // the export: number the arrow slots (the odd ones, else the chain's only slot) of each one-way chain; then _mark_cls
    let mut by_chain: IndexMap<i64, Vec<usize>> = IndexMap::new();
    for (k, f) in feats.iter().enumerate() {
        if f["properties"]["oneway"] == 1 {
            by_chain.entry(f["properties"]["chain"].as_i64().unwrap()).or_default().push(k);
        }
    }
    for (_, mut ks) in by_chain {
        ks.sort_by_key(|&k| feats[k]["properties"]["slot"].as_i64().unwrap());
        let odd: Vec<usize> = ks.iter().copied().filter(|&k| feats[k]["properties"]["slot"].as_i64().unwrap() % 2 != 0).collect();
        let pick = if odd.is_empty() { ks[..1].to_vec() } else { odd };
        for (n, k) in pick.into_iter().enumerate() {
            feats[k]["properties"]["__rs_arw"] = n.into();
        }
    }
    for f in feats.iter_mut() {
        let e = f["properties"]["__rs_edge"].as_u64().unwrap() as usize;
        f["properties"]["__rs_cls"] = roads[e].props.get("highway").cloned().unwrap_or(Value::Null);
    }
    crate::obj([("type", "geojson".into()), ("data", crate::obj([("type", "FeatureCollection".into()), ("features", Value::Array(feats))]))])
}
