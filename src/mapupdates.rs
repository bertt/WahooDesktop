//! Map update check against Wahoo's public map CDN.
//!
//! The ELEMNT app downloads `<CDN>/<version>/mappack-gzip.json.gz` (the list of tile packs of that
//! map version) and the device downloads tiles from `<CDN>/<version>/tiles/8/<x>/<y>.map.lzma`.
//! Both are readable without credentials.
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::Read;
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
pub fn check(tilepacks: &Value) -> Value {
    let mut installed: BTreeMap<u64, u32> = BTreeMap::new();
    for o in tilepacks.as_array().into_iter().flatten() {
        for (k, v) in o.as_object().into_iter().flatten() {
            if let (Ok(id), Some(ver)) = (k.parse::<u64>(), v.as_u64()) {
                installed.insert(id, ver as u32);
            }
        }
    }
    let Some(&newest) = installed.values().max() else {
        return json!({"status": "unknown", "message": "The device has no tilepack-versions.json; nothing to compare."});
    };
    let a = agent();
    let mut available = Vec::new();
    for v in newest..=newest + PROBE_AHEAD {
        if exists(&a, &format!("{CDN}/{v}/mappack-gzip.json.gz")) {
            available.push(v);
        }
    }
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
    fn flattens_nested_packs() {
        let j = json!([{"name": "Europe", "id": 1, "size": 5, "sub_packs": [{"name": "Netherlands", "id": 156, "size": 9}]}]);
        let mut m = BTreeMap::new();
        flatten(&j, "", &mut m);
        assert_eq!(m[&156].0, "Europe > Netherlands");
    }
}
