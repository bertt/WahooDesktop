//! Reader for the Valhalla graph tiles (`maps/routing/<level>/<aaa>/<bbb>/<ccc>.gph.lzma`) on the ELEMNT.
//!
//! The layout follows Valhalla 3.1.4 (`baldr/graphtileheader.h`, `nodeinfo.h`, `directededge.h`, `edgeinfo.h`).
//! Only what is needed to draw the road network is decoded: header, directed edges and edge info (shape, names).

use serde_json::{Map, Value, json};
use std::collections::HashSet;

const HEADER_SIZE: usize = 272;
const NODE_SIZE: usize = 32;
const TRANSITION_SIZE: usize = 8;
const EDGE_SIZE: usize = 48;

const ROAD_CLASS: [&str; 8] = ["motorway", "trunk", "primary", "secondary", "tertiary", "unclassified", "residential", "service_other"];
const SURFACE: [&str; 8] = ["paved_smooth", "paved", "paved_rough", "compacted", "dirt", "gravel", "path", "impassable"];

pub struct Export {
    pub geojson: String,
    pub tile_id: u32,
    pub level: u32,
    pub nodes: u32,
    pub directed_edges: u32,
    pub segments: usize,
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, String> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| "tile is truncated".to_string())
}

fn u64_at(b: &[u8], o: usize) -> Result<u64, String> {
    b.get(o..o + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| "tile is truncated".to_string())
}

fn bits(w: u64, lo: u32, n: u32) -> u64 {
    (w >> lo) & ((1u64 << n) - 1)
}

/// Returns the raw graph tile: decompresses an LZMA-alone stream (`.gph.lzma`), or passes a raw `.gph` through.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() > 13 && data[0] == 0x5d {
        let mut out = Vec::new();
        lzma_rs::lzma_decompress(&mut std::io::Cursor::new(data), &mut out).map_err(|e| format!("LZMA: {e}"))?;
        Ok(out)
    } else {
        Ok(data.to_vec())
    }
}

/// Varint shape: `lon, lat` pairs as zig-zag deltas in 1e-6 degree (Valhalla `encode7`).
fn decode_shape(b: &[u8]) -> Vec<(f64, f64)> {
    let mut vals = Vec::new();
    let (mut v, mut shift) = (0u64, 0);
    for &byte in b {
        v |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 != 0 {
            shift += 7;
        } else {
            vals.push(((v >> 1) as i64) ^ -((v & 1) as i64));
            v = 0;
            shift = 0;
        }
    }
    let (mut lat, mut lon) = (0i64, 0i64);
    vals.chunks_exact(2)
        .map(|c| {
            lat += c[0];
            lon += c[1];
            (lon as f64 * 1e-6, lat as f64 * 1e-6)
        })
        .collect()
}

fn cstr(b: &[u8], o: usize) -> String {
    let s = b.get(o..).unwrap_or(&[]);
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..end]).into_owned()
}

/// Converts an uncompressed graph tile into a GeoJSON FeatureCollection with one LineString per road segment.
/// Both directed edges of a segment share the same edge info; only the first one found is exported.
pub fn export_geojson(b: &[u8]) -> Result<Export, String> {
    if b.len() < HEADER_SIZE {
        return Err("not a graph tile (too small)".into());
    }
    let w0 = u64_at(b, 0)?;
    let (level, tile_id) = (bits(w0, 0, 3) as u32, bits(w0, 3, 22) as u32);
    let version = cstr(&b[16..32], 0);
    if !version.starts_with("3.") || level > 2 {
        return Err(format!("not a Valhalla 3.x graph tile (level {level}, version '{version}')"));
    }
    let counts = u64_at(b, 40)?;
    let (nodes, edges) = (bits(counts, 0, 21) as usize, bits(counts, 21, 21) as usize);
    let transitions = bits(u32_at(b, 48)? as u64, 0, 22) as usize;
    let edgeinfo_offset = u32_at(b, 104)? as usize;
    let text_offset = u32_at(b, 108)? as usize;
    let edges_start = HEADER_SIZE + NODE_SIZE * nodes + TRANSITION_SIZE * transitions;
    if edges_start + EDGE_SIZE * edges > b.len() || edgeinfo_offset > b.len() || text_offset > b.len() {
        return Err("graph tile layout does not match the header".into());
    }

    let mut seen = HashSet::new();
    let mut features = Vec::new();
    for i in 0..edges {
        let o = edges_start + EDGE_SIZE * i;
        let (w0, w1, w2, w3, w4) = (u64_at(b, o)?, u64_at(b, o + 8)?, u64_at(b, o + 16)?, u64_at(b, o + 24)?, u64_at(b, o + 32)?);
        let info_off = bits(w1, 0, 25) as usize;
        if !seen.insert(info_off) {
            continue;
        }
        let p = edgeinfo_offset + info_off;
        let (wayid, packed, packed2) = (u32_at(b, p)?, u32_at(b, p + 4)?, u32_at(b, p + 8)?);
        let name_count = bits(packed2 as u64, 0, 4) as usize;
        let shape_size = bits(packed2 as u64, 4, 16) as usize;
        let ext_size = bits(packed2 as u64, 28, 2) as usize;
        let shape_start = p + 12 + 4 * name_count;
        let shape = decode_shape(b.get(shape_start..shape_start + shape_size).ok_or("edge shape is truncated")?);
        if shape.len() < 2 {
            continue;
        }
        let mut way = wayid as u64 | (bits(packed as u64, 24, 8) << 32);
        if ext_size > 0 {
            way |= bits(packed2 as u64, 20, 8) << 40;
        }
        let mut names = Vec::new();
        for n in 0..name_count {
            let ni = u32_at(b, p + 12 + 4 * n)? as u64;
            if bits(ni, 29, 1) == 0 {
                names.push(cstr(b, text_offset + bits(ni, 0, 24) as usize));
            }
        }
        let class = bits(w2, 54, 3) as usize;
        let surface = bits(w2, 57, 3) as usize;
        let mut props = Map::new();
        props.insert("osm_way".into(), json!(way));
        props.insert("name".into(), json!(names.join("; ")));
        props.insert("road_class".into(), json!(ROAD_CLASS[class]));
        props.insert("use".into(), json!(use_name(bits(w2, 40, 6))));
        props.insert("surface".into(), json!(SURFACE[surface]));
        props.insert("speed_kph".into(), json!(bits(w2, 0, 8)));
        props.insert("speed_limit_kph".into(), json!(bits(packed as u64, 16, 8)));
        props.insert("lanes".into(), json!(bits(w2, 46, 4)));
        props.insert("length_m".into(), json!(bits(w4, 32, 24)));
        props.insert("mean_elevation_m".into(), json!(bits(packed as u64, 0, 12) as i64 * 2 - 500));
        props.insert("max_up_slope".into(), json!(bits(w3, 24, 5)));
        props.insert("max_down_slope".into(), json!(bits(w3, 29, 5)));
        props.insert("bridge".into(), json!(bits(w3, 50, 1) == 1));
        props.insert("tunnel".into(), json!(bits(w3, 49, 1) == 1));
        props.insert("roundabout".into(), json!(bits(w2, 61, 1) == 1));
        props.insert("toll".into(), json!(bits(w2, 60, 1) == 1));
        props.insert("leaves_tile".into(), json!(bits(w0, 62, 1) == 1));
        let coords: Vec<Value> = shape.iter().map(|(x, y)| json!([(x * 1e6).round() / 1e6, (y * 1e6).round() / 1e6])).collect();
        features.push(json!({"type": "Feature", "properties": props, "geometry": {"type": "LineString", "coordinates": coords}}).to_string());
    }

    let segments = features.len();
    let geojson = format!("{{\"type\":\"FeatureCollection\",\"features\":[{}]}}", features.join(",\n"));
    Ok(Export { geojson, tile_id, level, nodes: nodes as u32, directed_edges: edges as u32, segments })
}

fn use_name(u: u64) -> String {
    match u {
        0 => "road",
        1 => "ramp",
        2 => "turn_channel",
        3 => "track",
        4 => "driveway",
        5 => "alley",
        6 => "parking_aisle",
        7 => "emergency_access",
        8 => "drive_through",
        9 => "culdesac",
        10 => "living_street",
        20 => "cycleway",
        21 => "mountain_bike",
        24 => "sidewalk",
        25 => "footway",
        26 => "steps",
        27 => "path",
        28 => "pedestrian",
        29 => "bridleway",
        _ => return format!("other_{u}"),
    }
    .to_string()
}
