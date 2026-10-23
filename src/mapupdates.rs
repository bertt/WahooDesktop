//! Map update check against Wahoo's public map CDN.
//!
//! The ELEMNT app downloads `<CDN>/<version>/mappack-gzip.json.gz` (the list of tile packs of that
//! map version) and the device downloads tiles from `<CDN>/<version>/tiles/8/<x>/<y>.map.lzma`.
//! Both are readable without credentials.
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

pub const CDN: &str = "https://cdn.wahooligan.com/wahoo-maps";
/// How many versions above the newest installed one are probed.
const PROBE_AHEAD: u32 = 25;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).http_status_as_error(false).build().into()
}

fn exists(a: &ureq::Agent, url: &str) -> bool {
    a.head(url).call().map(|r| r.status().as_u16() == 200).unwrap_or(false)
}

fn mappack(a: &ureq::Agent, version: u32) -> Option<Value> {
    let mut r = a.get(format!("{CDN}/{version}/mappack-gzip.json.gz")).call().ok()?;
    if r.status().as_u16() != 200 {
        return None;
    }
    let bytes = r.body_mut().with_config().limit(20_000_000).read_to_vec().ok()?;
    let mut s = String::new();
    flate2::read::GzDecoder::new(&bytes[..]).read_to_string(&mut s).ok()?;
    serde_json::from_str(&s).ok()
}

/// "2026-10-21T20:47:21" -> "2026-10-21"
fn iso_date(s: &str) -> Option<String> {
    let d = s.split(['T', ' ']).next()?;
    let ok = d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-';
    ok.then(|| d.to_string())
}

fn flatten(packs: &Value, prefix: &str, out: &mut BTreeMap<u64, (String, u64)>) {
    for p in packs.as_array().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or("?");
        let full = if prefix.is_empty() { name.to_string() } else { format!("{prefix} > {name}") };
        if let Some(id) = p["id"].as_u64() {
            out.insert(id, (full.clone(), p["size"].as_u64().unwrap_or(0)));
        }
        flatten(&p["sub_packs"], &full, out);
    }
}

/// `tilepacks` is the content of maps/tilepack-versions.json: `[{"<pack id>": <version>}, ...]`.
fn installed_packs(tilepacks: &Value) -> BTreeMap<u64, u32> {
    let mut installed = BTreeMap::new();
    for o in tilepacks.as_array().into_iter().flatten() {
        for (k, v) in o.as_object().into_iter().flatten() {
            if let (Ok(id), Some(ver)) = (k.parse::<u64>(), v.as_u64()) {
                installed.insert(id, ver as u32);
            }
        }
    }
    installed
}

/// Published map versions in `newest..=newest + PROBE_AHEAD`.
fn available_versions(a: &ureq::Agent, newest: u32) -> Vec<u32> {
    (newest..=newest + PROBE_AHEAD).filter(|v| exists(a, &format!("{CDN}/{v}/mappack-gzip.json.gz"))).collect()
}

/// Zoom-8 tiles `(x, y)` that belong to this pack itself (not its sub packs).
fn pack_tiles(p: &Value) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for ts in p["tile_sets"].as_array().into_iter().flatten() {
        if ts["zoom"].as_u64() != Some(8) {
            continue;
        }
        for c in ts["tile_coordinates"].as_array().into_iter().flatten() {
            if let (Some(x), Some(y)) = (c[0].as_u64(), c[1].as_u64()) {
                out.push((x as u32, y as u32));
            }
        }
    }
    out
}

fn find_pack(packs: &Value, id: u64) -> Option<&Value> {
    for p in packs.as_array().into_iter().flatten() {
        if p["id"].as_u64() == Some(id) {
            return Some(p);
        }
        if let Some(f) = find_pack(&p["sub_packs"], id) {
            return Some(f);
        }
    }
    None
}

fn pack_node(p: &Value, installed: &BTreeMap<u64, u32>, dates: &BTreeMap<u32, String>) -> Value {
    let children: Vec<Value> = p["sub_packs"].as_array().into_iter().flatten().map(|c| pack_node(c, installed, dates)).collect();
    let id = p["id"].as_u64();
    let inst = id.and_then(|i| installed.get(&i)).copied();
    let tiles = pack_tiles(p).len();
    let installed_count = children.iter().map(|c| c["installed_count"].as_u64().unwrap_or(0)).sum::<u64>() + inst.is_some() as u64;
    json!({
        "id": id,
        "name": p["name"],
        "size_bytes": p["size"],
        "tiles": tiles,
        "routing_bytes": p["routing_size"],
        "routing_tiles": p["tile_sets"].as_array().into_iter().flatten().map(|t| t["routing_coordinates"].as_array().map_or(0, |a| a.len())).sum::<usize>(),
        "hidden": p["hidden"],
        "downloadable": tiles > 0,
        "installed_version": inst,
        "installed_date": inst.and_then(|v| dates.get(&v)),
        "installed_count": installed_count,
        "children": children,
    })
}

/// Pack tree (continent > country > region) of the newest map version, marked with what is installed on the device.
pub fn packs(tilepacks: &Value) -> Value {
    let installed = installed_packs(tilepacks);
    let Some(&newest) = installed.values().max() else {
        return json!({"status": "unknown", "message": "The device has no tilepack-versions.json; nothing to list."});
    };
    let a = agent();
    let Some(latest) = available_versions(&a, newest).last().copied() else {
        return json!({"status": "error", "message": "Could not reach the Wahoo map server. Check your internet connection.", "installed_max": newest});
    };
    let Some(list) = mappack(&a, latest) else {
        return json!({"status": "error", "message": format!("Could not read the pack list of map version v{latest}.")});
    };
    let mut dates: BTreeMap<u32, String> = BTreeMap::new();
    for v in installed.values().copied().collect::<std::collections::BTreeSet<u32>>() {
        let d = if v == latest { list["created_date"].as_str().and_then(iso_date) } else { mappack(&a, v).and_then(|j| j["created_date"].as_str().and_then(iso_date)) };
        if let Some(d) = d {
            dates.insert(v, d);
        }
    }
    let tree: Vec<Value> = list["tile_packs"].as_array().into_iter().flatten().map(|p| pack_node(p, &installed, &dates)).collect();
    json!({
        "status": "ok",
        "latest_version": latest,
        "latest_date": list["created_date"].as_str().and_then(iso_date),
        "installed_total": installed.len(),
        "installed_max": newest,
        "download_dir": download_dir().join(latest.to_string()).display().to_string(),
        "packs": tree,
    })
}

/// Local folder for downloaded tiles: `%LOCALAPPDATA%\WahooDesktop\maps`.
pub fn download_dir() -> PathBuf {
    crate::app::cache_dir().parent().map(|p| p.to_path_buf()).unwrap_or_default().join("maps")
}

#[derive(Serialize, Clone, Default)]
pub struct DownloadState {
    pub running: bool,
    pub pack_id: u64,
    pub pack_name: String,
    pub version: u32,
    pub tiles_done: usize,
    pub tiles_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current: String,
    pub error: Option<String>,
    pub finished: bool,
    pub dir: String,
}

static DOWNLOAD: Mutex<Option<DownloadState>> = Mutex::new(None);

pub fn download_state() -> Value {
    json!(DOWNLOAD.lock().unwrap().clone().unwrap_or_default())
}

fn update(f: impl FnOnce(&mut DownloadState)) {
    if let Some(s) = DOWNLOAD.lock().unwrap().as_mut() {
        f(s);
    }
}

/// Downloads all tiles of one pack to `dest/<version>/tiles/8/<x>/<y>.map.lzma`. Tiles that already exist with the
/// same size are skipped. Reports progress through the shared download state.
fn download_tiles(a: &ureq::Agent, version: u32, tiles: &[(u32, u32)], dest: &std::path::Path) -> Result<(), String> {
    for (x, y) in tiles {
        update(|s| s.current = format!("{x}/{y}"));
        let path = dest.join(version.to_string()).join("tiles").join("8").join(x.to_string()).join(format!("{y}.map.lzma"));
        let mut r = a.get(format!("{CDN}/{version}/tiles/8/{x}/{y}.map.lzma")).call().map_err(|e| format!("tile {x}/{y}: {e}"))?;
        if r.status().as_u16() != 200 {
            return Err(format!("tile {x}/{y}: HTTP {}", r.status().as_u16()));
        }
        let size = r.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
        if size.is_some() && std::fs::metadata(&path).map(|m| m.len()).ok() == size {
            update(|s| {
                s.tiles_done += 1;
                s.bytes_done += size.unwrap_or(0);
            });
            continue;
        }
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let part = path.with_extension("lzma.part");
        let mut f = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut body = r.body_mut().with_config().limit(200_000_000).reader();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = body.read(&mut buf).map_err(|e| format!("tile {x}/{y}: {e}"))?;
            if n == 0 {
                break;
            }
            f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            update(|s| s.bytes_done += n as u64);
        }
        drop(f);
        std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
        update(|s| s.tiles_done += 1);
    }
    Ok(())
}

/// Resolves a pack by id, full name ("Europe > Netherlands") or plain name ("Netherlands").
fn resolve_pack(packs: &Value, query: &str) -> Result<u64, String> {
    let mut all = BTreeMap::new();
    flatten(packs, "", &mut all);
    let q = query.trim().to_lowercase();
    if let Some(id) = q.parse::<u64>().ok().filter(|id| all.contains_key(id)) {
        return Ok(id);
    }
    let hits: Vec<(&u64, &String)> = all.iter().filter(|(_, (full, _))| { let f = full.to_lowercase(); f == q || f.rsplit(" > ").next() == Some(q.as_str()) }).map(|(id, (full, _))| (id, full)).collect();
    match hits.as_slice() {
        [] => Err(format!("no map pack matches '{query}'; list them with `wahoo_cli map-packs --all`")),
        [(id, _)] => Ok(**id),
        many => Err(format!("'{query}' is ambiguous: {}; use the id or the full name", many.iter().map(|(id, n)| format!("{id} = {n}")).collect::<Vec<_>>().join(", "))),
    }
}

/// Starts a background download of one pack (id or name). `version` defaults to the newest map version.
pub fn start_download(pack: &str, version: Option<u32>, tilepacks: &Value) -> Result<(), String> {
    if DOWNLOAD.lock().unwrap().as_ref().is_some_and(|s| s.running) {
        return Err("a map download is already running".into());
    }
    let installed = installed_packs(tilepacks);
    let a = download_agent();
    let version = match version {
        Some(v) => v,
        None => {
            let newest = installed.values().max().copied().ok_or("the device has no tilepack-versions.json")?;
            available_versions(&a, newest).last().copied().ok_or("could not reach the Wahoo map server")?
        }
    };
    let list = mappack(&a, version).ok_or_else(|| format!("could not read the pack list of map version v{version}"))?;
    let pack_id = resolve_pack(&list["tile_packs"], pack)?;
    let pack = find_pack(&list["tile_packs"], pack_id).ok_or_else(|| format!("pack {pack_id} does not exist in v{version}"))?;
    let tiles = pack_tiles(pack);
    if tiles.is_empty() {
        return Err("this pack has no tiles of its own; pick a country or region".into());
    }
    let name = pack["name"].as_str().unwrap_or("?").to_string();
    let dest = download_dir();
    let total = pack["size"].as_u64().unwrap_or(0);
    *DOWNLOAD.lock().unwrap() = Some(DownloadState {
        running: true,
        pack_id,
        pack_name: name,
        version,
        tiles_total: tiles.len(),
        bytes_total: total,
        dir: dest.join(version.to_string()).display().to_string(),
        ..Default::default()
    });
    std::thread::spawn(move || {
        let r = download_tiles(&a, version, &tiles, &dest);
        update(|s| {
            s.running = false;
            s.finished = r.is_ok();
            s.current.clear();
            s.error = r.err();
        });
    });
    Ok(())
}

pub(crate) fn download_agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_connect(Some(Duration::from_secs(10))).timeout_global(Some(Duration::from_secs(900))).http_status_as_error(false).build().into()
}

/// `tilepacks` is the content of maps/tilepack-versions.json: `[{"<pack id>": <version>}, ...]`.
pub fn check(tilepacks: &Value) -> Value {
    let installed = installed_packs(tilepacks);
    let Some(&newest) = installed.values().max() else {
        return json!({"status": "unknown", "message": "The device has no tilepack-versions.json; nothing to compare."});
    };
    let a = agent();
    let available = available_versions(&a, newest);
    let Some(&latest) = available.last() else {
        return json!({"status": "error", "message": "Could not reach the Wahoo map server. Check your internet connection.", "installed_max": newest});
    };
    let mut versions: BTreeMap<u32, Value> = BTreeMap::new();
    let mut want: Vec<u32> = installed.values().copied().collect();
    want.push(latest);
    want.sort_unstable();
    want.dedup();
    for v in want {
        versions.insert(v, mappack(&a, v).unwrap_or(Value::Null));
    }
    let date_of = |v: u32| versions.get(&v).and_then(|j| j["created_date"].as_str()).and_then(iso_date);
    let mut names: BTreeMap<u64, (String, u64)> = BTreeMap::new();
    if let Some(j) = versions.get(&latest) {
        flatten(&j["tile_packs"], "", &mut names);
    }
    let mut rows = Vec::new();
    let mut outdated = 0;
    for (id, ver) in &installed {
        let mut old: BTreeMap<u64, (String, u64)> = BTreeMap::new();
        if let Some(j) = versions.get(ver) {
            flatten(&j["tile_packs"], "", &mut old);
        }
        let n = names.get(id);
        let behind = *ver < latest;
        if behind {
            outdated += 1;
        }
        rows.push(json!({
            "id": id,
            "name": n.map(|x| x.0.clone()).or_else(|| old.get(id).map(|x| x.0.clone())),
            "installed_version": ver,
            "installed_date": date_of(*ver),
            "latest_version": latest,
            "latest_date": date_of(latest),
            "installed_bytes": old.get(id).map(|x| x.1),
            "latest_bytes": n.map(|x| x.1),
            "status": if behind { "update_available" } else { "up_to_date" },
        }));
    }
    let vmeta: Map<String, Value> = versions.iter().map(|(v, j)| (v.to_string(), json!({"created": date_of(*v), "size_bytes": j["size"]}))).collect();
    let (status, message) = if outdated == 0 {
        ("up_to_date", format!("All {} tile packs are on the latest map version (v{latest}).", installed.len()))
    } else {
        ("update_available", format!("{outdated} of {} tile packs can be updated to map version v{latest}{}.", installed.len(), date_of(latest).map(|d| format!(" (published {d})")).unwrap_or_default()))
    };
    json!({
        "status": status,
        "message": message,
        "latest_version": latest,
        "latest_date": date_of(latest),
        "available_versions": available,
        "versions": vmeta,
        "packs": rows,
        "outdated_packs": outdated,
        "tile_url": format!("{CDN}/{latest}/tiles/8/<x>/<y>.map.lzma"),
    })
}

/// Compares one device tile with the same tile in `version` (HEAD request: size and last-modified).
pub fn tile(version: u32, x: u32, y: u32, device_bytes: Option<u64>) -> Value {
    let url = format!("{CDN}/{version}/tiles/8/{x}/{y}.map.lzma");
    let a = agent();
    let r = match a.head(&url).call() {
        Ok(r) => r,
        Err(e) => return json!({"url": url, "status": "error", "message": e.to_string()}),
    };
    if r.status().as_u16() != 200 {
        return json!({"url": url, "status": "missing", "message": format!("HTTP {} (this tile does not exist in v{version})", r.status().as_u16())});
    }
    let size = r.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let modified = r.headers().get("last-modified").and_then(|v| v.to_str().ok()).map(str::to_string);
    let same = device_bytes.zip(size).map(|(d, s)| d == s);
    json!({
        "url": url,
        "status": match same { Some(true) => "same_size", Some(false) => "different", None => "unknown" },
        "remote_bytes": size,
        "device_bytes": device_bytes,
        "remote_modified": modified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(iso_date("2026-10-21T20:47:21").as_deref(), Some("2026-10-21"));
        assert_eq!(iso_date("x"), None);
    }

    #[test]
    fn resolves_packs_by_id_and_name() {
        let j = json!([{"name": "Europe", "id": 1, "sub_packs": [{"name": "Netherlands", "id": 156}, {"name": "Georgia", "id": 7}]}, {"name": "US", "id": 2, "sub_packs": [{"name": "Georgia", "id": 8}]}]);
        assert_eq!(resolve_pack(&j, "156"), Ok(156));
        assert_eq!(resolve_pack(&j, "netherlands"), Ok(156));
        assert_eq!(resolve_pack(&j, "Europe > Georgia"), Ok(7));
        assert!(resolve_pack(&j, "Georgia").is_err());
        assert!(resolve_pack(&j, "Narnia").is_err());
    }

    #[test]
    fn flattens_nested_packs() {
        let j = json!([{"name": "Europe", "id": 1, "size": 5, "sub_packs": [{"name": "Netherlands", "id": 156, "size": 9}]}]);
        let mut m = BTreeMap::new();
        flatten(&j, "", &mut m);
        assert_eq!(m[&156].0, "Europe > Netherlands");
    }
}
