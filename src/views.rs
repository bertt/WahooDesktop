//! Pure JSON views over cached device data. Shared by the web server and the CLI.
use crate::app::DeviceData;
use crate::fit::Summary;
use chrono::{Datelike, TimeZone, Utc};
use serde_json::{Value, json};
use std::collections::BTreeMap;

// Rough country boxes [west, south, east, north]; the smallest box containing a tile centre wins.
const COUNTRIES: &[(&str, [f64; 4])] = &[
    ("Netherlands", [3.3, 50.75, 7.25, 53.55]),
    ("Belgium", [2.5, 49.5, 6.4, 51.5]),
    ("Luxembourg", [5.7, 49.4, 6.55, 50.2]),
    ("Germany", [5.9, 47.3, 15.0, 55.1]),
    ("France", [-5.2, 42.3, 8.3, 51.1]),
    ("United Kingdom", [-8.2, 49.8, 1.8, 60.9]),
    ("Ireland", [-10.6, 51.4, -5.4, 55.4]),
    ("Spain", [-9.4, 36.0, 3.4, 43.8]),
    ("Portugal", [-9.6, 36.9, -6.2, 42.2]),
    ("Italy", [6.6, 36.6, 18.6, 47.1]),
    ("Switzerland", [5.9, 45.8, 10.5, 47.8]),
    ("Austria", [9.5, 46.4, 17.2, 49.0]),
    ("Czechia", [12.1, 48.5, 18.9, 51.1]),
    ("Poland", [14.1, 49.0, 24.2, 54.9]),
    ("Denmark", [8.0, 54.5, 15.2, 57.8]),
    ("Sweden", [11.0, 55.3, 24.2, 69.1]),
    ("Norway", [4.5, 58.0, 31.2, 71.2]),
    ("Finland", [20.5, 59.7, 31.6, 70.1]),
    ("Estonia", [21.7, 57.5, 28.2, 59.7]),
    ("Latvia", [20.9, 55.6, 28.3, 58.1]),
    ("Lithuania", [20.9, 53.9, 26.9, 56.5]),
    ("Hungary", [16.1, 45.7, 22.9, 48.6]),
    ("Slovakia", [16.8, 47.7, 22.6, 49.7]),
    ("Slovenia", [13.3, 45.4, 16.6, 46.9]),
    ("Croatia", [13.4, 42.4, 19.5, 46.6]),
    ("Greece", [19.3, 34.8, 28.3, 41.8]),
    ("Turkey", [25.6, 35.8, 44.8, 42.1]),
    ("Romania", [20.2, 43.6, 29.8, 48.3]),
    ("Bulgaria", [22.3, 41.2, 28.7, 44.3]),
    ("Iceland", [-24.6, 63.3, -13.4, 66.6]),
    ("Morocco", [-13.2, 27.6, -1.0, 35.9]),
    ("Canary Islands", [-18.2, 27.6, -13.3, 29.5]),
    ("United States", [-125.0, 24.5, -66.9, 49.4]),
    ("Canada", [-141.0, 41.7, -52.6, 70.0]),
    ("Mexico", [-118.5, 14.5, -86.7, 32.7]),
    ("Brazil", [-74.0, -33.8, -34.8, 5.3]),
    ("Argentina", [-73.6, -55.1, -53.6, -21.8]),
    ("Australia", [112.9, -43.7, 153.7, -10.6]),
    ("New Zealand", [166.4, -47.3, 178.6, -34.4]),
    ("Japan", [129.4, 31.0, 145.9, 45.6]),
    ("South Africa", [16.4, -34.9, 32.9, -22.1]),
];

pub fn tile_bbox(z: u32, x: u32, y: u32) -> [f64; 4] {
    let n = (1u64 << z) as f64;
    let lon = |x: f64| x / n * 360.0 - 180.0;
    let lat = |y: f64| (std::f64::consts::PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees();
    [lon(x as f64), lat(y as f64 + 1.0), lon(x as f64 + 1.0), lat(y as f64)]
}

fn region(c: [f64; 2]) -> String {
    COUNTRIES
        .iter()
        .filter(|(_, b)| c[0] >= b[0] && c[0] <= b[2] && c[1] >= b[1] && c[1] <= b[3])
        .min_by(|a, b| area(a.1).partial_cmp(&area(b.1)).unwrap())
        .map(|(n, _)| n.to_string())
        .unwrap_or_else(|| format!("{:.0}°{}, {:.0}°{}", c[1].abs(), if c[1] >= 0.0 { "N" } else { "S" }, c[0].abs(), if c[0] >= 0.0 { "E" } else { "W" }))
}

fn area(b: [f64; 4]) -> f64 {
    (b[2] - b[0]) * (b[3] - b[1])
}

pub fn info(d: &DeviceData) -> Value {
    let m = &d.manifest;
    let last = d.summaries.last();
    let sys: Vec<&str> = d.system_info.lines().collect();
    json!({
        "key": d.key,
        "name": m.device.name,
        "manufacturer": m.device.manufacturer,
        "model": m.device.model,
        "serial": if d.serial_txt.is_empty() { &m.device.serial } else { &d.serial_txt },
        "firmware": last.map(|s| s.firmware.clone()).unwrap_or_else(|| m.device.firmware.clone()),
        "hardware": last.map(|s| s.hardware.clone()),
        "descriptor": last.map(|s| s.device.clone()),
        "mtp_firmware": m.device.firmware,
        "factory_id": sys.first(),
        "factory_extra": sys.get(1),
        "pending_update": m.entries.iter().find(|e| !e.dir && e.path.starts_with("system_update_elemnt/")).map(|e| json!({"file": e.path, "bytes": e.size})),
        "synced_at": m.synced_at,
        "activities": d.summaries.len(),
        "first_activity": d.summaries.first().map(|s| s.start),
        "last_activity": last.map(|s| s.start),
        "power_level": m.device.power_level,
        "usb_id": m.device.pnp_id,
    })
}

pub fn firmware(d: &DeviceData) -> Value {
    let i = info(d);
    let mut v = crate::firmware::check(i["descriptor"].as_str().or(i["model"].as_str()).unwrap_or(""), i["firmware"].as_str().unwrap_or(""));
    v["pending_update"] = i["pending_update"].clone();
    v
}

pub fn battery(d: &DeviceData) -> Value {
    let mut hist = Vec::new();
    for s in &d.summaries {
        if let Some(b) = s.battery_start {
            hist.push(json!([s.start, b]));
        }
        if let Some(b) = s.battery_end {
            hist.push(json!([s.end, b]));
        }
    }
    // Discharge rate: percent per hour of recorded ride time.
    let (mut pct, mut hrs) = (0.0, 0.0);
    for s in &d.summaries {
        if let (Some(a), Some(b)) = (s.battery_start, s.battery_end) {
            if a >= b && s.elapsed > 600.0 {
                pct += a - b;
                hrs += s.elapsed / 3600.0;
            }
        }
    }
    let rate = if hrs > 0.0 { Some(pct / hrs) } else { None };
    json!({
        "level": d.manifest.device.power_level,
        "history": hist,
        "drain_per_hour": rate,
        "estimated_hours": rate.filter(|r| *r > 0.0).map(|r| 100.0 / r),
    })
}

pub fn storage(d: &DeviceData) -> Value {
    let mut cats: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for e in d.manifest.entries.iter().filter(|e| !e.dir) {
        let mut parts = e.path.split('/');
        let top = parts.next().unwrap_or("");
        let key = match (top, parts.next()) {
            ("maps", Some("tiles")) => "maps/tiles".to_string(),
            ("maps", Some("temp")) => "maps/temp".to_string(),
            ("maps", Some("routing")) => "maps/routing".to_string(),
            ("maps", _) => "maps/other".to_string(),
            _ => top.to_string(),
        };
        let c = cats.entry(key).or_default();
        c.0 += 1;
        c.1 += e.size;
    }
    let st = d.manifest.storages.first();
    let (cap, free) = st.map(|s| (s.capacity, s.free)).unwrap_or((0, 0));
    let used = cap.saturating_sub(free);
    let known: u64 = cats.values().map(|c| c.1).sum();
    let mut list: Vec<Value> = cats.iter().map(|(k, (n, b))| json!({"name": k, "files": n, "bytes": b})).collect();
    list.sort_by_key(|v| std::cmp::Reverse(v["bytes"].as_u64().unwrap_or(0)));
    json!({
        "name": st.map(|s| s.name.clone()),
        "fs": st.map(|s| s.fs.clone()),
        "capacity": cap, "free": free, "used": used,
        "categories": list,
        "unaccounted": used.saturating_sub(known),
        "note": "Routing graphs are not enumerated (≈50k small files), so they appear as unaccounted space together with system data.",
    })
}

pub fn maps(d: &DeviceData) -> Value {
    let mut tiles = Vec::new();
    let mut markers: BTreeMap<String, u64> = BTreeMap::new();
    let mut versions: BTreeMap<String, u64> = BTreeMap::new();
    let mut tile_mtime: BTreeMap<String, i64> = BTreeMap::new();
    let mut marker_mtime: BTreeMap<String, i64> = BTreeMap::new();
    let mut version_dates: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut pax = (0u64, 0u64);
    let mut other = Vec::new();
    for e in &d.manifest.entries {
        let p = e.path.as_str();
        if p.contains("PaxHeader") {
            if !e.dir {
                pax.0 += 1;
                pax.1 += e.size;
            }
            continue;
        }
        if let Some(rest) = p.strip_prefix("maps/tiles/") {
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() == 3 && !e.dir {
                let (z, x) = (parts[0].parse::<u32>(), parts[1].parse::<u32>());
                let file = parts[2];
                if let (Ok(z), Ok(x)) = (z, x) {
                    if let Some(ystr) = file.split('.').next() {
                        if let Ok(y) = ystr.parse::<u32>() {
                            let ver = file.rsplit('.').next().filter(|v| v.chars().all(|c| c.is_ascii_digit()) && file.ends_with(v) && !file.ends_with("lzma")).map(|v| v.to_string());
                            let key = format!("{z}/{x}/{y}");
                            if let Some(v) = ver {
                                markers.insert(key.clone(), v.parse().unwrap_or(0));
                                if let Some(m) = e.modified { marker_mtime.insert(key, m); }
                            } else if file.ends_with(".map.lzma") {
                                tiles.push((z, x, y, e.size));
                                if let Some(m) = e.modified { tile_mtime.insert(key, m); }
                            }
                        }
                    }
                }
            }
        } else if p.starts_with("maps/") && !e.dir && !p.starts_with("maps/temp/") && !p.starts_with("maps/routing/") {
            other.push(json!({"path": p, "bytes": e.size}));
        }
    }
    let mut regions: BTreeMap<String, (u32, u64)> = BTreeMap::new();
    let mut out_tiles = Vec::new();
    let mut total = 0u64;
    let mut missing_marker = 0u32;
    for (z, x, y, size) in &tiles {
        let b = tile_bbox(*z, *x, *y);
        let c = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
        let r = region(c);
        let e = regions.entry(r.clone()).or_default();
        e.0 += 1;
        e.1 += size;
        total += size;
        let key = format!("{z}/{x}/{y}");
        let ver = markers.get(&key).copied();
        if ver.is_none() {
            missing_marker += 1;
        } else {
            *versions.entry(ver.unwrap().to_string()).or_default() += 1;
        }
        let modified = tile_mtime.get(&key).copied().or_else(|| marker_mtime.get(&key).copied()).filter(|m| *m > 0);
        if let (Some(v), Some(m)) = (ver, modified) {
            let r = version_dates.entry(v.to_string()).or_insert((m, m));
            r.0 = r.0.min(m);
            r.1 = r.1.max(m);
        }
        out_tiles.push(json!({"z": z, "x": x, "y": y, "bytes": size, "version": ver, "modified": modified, "bbox": b, "region": r}));
    }
    let tile_keys: std::collections::HashSet<String> = tiles.iter().map(|(z, x, y, _)| format!("{z}/{x}/{y}")).collect();
    let orphan_markers = markers.keys().filter(|k| !tile_keys.contains(*k)).count();
    let temp: Vec<Value> = d.manifest.entries.iter().filter(|e| !e.dir && e.path.starts_with("maps/temp/")).map(|e| json!({"path": e.path, "bytes": e.size})).collect();
    let temp_bytes: u64 = d.manifest.entries.iter().filter(|e| !e.dir && e.path.starts_with("maps/temp/")).map(|e| e.size).sum();
    let mut reg: Vec<Value> = regions.into_iter().map(|(n, (t, b))| json!({"region": n, "tiles": t, "bytes": b})).collect();
    reg.sort_by_key(|v| std::cmp::Reverse(v["bytes"].as_u64().unwrap_or(0)));
    let mut warnings = Vec::new();
    if temp_bytes > 0 {
        warnings.push(format!("maps/temp holds {} leftover tile file(s) ({:.1} MB): extracted tiles the device no longer needs.", temp.len(), temp_bytes as f64 / 1e6));
    }
    if pax.0 > 0 {
        warnings.push(format!("{} stray PaxHeader files ({} bytes) from the tile archive extractor are stored on the device.", pax.0, pax.1));
    }
    if orphan_markers > 0 {
        warnings.push(format!("{orphan_markers} tile version markers have no matching tile file (not downloaded or removed)."));
    }
    if missing_marker > 0 {
        warnings.push(format!("{missing_marker} tiles have no version marker."));
    }
    if versions.len() > 1 {
        warnings.push(format!("Tiles come from {} different map versions: {:?}", versions.len(), versions));
    }
    json!({
        "tiles": out_tiles,
        "tile_count": tiles.len(),
        "tile_bytes": total,
        "regions": reg,
        "versions": versions,
        "version_dates": version_dates.iter().map(|(v, (a, b))| (v.clone(), json!({"oldest": a, "newest": b}))).collect::<serde_json::Map<String, Value>>(),
        "temp": temp,
        "temp_bytes": temp_bytes,
        "tilepacks": d.tilepacks,
        "other_files": other,
        "pax_headers": {"files": pax.0, "bytes": pax.1},
        "warnings": warnings,
        "can_download": true,
        "download_note": "Tiles are public on Wahoo's CDN (https://cdn.wahooligan.com/wahoo-maps/<version>/tiles/8/<x>/<y>.map.lzma). The ELEMNT does not fetch maps over USB; Wahoo Desktop can check for newer versions (see Map updates) but does not yet write tiles to the device.",
        "sources": {
            "found_in": "Strings inside the firmware package staged on the device (system_update_elemnt/<build>/BoltApp, an Android APK).",
            "maps_base": "https://cdn.wahooligan.com/wahoo-maps",
            "firmware_manifest": "https://bolt.wahoofitness.com/boltapp/version.json-bolt2",
            "other": ["https://bolt.wahoofitness.com/boltapp/version.json-bolt", "https://bolt.wahoofitness.com/boltapp/version.json-roam", "https://bolt.wahoofitness.com/boltapp/version.json-roam2"],
            "files_on_device": ["maps/tilepack-versions.json", "maps/tiles_md5", "maps/routing_md5"],
            "examples": [
                {"title": "Check latest firmware (public, works)", "request": "curl -i https://bolt.wahoofitness.com/boltapp/version.json-bolt2", "result": "200 OK – JSON with std-version and std-url"},
                {"title": "Newest build of the same tile", "request": "curl -I https://cdn.wahooligan.com/wahoo-maps/23/tiles/8/131/83.map.lzma", "result": "200 OK – compare Content-Length with the file on the device to see if it is newer"},
                {"title": "Tile pack list of a map version (used by the ELEMNT app)", "request": "curl -O https://cdn.wahooligan.com/wahoo-maps/23/mappack-gzip.json.gz", "result": "200 OK – gzip JSON with all tile packs of that map version"},
                {"title": "One map tile (zoom 8, x=131, y=83)", "request": "curl -I https://cdn.wahooligan.com/wahoo-maps/18/tiles/8/131/83.map.lzma", "result": "200 OK – Content-Length, Last-Modified and ETag identify the tile build"},
            ],
            "note": "Both URLs are readable without credentials. Only the bucket root is not listable. The Mapbox tilepack API (api.mapbox.com/tilepack/v1/...) also appears in the firmware, but only for Mapbox's own tiles.",
        },
    })
}

/// Parses `maps/routing_md5` (`<md5>  /data/media/0/maps/routing/<level>/<aaa>/<bbb>/<ccc>.gph.lzma`) into the
/// number of Valhalla graph tiles per level and a coverage grid of 1-degree cells for level 2 (local roads).
/// A Valhalla tile id is `row * columns + column` on a grid of 4 / 1 / 0.25 degree tiles for levels 0 / 1 / 2.
fn routing_listing(text: &str) -> Value {
    const NAMES: [&str; 3] = ["highways", "arterial roads", "local roads"];
    let mut levels: BTreeMap<u32, u64> = BTreeMap::new();
    let mut cells: BTreeMap<(i32, i32), u32> = BTreeMap::new();
    for line in text.lines() {
        let Some(path) = line.split_once(char::is_whitespace).map(|(_, p)| p.trim()) else { continue };
        let Some(rest) = path.split("maps/routing/").nth(1) else { continue };
        let Some(rest) = rest.strip_suffix(".gph.lzma").or_else(|| rest.strip_suffix(".gph")) else { continue };
        let mut parts = rest.split('/');
        let Some(level) = parts.next().and_then(|l| l.parse::<u32>().ok()) else { continue };
        let Ok(id) = parts.collect::<String>().parse::<u64>() else { continue };
        *levels.entry(level).or_default() += 1;
        if level == 2 {
            let (row, col) = ((id / 1440) as f64, (id % 1440) as f64);
            *cells.entry(((-90.0 + row * 0.25).floor() as i32, (-180.0 + col * 0.25).floor() as i32)).or_default() += 1;
        }
    }
    let total: u64 = levels.values().sum();
    let level_rows: Vec<Value> = levels.iter().map(|(l, n)| json!({"level": l, "name": NAMES.get(*l as usize), "tiles": n})).collect();
    let coverage: Vec<Value> = cells.iter().map(|((lat, lon), n)| json!({"bbox": [lon, lat, lon + 1, lat + 1], "tiles": n})).collect();
    json!({"tiles": total, "levels": level_rows, "coverage": coverage})
}

/// Valhalla routing data on the device (`maps/routing`): graph tile listing, checksum list and service config.
pub fn routing(d: &DeviceData) -> Value {
    let (mut folders, mut temp_files, mut temp_bytes) = (0u64, 0u64, 0u64);
    let (mut md5, mut config_file) = (Value::Null, Value::Null);
    for e in &d.manifest.entries {
        let p = e.path.as_str();
        if p.contains("PaxHeader") {
            continue;
        }
        if p == "maps/routing_md5" {
            md5 = json!({"bytes": e.size, "modified": e.modified});
        } else if p == "maps/routing/routing-config.json" {
            config_file = json!({"bytes": e.size, "modified": e.modified});
        } else if let Some(rest) = p.strip_prefix("maps/routing/") {
            if e.dir {
                folders += 1;
            } else if rest.starts_with("temp/") {
                temp_files += 1;
                temp_bytes += e.size;
            }
        }
    }
    let listing = std::fs::read(d.dir.join("files").join("maps").join("routing_md5")).ok().map(|b| routing_listing(&String::from_utf8_lossy(&b)));
    let mut warnings = Vec::new();
    if listing.is_none() {
        warnings.push("maps/routing_md5 is not in the local cache yet: run a sync to read the graph tile list.".to_string());
    }
    if temp_files > 0 {
        warnings.push(format!("maps/routing/temp holds {temp_files} leftover file(s) ({:.1} MB).", temp_bytes as f64 / 1e6));
    }
    json!({
        "graph_tiles": listing,
        "folders": folders,
        "temp": {"files": temp_files, "bytes": temp_bytes},
        "checksums": md5,
        "config_file": config_file,
        "config": d.routing_config,
        "warnings": warnings,
        "layout": "maps/routing/<level>/<aaa>/<bbb>/<ccc>.gph.lzma (LZMA-compressed Valhalla graph tiles; level 0 = highways, 1 = arterial roads, 2 = local roads). The tile list comes from maps/routing_md5, the device's checksum list; the sync does not walk the roughly 40,000 graph files themselves.",
    })
}

pub fn sensors(d: &DeviceData) -> Value {
    let latest = d.summaries.last().and_then(|s| crate::fit::parse_file(&d.dir.join("files").join("exports").join(&s.file), true).ok());
    let mut external: BTreeMap<String, u32> = BTreeMap::new();
    for s in &d.summaries {
        for n in &s.sensors {
            *external.entry(n.clone()).or_default() += 1;
        }
    }
    // Values the internal sensors recorded during the latest ride (the device keeps no live value over USB).
    let readings = latest.as_ref().map(|l| {
        let s = &l.series;
        let stat = |v: &[Option<f64>]| {
            let xs: Vec<f64> = v.iter().flatten().copied().collect();
            if xs.is_empty() {
                return Value::Null;
            }
            let last = v.iter().rposition(|x| x.is_some()).map(|i| (s.t[i], v[i].unwrap()));
            json!({
                "last": last.map(|x| x.1), "last_at": last.map(|x| x.0),
                "min": xs.iter().cloned().fold(f64::MAX, f64::min), "max": xs.iter().cloned().fold(f64::MIN, f64::max),
                "avg": xs.iter().sum::<f64>() / xs.len() as f64, "samples": xs.len(),
            })
        };
        json!({"activity": l.summary.id, "temperature": stat(&s.temp), "altitude": stat(&s.alt), "speed": stat(&s.speed), "heart_rate": stat(&s.hr), "power": stat(&s.power), "cadence": stat(&s.cadence)})
    });
    let temp_history: Vec<Value> = d.summaries.iter().filter(|s| s.avg_temp.is_some()).map(|s| json!([s.start, s.avg_temp, s.max_temp])).collect();
    // The FIT files hold no raw accelerometer samples, so acceleration is derived from the speed trace.
    let series = latest.as_ref().map(|l| {
        let s = &l.series;
        let accel: Vec<Option<f64>> = (0..s.t.len())
            .map(|i| match (i.checked_sub(1).and_then(|p| s.speed[p]), s.speed[i]) {
                (Some(a), Some(b)) if s.t[i] > s.t[i - 1] => Some((b - a) / (s.t[i] - s.t[i - 1]) as f64),
                _ => None,
            })
            .collect();
        json!({"t": s.t, "temp": s.temp, "alt": s.alt, "accel": accel})
    });
    json!({
        "from_activity": latest.as_ref().map(|l| l.summary.id.clone()),
        "devices": latest.map(|l| l.sensors).unwrap_or_default(),
        "readings": readings,
        "temp_history": temp_history,
        "series": series,
        "external_usage": external,
        "note": if external.is_empty() { "No external (ANT+/BLE) sensors were recorded in any activity." } else { "" },
    })
}

fn list_files(d: &DeviceData, dir: &str) -> Vec<Value> {
    d.manifest
        .entries
        .iter()
        .filter(|e| !e.dir && e.path.starts_with(&format!("{dir}/")))
        .map(|e| json!({"path": e.path, "name": e.path.rsplit('/').next(), "bytes": e.size, "modified": e.modified}))
        .collect()
}

pub fn routes(d: &DeviceData) -> Value {
    json!({"items": list_files(d, "routes")})
}

pub fn plans(d: &DeviceData) -> Value {
    json!({"items": list_files(d, "plans")})
}

pub fn alerts(d: &DeviceData) -> Value {
    let mut items: Vec<Value> = Vec::new();
    let mut add = |level: &str, title: &str, detail: String, when: Option<i64>| items.push(json!({"level": level, "title": title, "detail": detail, "when": when}));
    let m = &d.manifest;
    let synced = chrono::DateTime::parse_from_rfc3339(&m.synced_at).ok().map(|t| t.timestamp());
    let newest = |prefix: &str| m.entries.iter().filter(|e| !e.dir && e.path.starts_with(prefix)).filter_map(|e| e.modified).max();
    if let Some(s) = m.storages.first() {
        let pct = s.free as f64 / s.capacity.max(1) as f64 * 100.0;
        if pct < 10.0 {
            add("critical", "Storage almost full", format!("{pct:.1}% free"), synced);
        } else if pct < 20.0 {
            add("warning", "Storage getting full", format!("{pct:.1}% free"), synced);
        }
    }
    if let Some(l) = m.device.power_level {
        if l <= 20 {
            add("warning", "Battery low", format!("{l}%"), synced);
        }
    }
    if let Some(e) = m.entries.iter().find(|e| !e.dir && e.path.starts_with("system_update_elemnt/")) {
        add("info", "Firmware update file staged on device", format!("{} ({:.1} MB)", e.path, e.size as f64 / 1e6), e.modified);
    }
    let crashes: Vec<_> = m.entries.iter().filter(|e| !e.dir && e.path.starts_with(".crash/")).collect();
    if !crashes.is_empty() {
        add("warning", "Crash reports present", format!("{} file(s) in .crash", crashes.len()), newest(".crash/"));
    }
    let temp: u64 = m.entries.iter().filter(|e| !e.dir && e.path.starts_with("maps/temp/")).map(|e| e.size).sum();
    if temp > 0 {
        add("info", "Leftover map files", format!("{:.0} MB in maps/temp can be reclaimed", temp as f64 / 1e6), newest("maps/temp/"));
    }
    if let Some(s) = d.summaries.iter().rev().find(|s| s.battery_end.map(|b| b < 15.0).unwrap_or(false)) {
        add("info", "A ride ended with very low battery", format!("{} ended at {}%", s.id, s.battery_end.unwrap_or(0.0)), Some(s.end));
    }
    items.sort_by_key(|i| std::cmp::Reverse(i["when"].as_i64().unwrap_or(0)));
    json!({"items": items, "note": "The ELEMNT does not log its own alert history over USB; these are derived from the files on the device. Times come from file modification dates, ride end times or the last sync."})
}

fn ride_json(s: &Summary) -> Value {
    serde_json::to_value(s).unwrap_or(Value::Null)
}

pub fn history(d: &DeviceData) -> Value {
    let items: Vec<Value> = d.summaries.iter().rev().map(ride_json).collect();
    json!({"items": items})
}

pub fn stats(d: &DeviceData) -> Value {
    let s = &d.summaries;
    let sum = |f: &dyn Fn(&Summary) -> f64| s.iter().map(f).sum::<f64>();
    let mut by_month: BTreeMap<String, (u32, f64, f64, f64)> = BTreeMap::new();
    let mut by_year: BTreeMap<i32, (u32, f64, f64, f64)> = BTreeMap::new();
    let mut by_dow = [0u32; 7];
    let mut dow_dist = [0f64; 7];
    let mut by_hour = [0u32; 24];
    for r in s {
        let dt = chrono::Local.timestamp_opt(r.start, 0).single();
        if let Some(dt) = dt {
            let m = by_month.entry(dt.format("%Y-%m").to_string()).or_default();
            *m = (m.0 + 1, m.1 + r.distance, m.2 + r.timer, m.3 + r.ascent);
            let y = by_year.entry(dt.year()).or_default();
            *y = (y.0 + 1, y.1 + r.distance, y.2 + r.timer, y.3 + r.ascent);
            let wd = dt.weekday().num_days_from_monday() as usize;
            by_dow[wd] += 1;
            dow_dist[wd] += r.distance;
            by_hour[chrono::Timelike::hour(&dt) as usize] += 1;
        }
    }
    let best = |f: &dyn Fn(&Summary) -> f64| s.iter().max_by(|a, b| f(a).partial_cmp(&f(b)).unwrap()).map(|r| json!({"id": r.id, "start": r.start, "value": f(r)}));
    let mut sports: BTreeMap<String, u32> = BTreeMap::new();
    for r in s {
        *sports.entry(format!("{}/{}", r.sport, r.sub_sport)).or_default() += 1;
    }
    let total_time = sum(&|r| r.timer);
    let total_dist = sum(&|r| r.distance);
    let days: std::collections::BTreeSet<_> = s.iter().filter_map(|r| chrono::Local.timestamp_opt(r.start, 0).single()).map(|d| d.date_naive()).collect();
    let (mut streak, mut best_streak, mut prev) = (0u32, 0u32, None::<chrono::NaiveDate>);
    for dday in &days {
        streak = if prev.map(|p| (*dday - p).num_days() == 1).unwrap_or(false) { streak + 1 } else { 1 };
        best_streak = best_streak.max(streak);
        prev = Some(*dday);
    }
    json!({
        "rides": s.len(),
        "distance": total_dist,
        "moving_time": total_time,
        "elapsed_time": sum(&|r| r.elapsed),
        "ascent": sum(&|r| r.ascent),
        "calories": sum(&|r| r.calories.unwrap_or(0.0)),
        "avg_speed": if total_time > 0.0 { total_dist / total_time } else { 0.0 },
        "active_days": days.len(),
        "best_streak_days": best_streak,
        "by_month": by_month.iter().map(|(k, v)| json!({"month": k, "rides": v.0, "distance": v.1, "time": v.2, "ascent": v.3})).collect::<Vec<_>>(),
        "by_year": by_year.iter().map(|(k, v)| json!({"year": k, "rides": v.0, "distance": v.1, "time": v.2, "ascent": v.3})).collect::<Vec<_>>(),
        "by_weekday": by_dow, "weekday_distance": dow_dist, "by_hour": by_hour,
        "sports": sports,
        "records": {
            "longest": best(&|r| r.distance),
            "longest_time": best(&|r| r.timer),
            "most_ascent": best(&|r| r.ascent),
            "fastest_avg": best(&|r| r.avg_speed),
            "top_speed": best(&|r| r.max_speed),
            "highest_hr": best(&|r| r.max_hr.unwrap_or(0.0)),
            "max_power": best(&|r| r.max_power.unwrap_or(0.0)),
        },
        "points": s.iter().filter_map(|r| r.start_pos).collect::<Vec<_>>(),
    })
}

pub fn debug(d: &DeviceData) -> Value {
    let m = &d.manifest;
    let mut top: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for e in &m.entries {
        let k = e.path.split('/').next().unwrap_or("").to_string();
        let t = top.entry(k).or_default();
        t.0 += 1;
        t.1 += e.size;
    }
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for s in &d.summaries {
        *kinds.entry(s.sport.clone()).or_default() += 1;
    }
    json!({
        "device": m.device,
        "storages": m.storages,
        "walk_ms": m.walk_ms,
        "objects": m.entries.len(),
        "copied_last_sync": m.copied.len(),
        "synced_at": m.synced_at,
        "cache_dir": d.dir.display().to_string(),
        "parse_ms": d.parse_ms,
        "parse_cached": d.parse_cached,
        "top_level": top.iter().map(|(k, v)| json!({"path": k, "objects": v.0, "bytes": v.1})).collect::<Vec<_>>(),
        "system_info": d.system_info,
        "serial_txt": d.serial_txt,
        "tilepacks": d.tilepacks,
        "routing_config": d.routing_config,
        "synced_at_utc": Utc::now().to_rfc3339(),
    })
}
