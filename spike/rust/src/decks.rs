//! The 3D bridge decks (full port step 5): render_web._bridge_decks. The chains of bridge edges, the ramp, the cuts, the parts (shapely's
//! substring) and the properties are ported exactly. The polygons: a straight part (two points) is GEOS's buffer exactly (its offset arithmetic and
//! its ring order); a bent part uses mitre joins (buffer) and round joins (offset_curve) computed here, close to GEOS's but not the same.

use crate::layers::{py_round, Model};
use crate::read::Feature;
use crate::roads::truthy;
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};

type Pt = [f64; 2];

/// The deck settings (render's dk): the defaults with config.bridge_decks over them.
struct Dk {
    base_m: f64,
    thickness_m: f64,
    ramp_m: f64,
    step_m: f64,
    match_zoom: f64,
    width_scale: f64,
    casing_px: f64,
}

fn dk(look: &Value) -> Dk {
    let c = &look["config"]["bridge_decks"];
    let get = |k: &str, d: f64| c.get(k).and_then(Value::as_f64).unwrap_or(d);
    Dk { base_m: get("base_m", 5.0), thickness_m: get("thickness_m", 1.0), ramp_m: get("ramp_m", 40.0), step_m: get("step_m", 2.5),
         match_zoom: get("match_zoom", 18.0), width_scale: get("width_scale", 0.6), casing_px: get("casing_px", 2.0) }
}

/// The deck settings the layers read (opacity, flat_below).
pub fn deck_layer_values(look: &Value) -> (Value, Value) {
    let c = &look["config"]["bridge_decks"];
    (c.get("opacity").cloned().unwrap_or(json!(0.7)), c.get("flat_below").cloned().unwrap_or(json!(16.0)))
}

fn round6(x: f64) -> f64 {
    let v: f64 = format!("{x:.6}").parse().unwrap();
    if v == 0.0 { 0.0 } else { v }
}

fn node(c: &Pt) -> (f64, f64) {
    (round6(c[0]), round6(c[1]))
}

fn seg_len(a: Pt, b: Pt) -> f64 {
    ((b[0] - a[0]) * (b[0] - a[0]) + (b[1] - a[1]) * (b[1] - a[1])).sqrt()
}

/// GEOS's interpolate (LengthIndexedLine): the point at `d` along the line, clamped to its ends.
fn interpolate(c: &[Pt], d: f64) -> Pt {
    if d <= 0.0 {
        return c[0];
    }
    let mut total = 0.0;
    for w in c.windows(2) {
        let len = seg_len(w[0], w[1]);
        if total + len > d {
            let f = (d - total) / len;
            return [w[0][0] + f * (w[1][0] - w[0][0]), w[0][1] + f * (w[1][1] - w[0][1])];
        }
        total += len;
    }
    c[c.len() - 1]
}

/// shapely.ops.substring (start < end, both within the line, not normalized): the part of the line between two distances.
fn substring(c: &[Pt], length: f64, start: f64, end: f64) -> Option<Vec<Pt>> {
    if start == end || (start >= length && end >= length) {
        return None; // a point
    }
    let (sp, ep) = (interpolate(c, start), interpolate(c, end));
    let start = start.max(0.0);
    let mut out = vec![sp];
    let mut cur = 0.0f64;
    for w in c.windows(2) {
        if start < cur && cur < end {
            out.push(w[0]);
        } else if cur >= end {
            break;
        }
        cur += ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).powf(0.5);
    }
    out.push(ep);
    Some(out)
}

/// GEOS OffsetSegmentGenerator::computeOffsetSegment: a segment moved `d` to its left (side 1) or right (side -1).
fn offset_seg(p0: Pt, p1: Pt, side: f64, d: f64) -> (Pt, Pt) {
    let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
    let len = (dx * dx + dy * dy).sqrt();
    let ux = side * d * dx / len;
    let uy = side * d * dy / len;
    ([p0[0] - uy, p0[1] + ux], [p1[0] - uy, p1[1] + ux])
}

fn intersect(a0: Pt, a1: Pt, b0: Pt, b1: Pt) -> Option<Pt> {
    let (rx, ry, sx, sy) = (a1[0] - a0[0], a1[1] - a0[1], b1[0] - b0[0], b1[1] - b0[1]);
    let den = rx * sy - ry * sx;
    if den.abs() < 1e-12 {
        return None;
    }
    let t = ((b0[0] - a0[0]) * sy - (b0[1] - a0[1]) * sx) / den;
    Some([a0[0] + t * rx, a0[1] + t * ry])
}

/// One side of a line moved `d` (side 1 left, -1 right): the offset segments joined by a mitre (limit 5, else a bevel), or by a round join
/// (`round`: 8 segments a quarter circle) on the outer side of a bend; the inner side meets at the two offset segments' crossing.
fn side_line(c: &[Pt], side: f64, d: f64, round: bool) -> Vec<Pt> {
    let segs: Vec<(Pt, Pt)> = c.windows(2).filter(|w| w[0] != w[1]).map(|w| offset_seg(w[0], w[1], side, d)).collect();
    let verts: Vec<Pt> = { let mut v = vec![c[0]]; for w in c.windows(2) { if w[0] != w[1] { v.push(w[1]); } } v };
    let mut out = vec![segs[0].0];
    for i in 0..segs.len() - 1 {
        let (a, b) = (segs[i], segs[i + 1]);
        let v = verts[i + 1];
        let (d1, d2) = ([a.1[0] - a.0[0], a.1[1] - a.0[1]], [b.1[0] - b.0[0], b.1[1] - b.0[1]]);
        let turn = d1[0] * d2[1] - d1[1] * d2[0]; // > 0: a left turn
        let outer = (turn > 0.0) == (side < 0.0);
        if turn.abs() < 1e-18 {
            out.push(a.1);
            continue;
        }
        if outer && round {
            let r = d.abs();
            let (s0, s1) = ((a.1[1] - v[1]).atan2(a.1[0] - v[0]), (b.0[1] - v[1]).atan2(b.0[0] - v[0]));
            let mut delta = s1 - s0;
            if side > 0.0 { while delta > 0.0 { delta -= std::f64::consts::TAU } } else { while delta < 0.0 { delta += std::f64::consts::TAU } }
            let n = ((delta.abs() / (std::f64::consts::FRAC_PI_2 / 8.0)).floor() as usize).max(1);
            for k in 0..=n {
                let ang = s0 + delta * k as f64 / n as f64;
                out.push([v[0] + r * ang.cos(), v[1] + r * ang.sin()]);
            }
        } else if outer {
            match intersect(a.0, a.1, b.0, b.1).filter(|m| seg_len(*m, v) <= 5.0 * d.abs()) {
                Some(m) => out.push(m),
                None => { out.push(a.1); out.push(b.0); }
            }
        } else {
            out.push(intersect(a.0, a.1, b.0, b.1).unwrap_or(a.1));
        }
    }
    out.push(segs[segs.len() - 1].1);
    out
}

/// shapely's line.offset_curve(d): `d` > 0 to the left, in the line's direction; round joins.
fn offset_curve(c: &[Pt], d: f64) -> Vec<Pt> {
    side_line(c, if d >= 0.0 { 1.0 } else { -1.0 }, d.abs(), true)
}

/// shapely's line.buffer(h, cap_style=flat, join_style=mitre) as a ring: GEOS's order, from the end's left point, clockwise.
fn buffer(c: &[Pt], h: f64) -> Vec<Pt> {
    let left = side_line(c, 1.0, h, false);
    let right = side_line(c, -1.0, h, false);
    let mut ring = vec![left[left.len() - 1]];
    ring.extend(right.iter().rev());
    ring.extend(left.iter());
    ring.dedup();
    if ring.first() != ring.last() {
        ring.push(ring[0]);
    }
    ring
}

struct Rep {
    a: (f64, f64),
    z: (f64, f64),
    c: Vec<Pt>,
    fi: usize,
}

fn lt(x: ((f64, f64), (f64, f64)), y: ((f64, f64), (f64, f64))) -> bool {
    x.partial_cmp(&y) == Some(std::cmp::Ordering::Less)
}

/// _bridge_decks: the deck ribbons (one per directed edge) and the casing slabs, as a FeatureCollection; and how many parts were bent.
pub fn bridge_decks(roads: &[Feature], look: &Value, m: &Model) -> (Value, usize) {
    let dk = dk(look);
    let mut reps: Vec<Rep> = Vec::new();
    let key = |a: (f64, f64), z: (f64, f64)| if lt((z, a), (a, z)) { (z, a) } else { (a, z) };
    let mut pair_edges: HashMap<(u64, u64, u64, u64), Vec<usize>> = HashMap::new();
    let bits = |k: ((f64, f64), (f64, f64))| (k.0 .0.to_bits(), k.0 .1.to_bits(), k.1 .0.to_bits(), k.1 .1.to_bits());
    for (fi, f) in roads.iter().enumerate() {
        let p = &f.props;
        let Some(c) = f.coords.as_ref().filter(|c| c.len() >= 2) else { continue };
        let lvl = p.get("lvl").and_then(Value::as_f64).unwrap_or(0.0);
        if !truthy(p.get("__rs_bridge")) || lvl < 1.0 {
            continue;
        }
        let (a, z) = (node(&c[0]), node(&c[c.len() - 1]));
        pair_edges.entry(bits(key(a, z))).or_default().push(fi);
        if (truthy(p.get("__rs_twoway")) && lt((z, a), (a, z))) || truthy(p.get("__rs_dup")) {
            continue;
        }
        reps.push(Rep { a, z, c: c.clone(), fi });
    }
    let twin_of = |fi: usize, a: (f64, f64), z: (f64, f64)| pair_edges.get(&bits(key(a, z))).and_then(|v| v.iter().find(|&&f| f != fi).copied());
    let n = reps.len();
    let mut used = vec![false; n];
    let mut at: HashMap<(u64, u64), Vec<usize>> = HashMap::new();
    let nb = |p: (f64, f64)| (p.0.to_bits(), p.1.to_bits());
    for (i, r) in reps.iter().enumerate() {
        at.entry(nb(r.a)).or_default().push(i);
        at.entry(nb(r.z)).or_default().push(i);
    }
    let (base_m, thick) = (dk.base_m, dk.thickness_m);
    let (ramp, step) = (dk.ramp_m.max(1.0), dk.step_m.max(0.25));
    let mut feats: Vec<Value> = Vec::new();
    let mut bent = 0;
    let mut chain_i = 0;
    for start in 0..n {
        if used[start] {
            continue;
        }
        // walk: the chain through degree-2 nodes; spans: (end index in the chain, the rep's road, the edge with the chain, its reverse)
        let r0 = &reps[start];
        used[start] = true;
        let mut chain = r0.c.clone();
        let mut spans: Vec<(usize, usize, Option<usize>, Option<usize>)> = vec![(chain.len() - 1, r0.fi, Some(r0.fi), twin_of(r0.fi, r0.a, r0.z))];
        let mut ends = [false; 2];
        for (pi, prepend) in [false, true].into_iter().enumerate() {
            let mut nd = if prepend { r0.a } else { r0.z };
            loop {
                let here = &at[&nb(nd)];
                let cand: Vec<usize> = here.iter().copied().filter(|&j| !used[j]).collect();
                if here.len() != 2 || cand.len() != 1 {
                    break;
                }
                let j = cand[0];
                let r = &reps[j];
                let jt = twin_of(r.fi, r.a, r.z);
                used[j] = true;
                if prepend {
                    let (fwd, rev) = if r.z == nd { (Some(r.fi), jt) } else { (jt, Some(r.fi)) };
                    let seg: Vec<Pt> = if r.z == nd { r.c.clone() } else { r.c.iter().rev().copied().collect() };
                    let grown = seg.len() - 1;
                    let mut c2 = seg[..seg.len() - 1].to_vec();
                    c2.extend_from_slice(&chain);
                    chain = c2;
                    for s in spans.iter_mut() {
                        s.0 += grown;
                    }
                    spans.insert(0, (grown, r.fi, fwd, rev));
                    nd = if r.z == nd { r.a } else { r.z };
                } else {
                    let (fwd, rev) = if r.a == nd { (Some(r.fi), jt) } else { (jt, Some(r.fi)) };
                    let seg: Vec<Pt> = if r.a == nd { r.c.clone() } else { r.c.iter().rev().copied().collect() };
                    chain.extend_from_slice(&seg[1..]);
                    spans.push((chain.len() - 1, r.fi, fwd, rev));
                    nd = if r.a == nd { r.z } else { r.a };
                }
            }
            ends[pi] = at[&nb(nd)].len() == 1; // the sole bridge edge there: a ground end
        }
        let (ramp_head, ramp_tail) = (ends[1], ends[0]);
        chain_i += 1;
        let (lon0, lat0) = (chain[0][0], chain[0][1]);
        let kx = 111320.0 * lat0.to_radians().cos();
        let mpp = 156543.03392 * lat0.to_radians().cos() / 2f64.powf(dk.match_zoom);
        let pts: Vec<Pt> = chain.iter().map(|p| [(p[0] - lon0) * kx, (p[1] - lat0) * 111320.0]).collect();
        let total: f64 = pts.windows(2).map(|w| seg_len(w[0], w[1])).sum();
        let mut cum = vec![0.0f64];
        for k in 1..pts.len() {
            let (dx, dy) = (pts[k][0] - pts[k - 1][0], pts[k][1] - pts[k - 1][1]);
            let last = cum[cum.len() - 1];
            cum.push(last + (dx * dx + dy * dy).powf(0.5));
        }
        let bounds: Vec<(f64, usize, Option<usize>, Option<usize>)> = spans.iter().map(|s| (cum[s.0], s.1, s.2, s.3)).collect();
        let props_at = |d: f64| bounds.iter().find(|b| d <= b.0 + 1e-6).map_or_else(|| { let b = bounds[bounds.len() - 1]; (b.1, b.2, b.3) }, |b| (b.1, b.2, b.3));
        let rh = if ramp_head { ramp.min(total) } else { 0.0 };
        let rt = if ramp_tail { ramp.min(total - rh) } else { 0.0 };
        let mut cuts: Vec<f64> = vec![0.0, total];
        let mut d = step;
        while d < rh {
            cuts.push(d);
            d += step;
        }
        d = step;
        while d < rt {
            cuts.push(total - d);
            d += step;
        }
        cuts.push(rh);
        cuts.push(total - rt);
        cuts.extend(bounds.iter().map(|b| b.0).filter(|b| rh < *b && *b < total - rt));
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| a == b);
        let ll = |p: &Pt| json!([py_round(p[0] / kx + lon0, 6), py_round(p[1] / 111320.0 + lat0, 6)]);
        for w in cuts.windows(2) {
            let (d0, d1) = (w[0], w[1]);
            let Some(part) = substring(&pts, total, d0, d1) else { continue };
            if part.windows(2).map(|w| seg_len(w[0], w[1])).sum::<f64>() <= 0.0 {
                continue;
            }
            if part.len() > 2 {
                bent += 1;
            }
            let mid = (d0 + d1) / 2.0;
            let (pp, ffi, rfi) = props_at(mid);
            let pprops = &roads[pp].props;
            let px = m.deck_px(pprops.get("highway").and_then(Value::as_str), dk.match_zoom);
            let half = (px * mpp / 2.0).max(2.0) * dk.width_scale;
            let up = if ramp_head { mid / ramp } else { 1.0 };
            let dn = if ramp_tail { (total - mid) / ramp } else { 1.0 };
            let t = up.min(dn).min(1.0);
            let t = t * t * (3.0 - 2.0 * t);
            let mut ribbons: Vec<(Vec<Pt>, Option<usize>)> = Vec::new();
            if truthy(pprops.get("__rs_twoway")) && ffi.is_some() && rfi.is_some() {
                for (dlt, fdir) in [(-1.0, ffi), (1.0, rfi)] {
                    let ol = offset_curve(&part, dlt * half / 2.0);
                    ribbons.push((buffer(&ol, half / 2.0), fdir));
                }
            } else {
                ribbons.push((buffer(&part, half), ffi.or(rfi)));
            }
            for (ring, fdir) in &ribbons {
                let mut props: Map<String, Value> = match fdir { Some(f) => roads[*f].props.clone(), None => pprops.clone() };
                props.insert("__rs_base".into(), json!(py_round(base_m * t, 2)));
                props.insert("__rs_height".into(), json!(py_round(base_m * t + thick, 2)));
                props.insert("__rs_chain".into(), json!(chain_i));
                props.insert("__rs_edges".into(), json!(fdir.map_or(vec![], |f| vec![f])));
                feats.push(json!({"type": "Feature", "properties": props, "geometry": {"type": "Polygon", "coordinates": [ring.iter().map(ll).collect::<Vec<_>>()]}}));
            }
            if dk.casing_px != 0.0 {
                let cw = dk.casing_px * mpp;
                let edges: BTreeSet<usize> = ribbons.iter().filter_map(|r| r.1).collect();
                for sgn in [1.0, -1.0] {
                    let sl = offset_curve(&part, sgn * (half + cw / 2.0));
                    let ring = buffer(&sl, cw / 2.0);
                    feats.push(json!({"type": "Feature", "properties": {
                        "highway": pprops.get("highway").cloned().unwrap_or(Value::Null), "__rs_chain": chain_i, "__rs_casing_slab": 1,
                        "__rs_edges": edges.iter().collect::<Vec<_>>(), "__rs_base": py_round((base_m * t - 0.5).max(0.0), 2),
                        "__rs_height": py_round(base_m * t + thick * 0.35, 2)},
                        "geometry": {"type": "Polygon", "coordinates": [ring.iter().map(ll).collect::<Vec<_>>()]}}));
                }
            }
        }
    }
    (json!({"type": "FeatureCollection", "features": feats}), bent)
}
