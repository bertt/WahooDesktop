//! Installs one zoom-8 map tile from Wahoo's CDN onto the device, with verification and automatic restore.
//!
//! Sequence (see docs/MAPS.md): download + validate locally, delete the old tile, write the new one, read it back
//! byte for byte, delete the stale `maps/temp` render copy, set the version marker. If anything fails after the old
//! tile was removed, the previously installed version is restored from the CDN (device tiles are byte-identical to
//! the CDN tiles of the same version, so no local backup is needed).
use crate::mapupdates::{CDN, download_dir};
use crate::mtp;
use serde_json::{Value, json};
use std::io::Read;

const KNOWN_VERSIONS: [u32; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 12, 17, 18];
const MAX_TILE: u64 = 200_000_000;

fn tile_path(x: u32, y: u32) -> String {
    format!("maps/tiles/8/{x}/{y}.map.lzma")
}

fn marker_versions(dev: &str, x: u32, y: u32, extra: &[u32]) -> Result<Vec<u32>, String> {
    let mut candidates: Vec<u32> = KNOWN_VERSIONS.iter().chain(&[20, 21, 22, 23, 24, 25]).chain(extra).copied().collect();
    candidates.sort_unstable();
    candidates.dedup();
    existing_markers(dev, x, y, &candidates)
}

fn existing_markers(dev: &str, x: u32, y: u32, candidates: &[u32]) -> Result<Vec<u32>, String> {
    let mut found = Vec::new();
    for v in candidates {
        if mtp::stat(dev, &format!("{}.{v}", tile_path(x, y)))?.is_some() {
            found.push(*v);
        }
    }
    Ok(found)
}

/// Deletes a tile with its version markers and `maps/temp` render copy. `marker_candidates` are the marker versions
/// to look for (the device is not walked); returns false when the tile was not on the device.
pub fn remove_tile(dev: &str, x: u32, y: u32, marker_candidates: &[u32]) -> Result<bool, String> {
    let tile = tile_path(x, y);
    let existed = mtp::delete(dev, &tile)?;
    for m in marker_candidates {
        mtp::delete(dev, &format!("{tile}.{m}"))?;
    }
    mtp::delete(dev, &format!("maps/temp/z8x{x}y{y}.map"))?;
    Ok(existed)
}

pub(crate) fn remote_size(a: &ureq::Agent, version: u32, x: u32, y: u32) -> Result<u64, String> {
    let url = format!("{CDN}/{version}/tiles/8/{x}/{y}.map.lzma");
    let mut last = String::new();
    let mut resp = None;
    for attempt in 0..4 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(500 * attempt));
        }
        match a.head(&url).call() {
            Ok(r) => {
                let s = r.status().as_u16();
                if s == 200 {
                    resp = Some(r);
                    break;
                }
                if s == 403 || s == 404 {
                    return Err(format!("{url}: HTTP {s} (tile does not exist in v{version})"));
                }
                last = format!("{url}: HTTP {s} (CDN temporarily unavailable)");
            }
            Err(e) => last = format!("{url}: {e}"),
        }
    }
    let r = resp.ok_or(last)?;
    r.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).ok_or_else(|| format!("{url}: no Content-Length"))
}

/// Downloads a tile (or reuses the local copy when its size matches) and validates size and LZMA header.
fn fetch(a: &ureq::Agent, version: u32, x: u32, y: u32) -> Result<Vec<u8>, String> {
    let expected = remote_size(a, version, x, y)?;
    let path = download_dir().join(version.to_string()).join("tiles").join("8").join(x.to_string()).join(format!("{y}.map.lzma"));
    let data = match std::fs::read(&path) {
        Ok(d) if d.len() as u64 == expected => d,
        _ => {
            let url = format!("{CDN}/{version}/tiles/8/{x}/{y}.map.lzma");
            let mut r = a.get(&url).call().map_err(|e| format!("{url}: {e}"))?;
            let mut d = Vec::with_capacity(expected as usize);
            r.body_mut().with_config().limit(MAX_TILE).reader().read_to_end(&mut d).map_err(|e| format!("{url}: {e}"))?;
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, &d).map_err(|e| e.to_string())?;
            d
        }
    };
    if data.len() as u64 != expected {
        return Err(format!("downloaded {} bytes, expected {expected}", data.len()));
    }
    if data.len() < 13 || data[0] != 0x5d || data[5..13] != [0xff; 8] {
        return Err("download is not an LZMA-alone stream (expected props 5d and unknown-size marker ff*8)".into());
    }
    Ok(data)
}

fn put_and_verify(dev: &str, x: u32, y: u32, data: &[u8]) -> Result<(), String> {
    let p = tile_path(x, y);
    mtp::write_new(dev, &p, data)?;
    let back = mtp::read_path(dev, &p)?;
    if back != data {
        return Err(format!("read-back mismatch ({} bytes on device, expected {})", back.len(), data.len()));
    }
    Ok(())
}

/// Only replaces the version marker of an existing tile (no tile data is transferred).
pub fn set_marker(x: u32, y: u32, marker: u32, apply: bool) -> Result<Value, String> {
    let det = mtp::detect();
    let dev = det.mtp_devices.first().ok_or(det.message)?.id.clone();
    let tile = tile_path(x, y);
    let device_size = mtp::stat(&dev, &tile)?.ok_or_else(|| format!("{tile} is not on the device"))?;
    let markers = marker_versions(&dev, x, y, &[marker])?;
    let mut out = json!({"tile": format!("{x}/{y}"), "device_bytes": device_size, "device_markers": markers, "target_marker": marker, "applied": false});
    if apply {
        mtp::write_new(&dev, &format!("{tile}.{marker}"), &[]).or_else(|e| if markers.contains(&marker) { Ok(()) } else { Err(e) })?;
        for m in markers.iter().filter(|m| **m != marker) {
            mtp::delete(&dev, &format!("{tile}.{m}"))?;
        }
        out["applied"] = json!(true);
    }
    Ok(out)
}

/// Plans (`apply == false`, nothing is written) or performs the installation of tile `x/y` in map `version`.
/// `marker` is the version number of the marker file to leave behind; default: keep the existing marker, else `version`.
pub fn install(x: u32, y: u32, version: u32, marker: Option<u32>, apply: bool) -> Result<Value, String> {
    let det = mtp::detect();
    let dev = det.mtp_devices.first().ok_or(det.message)?.id.clone();
    let a = crate::mapupdates::download_agent();
    install_with(&dev, &a, x, y, version, marker, apply, false, None)
}

/// Like [`install`] on a known device. `allow_new` also writes tiles that are not on the device yet (the tile folder
/// is created when needed). `candidates` limits which marker versions are looked up (default: all known versions).
#[allow(clippy::too_many_arguments)]
pub(crate) fn install_with(dev: &str, a: &ureq::Agent, x: u32, y: u32, version: u32, marker: Option<u32>, apply: bool, allow_new: bool, candidates: Option<&[u32]>) -> Result<Value, String> {
    let tile = tile_path(x, y);
    let device_size = mtp::stat(dev, &tile)?;
    let markers = match candidates {
        Some(c) => {
            let mut c = c.to_vec();
            c.push(version);
            c.sort_unstable();
            c.dedup();
            existing_markers(dev, x, y, &c)?
        }
        None => marker_versions(dev, x, y, &[version])?,
    };
    let target_marker = marker.or_else(|| markers.first().copied()).unwrap_or(version);
    let temp = format!("maps/temp/z8x{x}y{y}.map");
    let temp_size = mtp::stat(dev, &temp)?;
    let mut plan = json!({
        "tile": format!("{x}/{y}"), "version": version, "device_bytes": device_size, "device_markers": markers,
        "target_marker": target_marker, "temp_copy_bytes": temp_size, "applied": false,
    });
    if device_size.is_none() && !allow_new {
        return Err(format!("{tile} is not on the device; only existing tiles are replaced"));
    }
    if !apply {
        plan["cdn_bytes"] = json!(remote_size(a, version, x, y)?);
        return Ok(plan);
    }

    let data = fetch(a, version, x, y)?;
    plan["cdn_bytes"] = json!(data.len());
    if device_size.is_none() {
        mtp::make_dir(dev, &format!("maps/tiles/8/{x}"))?;
        if let Err(e) = put_and_verify(dev, x, y, &data) {
            let _ = mtp::delete(dev, &tile);
            return Err(format!("{e}; the partially written tile was removed"));
        }
    } else {
        let previous = markers.iter().max_by_key(|v| **v).copied().filter(|v| *v != version);
        // The tile is only deleted after the replacement has been downloaded and validated.
        mtp::delete(dev, &tile)?;
        if let Err(e) = put_and_verify(dev, x, y, &data) {
            let _ = mtp::delete(dev, &tile);
            let restore = previous.ok_or_else(|| format!("{e}; no previous version known, tile {x}/{y} is missing on the device"))?;
            let old = fetch(a, restore, x, y).map_err(|e2| format!("{e}; restore of v{restore} failed: {e2}; tile is missing on the device"))?;
            put_and_verify(dev, x, y, &old).map_err(|e2| format!("{e}; restore of v{restore} failed: {e2}; tile is missing or damaged on the device"))?;
            return Err(format!("{e}; the previous tile (v{restore}) was restored"));
        }
    }
    plan["temp_copy_deleted"] = json!(mtp::delete(dev, &temp)?);
    for m in &markers {
        mtp::delete(dev, &format!("{tile}.{m}"))?;
    }
    mtp::write_new(dev, &format!("{tile}.{target_marker}"), &[])?;
    plan["applied"] = json!(true);
    plan["verified_bytes"] = json!(data.len());
    Ok(plan)
}
