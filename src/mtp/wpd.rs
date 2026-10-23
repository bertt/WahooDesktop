//! Native MTP access through the Windows Portable Devices (WPD) COM API.
use super::{DeviceProps, Entry, Manifest, Storage, sanitize};
use std::path::{Path, PathBuf};
use windows::Win32::Devices::PortableDevices::*;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::*;
use windows::core::{Error, GUID, PCWSTR, PWSTR, Result, w};

struct ComGuard;
impl ComGuard {
    fn new() -> Self {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        ComGuard
    }
}
impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

fn take_pwstr(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { p.to_string().unwrap_or_default() };
    unsafe { CoTaskMemFree(Some(p.0 as _)) };
    s
}

fn manager() -> Result<IPortableDeviceManager> {
    unsafe { CoCreateInstance(&PortableDeviceManager, None, CLSCTX_INPROC_SERVER) }
}

/// Returns (pnp id, friendly name) for each attached portable device.
pub fn list_devices() -> Result<Vec<(String, String)>> {
    let _g = ComGuard::new();
    let mgr = manager()?;
    unsafe {
        mgr.RefreshDeviceList()?;
        let mut count = 0u32;
        mgr.GetDevices(std::ptr::null_mut(), &mut count)?;
        let mut ids = vec![PWSTR::null(); count as usize];
        if count > 0 {
            mgr.GetDevices(ids.as_mut_ptr(), &mut count)?;
        }
        let mut out = Vec::new();
        for id in ids.into_iter().take(count as usize) {
            let mut len = 0u32;
            let pcw = PCWSTR(id.0);
            let _ = mgr.GetDeviceFriendlyName(pcw, PWSTR::null(), &mut len);
            let mut buf = vec![0u16; len.max(1) as usize];
            let _ = mgr.GetDeviceFriendlyName(pcw, PWSTR(buf.as_mut_ptr()), &mut len);
            let name = String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]);
            out.push((take_pwstr(id), name));
        }
        Ok(out)
    }
}

struct Session {
    props: IPortableDeviceProperties,
    content: IPortableDeviceContent,
    res: IPortableDeviceResources,
}

fn keys(list: &[PROPERTYKEY]) -> Result<IPortableDeviceKeyCollection> {
    unsafe {
        let c: IPortableDeviceKeyCollection =
            CoCreateInstance(&PortableDeviceKeyCollection, None, CLSCTX_INPROC_SERVER)?;
        for k in list {
            c.Add(k)?;
        }
        Ok(c)
    }
}

impl Session {
    fn open(pnp_id: &str) -> Result<Session> {
        Self::open_with(pnp_id, 0x8000_0000)
    }

    fn open_with(pnp_id: &str, access: u32) -> Result<Session> {
        unsafe {
            let dev: IPortableDevice =
                CoCreateInstance(&PortableDevice, None, CLSCTX_INPROC_SERVER)?;
            let info: IPortableDeviceValues =
                CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
            info.SetStringValue(&WPD_CLIENT_NAME, w!("WahooDesktop"))?;
            info.SetUnsignedIntegerValue(&WPD_CLIENT_DESIRED_ACCESS, access)?;
            let wide: Vec<u16> = pnp_id.encode_utf16().chain(Some(0)).collect();
            dev.Open(PCWSTR(wide.as_ptr()), &info)?;
            let content = dev.Content()?;
            let props = content.Properties()?;
            let res = content.Transfer()?;
            Ok(Session { props, content, res })
        }
    }

    fn values(&self, id: &str, k: &[PROPERTYKEY]) -> Option<IPortableDeviceValues> {
        let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
        unsafe { self.props.GetValues(PCWSTR(wide.as_ptr()), &keys(k).ok()?).ok() }
    }

    fn children(&self, id: &str) -> Vec<String> {
        let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
        let mut out = Vec::new();
        unsafe {
            let Ok(en) = self.content.EnumObjects(0, PCWSTR(wide.as_ptr()), None) else {
                return out;
            };
            loop {
                let mut batch = [PWSTR::null(); 64];
                let mut fetched = 0u32;
                let _ = en.Next(&mut batch, &mut fetched);
                if fetched == 0 {
                    break;
                }
                for p in batch.iter().take(fetched as usize) {
                    out.push(take_pwstr(*p));
                }
            }
        }
        out
    }

    fn read(&self, id: &str) -> Result<Vec<u8>> {
        let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let mut optimal = 0u32;
            let mut stream: Option<windows::Win32::System::Com::IStream> = None;
            self.res.GetStream(
                PCWSTR(wide.as_ptr()),
                &WPD_RESOURCE_DEFAULT,
                0, // STGM_READ
                &mut optimal,
                &mut stream,
            )?;
            let stream = stream.ok_or_else(|| Error::from_hresult(windows::core::HRESULT(-1)))?;
            let mut out = Vec::new();
            let mut buf = vec![0u8; optimal.max(65536) as usize];
            loop {
                let mut n = 0u32;
                let hr = stream.Read(buf.as_mut_ptr() as _, buf.len() as u32, Some(&mut n));
                if hr.is_err() {
                    return Err(Error::from_hresult(hr));
                }
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n as usize]);
            }
            Ok(out)
        }
    }

    fn walk(&self, id: &str, path: &str, out: &mut Vec<(String, Entry)>) {
        let k = [WPD_OBJECT_NAME, WPD_OBJECT_ORIGINAL_FILE_NAME, WPD_OBJECT_SIZE, WPD_OBJECT_CONTENT_TYPE, WPD_OBJECT_DATE_MODIFIED];
        for child in self.children(id) {
            let Some(v) = self.values(&child, &k) else { continue };
            let name = unsafe {
                v.GetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME)
                    .or_else(|_| v.GetStringValue(&WPD_OBJECT_NAME))
                    .map(take_pwstr)
                    .unwrap_or_default()
            };
            let size = unsafe { v.GetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE).unwrap_or(0) };
            let ctype: GUID = unsafe { v.GetGuidValue(&WPD_OBJECT_CONTENT_TYPE).unwrap_or_default() };
            let dir = ctype == WPD_CONTENT_TYPE_FOLDER || ctype == WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT;
            let p = if path.is_empty() { name.clone() } else { format!("{path}/{name}") };
            out.push((child.clone(), Entry { path: p.clone(), size, dir, modified: modified_of(&v) }));
            // Tile and routing trees hold tens of thousands of objects; stop at a fixed depth.
            // Routing graphs hold ~50k objects: stop early. Tile folders are listed one level deeper (x/y files).
            let probe = std::env::var("WAHOO_PROBE").ok().filter(|x| !x.is_empty());
            let depth = p.matches('/').count();
            let deep = p.contains("PaxHeader")
                || (p.starts_with("maps/tiles") && depth >= 4)
                || (p.starts_with("maps/routing/") && depth >= 3);
            let deep = deep && !probe.map(|x| p.starts_with(&x)).unwrap_or(false);
            if dir && !deep {
                self.walk(&child, &p, out);
            }
        }
    }
}

fn str_prop(v: &IPortableDeviceValues, k: &PROPERTYKEY) -> String {
    unsafe { v.GetStringValue(k).map(take_pwstr).unwrap_or_default() }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Reads a single file by device path (e.g. "system_update_elemnt/18001/BoltApp").
pub fn read_path(pnp_id: &str, path: &str) -> Result<Vec<u8>> {
    let _g = ComGuard::new();
    let s = Session::open(pnp_id)?;
    let not_found = || Error::from_hresult(windows::core::HRESULT(-2147024894));
    let mut cur = s.children("DEVICE").into_iter().next().ok_or_else(not_found)?;
    for part in path.split('/').filter(|p| !p.is_empty()) {
        cur = s.child_names(&cur).into_iter().find(|(_, n)| n.eq_ignore_ascii_case(part)).map(|(id, _)| id).ok_or_else(not_found)?;
    }
    s.read(&cur)
}

/// WPD reports the modification time as an OLE automation date (days since 1899-12-30).
fn modified_of(v: &IPortableDeviceValues) -> Option<i64> {
    let pv = unsafe { v.GetValue(&WPD_OBJECT_DATE_MODIFIED).ok()? };
    let days = unsafe { std::ptr::read_unaligned((&pv as *const _ as *const u8).add(8) as *const f64) };
    (days > 1.0).then(|| ((days - 25569.0) * 86400.0).round() as i64)
}

impl Session {
    fn child_names(&self, parent: &str) -> Vec<(String, String)> {
        self.children(parent)
            .into_iter()
            .filter_map(|id| {
                let v = self.values(&id, &[WPD_OBJECT_NAME, WPD_OBJECT_ORIGINAL_FILE_NAME])?;
                let n = unsafe { v.GetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME).or_else(|_| v.GetStringValue(&WPD_OBJECT_NAME)).map(take_pwstr).ok()? };
                Some((id, n))
            })
            .collect()
    }
}

/// Writes `data` as `file_name` into a top-level folder of the first storage.
/// Returns the final file name (a numeric suffix is added when it already exists).
pub fn upload(pnp_id: &str, folder: &str, file_name: &str, data: &[u8]) -> Result<String> {
    let _g = ComGuard::new();
    let s = Session::open_with(pnp_id, 0xC000_0000)?;
    let storage = s.children("DEVICE").into_iter().next().ok_or_else(|| Error::from_hresult(windows::core::HRESULT(-2147024894)))?;
    let parent = s
        .child_names(&storage)
        .into_iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(folder))
        .map(|(id, _)| id)
        .ok_or_else(|| Error::from_hresult(windows::core::HRESULT(-2147024894)))?;
    let existing: Vec<String> = s.child_names(&parent).into_iter().map(|(_, n)| n.to_lowercase()).collect();
    let (stem, ext) = file_name.rsplit_once('.').unwrap_or((file_name, ""));
    let mut name = file_name.to_string();
    let mut n = 1;
    while existing.contains(&name.to_lowercase()) {
        n += 1;
        name = if ext.is_empty() { format!("{stem}-{n}") } else { format!("{stem}-{n}.{ext}") };
    }
    unsafe {
        let v: IPortableDeviceValues = CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
        let (pw, nw) = (wide(&parent), wide(&name));
        v.SetStringValue(&WPD_OBJECT_PARENT_ID, PCWSTR(pw.as_ptr()))?;
        v.SetStringValue(&WPD_OBJECT_NAME, PCWSTR(nw.as_ptr()))?;
        v.SetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME, PCWSTR(nw.as_ptr()))?;
        v.SetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE, data.len() as u64)?;
        v.SetGuidValue(&WPD_OBJECT_CONTENT_TYPE, &WPD_CONTENT_TYPE_UNSPECIFIED)?;
        v.SetGuidValue(&WPD_OBJECT_FORMAT, &WPD_OBJECT_FORMAT_UNSPECIFIED)?;
        let mut stream: Option<IStream> = None;
        let mut optimal = 0u32;
        s.content.CreateObjectWithPropertiesAndData(&v, &mut stream, &mut optimal, std::ptr::null_mut())?;
        let stream = stream.ok_or_else(|| Error::from_hresult(windows::core::HRESULT(-1)))?;
        let chunk = optimal.max(65536) as usize;
        for part in data.chunks(chunk) {
            let mut written = 0u32;
            let hr = stream.Write(part.as_ptr() as _, part.len() as u32, Some(&mut written));
            if hr.is_err() {
                return Err(Error::from_hresult(hr));
            }
        }
        stream.Commit(STGC_DEFAULT)?;
    }
    Ok(name)
}

/// Level-2 Valhalla graph tiles (0.25 degree) around Utrecht: lat 52.0-52.25, lon 4.75-5.5 (ids 818659-818661).
const SAMPLE_ROUTING_TILES: &[&str] = &[
    "maps/routing/2/000/818/659.gph.lzma",
    "maps/routing/2/000/818/660.gph.lzma",
    "maps/routing/2/000/818/661.gph.lzma",
];

/// Walks the whole device tree and mirrors small/needed files into `cache`.
pub fn sync(cache: &Path, pnp_id: &str, friendly: &str) -> Result<(Manifest, PathBuf)> {
    let _g = ComGuard::new();
    let s = Session::open(pnp_id)?;
    let t0 = std::time::Instant::now();

    let mut device = DeviceProps { pnp_id: pnp_id.into(), name: friendly.into(), ..Default::default() };
    if let Some(v) = s.values(
        "DEVICE",
        &[WPD_DEVICE_MANUFACTURER, WPD_DEVICE_MODEL, WPD_DEVICE_SERIAL_NUMBER, WPD_DEVICE_FIRMWARE_VERSION, WPD_DEVICE_POWER_LEVEL],
    ) {
        device.manufacturer = str_prop(&v, &WPD_DEVICE_MANUFACTURER);
        device.model = str_prop(&v, &WPD_DEVICE_MODEL);
        device.serial = str_prop(&v, &WPD_DEVICE_SERIAL_NUMBER);
        device.firmware = str_prop(&v, &WPD_DEVICE_FIRMWARE_VERSION);
        device.power_level = unsafe { v.GetUnsignedIntegerValue(&WPD_DEVICE_POWER_LEVEL).ok() };
    }

    let dir = cache.join(sanitize(friendly));
    let files = dir.join("files");
    let mut storages = Vec::new();
    let mut flat: Vec<(String, Entry)> = Vec::new();
    for st in s.children("DEVICE") {
        if let Some(v) = s.values(
            &st,
            &[WPD_OBJECT_NAME, WPD_STORAGE_CAPACITY, WPD_STORAGE_FREE_SPACE_IN_BYTES, WPD_STORAGE_FILE_SYSTEM_TYPE, WPD_STORAGE_SERIAL_NUMBER],
        ) {
            storages.push(Storage {
                name: str_prop(&v, &WPD_OBJECT_NAME),
                capacity: unsafe { v.GetUnsignedLargeIntegerValue(&WPD_STORAGE_CAPACITY).unwrap_or(0) },
                free: unsafe { v.GetUnsignedLargeIntegerValue(&WPD_STORAGE_FREE_SPACE_IN_BYTES).unwrap_or(0) },
                fs: str_prop(&v, &WPD_STORAGE_FILE_SYSTEM_TYPE),
                serial: str_prop(&v, &WPD_STORAGE_SERIAL_NUMBER),
            });
        }
        for top in s.children(&st) {
            let k = [WPD_OBJECT_NAME, WPD_OBJECT_ORIGINAL_FILE_NAME, WPD_OBJECT_SIZE, WPD_OBJECT_CONTENT_TYPE, WPD_OBJECT_DATE_MODIFIED];
            let Some(v) = s.values(&top, &k) else { continue };
            let name = unsafe {
                v.GetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME).or_else(|_| v.GetStringValue(&WPD_OBJECT_NAME)).map(take_pwstr).unwrap_or_default()
            };
            let size = unsafe { v.GetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE).unwrap_or(0) };
            let ctype: GUID = unsafe { v.GetGuidValue(&WPD_OBJECT_CONTENT_TYPE).unwrap_or_default() };
            let dir = ctype == WPD_CONTENT_TYPE_FOLDER || ctype == WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT;
            flat.push((top.clone(), Entry { path: name.clone(), size, dir, modified: modified_of(&v) }));
            if dir {
                s.walk(&top, &name, &mut flat);
            }
        }
    }
    let walk_ms = t0.elapsed().as_millis();

    let wanted = |e: &Entry| {
        !e.dir
            && (e.path.starts_with("exports/")
                || e.path.starts_with("routes/")
                || e.path.starts_with("plans/")
                || e.path == "factory/system_info.txt"
                || e.path == "factory/serial_number.txt"
                || e.path == "maps/tilepack-versions.json"
                || e.path == "maps/routing/routing-config.json"
                || e.path == "maps/routing_md5"
                || e.path.starts_with("maps/temp/")
                || (e.path.starts_with("logs/") && e.size < 2_000_000))
    };
    let mut copied = Vec::new();
    for (id, e) in &flat {
        if !wanted(e) {
            continue;
        }
        let local = files.join(&e.path);
        if local.metadata().map(|m| m.len() == e.size && e.size > 0).unwrap_or(false) {
            continue;
        }
        if let Ok(bytes) = s.read(id) {
            if let Some(p) = local.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            if std::fs::write(&local, bytes).is_ok() {
                copied.push(e.path.clone());
            }
        }
    }

    // The routing tree is not walked, so fetch a few sample graph tiles around Utrecht by path.
    for path in SAMPLE_ROUTING_TILES {
        let local = files.join(path);
        if local.metadata().map(|m| m.len() > 0).unwrap_or(false) {
            continue;
        }
        let Some(storage) = s.children("DEVICE").into_iter().next() else { break };
        let mut cur = storage;
        let mut found = true;
        for part in path.split('/') {
            match s.child_names(&cur).into_iter().find(|(_, n)| n.eq_ignore_ascii_case(part)) {
                Some((id, _)) => cur = id,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if !found {
            continue;
        }
        if let Ok(bytes) = s.read(&cur) {
            if let Some(p) = local.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            if std::fs::write(&local, bytes).is_ok() {
                copied.push((*path).to_string());
            }
        }
    }

    let manifest = Manifest {
        device,
        storages,
        entries: flat.into_iter().map(|(_, e)| e).collect(),
        synced_at: chrono::Local::now().to_rfc3339(),
        walk_ms,
        copied,
    };
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec(&manifest).unwrap()).ok();
    Ok((manifest, dir))
}


