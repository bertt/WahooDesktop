//! Firmware update check against Wahoo's public version manifest.
use serde_json::{Value, json};
use std::time::Duration;

const BASE: &str = "https://bolt.wahoofitness.com/boltapp/version.json-";

/// Manifest suffix for a device descriptor such as "ELEMNT BOLT F9F1".
fn channel(descriptor: &str) -> &'static str {
    let d = descriptor.to_lowercase();
    if d.contains("ace") {
        "ace"
    } else if d.contains("roam") {
        if d.contains("roam 2") || d.contains("roam2") || d.contains("v2") { "roam2" } else { "roam" }
    } else {
        "bolt2"
    }
}

/// "180.01" -> 18001
fn parse_installed(fw: &str) -> Option<i64> {
    let digits: String = fw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || fw.chars().any(|c| !(c.is_ascii_digit() || c == '.')) {
        return None;
    }
    digits.parse().ok()
}

/// 18001 -> "180.01"
fn pretty(v: i64) -> String {
    format!("{}.{:02}", v / 100, v % 100)
}

fn fetch(channel: &str) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(8))).build().into();
    let mut resp = agent.get(format!("{BASE}{channel}")).call().map_err(|e| e.to_string())?;
    resp.body_mut().read_json::<Value>().map_err(|e| e.to_string())
}

pub fn check(descriptor: &str, installed: &str) -> Value {
    let ch = channel(descriptor);
    let url = format!("{BASE}{ch}");
    let mut out = json!({"installed": installed, "channel": ch, "manifest_url": url});
    let m = match fetch(ch) {
        Ok(m) => m,
        Err(e) => {
            out["status"] = json!("error");
            out["message"] = json!(format!("Could not reach the Wahoo firmware server ({e}). Check your internet connection."));
            return out;
        }
    };
    let latest = m["std-version"].as_i64();
    let beta = m["beta-version"].as_i64().filter(|b| Some(*b) != latest);
    out["latest"] = latest.map(pretty).into();
    out["latest_raw"] = latest.into();
    out["latest_url"] = m["std-url"].clone();
    out["beta"] = beta.map(pretty).into();
    let (Some(inst), Some(lat)) = (parse_installed(installed), latest) else {
        out["status"] = json!("unknown");
        out["message"] = json!("Could not compare the installed version with the latest one.");
        return out;
    };
    // The ACE uses a different version scheme than the BOLT/ROAM family.
    if ch == "ace" {
        out["status"] = json!("unknown");
        out["message"] = json!(format!("Latest published ACE build is {lat}; its version format cannot be compared with the installed {installed}."));
        return out;
    }
    let (status, msg) = match inst.cmp(&lat) {
        std::cmp::Ordering::Less => ("update_available", format!("Update available: {} (installed {installed}).", pretty(lat))),
        std::cmp::Ordering::Equal => ("up_to_date", format!("You are on the latest firmware ({installed}).")),
        std::cmp::Ordering::Greater => ("newer", format!("Installed {installed} is newer than the latest public release ({}); probably a beta.", pretty(lat))),
    };
    out["status"] = json!(status);
    out["message"] = json!(msg);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_installed("180.01"), Some(18001));
        assert_eq!(parse_installed("abc"), None);
        assert_eq!(pretty(18001), "180.01");
        assert_eq!(channel("ELEMNT BOLT F9F1"), "bolt2");
        assert_eq!(channel("ELEMNT ROAM 2"), "roam2");
        assert_eq!(channel("ELEMNT ACE"), "ace");
    }
}
