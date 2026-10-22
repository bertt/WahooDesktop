//! GPX -> FIT course conversion (the ELEMNT reads courses from `routes/*.fit`).
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct Course {
    pub name: String,
    pub points: usize,
    pub distance_m: f64,
    pub bbox: [f64; 4],
}

struct Pt {
    lat: f64,
    lon: f64,
    ele: Option<f64>,
    time: Option<i64>,
}

const FIT_EPOCH: i64 = 631_065_600;
const MAX_POINTS: usize = 10_000;

fn attr(tag: &str, name: &str) -> Option<f64> {
    let i = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let rest = &tag[i..];
    rest[..rest.find('"')?].parse().ok()
}

fn text(body: &str, tag: &str) -> Option<String> {
    let s = body.find(&format!("<{tag}>"))? + tag.len() + 2;
    let e = body[s..].find(&format!("</{tag}>"))? + s;
    let t = body[s..e].trim();
    let t = t.strip_prefix("<![CDATA[").and_then(|x| x.strip_suffix("]]>")).unwrap_or(t);
    Some(t.trim().to_string())
}

fn parse_time(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp())
}

fn points_of(xml: &str, tag: &str) -> Vec<Pt> {
    let open = format!("<{tag} ");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = xml[pos..].find(&open) {
        let start = pos + i;
        let Some(gt) = xml[start..].find('>') else { break };
        let head = &xml[start..start + gt];
        let (body, next) = if head.ends_with('/') {
            ("", start + gt + 1)
        } else {
            let end = xml[start..].find(&close).map(|e| start + e).unwrap_or(xml.len());
            (&xml[start + gt + 1..end], end + close.len().min(xml.len() - end))
        };
        if let (Some(lat), Some(lon)) = (attr(head, "lat"), attr(head, "lon")) {
            out.push(Pt {
                lat,
                lon,
                ele: text(body, "ele").and_then(|e| e.parse().ok()),
                time: text(body, "time").and_then(|t| parse_time(&t)),
            });
        }
        pos = next.max(start + 1);
    }
    out
}

fn haversine(a: &Pt, b: &Pt) -> f64 {
    let r = 6_371_000.0;
    let (p1, p2) = (a.lat.to_radians(), b.lat.to_radians());
    let dl = (b.lon - a.lon).to_radians();
    let h = ((p2 - p1) / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * r * h.sqrt().asin()
}

struct Crc(u16);
impl Crc {
    fn feed(&mut self, bytes: &[u8]) {
        const T: [u16; 16] = [0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800, 0xB401, 0x5000, 0x9C01, 0x8801, 0x4400];
        for &b in bytes {
            let mut c = self.0;
            c = (c >> 4) ^ T[(c & 0xF) as usize] ^ T[(b & 0xF) as usize];
            c = (c >> 4) ^ T[(c & 0xF) as usize] ^ T[(b >> 4) as usize];
            self.0 = c;
        }
    }
}

fn semi(deg: f64) -> i32 {
    (deg * 2_147_483_648.0 / 180.0).round() as i32
}

fn def(buf: &mut Vec<u8>, local: u8, global: u16, fields: &[(u8, u8, u8)]) {
    buf.extend([0x40 | local, 0, 0]);
    buf.extend(global.to_le_bytes());
    buf.push(fields.len() as u8);
    for f in fields {
        buf.extend([f.0, f.1, f.2]);
    }
}

fn name16(name: &str) -> [u8; 16] {
    let mut n = [0u8; 16];
    let mut len = 0;
    for ch in name.chars().filter(|c| c.is_ascii() && !c.is_control()) {
        if len == 15 {
            break;
        }
        n[len] = ch as u8;
        len += 1;
    }
    n
}

/// Converts GPX text into FIT course bytes.
pub fn gpx_to_fit(gpx: &str, fallback_name: &str) -> Result<(Vec<u8>, Course), String> {
    let mut pts = points_of(gpx, "trkpt");
    if pts.len() < 2 {
        pts = points_of(gpx, "rtept");
    }
    if pts.len() < 2 {
        return Err("GPX contains no track or route (needs at least 2 points)".into());
    }
    if pts.len() > MAX_POINTS {
        let step = pts.len() as f64 / MAX_POINTS as f64;
        pts = (0..MAX_POINTS).map(|i| (i as f64 * step) as usize).map(|i| std::mem::replace(&mut pts[i], Pt { lat: 0.0, lon: 0.0, ele: None, time: None })).collect();
    }
    let name = text(gpx, "name").filter(|n| !n.is_empty()).unwrap_or_else(|| fallback_name.to_string());

    let base = pts.iter().find_map(|p| p.time).unwrap_or_else(|| chrono::Utc::now().timestamp());
    let mut dist = Vec::with_capacity(pts.len());
    let mut total = 0.0;
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            total += haversine(&pts[i - 1], p);
        }
        dist.push(total);
    }
    // Courses need strictly increasing timestamps; fall back to 1 s per point.
    let mut ts = Vec::with_capacity(pts.len());
    let mut last = base - 1;
    for p in &pts {
        let t = p.time.filter(|t| *t > last).unwrap_or(last + 1);
        ts.push(t);
        last = t;
    }
    let fit_t = |t: i64| (t - FIT_EPOCH).max(0) as u32;
    let elapsed = (ts[ts.len() - 1] - ts[0]).max(1) as u32 * 1000;

    let mut d = Vec::new();
    def(&mut d, 0, 0, &[(0, 1, 0), (1, 2, 0x84), (2, 2, 0x84), (4, 4, 0x86)]);
    d.push(0);
    d.extend([6u8]);
    d.extend(255u16.to_le_bytes());
    d.extend(0u16.to_le_bytes());
    d.extend(fit_t(base).to_le_bytes());

    def(&mut d, 1, 31, &[(4, 1, 0), (5, 16, 7)]);
    d.push(1);
    d.push(2); // cycling
    d.extend(name16(&name));

    let (first, lastp) = (&pts[0], &pts[pts.len() - 1]);
    def(&mut d, 2, 19, &[(253, 4, 0x86), (2, 4, 0x86), (3, 4, 0x85), (4, 4, 0x85), (5, 4, 0x85), (6, 4, 0x85), (7, 4, 0x86), (8, 4, 0x86), (9, 4, 0x86)]);
    d.push(2);
    d.extend(fit_t(ts[ts.len() - 1]).to_le_bytes());
    d.extend(fit_t(ts[0]).to_le_bytes());
    for v in [semi(first.lat), semi(first.lon), semi(lastp.lat), semi(lastp.lon)] {
        d.extend(v.to_le_bytes());
    }
    d.extend(elapsed.to_le_bytes());
    d.extend(elapsed.to_le_bytes());
    d.extend(((total * 100.0) as u32).to_le_bytes());

    def(&mut d, 3, 20, &[(253, 4, 0x86), (0, 4, 0x85), (1, 4, 0x85), (2, 2, 0x84), (5, 4, 0x86)]);
    for (i, p) in pts.iter().enumerate() {
        d.push(3);
        d.extend(fit_t(ts[i]).to_le_bytes());
        d.extend(semi(p.lat).to_le_bytes());
        d.extend(semi(p.lon).to_le_bytes());
        let alt = p.ele.map(|e| ((e + 500.0) * 5.0).clamp(0.0, 65534.0) as u16).unwrap_or(0xFFFF);
        d.extend(alt.to_le_bytes());
        d.extend(((dist[i] * 100.0) as u32).to_le_bytes());
    }

    let mut out = Vec::with_capacity(d.len() + 16);
    out.extend([14u8, 0x20]);
    out.extend(2132u16.to_le_bytes());
    out.extend((d.len() as u32).to_le_bytes());
    out.extend(b".FIT");
    let mut hc = Crc(0);
    hc.feed(&out);
    out.extend(hc.0.to_le_bytes());
    out.extend(&d);
    let mut c = Crc(0);
    c.feed(&out);
    out.extend(c.0.to_le_bytes());

    let lats = pts.iter().map(|p| p.lat);
    let lons = pts.iter().map(|p| p.lon);
    let bbox = [
        lons.clone().fold(f64::MAX, f64::min),
        lats.clone().fold(f64::MAX, f64::min),
        lons.fold(f64::MIN, f64::max),
        lats.fold(f64::MIN, f64::max),
    ];
    Ok((out, Course { name, points: pts.len(), distance_m: total, bbox }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_through_fit_parser() {
        let gpx = r#"<gpx><trk><name>Test loop</name><trkseg>
            <trkpt lat="52.0" lon="5.0"><ele>10</ele></trkpt>
            <trkpt lat="52.001" lon="5.001"><ele>12</ele></trkpt>
            <trkpt lat="52.002" lon="5.003"/></trkseg></trk></gpx>"#;
        let (bytes, c) = gpx_to_fit(gpx, "x").unwrap();
        assert_eq!(c.points, 3);
        let recs = fitparser::from_bytes(&bytes).unwrap();
        assert!(recs.iter().filter(|r| r.kind() == fitparser::profile::MesgNum::Record).count() == 3);
    }
}
