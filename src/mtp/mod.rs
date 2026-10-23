//! Device access. Windows only for now (native WPD API); macOS/Linux backends are planned.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[cfg(windows)]
mod wpd;
#[cfg(windows)]
use wpd as backend;

#[cfg(not(windows))]
compile_error!("Only Windows is supported at the moment; macOS/Linux backends are not implemented yet.");

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub dir: bool,
    #[serde(default)]
    pub modified: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Storage {
    pub name: String,
    pub capacity: u64,
    pub free: u64,
    pub fs: String,
    pub serial: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DeviceProps {
    pub pnp_id: String,
    pub name: String,
    pub manufacturer: String,
    pub model: String,
    pub serial: String,
    pub firmware: String,
    pub power_level: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Manifest {
    pub device: DeviceProps,
    pub storages: Vec<Storage>,
    pub entries: Vec<Entry>,
    pub synced_at: String,
    pub walk_ms: u128,
    pub copied: Vec<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct UsbInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub product: String,
    pub manufacturer: String,
    pub serial: String,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// MTP storage is reachable and can be read.
    Online,
    /// Seen on the USB bus, but no MTP storage: powered off, charging-only or still booting.
    UsbNoMtp,
    /// Listed as a portable device, but opening it failed (locked, busy, other app has it).
    OpenFailed,
    /// Nothing found.
    Absent,
}

#[derive(Serialize, Clone, Debug)]
pub struct Detection {
    pub status: Status,
    pub message: String,
    pub usb: Vec<UsbInfo>,
    pub mtp_devices: Vec<MtpDevice>,
}

#[derive(Serialize, Clone, Debug)]
pub struct MtpDevice {
    pub id: String,
    pub name: String,
}

fn is_wahoo(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("elemnt") || n.contains("wahoo")
}

fn usb_devices() -> Vec<UsbInfo> {
    use nusb::MaybeFuture;
    let Ok(list) = nusb::list_devices().wait() else { return Vec::new() };
    list.filter(|d| {
        is_wahoo(d.product_string().unwrap_or_default()) || is_wahoo(d.manufacturer_string().unwrap_or_default())
    })
    .map(|d| UsbInfo {
        vendor_id: d.vendor_id(),
        product_id: d.product_id(),
        product: d.product_string().unwrap_or_default().into(),
        manufacturer: d.manufacturer_string().unwrap_or_default().into(),
        serial: d.serial_number().unwrap_or_default().into(),
    })
    .collect()
}

pub fn list_devices() -> Result<Vec<MtpDevice>, String> {
    let all = backend::list_devices().map_err(|e| e.to_string())?;
    Ok(all.into_iter().filter(|(_, n)| is_wahoo(n)).map(|(id, name)| MtpDevice { id, name }).collect())
}

/// Cheap check that tells apart "not connected", "connected but off" and "ready".
pub fn detect() -> Detection {
    let usb = usb_devices();
    let mtp = list_devices().unwrap_or_default();
    let (status, message) = if !mtp.is_empty() {
        (Status::Online, format!("{} ready", mtp[0].name))
    } else if !usb.is_empty() {
        (
            Status::UsbNoMtp,
            format!(
                "{} is attached over USB but exposes no storage. It is probably powered off, in charge-only mode \
                 or still booting. Switch it on and wait until it shows the USB connection screen.",
                usb[0].product
            ),
        )
    } else {
        (Status::Absent, "No Wahoo found. Connect it with a data-capable USB cable.".into())
    };
    Detection { status, message, usb, mtp_devices: mtp }
}

pub fn sync(cache: &Path, id: &str, name: &str) -> Result<(Manifest, PathBuf), String> {
    backend::sync(cache, id, name).map_err(|e| e.to_string())
}

pub fn read_path(id: &str, path: &str) -> Result<Vec<u8>, String> {
    backend::read_path(id, path).map_err(|e| e.to_string())
}

pub fn upload(id: &str, folder: &str, file_name: &str, data: &[u8]) -> Result<String, String> {
    backend::upload(id, folder, file_name, data).map_err(|e| e.to_string())
}

pub fn stat(id: &str, path: &str) -> Result<Option<u64>, String> {
    backend::stat(id, path).map_err(|e| e.to_string())
}

pub fn delete(id: &str, path: &str) -> Result<bool, String> {
    backend::delete(id, path).map_err(|e| e.to_string())
}

/// Creates a new file; fails if it already exists or the parent folder is missing.
pub fn write_new(id: &str, path: &str, data: &[u8]) -> Result<(), String> {
    backend::write_new(id, path, data).map_err(|e| e.to_string())
}

pub fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}
