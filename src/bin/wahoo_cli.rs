use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::sync::Arc;
use wahoo_desktop::{app::App, mtp, views};

#[derive(Parser)]
#[command(name = "wahoo_cli", about = "Inspect a Wahoo ELEMNT connected over USB", version)]
struct Cli {
    /// Device cache key (defaults to the first synced device)
    #[arg(short, long, global = true)]
    device: Option<String>,
    /// Print raw JSON instead of formatted text
    #[arg(short, long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show USB/MTP connection state (online, attached but off, absent)
    Status,
    /// Read the device over USB and refresh the local cache
    Sync,
    /// Add a route (.gpx is converted to a FIT course) to /routes on the device
    AddRoute { file: std::path::PathBuf },
    /// Copy one raw file from the device to a local path
    Pull { path: String, out: std::path::PathBuf },
    /// Device information: model, firmware, serial: model, firmware, serial
    Info,
    /// Check whether a newer firmware is available (needs internet)
    Firmware,
    /// Battery level and drain statistics
    Battery,
    /// Storage capacity and breakdown
    Storage,
    /// Installed map tiles, regions, versions and warnings
    Maps,
    /// Check whether newer map versions exist (needs internet); `--tile x/y` checks one tile
    MapUpdates {
        #[arg(long)]
        tile: Option<String>,
    },
    /// List the map pack tree (continent > country > region); installed packs only unless `--all`
    MapPacks {
        #[arg(long)]
        all: bool,
    },
    /// Download a map pack (id or name, e.g. Netherlands) from Wahoo's CDN to the local maps folder
    MapDownload {
        pack: String,
        /// Map version (defaults to the newest)
        #[arg(long)]
        version: Option<u32>,
    },
    /// Replace one device tile (x/y) by the same tile of a CDN map version; dry run unless `--apply`
    InstallTile {
        /// Tile as x/y, e.g. 131/83
        tile: String,
        /// Map version to install (e.g. 21; use the old version, e.g. 18, to roll back)
        #[arg(long)]
        version: u32,
        /// Version marker to leave next to the tile (default: keep the existing marker)
        #[arg(long)]
        marker: Option<u32>,
        /// Only set the marker (needs --marker); no tile data is transferred
        #[arg(long, requires = "marker")]
        marker_only: bool,
        /// Actually write to the device (without it only a plan is printed)
        #[arg(long)]
        apply: bool,
    },
    /// Valhalla routing data on the device: graph tiles per level, checksums and service config
    Routing,
    /// Export a Valhalla graph tile (.gph.lzma, .gph or a tile id such as 818660 from the cache) to GeoJSON
    RoutingExport {
        tile: String,
        /// Output file (default: %LOCALAPPDATA%\WahooDesktop\export\<tile id>.geojson)
        #[arg(short, long)]
        out: Option<std::path::PathBuf>,
    },
    /// Sensors seen by the device
    Sensors,
    /// Saved routes
    Routes,
    /// Planned workouts
    Plans,
    /// Derived alerts and warnings
    Alerts,
    /// Ride history (newest first)
    History {
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    /// Details of one activity by id (see `history`)
    Activity { id: String },
    /// Overall statistics
    Stats,
    /// Technical details: sync timings, system files, tile packs
    Debug,
    /// List raw device files (optionally filtered by path prefix)
    Ls {
        #[arg(default_value = "")]
        prefix: String,
    },
}

fn bytes(n: u64) -> String {
    let n = n as f64;
    if n >= 1e9 { format!("{:.2} GB", n / 1e9) } else if n >= 1e6 { format!("{:.1} MB", n / 1e6) } else if n >= 1e3 { format!("{:.0} kB", n / 1e3) } else { format!("{n:.0} B") }
}

fn dur(s: f64) -> String {
    let s = s as u64;
    format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

fn when(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

fn f(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap_or(0.0)
}

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap());
}

/// A tile argument is a file path, or a level-2 tile id (e.g. 818660) looked up in the device caches.
fn resolve_tile(arg: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::PathBuf::from(arg);
    if p.is_file() {
        return Ok(p);
    }
    let id: u32 = arg.parse().map_err(|_| format!("'{arg}' is neither a file nor a tile id"))?;
    let digits = format!("{id:09}");
    let rel = std::path::Path::new("files").join("maps").join("routing").join("2").join(&digits[0..3]).join(&digits[3..6]).join(format!("{}.gph.lzma", &digits[6..9]));
    for dev in std::fs::read_dir(wahoo_desktop::app::cache_dir()).map_err(|e| e.to_string())?.flatten() {
        let c = dev.path().join(&rel);
        if c.is_file() {
            return Ok(c);
        }
    }
    Err(format!("tile {id} is not in the cache (the sync only fetches a few sample tiles around Utrecht; use `pull` for others)"))
}

fn pick(app: &Arc<App>, key: &Option<String>) -> Result<Arc<wahoo_desktop::app::DeviceData>, String> {
    let devs = app.devices.read().unwrap();
    match key {
        Some(k) => devs.get(k).cloned().ok_or_else(|| format!("unknown device '{k}'; known: {:?}", devs.keys().collect::<Vec<_>>())),
        None => devs.values().next().cloned().ok_or_else(|| "no cached device yet - run `wahoo_cli sync` with the Wahoo connected".to_string()),
    }
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match &cli.cmd {
        Cmd::Status => {
            let d = mtp::detect();
            if cli.json {
                print_json(&serde_json::to_value(&d).unwrap());
            } else {
                println!("Status : {:?}\n{}", d.status, d.message);
                for u in &d.usb {
                    println!("USB    : {} {} ({:04x}:{:04x}) serial {}", u.manufacturer, u.product, u.vendor_id, u.product_id, u.serial);
                }
            }
            return Ok(());
        }
        Cmd::Sync => {
            let app = App::new();
            let t = std::time::Instant::now();
            app.run_sync();
            let s = app.sync.lock().unwrap().clone();
            for l in &s.log {
                println!("{l}");
            }
            if let Some(e) = s.error {
                return Err(e);
            }
            println!("Done in {:.1}s", t.elapsed().as_secs_f32());
            return Ok(());
        }
        Cmd::Pull { path, out } => {
            let det = mtp::detect();
            let dev = det.mtp_devices.first().ok_or(det.message)?;
            let data = mtp::read_path(&dev.id, path)?;
            std::fs::write(out, &data).map_err(|e| e.to_string())?;
            println!("Wrote {} bytes to {}", data.len(), out.display());
            return Ok(());
        }
        Cmd::AddRoute { file } => {
            let data = std::fs::read(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
            let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("route.gpx");
            let app = App::new();
            let (saved, info) = app.add_route(cli.device.as_deref(), name, &data)?;
            match info {
                Some(c) => println!("Added {saved}: {} points, {:.1} km", c.points, c.distance_m / 1000.0),
                None => println!("Added {saved}"),
            }
            return Ok(());
        }
        Cmd::InstallTile { tile, version, marker, marker_only, apply } => {
            let (x, y) = tile.split_once('/').and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?))).ok_or("tile must be x/y, e.g. 131/83")?;
            if *marker_only {
                let r = wahoo_desktop::tileinstall::set_marker(x, y, marker.unwrap_or(*version), *apply)?;
                if cli.json {
                    print_json(&r);
                } else {
                    println!("Tile {}: markers {} -> .{}{}", r["tile"], r["device_markers"], r["target_marker"], if r["applied"] == true { " (done)" } else { " (dry run, add --apply)" });
                }
                return Ok(());
            }
            let r = wahoo_desktop::tileinstall::install(x, y, *version, *marker, *apply)?;
            if cli.json {
                print_json(&r);
            } else {
                println!("Tile {}: device {} bytes, markers {}, v{} on CDN {} bytes", r["tile"], r["device_bytes"], r["device_markers"], r["version"], r["cdn_bytes"]);
                if r["applied"] == true {
                    println!("Installed and verified byte for byte; marker .{} set; temp copy deleted: {}", r["target_marker"], r["temp_copy_deleted"]);
                } else {
                    println!("Dry run: nothing written (temp copy: {}). Add --apply to install; restart the device afterwards.", r["temp_copy_bytes"]);
                }
            }
            return Ok(());
        }
        Cmd::RoutingExport { tile, out } => {
            let path = resolve_tile(tile)?;
            let raw = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let ex = wahoo_desktop::valhalla::export_geojson(&wahoo_desktop::valhalla::decompress(&raw)?)?;
            let out = out.clone().unwrap_or_else(|| wahoo_desktop::app::cache_dir().with_file_name("export").join(format!("{}.geojson", ex.tile_id)));
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            std::fs::write(&out, &ex.geojson).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            if cli.json {
                print_json(&json!({"tile_id": ex.tile_id, "level": ex.level, "nodes": ex.nodes, "directed_edges": ex.directed_edges, "segments": ex.segments, "file": out.display().to_string(), "bytes": ex.geojson.len()}));
            } else {
                println!("Tile {} (level {}): {} nodes, {} directed edges, {} road segments", ex.tile_id, ex.level, ex.nodes, ex.directed_edges, ex.segments);
                println!("Wrote {} ({})", out.display(), bytes(ex.geojson.len() as u64));
            }
            return Ok(());
        }
        _ => {}
    }

    let app = App::new();
    let d = pick(&app, &cli.device)?;
    let value = match &cli.cmd {
        Cmd::Info => views::info(&d),
        Cmd::Firmware => views::firmware(&d),
        Cmd::Battery => views::battery(&d),
        Cmd::Storage => views::storage(&d),
        Cmd::Maps => views::maps(&d),
        Cmd::Routing => views::routing(&d),
        Cmd::MapUpdates { tile } => {
            let r = wahoo_desktop::mapupdates::check(&d.tilepacks, &wahoo_desktop::mapupdates::tile_markers(&d.manifest));
            let Some(t) = tile else {
                if cli.json {
                    print_json(&r);
                } else {
                    println!("{}", r["message"].as_str().unwrap_or(""));
                    for p in r["packs"].as_array().into_iter().flatten() {
                        println!("  {:>4} {:<42} v{:<3} {:<10} -> v{} {}", p["id"], p["name"].as_str().unwrap_or("?"), p["installed_version"], p["installed_date"].as_str().unwrap_or("?"), p["latest_version"], if p["status"] == "update_available" { "UPDATE" } else { "ok" });
                    }
                    println!("Latest tiles: {}", r["tile_url"].as_str().unwrap_or(""));
                }
                return Ok(());
            };
            let parts: Vec<u32> = t.split('/').filter_map(|p| p.trim().parse().ok()).collect();
            let [x, y] = parts[..] else { return Err("tile must be written x/y, e.g. 131/83".into()) };
            let m = views::maps(&d);
            let dev = m["tiles"].as_array().and_then(|a| a.iter().find(|v| v["x"] == x && v["y"] == y)).cloned().ok_or("that tile is not installed on the device")?;
            let latest = r["latest_version"].as_u64().ok_or("could not determine the latest map version (offline?)")? as u32;
            let remote = wahoo_desktop::mapupdates::tile(latest, x, y, dev["bytes"].as_u64());
            let out = json!({"tile": t, "device": {"version": dev["version"], "bytes": dev["bytes"], "modified": dev["modified"]}, "latest_version": latest, "remote": remote});
            if cli.json {
                print_json(&out);
            } else {
                println!("Tile {t} on device: v{} {} ({})", dev["version"], bytes(dev["bytes"].as_u64().unwrap_or(0)), dev["modified"].as_i64().map(when).unwrap_or_default());
                println!("Latest v{latest}:   {} (last modified {})", remote["remote_bytes"].as_u64().map(bytes).unwrap_or_else(|| "?".into()), remote["remote_modified"].as_str().unwrap_or("?"));
                println!("Status:            {}", if dev["version"].as_u64() == Some(latest as u64) { "on the latest version" } else { "a newer version of this tile is available" });
                println!("Download:          {}", remote["url"].as_str().unwrap_or(""));
            }
            return Ok(());
        }
        Cmd::MapPacks { all } => {
            let r = wahoo_desktop::mapupdates::packs(&d.tilepacks, &wahoo_desktop::mapupdates::tile_markers(&d.manifest));
            if cli.json {
                print_json(&r);
            } else if r["status"] != "ok" {
                return Err(r["message"].as_str().unwrap_or("unknown error").to_string());
            } else {
                println!("Map version v{} ({}); {} packs installed on the device. Download folder: {}", r["latest_version"], r["latest_date"].as_str().unwrap_or("?"), r["installed_total"], r["download_dir"].as_str().unwrap_or(""));
                fn walk(v: &Value, depth: usize, all: bool) {
                    for p in v.as_array().into_iter().flatten() {
                        if !all && p["installed_count"].as_u64().unwrap_or(0) == 0 {
                            continue;
                        }
                        let mark = match p["installed_version"].as_u64() { Some(v) => format!("installed v{v} {}", p["installed_date"].as_str().unwrap_or("")), None => String::new() };
                        println!("{}{:>4} {:<30} {:>9}  {}", "  ".repeat(depth), p["id"], p["name"].as_str().unwrap_or("?"), bytes(p["size_bytes"].as_u64().unwrap_or(0)), mark);
                        walk(&p["children"], depth + 1, all);
                    }
                }
                walk(&r["packs"], 0, *all);
            }
            return Ok(());
        }
        Cmd::MapDownload { pack, version } => {
            wahoo_desktop::mapupdates::start_download(pack, *version, &d.tilepacks)?;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let s = wahoo_desktop::mapupdates::download_state();
                if !cli.json {
                    eprint!("\r{} v{}: tile {}/{}  {} of {}   ", s["pack_name"].as_str().unwrap_or(""), s["version"], s["tiles_done"], s["tiles_total"], bytes(s["bytes_done"].as_u64().unwrap_or(0)), bytes(s["bytes_total"].as_u64().unwrap_or(0)));
                }
                if s["running"] == false {
                    if let Some(e) = s["error"].as_str() {
                        return Err(e.to_string());
                    }
                    if cli.json {
                        print_json(&s);
                    } else {
                        eprintln!();
                        println!("Downloaded {} tiles to {}", s["tiles_done"], s["dir"].as_str().unwrap_or(""));
                    }
                    return Ok(());
                }
            }
        }
        Cmd::Sensors => views::sensors(&d),
        Cmd::Routes => views::routes(&d),
        Cmd::Plans => views::plans(&d),
        Cmd::Alerts => views::alerts(&d),
        Cmd::History { .. } => views::history(&d),
        Cmd::Stats => views::stats(&d),
        Cmd::Debug => views::debug(&d),
        Cmd::Ls { prefix } => {
            let items: Vec<_> = d.manifest.entries.iter().filter(|e| e.path.starts_with(prefix.as_str())).collect();
            if cli.json {
                print_json(&serde_json::to_value(items).unwrap());
            } else {
                for e in items {
                    println!("{} {:>10}  {}", if e.dir { "D" } else { "F" }, if e.dir { String::new() } else { bytes(e.size) }, e.path);
                }
            }
            return Ok(());
        }
        Cmd::Activity { id } => {
            let s = d.summaries.iter().find(|s| s.id == *id || s.id.contains(id.as_str())).ok_or("activity not found")?;
            let det = wahoo_desktop::fit::parse_file(&d.dir.join("files").join("exports").join(&s.file), true)?;
            if cli.json {
                print_json(&serde_json::to_value(&det).unwrap());
            } else {
                let s = &det.summary;
                println!("{}  ({}/{})", s.id, s.sport, s.sub_sport);
                println!("Start     {}\nEnd       {}", when(s.start), when(s.end));
                println!("Distance  {:.2} km\nMoving    {}\nElapsed   {}", s.distance / 1000.0, dur(s.timer), dur(s.elapsed));
                println!("Speed     avg {:.1} / max {:.1} km/h", s.avg_speed * 3.6, s.max_speed * 3.6);
                println!("Ascent    {:.0} m  Descent {:.0} m", s.ascent, s.descent);
                if let Some(h) = s.avg_hr { println!("HR        avg {h:.0} / max {:.0}", s.max_hr.unwrap_or(0.0)); }
                if let Some(p) = s.avg_power { println!("Power     avg {p:.0} / max {:.0} W", s.max_power.unwrap_or(0.0)); }
                println!("Battery   {:?}% -> {:?}%", s.battery_start, s.battery_end);
                println!("Firmware  {}  Hardware {}", s.firmware, s.hardware);
                println!("Records   {}  Track pts {}  Laps {}", s.records, det.track.len(), det.laps.len());
                if let Some(b) = s.bbox { println!("BBox      {:.4},{:.4} .. {:.4},{:.4}", b[0], b[1], b[2], b[3]); }
            }
            return Ok(());
        }
        _ => unreachable!(),
    };
    if cli.json {
        print_json(&value);
        return Ok(());
    }
    match &cli.cmd {
        Cmd::Info => {
            for (k, v) in value.as_object().unwrap() {
                if !v.is_null() {
                    println!("{k:<16} {}", v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string()));
                }
            }
        }
        Cmd::Firmware => {
            println!("Installed  {}", value["installed"].as_str().unwrap_or("?"));
            println!("Latest     {}", value["latest"].as_str().unwrap_or("?"));
            if let Some(b) = value["beta"].as_str() { println!("Beta       {b}"); }
            println!("Status     {}", value["message"].as_str().unwrap_or(""));
            if value["status"] == "update_available" {
                if let Some(u) = value["latest_url"].as_str() { println!("Download   {u}"); }
            }
        }
        Cmd::Battery => {
            match value["level"].as_u64() { Some(l) => println!("Level now     {l}%"), None => println!("Level now     unknown") }
            if let Some(r) = value["drain_per_hour"].as_f64() { println!("Drain         {r:.2} %/h riding"); }
            if let Some(h) = value["estimated_hours"].as_f64() { println!("Est. runtime  {h:.1} h"); }
            println!("History pts   {}", value["history"].as_array().map(|a| a.len()).unwrap_or(0));
        }
        Cmd::Storage => {
            let (c, fr, u) = (value["capacity"].as_u64().unwrap_or(0), value["free"].as_u64().unwrap_or(0), value["used"].as_u64().unwrap_or(0));
            println!("{} ({})\nTotal {}  Used {} ({:.0}%)  Free {}", value["name"].as_str().unwrap_or(""), value["fs"].as_str().unwrap_or(""), bytes(c), bytes(u), u as f64 / c.max(1) as f64 * 100.0, bytes(fr));
            for c in value["categories"].as_array().unwrap() {
                println!("  {:<16} {:>6} files {:>10}", c["name"].as_str().unwrap_or(""), c["files"], bytes(c["bytes"].as_u64().unwrap_or(0)));
            }
            println!("  {:<16} {:>6}       {:>10}", "unaccounted", "", bytes(value["unaccounted"].as_u64().unwrap_or(0)));
        }
        Cmd::Maps => {
            println!("{} tiles, {}", value["tile_count"], bytes(value["tile_bytes"].as_u64().unwrap_or(0)));
            println!("Versions: {}", value["versions"]);
            for r in value["regions"].as_array().unwrap() {
                println!("  {:<22} {:>3} tiles {:>10}", r["region"].as_str().unwrap_or(""), r["tiles"], bytes(r["bytes"].as_u64().unwrap_or(0)));
            }
            for w in value["warnings"].as_array().unwrap() {
                println!("! {}", w.as_str().unwrap_or(""));
            }
            println!("{}", value["download_note"].as_str().unwrap_or(""));
        }
        Cmd::Routing => {
            let g = &value["graph_tiles"];
            if g.is_null() {
                println!("Graph tiles   unknown (run `wahoo_cli sync` to read maps/routing_md5)");
            } else {
                println!("Graph tiles   {} (from maps/routing_md5)", g["tiles"]);
                for l in g["levels"].as_array().into_iter().flatten() {
                    println!("  level {} {:<15} {:>7} tiles", l["level"], l["name"].as_str().unwrap_or(""), l["tiles"]);
                }
                println!("Coverage      {} cells of 1 degree (level 2)", g["coverage"].as_array().map_or(0, |a| a.len()));
            }
            println!("Folders       {}", value["folders"]);
            if let Some(b) = value["checksums"]["bytes"].as_u64() { println!("routing_md5   {}", bytes(b)); }
            if let Some(b) = value["config_file"]["bytes"].as_u64() { println!("routing-config.json {}", bytes(b)); }
            for w in value["warnings"].as_array().into_iter().flatten() {
                println!("! {}", w.as_str().unwrap_or(""));
            }
            println!("{}", value["layout"].as_str().unwrap_or(""));
        }
        Cmd::Alerts => {
            for a in value["items"].as_array().unwrap() {
                println!("[{}] {} - {}", a["level"].as_str().unwrap_or(""), a["title"].as_str().unwrap_or(""), a["detail"].as_str().unwrap_or(""));
            }
        }
        Cmd::History { limit } => {
            println!("{:<17} {:>9} {:>9} {:>7} {:>8}  {}", "Start", "Dist km", "Moving", "km/h", "Ascent", "Id");
            for s in value["items"].as_array().unwrap().iter().take(*limit) {
                println!("{:<17} {:>9.1} {:>9} {:>7.1} {:>7.0}m  {}", when(s["start"].as_i64().unwrap_or(0)), f(s, "distance") / 1000.0, dur(f(s, "timer")), f(s, "avg_speed") * 3.6, f(s, "ascent"), s["id"].as_str().unwrap_or(""));
            }
        }
        Cmd::Stats => {
            println!("Rides        {}\nDistance     {:.0} km\nMoving time  {:.1} h\nAscent       {:.0} m\nAvg speed    {:.1} km/h\nActive days  {}  best streak {}", value["rides"], f(&value, "distance") / 1000.0, f(&value, "moving_time") / 3600.0, f(&value, "ascent"), f(&value, "avg_speed") * 3.6, value["active_days"], value["best_streak_days"]);
            for y in value["by_year"].as_array().unwrap() {
                println!("  {}: {} rides, {:.0} km", y["year"], y["rides"], f(y, "distance") / 1000.0);
            }
        }
        _ => print_json(&value),
    }
    Ok(())
}
