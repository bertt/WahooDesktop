//! Stub backend for platforms without device access; every call reports "unsupported".
use super::Manifest;
use std::path::{Path, PathBuf};

const MSG: &str = "Device access over USB/MTP is only supported on Windows; use the cache (synced on Windows) instead";

pub fn list_devices() -> Result<Vec<(String, String)>, String> {
    Ok(Vec::new())
}

pub fn read_path(_pnp_id: &str, _path: &str) -> Result<Vec<u8>, String> {
    Err(MSG.into())
}

pub fn upload(_pnp_id: &str, _folder: &str, _file_name: &str, _data: &[u8]) -> Result<String, String> {
    Err(MSG.into())
}

pub fn stat(_pnp_id: &str, _path: &str) -> Result<Option<u64>, String> {
    Err(MSG.into())
}

pub fn delete(_pnp_id: &str, _path: &str) -> Result<bool, String> {
    Err(MSG.into())
}

pub fn write_new(_pnp_id: &str, _path: &str, _data: &[u8]) -> Result<(), String> {
    Err(MSG.into())
}

pub fn make_dir(_pnp_id: &str, _path: &str) -> Result<(), String> {
    Err(MSG.into())
}

pub fn sync(_cache: &Path, _pnp_id: &str, _friendly: &str) -> Result<(Manifest, PathBuf), String> {
    Err(MSG.into())
}
