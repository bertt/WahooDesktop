//! Application state: cached device data, summary cache and the background sync job.
use crate::{fit, mtp};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

pub struct DeviceData {
    pub key: String,
    pub dir: PathBuf,
    pub manifest: mtp::Manifest,
    pub summaries: Vec<fit::Summary>,
    pub system_info: String,
    pub serial_txt: String,
    pub tilepacks: serde_json::Value,
    pub routing_config: serde_json::Value,
    pub parse_ms: u128,
    pub parse_cached: usize,
}

#[derive(Serialize, Clone, Default)]
pub struct SyncState {
    pub running: bool,
    pub phase: String,
    pub started: i64,
    pub finished: i64,
    pub error: Option<String>,
    pub log: Vec<String>,
}

pub struct App {
    pub cache: PathBuf,
    pub devices: RwLock<HashMap<String, Arc<DeviceData>>>,
    pub sync: Mutex<SyncState>,
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("WahooDesktop")
        .join("cache")
}

#[derive(Serialize, Deserialize, Default)]
struct SummaryCache {
    version: u32,
    items: HashMap<String, fit::Summary>,
}

const CACHE_VERSION: u32 = 2;

fn read_text(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default().trim().to_string()
}

fn load_summaries(dir: &Path) -> (Vec<fit::Summary>, usize, u128) {
    let t0 = std::time::Instant::now();
    let cache_path = dir.join("summaries.json");
    let cache: SummaryCache = std::fs::read(&cache_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .filter(|c: &SummaryCache| c.version == CACHE_VERSION)
        .unwrap_or_default();

    let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("files").join("exports"))
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x == "fit").unwrap_or(false)).collect())
        .unwrap_or_default();
    files.sort();

    let (mut known, mut todo) = (Vec::new(), Vec::new());
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let size = f.metadata().map(|m| m.len()).unwrap_or(0);
        match cache.items.get(&name) {
            Some(s) if s.bytes == size => known.push(s.clone()),
            _ => todo.push(f),
        }
    }
    let cached = known.len();
    let parsed = std::sync::Mutex::new(Vec::new());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    std::thread::scope(|sc| {
        for _ in 0..workers {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(p) = todo.get(i) else { break };
                if let Ok(d) = fit::parse_file(p, false) {
                    parsed.lock().unwrap().push(d.summary);
                }
            });
        }
    });
    let parsed = parsed.into_inner().unwrap();
    let wrote_new = !parsed.is_empty();
    known.extend(parsed);
    known.sort_by_key(|s| s.start);
    if wrote_new {
        let sc = SummaryCache { version: CACHE_VERSION, items: known.iter().map(|s| (s.file.clone(), s.clone())).collect() };
        let _ = std::fs::write(&cache_path, serde_json::to_vec(&sc).unwrap_or_default());
    }
    (known, cached, t0.elapsed().as_millis())
}

pub fn load_device(dir: &Path) -> Option<DeviceData> {
    let manifest: mtp::Manifest = serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).ok()?).ok()?;
    let files = dir.join("files");
    let json = |p: PathBuf| serde_json::from_slice(&std::fs::read(p).unwrap_or_default()).unwrap_or(serde_json::Value::Null);
    let (summaries, parse_cached, parse_ms) = load_summaries(dir);
    Some(DeviceData {
        key: dir.file_name()?.to_string_lossy().to_string(),
        dir: dir.to_path_buf(),
        manifest,
        summaries,
        system_info: read_text(&files.join("factory").join("system_info.txt")),
        serial_txt: read_text(&files.join("factory").join("serial_number.txt")),
        tilepacks: json(files.join("maps").join("tilepack-versions.json")),
        routing_config: json(files.join("maps").join("routing").join("routing-config.json")),
        parse_ms,
        parse_cached,
    })
}

impl App {
    pub fn new() -> Arc<App> {
        let app = Arc::new(App { cache: cache_dir(), devices: RwLock::new(HashMap::new()), sync: Mutex::new(SyncState::default()) });
        app.reload_all();
        app
    }

    pub fn reload_all(&self) {
        let mut map = HashMap::new();
        if let Ok(rd) = std::fs::read_dir(&self.cache) {
            for e in rd.flatten() {
                if e.path().join("manifest.json").exists() {
                    if let Some(d) = load_device(&e.path()) {
                        map.insert(d.key.clone(), Arc::new(d));
                    }
                }
            }
        }
        *self.devices.write().unwrap() = map;
    }

    /// Converts (GPX) or validates (FIT) a route file, copies it to `routes/` on the Wahoo and re-syncs.
    pub fn add_route(self: &Arc<Self>, device_key: Option<&str>, orig_name: &str, data: &[u8]) -> Result<(String, Option<crate::course::Course>), String> {
        let (stem, ext) = orig_name.rsplit_once('.').map(|(s, e)| (s.to_string(), e.to_lowercase())).unwrap_or((orig_name.to_string(), String::new()));
        let stem: String = stem.chars().map(|c| if c.is_alphanumeric() || " -_".contains(c) { c } else { '_' }).collect();
        let stem = if stem.trim().is_empty() { "route".to_string() } else { stem.trim().to_string() };
        let (bytes, info) = match ext.as_str() {
            "gpx" => {
                let (b, c) = crate::course::gpx_to_fit(&String::from_utf8_lossy(data), &stem)?;
                (b, Some(c))
            }
            "fit" => {
                if data.len() < 14 || &data[8..12] != b".FIT" {
                    return Err("Not a valid FIT file".into());
                }
                (data.to_vec(), None)
            }
            _ => return Err("Only .fit and .gpx files are supported".into()),
        };
        let det = mtp::detect();
        let dev = det
            .mtp_devices
            .iter()
            .find(|m| device_key.is_none_or(|k| mtp::sanitize(&m.name) == k))
            .ok_or_else(|| "Device is not connected. Connect it over USB and make sure it is switched on.".to_string())?;
        let name = mtp::upload(&dev.id, "routes", &format!("{stem}.fit"), &bytes)?;
        self.run_sync();
        Ok((name, info))
    }

    pub fn log(&self, phase: &str, line: impl Into<String>) {
        let mut s = self.sync.lock().unwrap();
        s.phase = phase.into();
        s.log.push(line.into());
    }

    /// Runs a full sync for the first reachable Wahoo. Blocking; call from spawn_blocking.
    pub fn run_sync(self: &Arc<Self>) {
        {
            let mut s = self.sync.lock().unwrap();
            if s.running {
                return;
            }
            *s = SyncState { running: true, phase: "detect".into(), started: chrono::Utc::now().timestamp(), ..Default::default() };
        }
        let result = (|| -> Result<String, String> {
            let det = mtp::detect();
            let dev = det.mtp_devices.first().ok_or(det.message)?.clone();
            self.log("read", format!("Reading {} over USB (MTP)…", dev.name));
            let (m, dir) = mtp::sync(&self.cache, &dev.id, &dev.name)?;
            self.log("read", format!("Tree walk: {} objects in {} ms, {} new files copied", m.entries.len(), m.walk_ms, m.copied.len()));
            self.log("parse", "Parsing FIT activities…");
            let d = load_device(&dir).ok_or("Failed to load cached data")?;
            self.log("parse", format!("{} activities ({} from cache) in {} ms", d.summaries.len(), d.parse_cached, d.parse_ms));
            let key = d.key.clone();
            self.devices.write().unwrap().insert(key.clone(), Arc::new(d));
            Ok(key)
        })();
        let mut s = self.sync.lock().unwrap();
        s.running = false;
        s.finished = chrono::Utc::now().timestamp();
        match result {
            Ok(_) => s.phase = "done".into(),
            Err(e) => {
                s.phase = "error".into();
                s.error = Some(e);
            }
        }
    }
}
