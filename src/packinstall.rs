//! Pack-level map management: install a map pack in a chosen version (new pack or switch version) and remove a pack.
//!
//! A plan is computed first (nothing is written): which tiles are written, replaced, skipped or removed, how much
//! has to be downloaded and whether it fits. The job then works tile by tile (see `tileinstall`), stops at the first
//! error, and only after every tile succeeded it updates `maps/tilepack-versions.json` (with a local backup).
//! Tiles that also belong to another installed pack are never removed.
use crate::mapupdates::{self, download_agent, download_dir, find_pack, installed_packs, mappack_cached, overlay_markers, pack_tiles, resolve_pack};
use crate::mtp::{self, Manifest};
use crate::tileinstall;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

type Tile = (u32, u32);

const TILEPACKS: &str = "maps/tilepack-versions.json";
/// Free space that must remain after the install.
const SPACE_MARGIN: u64 = 100_000_000;
/// Rough USB + CDN throughput and per-tile overhead, only used for the estimate shown before starting.
const EST_BYTES_PER_SEC: f64 = 2_000_000.0;
const EST_SECS_PER_TILE: f64 = 4.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Version(u32),
    Remove,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Tile is not on the device yet.
    Write,
    /// Tile is on the device in another version (or differs in size).
    Replace,
    /// Tile is already the requested version.
    Skip,
    Remove,
}

#[derive(Clone, Debug, Serialize)]
pub struct Step {
    pub x: u32,
    pub y: u32,
    pub op: Op,
    /// Size of the CDN tile (write/replace/skip) or of the device tile (remove).
    pub bytes: u64,
    pub device_bytes: u64,
}

/// What the cached device state says (from the last sync).
pub struct Inputs {
    pub tilepacks: Value,
    pub markers: BTreeMap<Tile, u32>,
    pub sizes: BTreeMap<Tile, u64>,
    pub free: u64,
}

impl Inputs {
    pub fn new(tilepacks: &Value, manifest: &Manifest) -> Self {
        let mut sizes = BTreeMap::new();
        for e in manifest.entries.iter().filter(|e| !e.dir) {
            let Some(rest) = e.path.strip_prefix("maps/tiles/8/") else { continue };
            if let Some((x, file)) = rest.split_once('/') {
                if let Some(y) = file.strip_suffix(".map.lzma") {
                    if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                        sizes.insert((x, y), e.size);
                    }
                }
            }
        }
        Inputs { tilepacks: tilepacks.clone(), markers: mapupdates::tile_markers(manifest), sizes, free: manifest.storages.first().map_or(0, |s| s.free) }
    }
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub pack_id: u64,
    pub pack_name: String,
    pub target: Target,
    pub from_version: Option<u32>,
    pub steps: Vec<Step>,
    pub kept_shared: Vec<Tile>,
    pub warnings: Vec<String>,
    pub free: u64,
}

impl Plan {
    fn count(&self, op: Op) -> usize {
        self.steps.iter().filter(|s| s.op == op).count()
    }

    fn download_bytes(&self) -> u64 {
        self.steps.iter().filter(|s| matches!(s.op, Op::Write | Op::Replace)).map(|s| s.bytes).sum()
    }

    /// Bytes the device grows by (negative when it shrinks).
    fn net_bytes(&self) -> i64 {
        self.steps
            .iter()
            .map(|s| match s.op {
                Op::Write => s.bytes as i64,
                Op::Replace => s.bytes as i64 - s.device_bytes as i64,
                Op::Remove => -(s.device_bytes as i64),
                Op::Skip => 0,
            })
            .sum()
    }

    fn enough_space(&self) -> bool {
        self.net_bytes() <= 0 || self.free == 0 || self.net_bytes() as u64 + SPACE_MARGIN <= self.free
    }

    fn work(&self) -> usize {
        self.steps.iter().filter(|s| s.op != Op::Skip).count()
    }

    pub fn estimated_seconds(&self) -> u64 {
        (self.download_bytes() as f64 / EST_BYTES_PER_SEC + self.work() as f64 * EST_SECS_PER_TILE) as u64
    }

    pub fn action(&self) -> &'static str {
        match (self.target, self.from_version) {
            (Target::Remove, _) => "remove",
            (Target::Version(_), None) => "install",
            (Target::Version(_), Some(_)) => "switch",
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "pack_id": self.pack_id,
            "pack_name": self.pack_name,
            "action": self.action(),
            "from_version": self.from_version,
            "to_version": match self.target { Target::Version(v) => json!(v), Target::Remove => Value::Null },
            "counts": {"write": self.count(Op::Write), "replace": self.count(Op::Replace), "skip": self.count(Op::Skip), "remove": self.count(Op::Remove), "kept_shared": self.kept_shared.len()},
            "download_bytes": self.download_bytes(),
            "net_bytes": self.net_bytes(),
            "free_bytes": self.free,
            "enough_space": self.enough_space(),
            "estimated_seconds": self.estimated_seconds(),
            "nothing_to_do": self.work() == 0 && self.from_version == match self.target { Target::Version(v) => Some(v), Target::Remove => None },
            "warnings": self.warnings,
            "tiles": self.steps,
        })
    }
}

/// Decides per tile what to do. `old` are the tiles of the currently installed version of the pack, `new` those of
/// the target version (empty when removing); `shared` are tiles of other installed packs, which are never removed.
fn plan_steps(target: Target, new: &[Tile], old: &[Tile], markers: &BTreeMap<Tile, u32>, sizes: &BTreeMap<Tile, u64>, cdn: &BTreeMap<Tile, u64>, shared: &BTreeSet<Tile>) -> (Vec<Step>, Vec<Tile>) {
    let mut steps = Vec::new();
    let mut kept = Vec::new();
    if let Target::Version(v) = target {
        for t in new {
            let device = sizes.get(t).copied();
            let remote = cdn.get(t).copied().unwrap_or(0);
            let op = match device {
                None => Op::Write,
                Some(d) if markers.get(t) == Some(&v) && Some(d) == cdn.get(t).copied() => Op::Skip,
                Some(_) => Op::Replace,
            };
            steps.push(Step { x: t.0, y: t.1, op, bytes: remote, device_bytes: device.unwrap_or(0) });
        }
    }
    let new: BTreeSet<Tile> = new.iter().copied().collect();
    for t in old.iter().filter(|t| !new.contains(*t)) {
        let Some(d) = sizes.get(t).copied() else { continue };
        if shared.contains(t) {
            kept.push(*t);
        } else {
            steps.push(Step { x: t.0, y: t.1, op: Op::Remove, bytes: d, device_bytes: d });
        }
    }
    (steps, kept)
}

/// Tiles owned by installed packs other than `exclude`, as tile -> pack ids.
fn other_owners(list: &Value, installed: &BTreeMap<u64, u32>, exclude: u64) -> BTreeMap<Tile, Vec<u64>> {
    let mut out: BTreeMap<Tile, Vec<u64>> = BTreeMap::new();
    for id in installed.keys().filter(|id| **id != exclude) {
        if let Some(p) = find_pack(&list["tile_packs"], *id) {
            for t in pack_tiles(p) {
                out.entry(t).or_default().push(*id);
            }
        }
    }
    out
}

/// `tilepack-versions.json` with `id` set to `version` (added when missing) or removed (`None`).
pub(crate) fn patch_tilepacks(cur: &Value, id: u64, version: Option<u32>) -> Value {
    let key = id.to_string();
    let mut items: Vec<Value> = cur.as_array().cloned().unwrap_or_default();
    let mut found = false;
    for o in items.iter_mut() {
        if let Some(m) = o.as_object_mut() {
            if m.contains_key(&key) {
                found = true;
                match version {
                    Some(v) => {
                        m.insert(key.clone(), json!(v));
                    }
                    None => {
                        m.remove(&key);
                    }
                }
            }
        }
    }
    if !found {
        if let Some(v) = version {
            items.push(json!({ key: v }));
        }
    }
    items.retain(|o| o.as_object().is_none_or(|m| !m.is_empty()));
    Value::Array(items)
}

/// Computes what installing `pack` in `target` (or removing it) would do. Needs internet for the CDN pack lists.
pub fn plan(inp: &Inputs, pack: &str, target: Target) -> Result<Plan, String> {
    let a = mapupdates::quick_agent();
    let installed = installed_packs(&inp.tilepacks);
    let newest = installed.values().max().copied().ok_or("the device has no tilepack-versions.json; sync first")?;
    let list_version = match target {
        Target::Version(v) => v,
        Target::Remove => newest,
    };
    let list = mappack_cached(&a, list_version).ok_or_else(|| format!("could not read the pack list of map version v{list_version} (offline, or the version does not exist)"))?;
    let pack_id = match pack.trim().parse::<u64>() {
        Ok(id) if installed.contains_key(&id) || find_pack(&list["tile_packs"], id).is_some() => id,
        _ => resolve_pack(&list["tile_packs"], pack)?,
    };
    let mut effective = installed.clone();
    overlay_markers(&mut effective, &list, &inp.markers);
    let from_version = effective.get(&pack_id).copied();
    let node = find_pack(&list["tile_packs"], pack_id);
    let pack_name = node.and_then(|p| p["name"].as_str()).map(String::from).unwrap_or_else(|| format!("pack {pack_id}"));

    let new_tiles: Vec<Tile> = match target {
        Target::Version(v) => {
            let n = node.ok_or_else(|| format!("pack {pack_id} does not exist in map version v{v}"))?;
            let t = pack_tiles(n);
            if t.is_empty() {
                return Err("this pack has no tiles of its own; pick a country or region".into());
            }
            t
        }
        Target::Remove => Vec::new(),
    };
    let mut old_tiles: Vec<Tile> = Vec::new();
    if let Some(fv) = from_version {
        if let Some(l) = mappack_cached(&a, fv) {
            old_tiles = find_pack(&l["tile_packs"], pack_id).map(pack_tiles).unwrap_or_default();
        }
    }
    if target == Target::Remove {
        old_tiles.extend(node.map(pack_tiles).unwrap_or_default());
        old_tiles.sort_unstable();
        old_tiles.dedup();
        if from_version.is_none() && !installed.contains_key(&pack_id) {
            return Err(format!("{pack_name} is not installed on the device"));
        }
    }

    let owners = other_owners(&list, &installed, pack_id);
    let shared: BTreeSet<Tile> = owners.keys().copied().collect();
    let mut cdn = BTreeMap::new();
    if let Target::Version(v) = target {
        let results = Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for chunk in new_tiles.chunks(new_tiles.len().div_ceil(6).max(1)) {
                let (results, a) = (&results, &a);
                s.spawn(move || {
                    for t in chunk {
                        results.lock().unwrap().push((*t, tileinstall::remote_size(a, v, t.0, t.1)));
                    }
                });
            }
        });
        for (t, r) in results.into_inner().unwrap() {
            cdn.insert(t, r.map_err(|e| format!("tile {}/{}: {e}", t.0, t.1))?);
        }
    }

    let (steps, kept_shared) = plan_steps(target, &new_tiles, &old_tiles, &inp.markers, &inp.sizes, &cdn, &shared);
    let mut warnings = Vec::new();
    if let (Target::Version(v), Some(fv)) = (target, from_version) {
        if v < fv {
            warnings.push(format!("v{v} is older than the installed v{fv}; the ELEMNT firmware and routing data may expect a newer map."));
        }
    }
    if let Target::Version(v) = target {
        let mut conflicts: BTreeSet<String> = BTreeSet::new();
        for id in new_tiles.iter().filter_map(|t| owners.get(t)).flatten() {
            if effective.get(id).is_some_and(|ev| *ev != v) {
                let n = find_pack(&list["tile_packs"], *id).and_then(|p| p["name"].as_str()).map(String::from).unwrap_or_else(|| format!("pack {id}"));
                conflicts.insert(format!("{n} (v{})", effective[id]));
            }
        }
        if !conflicts.is_empty() {
            warnings.push(format!("Some tiles are shared with other installed packs: {}. Those tiles move to v{v} for those packs too.", conflicts.into_iter().collect::<Vec<_>>().join(", ")));
        }
    }
    if !kept_shared.is_empty() {
        warnings.push(format!("{} tile(s) are also used by other installed packs and are kept on the device.", kept_shared.len()));
    }
    let plan = Plan { pack_id, pack_name, target, from_version, steps, kept_shared, warnings, free: inp.free };
    let mut plan = plan;
    if !plan.enough_space() {
        let msg = format!("Not enough free space on the device: needs {} MB more plus a {} MB margin, {} MB free (as of the last sync).", plan.net_bytes() / 1_000_000, SPACE_MARGIN / 1_000_000, plan.free / 1_000_000);
        plan.warnings.push(msg);
    }
    Ok(plan)
}

#[derive(Serialize, Clone, Default)]
pub struct InstallState {
    pub running: bool,
    pub finished: bool,
    pub cancelled: bool,
    pub action: String,
    pub pack_id: u64,
    pub pack_name: String,
    pub version: Option<u32>,
    pub tiles_total: usize,
    pub tiles_done: usize,
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub current: String,
    pub written: usize,
    pub replaced: usize,
    pub removed: usize,
    pub skipped: usize,
    pub error: Option<String>,
    pub failed_tile: Option<String>,
    pub tilepacks_updated: bool,
    pub backup: Option<String>,
    pub started: i64,
    pub finished_at: i64,
}

static STATE: Mutex<Option<InstallState>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn state() -> Value {
    json!(STATE.lock().unwrap().clone().unwrap_or_default())
}

/// True while a pack job is running; a sync must not run at the same time.
pub fn busy() -> bool {
    STATE.lock().unwrap().as_ref().is_some_and(|s| s.running)
}

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

fn update(f: impl FnOnce(&mut InstallState)) {
    if let Some(s) = STATE.lock().unwrap().as_mut() {
        f(s);
    }
}

/// Starts the job in the background. Fails when a job or a sync is running, the plan is invalid or does not fit.
pub fn start(inp: &Inputs, pack: &str, target: Target, sync_running: bool) -> Result<(), String> {
    if sync_running {
        return Err("a sync is running; wait until it has finished".into());
    }
    if busy() {
        return Err("a map install is already running".into());
    }
    let plan = plan(inp, pack, target)?;
    if !plan.enough_space() {
        return Err(plan.warnings.last().cloned().unwrap_or_default());
    }
    let det = mtp::detect();
    let dev = det.mtp_devices.first().ok_or(det.message)?.id.clone();
    {
        let mut st = STATE.lock().unwrap();
        if st.as_ref().is_some_and(|s| s.running) {
            return Err("a map install is already running".into());
        }
        CANCEL.store(false, Ordering::SeqCst);
        *st = Some(InstallState {
            running: true,
            action: plan.action().into(),
            pack_id: plan.pack_id,
            pack_name: plan.pack_name.clone(),
            version: match target { Target::Version(v) => Some(v), Target::Remove => None },
            tiles_total: plan.steps.len(),
            bytes_total: plan.steps.iter().filter(|s| s.op != Op::Skip).map(|s| s.bytes).sum(),
            started: chrono::Utc::now().timestamp(),
            ..Default::default()
        });
    }
    let markers = inp.markers.clone();
    std::thread::spawn(move || {
        let r = run(&plan, &dev, &markers);
        update(|s| {
            s.running = false;
            s.current.clear();
            s.finished_at = chrono::Utc::now().timestamp();
            s.cancelled = CANCEL.load(Ordering::SeqCst) && r.is_err();
            s.finished = r.is_ok();
            s.error = r.err();
        });
    });
    Ok(())
}

/// Deletes the leftover render copies in `maps/temp` (the device re-extracts a tile when it needs one).
/// Returns (files deleted, bytes freed). Stops at the first error.
pub fn clean_temp(manifest: &Manifest, sync_running: bool) -> Result<(usize, u64), String> {
    if sync_running {
        return Err("a sync is running; wait until it has finished".into());
    }
    if busy() {
        return Err("a map install is running; wait until it has finished".into());
    }
    let files: Vec<(&str, u64)> = manifest.entries.iter().filter(|e| !e.dir && e.path.starts_with("maps/temp/")).map(|e| (e.path.as_str(), e.size)).collect();
    let det = mtp::detect();
    let dev = det.mtp_devices.first().ok_or(det.message)?.id.clone();
    let (mut n, mut bytes) = (0, 0);
    for (p, size) in files {
        mtp::delete(&dev, p).map_err(|e| format!("{p}: {e} ({n} file(s) were already deleted)"))?;
        n += 1;
        bytes += size;
    }
    Ok((n, bytes))
}

fn run(plan: &Plan, dev: &str, markers: &BTreeMap<Tile, u32>) -> Result<(), String> {
    let a = download_agent();
    // Installs first, removals last: a failure never leaves less data on the device than necessary.
    let mut steps: Vec<&Step> = plan.steps.iter().collect();
    steps.sort_by_key(|s| s.op == Op::Remove);
    for s in steps {
        if CANCEL.load(Ordering::SeqCst) {
            return Err(format!("cancelled; {} of {} tiles were processed, tilepack-versions.json was not changed", STATE.lock().unwrap().as_ref().map_or(0, |s| s.tiles_done), plan.steps.len()));
        }
        let tile = (s.x, s.y);
        update(|st| st.current = format!("{}/{}", s.x, s.y));
        let mut candidates: Vec<u32> = markers.get(&tile).copied().into_iter().chain(plan.from_version).collect();
        candidates.sort_unstable();
        candidates.dedup();
        let result = match (s.op, plan.target) {
            (Op::Skip, _) => Ok(()),
            (Op::Write | Op::Replace, Target::Version(v)) => tileinstall::install_with(dev, &a, s.x, s.y, v, Some(v), true, true, Some(&candidates)).map(|_| ()),
            (Op::Remove, _) => tileinstall::remove_tile(dev, s.x, s.y, &candidates).map(|_| ()),
            _ => Err("invalid plan".into()),
        };
        if let Err(e) = result {
            update(|st| st.failed_tile = Some(format!("{}/{}", s.x, s.y)));
            return Err(format!("tile {}/{}: {e}. {} tile(s) were already processed; tilepack-versions.json was not changed.", s.x, s.y, STATE.lock().unwrap().as_ref().map_or(0, |s| s.tiles_done)));
        }
        update(|st| {
            st.tiles_done += 1;
            st.bytes_done += if s.op == Op::Skip { 0 } else { s.bytes };
            match s.op {
                Op::Write => st.written += 1,
                Op::Replace => st.replaced += 1,
                Op::Remove => st.removed += 1,
                Op::Skip => st.skipped += 1,
            }
        });
    }
    update(|st| st.current = TILEPACKS.into());
    let version = match plan.target {
        Target::Version(v) => Some(v),
        Target::Remove => None,
    };
    let backup = update_tilepacks(dev, plan.pack_id, version).map_err(|e| format!("all tiles were processed but {TILEPACKS} could not be updated: {e}"))?;
    update(|st| {
        st.tilepacks_updated = true;
        st.backup = Some(backup);
    });
    Ok(())
}

/// Reads `tilepack-versions.json` from the device, saves it locally, writes the patched version and verifies it.
/// On failure the original is put back. Returns the backup path.
fn update_tilepacks(dev: &str, id: u64, version: Option<u32>) -> Result<String, String> {
    let original = mtp::read_path(dev, TILEPACKS)?;
    let cur: Value = serde_json::from_slice(&original).map_err(|e| format!("cannot parse the file on the device: {e}"))?;
    let dir = download_dir().join("backup");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let backup = dir.join(format!("tilepack-versions.{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::write(&backup, &original).map_err(|e| format!("cannot write the backup: {e}"))?;
    let new = serde_json::to_vec(&patch_tilepacks(&cur, id, version)).map_err(|e| e.to_string())?;
    let write = |data: &[u8]| -> Result<(), String> {
        mtp::delete(dev, TILEPACKS)?;
        mtp::write_new(dev, TILEPACKS, data)?;
        let back = mtp::read_path(dev, TILEPACKS)?;
        if back != data { Err("read-back mismatch".into()) } else { Ok(()) }
    };
    if let Err(e) = write(&new) {
        return Err(match write(&original) {
            Ok(()) => format!("{e}; the original file was restored"),
            Err(e2) => format!("{e}; restoring the original failed too ({e2}); a backup is at {}", backup.display()),
        });
    }
    Ok(backup.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map<T: Copy>(items: &[(Tile, T)]) -> BTreeMap<Tile, T> {
        items.iter().copied().collect()
    }

    #[test]
    fn install_classifies_tiles() {
        let new = [(1, 1), (1, 2), (1, 3)];
        let markers = map(&[((1, 1), 23), ((1, 2), 18)]);
        let sizes = map(&[((1, 1), 100), ((1, 2), 80)]);
        let cdn = map(&[((1, 1), 100), ((1, 2), 90), ((1, 3), 50)]);
        let (steps, kept) = plan_steps(Target::Version(23), &new, &[], &markers, &sizes, &cdn, &BTreeSet::new());
        let ops: Vec<Op> = steps.iter().map(|s| s.op).collect();
        assert_eq!(ops, [Op::Skip, Op::Replace, Op::Write]);
        assert!(kept.is_empty());
    }

    #[test]
    fn same_marker_but_other_size_is_replaced() {
        let (steps, _) = plan_steps(Target::Version(23), &[(1, 1)], &[], &map(&[((1, 1), 23)]), &map(&[((1, 1), 100)]), &map(&[((1, 1), 101)]), &BTreeSet::new());
        assert_eq!(steps[0].op, Op::Replace);
    }

    #[test]
    fn switching_removes_tiles_that_left_the_pack_unless_shared() {
        let old = [(1, 1), (2, 2), (3, 3)];
        let sizes = map(&[((1, 1), 10), ((2, 2), 20)]);
        let shared: BTreeSet<Tile> = [(2, 2)].into();
        let cdn = map(&[((1, 1), 10)]);
        let (steps, kept) = plan_steps(Target::Version(23), &[(1, 1)], &old, &BTreeMap::new(), &sizes, &cdn, &shared);
        assert_eq!(steps.iter().filter(|s| s.op == Op::Remove).count(), 0);
        assert_eq!(kept, [(2, 2)]);
        assert_eq!(steps.len(), 1);
    }

    #[test]
    fn remove_skips_missing_and_shared_tiles() {
        let sizes = map(&[((1, 1), 10), ((2, 2), 20)]);
        let shared: BTreeSet<Tile> = [(2, 2)].into();
        let (steps, kept) = plan_steps(Target::Remove, &[], &[(1, 1), (2, 2), (9, 9)], &BTreeMap::new(), &sizes, &BTreeMap::new(), &shared);
        assert_eq!(steps.len(), 1);
        assert_eq!((steps[0].x, steps[0].op), (1, Op::Remove));
        assert_eq!(kept, [(2, 2)]);
    }

    #[test]
    fn space_and_totals() {
        let steps = vec![
            Step { x: 1, y: 1, op: Op::Write, bytes: 500_000_000, device_bytes: 0 },
            Step { x: 1, y: 2, op: Op::Replace, bytes: 100, device_bytes: 40 },
            Step { x: 1, y: 3, op: Op::Remove, bytes: 30, device_bytes: 30 },
            Step { x: 1, y: 4, op: Op::Skip, bytes: 7, device_bytes: 7 },
        ];
        let mut p = Plan { pack_id: 1, pack_name: "x".into(), target: Target::Version(23), from_version: None, steps, kept_shared: vec![], warnings: vec![], free: 700_000_000 };
        assert_eq!(p.download_bytes(), 500_000_100);
        assert_eq!(p.net_bytes(), 500_000_000 + 60 - 30);
        assert!(p.enough_space());
        p.free = 550_000_000;
        assert!(!p.enough_space());
        assert_eq!(p.work(), 3);
    }

    #[test]
    fn patches_tilepack_versions() {
        let cur = json!([{"156": 18}, {"127": 18}]);
        assert_eq!(patch_tilepacks(&cur, 156, Some(23)), json!([{"156": 23}, {"127": 18}]));
        assert_eq!(patch_tilepacks(&cur, 166, Some(23)), json!([{"156": 18}, {"127": 18}, {"166": 23}]));
        assert_eq!(patch_tilepacks(&cur, 156, None), json!([{"127": 18}]));
        assert_eq!(patch_tilepacks(&json!([{"156": 18, "127": 18}]), 156, None), json!([{"127": 18}]));
        assert_eq!(patch_tilepacks(&Value::Null, 5, Some(1)), json!([{"5": 1}]));
    }

    #[test]
    fn owners_exclude_the_pack_itself() {
        let list = json!({"tile_packs": [{"id": 1, "tile_sets": [{"zoom": 8, "tile_coordinates": [[1, 1], [2, 2]]}]}, {"id": 2, "tile_sets": [{"zoom": 8, "tile_coordinates": [[2, 2]]}]}]});
        let installed: BTreeMap<u64, u32> = [(1, 18), (2, 18)].into();
        let o = other_owners(&list, &installed, 1);
        assert_eq!(o.keys().copied().collect::<Vec<_>>(), [(2, 2)]);
        assert_eq!(o[&(2, 2)], [2]);
    }
}
