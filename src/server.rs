//! HTTP API and static UI.
use crate::{app::App, fit, mtp, views};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};

type St = State<Arc<App>>;
type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn not_found(msg: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::NOT_FOUND, Json(json!({"error": msg})))
}

fn device(app: &App, key: &str) -> Result<Arc<crate::app::DeviceData>, (StatusCode, Json<Value>)> {
    app.devices.read().unwrap().get(key).cloned().ok_or_else(|| not_found("unknown device"))
}

fn asset(body: &'static str, mime: &'static str) -> Response {
    ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "no-cache")], body).into_response()
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/", get(|| async { asset(include_str!("../static/index.html"), "text/html; charset=utf-8") }))
        .route("/app.js", get(|| async { asset(include_str!("../static/app.js"), "text/javascript; charset=utf-8") }))
        .route("/style.css", get(|| async { asset(include_str!("../static/style.css"), "text/css; charset=utf-8") }))
        .route("/api/status", get(status))
        .route("/api/sync", post(start_sync))
        .route("/api/sync/state", get(sync_state))
        .route("/api/devices/{key}/mappacks/{id}/download", post(start_map_download))
        .route("/api/mapdownload", get(|| async { Json(crate::mapupdates::download_state()) }))
        .route("/api/mapdownload/open", post(open_download_dir))
        .route("/api/devices", get(devices))
        .route("/api/devices/{key}/{section}", get(section))
        .route("/api/devices/{key}/activity/{id}", get(activity))
        .route("/api/devices/{key}/route/{name}", get(route_detail))
        .route("/api/devices/{key}/file", get(file))
        .route("/api/devices/{key}/routes/upload", post(upload_route).layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024)))
        .with_state(app)
}

// Opens the map download folder (or the last download's folder inside it) in Explorer.
async fn open_download_dir() -> Result<Json<Value>, Fail> {
    let root = crate::mapupdates::download_dir();
    let last = std::path::PathBuf::from(crate::mapupdates::download_state()["dir"].as_str().unwrap_or(""));
    let dir = if !last.as_os_str().is_empty() && last.starts_with(&root) && last.is_dir() { last } else { root };
    std::fs::create_dir_all(&dir).map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    std::process::Command::new("explorer").arg(&dir).spawn().map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({"opened": dir.display().to_string()})))
}

type Fail = (StatusCode, Json<Value>);
fn fail(code: StatusCode, msg: impl ToString) -> Fail {
    (code, Json(json!({"error": msg.to_string()})))
}

/// Accepts a .fit or .gpx file, converts GPX to a FIT course and writes it to `routes/` on the device.
async fn upload_route(State(app): St, Path(key): Path<String>, Query(q): Query<HashMap<String, String>>, body: axum::body::Bytes) -> ApiResult {
    let orig = q.get("name").cloned().unwrap_or_else(|| "route.gpx".into());
    let a = app.clone();
    let (name, info) = tokio::task::spawn_blocking(move || a.add_route(Some(&key), &orig, &body))
        .await
        .unwrap()
        .map_err(|e| fail(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(json!({"file": name, "converted": info})))
}

/// Starts downloading the tiles of one map pack to the local maps folder (not to the device).
async fn start_map_download(State(app): St, Path((key, id)): Path<(String, u64)>, Query(q): Query<HashMap<String, String>>) -> ApiResult {
    let d = device(&app, &key)?;
    let version = q.get("version").and_then(|v| v.parse::<u32>().ok());
    tokio::task::spawn_blocking(move || crate::mapupdates::start_download(&id.to_string(), version, &d.tilepacks))
        .await
        .unwrap()
        .map_err(|e| fail(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(crate::mapupdates::download_state()))
}

async fn status() -> Json<Value> {
    let d = tokio::task::spawn_blocking(mtp::detect).await.unwrap();
    Json(serde_json::to_value(d).unwrap())
}

async fn start_sync(State(app): St) -> Json<Value> {
    if app.sync.lock().unwrap().running {
        return Json(json!({"started": false}));
    }
    let a = app.clone();
    tokio::task::spawn_blocking(move || a.run_sync());
    Json(json!({"started": true}))
}

async fn sync_state(State(app): St) -> Json<Value> {
    Json(serde_json::to_value(app.sync.lock().unwrap().clone()).unwrap())
}

async fn devices(State(app): St) -> Json<Value> {
    let det = tokio::task::spawn_blocking(mtp::detect).await.unwrap();
    let list: Vec<Value> = app
        .devices
        .read()
        .unwrap()
        .values()
        .map(|d| {
            let online = det.mtp_devices.iter().any(|m| mtp::sanitize(&m.name) == d.key);
            let mut v = views::info(d);
            v["online"] = json!(online);
            v["storage"] = views::storage(d);
            v
        })
        .collect();
    Json(json!({"devices": list, "detection": det}))
}

async fn section(State(app): St, Path((key, section)): Path<(String, String)>) -> ApiResult {
    let d = device(&app, &key)?;
    let v = tokio::task::spawn_blocking(move || match section.as_str() {
        "info" => Some(views::info(&d)),
        "firmware" => Some(views::firmware(&d)),
        "mapupdates" => Some(crate::mapupdates::check(&d.tilepacks, &crate::mapupdates::tile_markers(&d.manifest))),
        "mappacks" => Some(crate::mapupdates::packs(&d.tilepacks, &crate::mapupdates::tile_markers(&d.manifest))),
        "battery" => Some(views::battery(&d)),
        "storage" => Some(views::storage(&d)),
        "maps" => Some(views::maps(&d)),
        "routing" => Some(views::routing(&d)),
        "sensors" => Some(views::sensors(&d)),
        "routes" => Some(views::routes(&d)),
        "plans" => Some(views::plans(&d)),
        "alerts" => Some(views::alerts(&d)),
        "history" => Some(views::history(&d)),
        "stats" => Some(views::stats(&d)),
        "debug" => Some(views::debug(&d)),
        "entries" => Some(json!({"entries": d.manifest.entries})),
        _ => None,
    })
    .await
    .unwrap();
    v.map(Json).ok_or_else(|| not_found("unknown section"))
}

async fn activity(State(app): St, Path((key, id)): Path<(String, String)>) -> ApiResult {
    let d = device(&app, &key)?;
    let s = d.summaries.iter().find(|s| s.id == id).ok_or_else(|| not_found("unknown activity"))?;
    let p = d.dir.join("files").join("exports").join(&s.file);
    let det = tokio::task::spawn_blocking(move || fit::parse_file(&p, true)).await.unwrap();
    det.map(|d| Json(serde_json::to_value(d).unwrap())).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))
}

async fn route_detail(State(app): St, Path((key, name)): Path<(String, String)>) -> ApiResult {
    let d = device(&app, &key)?;
    let safe = name.replace(['/', '\\'], "");
    let p = d.dir.join("files").join("routes").join(&safe);
    let det = tokio::task::spawn_blocking(move || fit::parse_file(&p, true)).await.unwrap();
    det.map(|d| Json(serde_json::to_value(d).unwrap())).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))
}

// Raw cached text files for the technical view; restricted to the device's cache folder.
async fn file(State(app): St, Path(key): Path<String>, Query(q): Query<HashMap<String, String>>) -> Result<String, (StatusCode, Json<Value>)> {
    let d = device(&app, &key)?;
    let rel = q.get("path").ok_or_else(|| not_found("path missing"))?;
    let base = d.dir.join("files").canonicalize().map_err(|_| not_found("no cache"))?;
    let p = base.join(rel).canonicalize().map_err(|_| not_found("file not cached"))?;
    if !p.starts_with(&base) {
        return Err(not_found("outside cache"));
    }
    let bytes = std::fs::read(&p).map_err(|_| not_found("unreadable"))?;
    Ok(String::from_utf8_lossy(&bytes[..bytes.len().min(200_000)]).into_owned())
}
