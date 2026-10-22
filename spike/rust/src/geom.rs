//! Geometry from WKB for the base map (full port step 3): GeoJSON, and the area and the centroid as shapely (GEOS) computes them. DuckDB's
//! core has no ST_AsGeoJSON, ST_Area or ST_Centroid, so the worker reads WKB (ST_AsWKB) and this file does the rest, as Pyodide's shapely does.

use serde_json::{json, Value};

type Pt = [f64; 2];
type Ring = Vec<Pt>;

pub enum Geom {
    Point(Pt),
    LineString(Vec<Pt>),
    Polygon(Vec<Ring>),
    MultiPoint(Vec<Pt>),
    MultiLineString(Vec<Vec<Pt>>),
    MultiPolygon(Vec<Vec<Ring>>),
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.at)?;
        self.at += 1;
        Some(v)
    }
    fn u32(&mut self, le: bool) -> Option<u32> {
        let w: [u8; 4] = self.b.get(self.at..self.at + 4)?.try_into().ok()?;
        self.at += 4;
        Some(if le { u32::from_le_bytes(w) } else { u32::from_be_bytes(w) })
    }
    fn f64(&mut self, le: bool) -> Option<f64> {
        let w: [u8; 8] = self.b.get(self.at..self.at + 8)?.try_into().ok()?;
        self.at += 8;
        Some(if le { f64::from_le_bytes(w) } else { f64::from_be_bytes(w) })
    }
    fn pt(&mut self, le: bool) -> Option<Pt> {
        Some([self.f64(le)?, self.f64(le)?])
    }
    fn pts(&mut self, le: bool) -> Option<Vec<Pt>> {
        let n = self.u32(le)? as usize;
        (0..n).map(|_| self.pt(le)).collect()
    }
    fn rings(&mut self, le: bool) -> Option<Vec<Ring>> {
        let n = self.u32(le)? as usize;
        (0..n).map(|_| self.pts(le)).collect()
    }
    fn geom(&mut self) -> Option<Geom> {
        let le = self.u8()? == 1;
        let t = self.u32(le)?;
        Some(match t {
            1 => Geom::Point(self.pt(le)?),
            2 => Geom::LineString(self.pts(le)?),
            3 => Geom::Polygon(self.rings(le)?),
            4 | 5 | 6 => {
                let n = self.u32(le)? as usize;
                let parts: Vec<Geom> = (0..n).map(|_| self.geom()).collect::<Option<_>>()?;
                match t {
                    4 => Geom::MultiPoint(parts.into_iter().map(|g| if let Geom::Point(p) = g { Some(p) } else { None }).collect::<Option<_>>()?),
                    5 => Geom::MultiLineString(parts.into_iter().map(|g| if let Geom::LineString(l) = g { Some(l) } else { None }).collect::<Option<_>>()?),
                    _ => Geom::MultiPolygon(parts.into_iter().map(|g| if let Geom::Polygon(p) = g { Some(p) } else { None }).collect::<Option<_>>()?),
                }
            }
            _ => return None, // a 3D, measured or collection geometry: not in a duckOSM base map
        })
    }
}

/// A 2D WKB geometry (point, line, polygon, or their multi forms).
pub fn from_wkb(b: &[u8]) -> Option<Geom> {
    Reader { b, at: 0 }.geom()
}

fn coords(p: &[Pt]) -> Value {
    json!(p)
}

impl Geom {
    /// The GeoJSON geometry (shapely's mapping, DuckDB's ST_AsGeoJSON): the rings as stored.
    pub fn geojson(&self) -> Value {
        match self {
            Geom::Point(p) => json!({"type": "Point", "coordinates": p}),
            Geom::LineString(l) => json!({"type": "LineString", "coordinates": coords(l)}),
            Geom::Polygon(r) => json!({"type": "Polygon", "coordinates": r}),
            Geom::MultiPoint(p) => json!({"type": "MultiPoint", "coordinates": p}),
            Geom::MultiLineString(l) => json!({"type": "MultiLineString", "coordinates": l}),
            Geom::MultiPolygon(p) => json!({"type": "MultiPolygon", "coordinates": p}),
        }
    }

    /// GEOS's area: a polygon's shell less its holes (Area::ofRing, the shoelace from the first point); a line or a point has none.
    pub fn area(&self) -> f64 {
        fn ring(r: &Ring) -> f64 {
            if r.len() < 3 {
                return 0.0;
            }
            let x0 = r[0][0];
            let mut sum = 0.0;
            for i in 1..r.len() - 1 {
                let x = r[i][0] - x0;
                sum += x * (r[i - 1][1] - r[i + 1][1]);
            }
            sum / 2.0
        }
        fn polygon(p: &[Ring]) -> f64 {
            p.first().map_or(0.0, |s| ring(s).abs()) - p.iter().skip(1).map(|h| ring(h).abs()).sum::<f64>()
        }
        match self {
            Geom::Polygon(p) => polygon(p),
            Geom::MultiPolygon(ps) => ps.iter().map(|p| polygon(p)).sum(),
            _ => 0.0,
        }
    }

    /// GEOS's Centroid: the area-weighted centre of the triangles from the area base point; else the length-weighted centre of the lines;
    /// else the mean of the points.
    pub fn centroid(&self) -> Pt {
        let mut c = Centroid::default();
        c.add(self);
        c.get()
    }
}

#[derive(Default)]
struct Centroid {
    base: Option<Pt>,
    cg3: Pt,
    area2: f64,
    line: Pt,
    length: f64,
    pts: Pt,
    n: f64,
}

fn area2(p1: Pt, p2: Pt, p3: Pt) -> f64 {
    (p2[0] - p1[0]) * (p3[1] - p1[1]) - (p3[0] - p1[0]) * (p2[1] - p1[1])
}

/// Orientation::isCCW by the signed area (the rings of a base map are not degenerate).
fn ccw(r: &Ring) -> bool {
    let mut s = 0.0;
    for w in r.windows(2) {
        s += (w[1][0] - w[0][0]) * (w[1][1] + w[0][1]);
    }
    s < 0.0
}

impl Centroid {
    fn add(&mut self, g: &Geom) {
        match g {
            Geom::Point(p) => self.point(*p),
            Geom::MultiPoint(ps) => ps.iter().for_each(|p| self.point(*p)),
            Geom::LineString(l) => self.segments(l),
            Geom::MultiLineString(ls) => ls.iter().for_each(|l| self.segments(l)),
            Geom::Polygon(p) => self.polygon(p),
            Geom::MultiPolygon(ps) => ps.iter().for_each(|p| self.polygon(p)),
        }
    }
    fn point(&mut self, p: Pt) {
        self.n += 1.0;
        self.pts[0] += p[0];
        self.pts[1] += p[1];
    }
    fn segments(&mut self, l: &[Pt]) {
        let mut len = 0.0;
        for w in l.windows(2) {
            let d = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
            if d == 0.0 {
                continue;
            }
            len += d;
            self.line[0] += d * ((w[0][0] + w[1][0]) / 2.0); // GEOS: segmentLen * midpoint
            self.line[1] += d * ((w[0][1] + w[1][1]) / 2.0);
        }
        self.length += len;
        if len == 0.0 && !l.is_empty() {
            self.point(l[0]);
        }
    }
    fn triangle(&mut self, p0: Pt, p1: Pt, p2: Pt, positive: bool) {
        let sign = if positive { 1.0 } else { -1.0 };
        let (cx, cy) = (p0[0] + p1[0] + p2[0], p0[1] + p1[1] + p2[1]);
        let a2 = area2(p0, p1, p2);
        self.cg3[0] += sign * a2 * cx;
        self.cg3[1] += sign * a2 * cy;
        self.area2 += sign * a2;
    }
    fn polygon(&mut self, p: &[Ring]) {
        let Some(shell) = p.first() else { return };
        if !shell.is_empty() && self.base.is_none() {
            self.base = Some(shell[0]);
        }
        let base = self.base.unwrap_or([0.0, 0.0]);
        let pos = !ccw(shell);
        for w in shell.windows(2) {
            self.triangle(base, w[0], w[1], pos);
        }
        self.segments(shell);
        for h in &p[1..] {
            let pos = ccw(h);
            for w in h.windows(2) {
                self.triangle(base, w[0], w[1], pos);
            }
            self.segments(h);
        }
    }
    fn get(&self) -> Pt {
        if self.area2.abs() > 0.0 {
            [self.cg3[0] / 3.0 / self.area2, self.cg3[1] / 3.0 / self.area2]
        } else if self.length > 0.0 {
            [self.line[0] / self.length, self.line[1] / self.length]
        } else if self.n > 0.0 {
            [self.pts[0] / self.n, self.pts[1] / self.n]
        } else {
            [f64::NAN, f64::NAN]
        }
    }
}
