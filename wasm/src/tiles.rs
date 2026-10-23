//! The style's sources as vector tiles (Mapbox Vector Tile 2.1), made when the map asks for them: the page gives MapLibre each
//! source as `{"type": "vector", "tiles": ["<prefix><source>/{z}/{x}/{y}"]}` and answers the requests with `Tiles::tile`. One
//! GeoJSON text of all the roads (Tartu: 150 MB) is then not made, parsed and cut into tiles by MapLibre again.
//!
//! Each feature keeps its id (`id`, or its position for a source with `generateId`, as MapLibre gives it), its properties (a null
//! value is left out: MapLibre reads a missing one as null) and its geometry, clipped to the tile and a buffer around it.

use rstar::{primitives::GeomWithData, primitives::Rectangle, RTree, AABB};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// The tile's size in its own units, and the buffer around it (in the same units: 1/8 of a pixel each at 512 px): wide lines, their
/// offsets and the labels along them reach over the tile's edge.
const EXTENT: f64 = 4096.0;
const BUFFER: f64 = 512.0;
/// The last zoom of the tiles; MapLibre zooms them in after it. 0.15 m a unit at 60 degrees north.
pub const MAX_ZOOM: u32 = 16;

type P = [f64; 2];

enum Geom {
    Points(Vec<P>),
    Lines(Vec<Vec<P>>),
    Polygons(Vec<Vec<Vec<P>>>),
}

#[derive(Clone, PartialEq)]
enum Val {
    Str(Box<str>),
    Bool(bool),
    Int(i64),
    UInt(u64),
    Dbl(f64),
}

struct Feat {
    id: Option<u64>,
    geom: Geom,
    props: Vec<(u32, Val)>,
}

struct Layer {
    keys: Vec<String>,
    feats: Vec<Feat>,
    tree: RTree<GeomWithData<Rectangle<P>, u32>>,
}

/// The sources of a style, ready to be cut into tiles.
#[derive(Default)]
pub struct Tiles {
    layers: HashMap<String, Layer>,
}

/// Web Mercator in [0, 1] (x to the east, y to the south), as the tiles count.
fn merc(c: &Value) -> Option<P> {
    let (lon, lat) = (c.get(0)?.as_f64()?, c.get(1)?.as_f64()?.clamp(-85.051_128_78, 85.051_128_78));
    let y = (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln();
    Some([(lon + 180.0) / 360.0, 0.5 - y / (2.0 * std::f64::consts::PI)])
}

fn line(c: &Value) -> Vec<P> {
    c.as_array().map_or(Vec::new(), |a| a.iter().filter_map(merc).collect())
}

fn rings(c: &Value) -> Vec<Vec<P>> {
    c.as_array().map_or(Vec::new(), |a| a.iter().map(line).collect())
}

fn geom(g: &Value) -> Option<Geom> {
    let c = &g["coordinates"];
    let list = |f: &dyn Fn(&Value) -> Vec<Vec<P>>| c.as_array().map_or(Vec::new(), |a| a.iter().map(f).collect());
    Some(match g["type"].as_str()? {
        "Point" => Geom::Points(vec![merc(c)?]),
        "MultiPoint" => Geom::Points(c.as_array()?.iter().filter_map(merc).collect()),
        "LineString" => Geom::Lines(vec![line(c)]),
        "MultiLineString" => Geom::Lines(rings(c)),
        "Polygon" => Geom::Polygons(vec![rings(c)]),
        "MultiPolygon" => Geom::Polygons(list(&rings)),
        _ => return None,
    })
}

fn val(v: &Value) -> Option<Val> {
    Some(match v {
        Value::String(s) => Val::Str(s.as_str().into()),
        Value::Bool(b) => Val::Bool(*b),
        Value::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => Val::Int(i),
            (_, Some(u)) => Val::UInt(u),
            _ => Val::Dbl(n.as_f64()?),
        },
        Value::Null => return None,
        other => Val::Str(other.to_string().into()), // the sources have no lists or objects; as text if one comes
    })
}

fn bounds(g: &Geom) -> Option<(P, P)> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut add = |p: &P| {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    };
    match g {
        Geom::Points(ps) => ps.iter().for_each(&mut add),
        Geom::Lines(ls) => ls.iter().flatten().for_each(&mut add),
        Geom::Polygons(ps) => ps.iter().flatten().flatten().for_each(&mut add),
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

impl Tiles {
    /// Add a GeoJSON source of the style (`{"type": "geojson", "data": FeatureCollection, ...}`) under its name, and free its tree.
    /// Only the properties in `keep` go into the tiles.
    pub fn add(&mut self, name: &str, source: Value, keep: &HashSet<String>) {
        let generate = source["generateId"].as_bool() == Some(true);
        let Value::Object(mut s) = source else { return };
        let features = match s.remove("data").map(|mut d| d["features"].take()) {
            Some(Value::Array(f)) => f,
            _ => Vec::new(),
        };
        drop(s);
        let mut keys: Vec<String> = Vec::new();
        let mut key_at: HashMap<String, u32> = HashMap::new();
        let mut feats = Vec::with_capacity(features.len());
        let mut boxes = Vec::new();
        for (i, f) in features.into_iter().enumerate() {
            let Value::Object(mut f) = f else { continue };
            let Some(g) = f.get("geometry").and_then(geom) else { continue };
            let Some((lo, hi)) = bounds(&g) else { continue };
            let id = if generate { Some(i as u64) } else { f.get("id").and_then(Value::as_u64) };
            let props = match f.remove("properties") {
                Some(Value::Object(p)) => p,
                _ => Map::new(),
            };
            let props = props
                .into_iter()
                .filter_map(|(k, v)| {
                    if !keep.contains(&k) {
                        return None;
                    }
                    let v = val(&v)?;
                    let at = *key_at.entry(k).or_insert_with_key(|k| {
                        keys.push(k.clone());
                        keys.len() as u32 - 1
                    });
                    Some((at, v))
                })
                .collect();
            boxes.push(GeomWithData::new(Rectangle::from_corners(lo, hi), feats.len() as u32));
            feats.push(Feat { id, geom: g, props });
        }
        self.layers.insert(name.to_string(), Layer { keys, feats, tree: RTree::bulk_load(boxes) });
    }

    /// The properties of the feature `id` of the source `name` (those in the tiles), as JSON; `null` when there is no such feature.
    /// The map draws the roads from other sources, so a click on a road asks for its properties here (`__rs_edge` is its id).
    pub fn props(&self, name: &str, id: u64) -> String {
        let Some(layer) = self.layers.get(name) else { return "null".into() };
        let Some(f) = layer.feats.iter().find(|f| f.id == Some(id)) else { return "null".into() };
        let m: Map<String, Value> = f.props.iter().map(|(k, v)| {
            let v = match v {
                Val::Str(s) => Value::from(&**s),
                Val::Bool(b) => Value::from(*b),
                Val::Int(i) => Value::from(*i),
                Val::UInt(u) => Value::from(*u),
                Val::Dbl(d) => Value::from(*d),
            };
            (layer.keys[*k as usize].clone(), v)
        }).collect();
        Value::Object(m).to_string()
    }

    /// The tile z/x/y of the source `name` as a vector tile with one layer of that name; empty when nothing is in it.
    pub fn tile(&self, name: &str, z: u32, x: u32, y: u32) -> Vec<u8> {
        let Some(layer) = self.layers.get(name) else { return Vec::new() };
        let n = (1u64 << z) as f64;
        let b = BUFFER / EXTENT / n;
        let (x0, y0) = (x as f64 / n, y as f64 / n);
        let found = layer.tree.locate_in_envelope_intersecting(&AABB::from_corners([x0 - b, y0 - b], [x0 + 1.0 / n + b, y0 + 1.0 / n + b]));
        let mut hits: Vec<u32> = found.map(|h| h.data).collect();
        hits.sort_unstable(); // the order of the source: the drawing order of features in one layer
        // the tile's own units: (merc * 2^z - x) * EXTENT
        let to = |p: &P| [(p[0] * n - x as f64) * EXTENT, (p[1] * n - y as f64) * EXTENT];
        let (lo, hi) = (-BUFFER, EXTENT + BUFFER);
        let mut enc = Encoder::default();
        for i in hits {
            let f = &layer.feats[i as usize];
            let (kind, geometry) = match &f.geom {
                Geom::Points(ps) => {
                    let ps: Vec<[i32; 2]> =
                        ps.iter().map(to).filter(|p| p[0] >= 0.0 && p[0] < EXTENT && p[1] >= 0.0 && p[1] < EXTENT).map(round).collect();
                    (1, points_cmd(&ps))
                }
                Geom::Lines(ls) => {
                    let parts: Vec<Vec<[i32; 2]>> = ls
                        .iter()
                        .flat_map(|l| clip_line(&l.iter().map(to).collect::<Vec<_>>(), lo, hi))
                        .map(|l| dedup(l.into_iter().map(round)))
                        .filter(|l| l.len() >= 2)
                        .collect();
                    (2, lines_cmd(&parts))
                }
                Geom::Polygons(ps) => {
                    let mut rings = Vec::new();
                    for poly in ps {
                        for (k, r) in poly.iter().enumerate() {
                            let mut r = dedup(clip_ring(&r.iter().map(to).collect::<Vec<_>>(), lo, hi).into_iter().map(round));
                            if r.len() > 1 && r[0] == r[r.len() - 1] {
                                r.pop();
                            }
                            let a = area(&r);
                            if r.len() < 3 || a == 0 {
                                if k == 0 {
                                    break; // no outer ring: no polygon
                                }
                                continue;
                            }
                            // the outer ring has a positive area in the tile's units (y down), a hole a negative one
                            if (k == 0) != (a > 0) {
                                r.reverse();
                            }
                            rings.push(r);
                        }
                    }
                    (3, rings_cmd(&rings))
                }
            };
            if !geometry.is_empty() {
                enc.feature(f, kind, &geometry, &layer.keys);
            }
        }
        enc.finish(name)
    }
}

fn round(p: P) -> [i32; 2] {
    [p[0].round() as i32, p[1].round() as i32]
}

fn dedup(ps: impl Iterator<Item = [i32; 2]>) -> Vec<[i32; 2]> {
    let mut out: Vec<[i32; 2]> = Vec::new();
    for p in ps {
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    out
}

/// Twice the signed area by the surveyor's formula.
fn area(r: &[[i32; 2]]) -> i64 {
    (0..r.len()).map(|i| {
        let (a, b) = (r[i], r[(i + 1) % r.len()]);
        a[0] as i64 * b[1] as i64 - b[0] as i64 * a[1] as i64
    }).sum()
}

/// The parts of a line inside the square [lo, hi]^2 (Liang-Barsky on each segment).
fn clip_line(ps: &[P], lo: f64, hi: f64) -> Vec<Vec<P>> {
    let mut out = Vec::new();
    let mut cur: Vec<P> = Vec::new();
    for w in ps.windows(2) {
        match clip_seg(w[0], w[1], lo, hi) {
            Some((a, b)) => {
                if cur.last() != Some(&a) {
                    if cur.len() >= 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(a);
                }
                cur.push(b);
                if b != w[1] {
                    out.push(std::mem::take(&mut cur)); // the line leaves the square here
                }
            }
            None => {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

fn clip_seg(a: P, b: P, lo: f64, hi: f64) -> Option<(P, P)> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for k in 0..2 {
        for (p, q) in [(-d[k], a[k] - lo), (d[k], hi - a[k])] {
            if p == 0.0 {
                if q < 0.0 {
                    return None;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
    }
    if t0 > t1 {
        return None;
    }
    let at = |t: f64| if t == 0.0 { a } else if t == 1.0 { b } else { [a[0] + t * d[0], a[1] + t * d[1]] };
    Some((at(t0), at(t1)))
}

/// A ring cut to the square [lo, hi]^2 (Sutherland-Hodgman); the parts outside become the square's edge, which the map does not draw.
fn clip_ring(r: &[P], lo: f64, hi: f64) -> Vec<P> {
    let mut pts = r.to_vec();
    for (k, edge, keep_above) in [(0, lo, true), (0, hi, false), (1, lo, true), (1, hi, false)] {
        if pts.is_empty() {
            break;
        }
        let inside = |p: &P| if keep_above { p[k] >= edge } else { p[k] <= edge };
        let mut out = Vec::with_capacity(pts.len());
        for i in 0..pts.len() {
            let (a, b) = (pts[(i + pts.len() - 1) % pts.len()], pts[i]);
            if inside(&b) {
                if !inside(&a) {
                    out.push(cut(a, b, k, edge));
                }
                out.push(b);
            } else if inside(&a) {
                out.push(cut(a, b, k, edge));
            }
        }
        pts = out;
    }
    pts
}

fn cut(a: P, b: P, k: usize, edge: f64) -> P {
    let t = (edge - a[k]) / (b[k] - a[k]);
    let mut p = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
    p[k] = edge;
    p
}

// --- the geometry commands (MoveTo 1, LineTo 2, ClosePath 7) with zigzag deltas ---

fn command(id: u32, count: usize) -> u32 {
    (id & 7) | ((count as u32) << 3)
}

fn zigzag(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

struct Cursor(Vec<u32>, [i32; 2]);

impl Cursor {
    fn to(&mut self, p: [i32; 2]) {
        self.0.push(zigzag(p[0] - self.1[0]));
        self.0.push(zigzag(p[1] - self.1[1]));
        self.1 = p;
    }
}

fn points_cmd(ps: &[[i32; 2]]) -> Vec<u32> {
    let mut c = Cursor(Vec::new(), [0, 0]);
    if !ps.is_empty() {
        c.0.push(command(1, ps.len()));
        ps.iter().for_each(|&p| c.to(p));
    }
    c.0
}

fn lines_cmd(ls: &[Vec<[i32; 2]>]) -> Vec<u32> {
    let mut c = Cursor(Vec::new(), [0, 0]);
    for l in ls {
        c.0.push(command(1, 1));
        c.to(l[0]);
        c.0.push(command(2, l.len() - 1));
        l[1..].iter().for_each(|&p| c.to(p));
    }
    c.0
}

fn rings_cmd(rs: &[Vec<[i32; 2]>]) -> Vec<u32> {
    let mut c = Cursor(Vec::new(), [0, 0]);
    for r in rs {
        c.0.push(command(1, 1));
        c.to(r[0]);
        c.0.push(command(2, r.len() - 1));
        r[1..].iter().for_each(|&p| c.to(p));
        c.0.push(command(7, 1));
    }
    c.0
}

// --- protocol buffers: the tile with one layer ---

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn bytes_field(out: &mut Vec<u8>, field: u32, data: &[u8]) {
    varint(out, ((field << 3) | 2) as u64);
    varint(out, data.len() as u64);
    out.extend_from_slice(data);
}

fn packed(out: &mut Vec<u8>, field: u32, vs: &[u32]) {
    let mut d = Vec::with_capacity(vs.len() * 2);
    vs.iter().for_each(|&v| varint(&mut d, v as u64));
    bytes_field(out, field, &d);
}

#[derive(Default)]
struct Encoder {
    features: Vec<u8>,
    keys: Vec<String>,
    key_at: HashMap<u32, u32>,
    values: Vec<Vec<u8>>,
    value_at: HashMap<Vec<u8>, u32>,
}

impl Encoder {
    fn feature(&mut self, f: &Feat, kind: u32, geometry: &[u32], names: &[String]) {
        let mut tags = Vec::with_capacity(f.props.len() * 2);
        for (k, v) in &f.props {
            let key = *self.key_at.entry(*k).or_insert_with(|| {
                self.keys.push(names[*k as usize].clone());
                self.keys.len() as u32 - 1
            });
            let mut m = Vec::new(); // the Value message
            match v {
                Val::Str(s) => bytes_field(&mut m, 1, s.as_bytes()),
                Val::Dbl(d) => {
                    m.push((3 << 3) | 1);
                    m.extend_from_slice(&d.to_le_bytes());
                }
                Val::Int(i) if *i < 0 => {
                    m.push(6 << 3);
                    varint(&mut m, ((*i << 1) ^ (*i >> 63)) as u64);
                }
                Val::Int(i) => {
                    m.push(5 << 3);
                    varint(&mut m, *i as u64);
                }
                Val::UInt(u) => {
                    m.push(5 << 3);
                    varint(&mut m, *u);
                }
                Val::Bool(b) => {
                    m.push(7 << 3);
                    m.push(*b as u8);
                }
            }
            let at = match self.value_at.get(&m) {
                Some(&at) => at,
                None => {
                    self.values.push(m.clone());
                    self.value_at.insert(m, self.values.len() as u32 - 1);
                    self.values.len() as u32 - 1
                }
            };
            tags.push(key);
            tags.push(at);
        }
        let mut msg = Vec::new();
        if let Some(id) = f.id {
            msg.push(1 << 3);
            varint(&mut msg, id);
        }
        packed(&mut msg, 2, &tags);
        msg.push(3 << 3);
        varint(&mut msg, kind as u64);
        packed(&mut msg, 4, geometry);
        bytes_field(&mut self.features, 2, &msg);
    }

    fn finish(self, name: &str) -> Vec<u8> {
        if self.features.is_empty() {
            return Vec::new();
        }
        let mut layer = Vec::new();
        layer.push(15 << 3);
        varint(&mut layer, 2); // version
        bytes_field(&mut layer, 1, name.as_bytes());
        layer.extend_from_slice(&self.features);
        self.keys.iter().for_each(|k| bytes_field(&mut layer, 3, k.as_bytes()));
        self.values.iter().for_each(|v| bytes_field(&mut layer, 4, v));
        layer.push(5 << 3);
        varint(&mut layer, EXTENT as u64);
        let mut tile = Vec::with_capacity(layer.len() + 8);
        bytes_field(&mut tile, 3, &layer);
        tile
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn keys(k: &[&str]) -> HashSet<String> {
        k.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_line_across_the_tile_is_cut_at_the_buffer() {
        let parts = clip_line(&[[-1000.0, 100.0], [5000.0, 100.0], [5000.0, 6000.0]], -512.0, 4608.0);
        assert_eq!(parts, vec![vec![[-512.0, 100.0], [4608.0, 100.0]]]);
        let inside = clip_line(&[[10.0, 10.0], [20.0, 20.0], [30.0, 10.0]], -512.0, 4608.0);
        assert_eq!(inside, vec![vec![[10.0, 10.0], [20.0, 20.0], [30.0, 10.0]]]);
    }

    #[test]
    fn a_tile_has_the_feature_its_id_and_properties() {
        let mut t = Tiles::default();
        t.add("roads", json!({"type": "geojson", "data": {"type": "FeatureCollection", "features": [
            {"type": "Feature", "id": 7, "properties": {"name": "A", "lanes": 2, "w": 1.5, "bridge": true, "gone": null, "n": -3},
             "geometry": {"type": "LineString", "coordinates": [[1.0, 1.0], [10.0, 10.0]]}}]}}), &keys(&["name", "lanes", "w", "bridge", "gone", "n"]));
        let tile = t.tile("roads", 0, 0, 0);
        assert!(!tile.is_empty());
        assert!(!t.tile("roads", 3, 4, 3).is_empty(), "the road is in tile 3/4/3");
        assert!(t.tile("roads", 3, 2, 5).is_empty(), "and not in a tile far from it");
        // the bytes have the layer's name, the keys and a string value, and no key for the null value
        let text = String::from_utf8_lossy(&tile);
        assert!(text.contains("roads") && text.contains("name") && text.contains("lanes") && !text.contains("gone"));
        assert_eq!(t.props("roads", 7), r#"{"name":"A","lanes":2,"w":1.5,"bridge":true,"n":-3}"#);
        assert_eq!(t.props("roads", 8), "null");
    }

    #[test]
    fn the_outer_ring_has_a_positive_area() {
        let mut t = Tiles::default();
        // a square around (1, 1) degrees, counterclockwise in lon / lat (clockwise on the screen after y goes down)
        t.add("land", json!({"type": "geojson", "generateId": true, "data": {"type": "FeatureCollection", "features": [
            {"type": "Feature", "properties": {}, "geometry": {"type": "Polygon",
             "coordinates": [[[0.5, 0.5], [1.5, 0.5], [1.5, 1.5], [0.5, 1.5], [0.5, 0.5]]]}}]}}), &keys(&[]));
        let f = &t.layers["land"].feats[0];
        assert_eq!(f.id, Some(0));
        let Geom::Polygons(ps) = &f.geom else { panic!() };
        let r: Vec<[i32; 2]> = ps[0][0].iter().map(|p| round([p[0] * 4096.0 * 64.0, p[1] * 4096.0 * 64.0])).collect();
        assert!(area(&r) < 0, "lon / lat counterclockwise is negative in the tile's units");
        assert!(!t.tile("land", 0, 0, 0).is_empty());
    }
}
