//! FIT parsing: activity summaries (cheap, cached) and full details (on demand).
use fitparser::{FitDataRecord, Value};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const INTERNAL: [&str; 4] = ["Barometer", "Accelerometer", "Temperature", "GPS"];

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Sensor {
    pub name: String,
    pub kind: String,
    pub manufacturer: String,
    pub serial: String,
    pub software: String,
    pub hardware: String,
    pub battery_status: String,
    pub battery_voltage: Option<f64>,
    pub battery_pct: Option<f64>,
    pub ant_id: Option<u64>,
    pub source: String,
    pub internal: bool,
    pub last_seen: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Summary {
    pub id: String,
    pub file: String,
    pub bytes: u64,
    pub start: i64,
    pub end: i64,
    pub sport: String,
    pub sub_sport: String,
    pub name: String,
    pub elapsed: f64,
    pub timer: f64,
    pub distance: f64,
    pub ascent: f64,
    pub descent: f64,
    pub avg_speed: f64,
    pub max_speed: f64,
    pub avg_hr: Option<f64>,
    pub max_hr: Option<f64>,
    pub avg_power: Option<f64>,
    pub max_power: Option<f64>,
    pub avg_cadence: Option<f64>,
    pub calories: Option<f64>,
    pub avg_temp: Option<f64>,
    pub max_temp: Option<f64>,
    pub min_alt: Option<f64>,
    pub max_alt: Option<f64>,
    pub bbox: Option<[f64; 4]>,
    pub start_pos: Option<[f64; 2]>,
    pub battery_start: Option<f64>,
    pub battery_end: Option<f64>,
    pub firmware: String,
    pub device: String,
    pub hardware: String,
    pub sensors: Vec<String>,
    pub laps: u32,
    pub records: u32,
    pub seq: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Lap {
    pub start: i64,
    pub elapsed: f64,
    pub timer: f64,
    pub distance: f64,
    pub avg_speed: f64,
    pub max_speed: f64,
    pub avg_hr: Option<f64>,
    pub avg_power: Option<f64>,
    pub ascent: f64,
    pub descent: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Series {
    pub t: Vec<i64>,
    pub speed: Vec<Option<f64>>,
    pub hr: Vec<Option<f64>>,
    pub power: Vec<Option<f64>>,
    pub cadence: Vec<Option<f64>>,
    pub alt: Vec<Option<f64>>,
    pub dist: Vec<Option<f64>>,
    pub temp: Vec<Option<f64>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct EventRow {
    pub t: i64,
    pub event: String,
    pub event_type: String,
    pub extra: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct RawMsg {
    pub kind: String,
    pub count: usize,
    pub sample: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Detail {
    pub summary: Summary,
    pub laps: Vec<Lap>,
    pub track: Vec<[f64; 2]>,
    pub series: Series,
    pub events: Vec<EventRow>,
    pub sensors: Vec<Sensor>,
    pub battery: Vec<(i64, f64)>,
    pub hr_zones: Vec<f64>,
    pub power_zones: Vec<f64>,
    pub raw: Vec<RawMsg>,
}

type Fields<'a> = HashMap<&'a str, &'a Value>;

fn fields(r: &FitDataRecord) -> Fields<'_> {
    let mut m = HashMap::new();
    for f in r.fields() {
        m.entry(f.name()).or_insert(f.value());
    }
    m
}

fn num(v: &Value) -> Option<f64> {
    Some(match v {
        Value::Byte(x) | Value::Enum(x) | Value::UInt8(x) | Value::UInt8z(x) => *x as f64,
        Value::SInt8(x) => *x as f64,
        Value::SInt16(x) => *x as f64,
        Value::UInt16(x) | Value::UInt16z(x) => *x as f64,
        Value::SInt32(x) => *x as f64,
        Value::UInt32(x) | Value::UInt32z(x) => *x as f64,
        Value::SInt64(x) => *x as f64,
        Value::UInt64(x) | Value::UInt64z(x) => *x as f64,
        Value::Float32(x) => *x as f64,
        Value::Float64(x) => *x,
        _ => return None,
    })
}

fn g(m: &Fields, k: &str) -> Option<f64> {
    m.get(k).and_then(|v| num(v)).filter(|x| x.is_finite())
}

fn first(m: &Fields, ks: &[&str]) -> Option<f64> {
    ks.iter().find_map(|k| g(m, k))
}

fn s(m: &Fields, k: &str) -> String {
    m.get(k).map(|v| text(v)).unwrap_or_default()
}

fn text(v: &Value) -> String {
    match v {
        Value::Timestamp(t) => t.timestamp().to_string(),
        other => other.to_string(),
    }
}

fn ts(m: &Fields, k: &str) -> i64 {
    match m.get(k) {
        Some(Value::Timestamp(t)) => t.timestamp(),
        _ => 0,
    }
}

fn semi(v: f64) -> f64 {
    v * 180.0 / 2147483648.0
}

fn merge_sensor(list: &mut Vec<Sensor>, new: Sensor) {
    let key = |s: &Sensor| (s.name.clone(), s.serial.clone(), s.ant_id, s.kind.clone());
    if let Some(e) = list.iter_mut().find(|e| key(e) == key(&new)) {
        if new.last_seen >= e.last_seen {
            e.last_seen = new.last_seen;
            if new.battery_voltage.is_some() {
                e.battery_voltage = new.battery_voltage;
            }
            if new.battery_pct.is_some() {
                e.battery_pct = new.battery_pct;
            }
            if !new.battery_status.is_empty() {
                e.battery_status = new.battery_status;
            }
            if !new.software.is_empty() {
                e.software = new.software;
            }
        }
    } else {
        list.push(new);
    }
}

pub fn parse_file(path: &Path, full: bool) -> Result<Detail, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let bytes = f.metadata().map(|m| m.len()).unwrap_or(0);
    let recs = fitparser::from_reader(&mut f).map_err(|e| e.to_string())?;
    let file = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();

    let mut sum = Summary { id: file.trim_end_matches(".fit").to_string(), file: file.clone(), bytes, ..Default::default() };
    sum.seq = file.trim_end_matches(".fit").rsplit('-').nth(1).and_then(|x| x.parse().ok());
    let mut d = Detail::default();
    let mut counts: BTreeMap<String, (usize, BTreeMap<String, String>)> = BTreeMap::new();
    let (mut lat_min, mut lat_max, mut lon_min, mut lon_max) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    let mut have_session = false;
    let (mut hr_n, mut hr_sum, mut hr_max) = (0u32, 0.0, 0.0f64);
    let (mut pw_n, mut pw_sum, mut pw_max) = (0u32, 0.0, 0.0f64);
    let (mut first_ts, mut last_ts) = (0i64, 0i64);
    let mut all_points = Vec::new();
    let mut points_seen = 0u32;
    let mut series_rows: Vec<(i64, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>)> = Vec::new();
    let mut max_speed_rec = 0.0f64;
    let mut dist_last = 0.0;

    for r in &recs {
        let kind = r.kind().to_string();
        let m = fields(r);
        let e = counts.entry(kind.clone()).or_default();
        e.0 += 1;
        if e.0 == 1 || (kind == "record" && e.0 == 500) {
            e.1.clear();
            for f in r.fields() {
                e.1.insert(f.name().to_string(), format!("{}{}", text(f.value()), if f.units().is_empty() { String::new() } else { format!(" {}", f.units()) }));
            }
        }
        match kind.as_str() {
            "file_id" => {}
            "record" => {
                let t = ts(&m, "timestamp");
                if first_ts == 0 {
                    first_ts = t;
                }
                last_ts = t;
                points_seen += 1;
                if let (Some(la), Some(lo)) = (g(&m, "position_lat"), g(&m, "position_long")) {
                    let (la, lo) = (semi(la), semi(lo));
                    if la.abs() <= 90.0 && lo.abs() <= 180.0 && (la != 0.0 || lo != 0.0) {
                        lat_min = lat_min.min(la);
                        lat_max = lat_max.max(la);
                        lon_min = lon_min.min(lo);
                        lon_max = lon_max.max(lo);
                        if sum.start_pos.is_none() {
                            sum.start_pos = Some([la, lo]);
                        }
                        if full {
                            all_points.push([la, lo]);
                        }
                    }
                }
                let speed = first(&m, &["enhanced_speed", "speed"]);
                if let Some(sp) = speed {
                    max_speed_rec = max_speed_rec.max(sp);
                }
                let hr = g(&m, "heart_rate");
                if let Some(h) = hr {
                    hr_n += 1;
                    hr_sum += h;
                    hr_max = hr_max.max(h);
                }
                let pw = g(&m, "power");
                if let Some(p) = pw {
                    pw_n += 1;
                    pw_sum += p;
                    pw_max = pw_max.max(p);
                }
                let dist = g(&m, "distance");
                if let Some(x) = dist {
                    dist_last = x;
                }
                if full {
                    series_rows.push((t, speed, hr, pw, g(&m, "cadence"), first(&m, &["enhanced_altitude", "altitude"]), dist, g(&m, "temperature")));
                }
            }
            "session" if !have_session => {
                have_session = true;
                sum.sport = s(&m, "sport");
                sum.sub_sport = s(&m, "sub_sport");
                sum.start = ts(&m, "start_time");
                sum.end = ts(&m, "timestamp");
                sum.elapsed = g(&m, "total_elapsed_time").unwrap_or(0.0);
                sum.timer = g(&m, "total_timer_time").unwrap_or(0.0);
                sum.distance = g(&m, "total_distance").unwrap_or(0.0);
                sum.ascent = g(&m, "total_ascent").unwrap_or(0.0);
                sum.descent = g(&m, "total_descent").unwrap_or(0.0);
                sum.avg_speed = first(&m, &["enhanced_avg_speed", "avg_speed"]).unwrap_or(0.0);
                sum.max_speed = first(&m, &["enhanced_max_speed", "max_speed"]).unwrap_or(0.0);
                sum.avg_hr = g(&m, "avg_heart_rate");
                sum.max_hr = g(&m, "max_heart_rate");
                sum.avg_power = g(&m, "avg_power");
                sum.max_power = g(&m, "max_power");
                sum.avg_cadence = g(&m, "avg_cadence");
                sum.calories = g(&m, "total_calories");
                sum.avg_temp = g(&m, "avg_temperature");
                sum.max_temp = g(&m, "max_temperature");
                sum.min_alt = first(&m, &["enhanced_min_altitude", "min_altitude"]);
                sum.max_alt = first(&m, &["enhanced_max_altitude", "max_altitude"]);
                sum.laps = g(&m, "num_laps").unwrap_or(0.0) as u32;
            }
            "lap" if full => d.laps.push(Lap {
                start: ts(&m, "start_time"),
                elapsed: g(&m, "total_elapsed_time").unwrap_or(0.0),
                timer: g(&m, "total_timer_time").unwrap_or(0.0),
                distance: g(&m, "total_distance").unwrap_or(0.0),
                avg_speed: first(&m, &["enhanced_avg_speed", "avg_speed"]).unwrap_or(0.0),
                max_speed: first(&m, &["enhanced_max_speed", "max_speed"]).unwrap_or(0.0),
                avg_hr: g(&m, "avg_heart_rate"),
                avg_power: g(&m, "avg_power"),
                ascent: g(&m, "total_ascent").unwrap_or(0.0),
                descent: g(&m, "total_descent").unwrap_or(0.0),
            }),
            "workout" => sum.name = s(&m, "wkt_name"),
            "sport" if sum.sport.is_empty() => {
                sum.sport = s(&m, "sport");
                sum.sub_sport = s(&m, "sub_sport");
            }
            "hr_zone" if full => d.hr_zones.extend(g(&m, "high_bpm")),
            "power_zone" if full => d.power_zones.extend(g(&m, "high_value")),
            "event" => {
                let ev = s(&m, "event");
                if full {
                    let mut extra = BTreeMap::new();
                    for f in r.fields() {
                        if !["event", "event_type", "timestamp"].contains(&f.name()) {
                            extra.insert(f.name().to_string(), text(f.value()));
                        }
                    }
                    d.events.push(EventRow { t: ts(&m, "timestamp"), event: ev, event_type: s(&m, "event_type"), extra });
                }
            }
            "device_info" => {
                let t = ts(&m, "timestamp");
                let name = {
                    let n = s(&m, "product_name");
                    if n.is_empty() { s(&m, "descriptor") } else { n }
                };
                let idx = s(&m, "device_index");
                if idx == "creator" {
                    sum.firmware = s(&m, "software_version");
                    sum.hardware = s(&m, "hardware_version");
                    sum.device = s(&m, "descriptor");
                    if let Some(c) = g(&m, "charge") {
                        if sum.battery_start.is_none() {
                            sum.battery_start = Some(c);
                        }
                        sum.battery_end = Some(c);
                        if full {
                            d.battery.push((t, c));
                        }
                    }
                }
                if idx != "creator" || full {
                    let s_ = Sensor {
                        internal: INTERNAL.contains(&name.as_str()) || idx == "creator",
                        kind: s(&m, "device_type"),
                        source: s(&m, "source_type"),
                        name: if name.is_empty() { format!("Device {idx}") } else { name },
                        manufacturer: s(&m, "manufacturer"),
                        serial: s(&m, "serial_number"),
                        software: s(&m, "software_version"),
                        hardware: s(&m, "hardware_version"),
                        battery_status: s(&m, "battery_status"),
                        battery_voltage: g(&m, "battery_voltage"),
                        battery_pct: g(&m, "charge"),
                        ant_id: g(&m, "ant_device_number").map(|x| x as u64),
                        last_seen: t,
                    };
                    merge_sensor(&mut d.sensors, s_);
                }
            }
            _ => {}
        }
    }

    if !have_session {
        sum.start = first_ts;
        sum.end = last_ts;
        sum.elapsed = (last_ts - first_ts) as f64;
        sum.timer = sum.elapsed;
        sum.distance = dist_last;
        sum.max_speed = max_speed_rec;
        if sum.elapsed > 0.0 {
            sum.avg_speed = dist_last / sum.elapsed;
        }
    }
    if sum.avg_hr.is_none() && hr_n > 0 {
        sum.avg_hr = Some(hr_sum / hr_n as f64);
        sum.max_hr = Some(hr_max);
    }
    if sum.avg_power.is_none() && pw_n > 0 {
        sum.avg_power = Some(pw_sum / pw_n as f64);
        sum.max_power = Some(pw_max);
    }
    if lat_min <= lat_max {
        sum.bbox = Some([lon_min, lat_min, lon_max, lat_max]);
    }
    sum.records = points_seen;
    sum.sensors = d.sensors.iter().filter(|x| !x.internal).map(|x| x.name.clone()).collect();

    if full {
        let stride = (all_points.len() / 2000).max(1);
        d.track = all_points.into_iter().step_by(stride).collect();
        let st = (series_rows.len() / 600).max(1);
        for row in series_rows.into_iter().step_by(st) {
            d.series.t.push(row.0);
            d.series.speed.push(row.1);
            d.series.hr.push(row.2);
            d.series.power.push(row.3);
            d.series.cadence.push(row.4);
            d.series.alt.push(row.5);
            d.series.dist.push(row.6);
            d.series.temp.push(row.7);
        }
        d.raw = counts.into_iter().map(|(kind, (count, sample))| RawMsg { kind, count, sample }).collect();
    }
    d.summary = sum;
    Ok(d)
}
