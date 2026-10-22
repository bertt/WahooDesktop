<p align="center"><img src="assets/icon.png" width="96" alt="Wahoo Desktop icon"></p>

# Wahoo Desktop

## Introduction

Wahoo Desktop is a fast, local Rust app that reads a Wahoo ELEMNT (tested with an ELEMNT BOLT2) over **USB (MTP)** and shows everything it can find in a clean native window: firmware, battery, storage, installed maps, sensors, routes, workouts, alerts, ride history and statistics. No cloud account, no phone and no web server are needed; files are mirrored to a local cache so the UI is instant and works offline.

**Website:** https://bertt.github.io/WahooDesktop/

**Platform:** Windows only for now (native WPD COM API). macOS/Linux are planned. This is version 0.1.

### Screenshots

| Maps | Sensors | Statistics |
| --- | --- | --- |
| ![Maps](docs/screenshots/maps.png) | ![Sensors](docs/screenshots/sensors.png) | ![Statistics](docs/screenshots/statistics.png) |

## Supported devices

The app reads the device as a standard USB MTP storage device and parses its FIT files, so it should work with Wahoo ELEMNT head units that expose such storage. Only the BOLT2 has been verified.

| Device | Firmware | Status |
| --- | --- | --- |
| ELEMNT BOLT2 (BOLT F9F1) | 180.01 | Tested (all features) |
| ELEMNT BOLT (v1) | - | Untested, expected to work |
| ELEMNT ROAM / ROAM 2 | - | Untested, expected to work |
| ELEMNT ACE | - | Untested, expected to work |
| ELEMNT (original) | - | Untested; older firmware, folder layout may differ |

Notes: map, routing and storage details depend on the firmware's folder layout (`maps/`, `routes/`, `plans/`, `exports/`). Other devices may show missing sections. Reports with the **Technical** tab output are welcome. Wahoo KICKR/trainers and sensors are not supported.

## Installation

**Installer (recommended).** Download `WahooDesktop-Setup-<version>.exe` from the [Releases](../../releases) page and run it. It installs per user by default (no admin rights needed), adds a Start menu entry and optionally a desktop shortcut, and includes `wahoo_cli.exe`. Requires Windows 10/11 with the WebView2 runtime (preinstalled on current Windows).

**From source.** See [Build and run](#build-and-run).

**Using it.** Connect the ELEMNT with a data cable, switch it on, start Wahoo Desktop and click **Sync device**.

## Features

- Device list with online/offline state, battery and storage
- Overview, firmware/hardware, serial and factory info
- Firmware update check: compares the installed version with Wahoo's public manifest (`version.json-<model>`) and shows "up to date" or "update available" (also `wahoo_cli firmware`; the only feature that uses the internet, and it fails gracefully offline)
- Battery history, drain and estimated runtime
- Storage breakdown by folder
- Maps: installed tiles on a map (colour by size/version/marker), regions, versions, leftover temp files, routing config, warnings
- Sensors (from FIT files), saved routes (click to view on a map; drag and drop a .gpx/.fit to add one to the device), planned workouts, derived alerts
- History: sortable/filterable activity list; details with map, charts, laps, events and raw FIT message debug
- Statistics: totals, per month/year/weekday/hour, personal records
- Technical tab: raw MTP info, raw file browser
- Connection check: distinguishes "attached and ready" from "USB attached but device off / charge-only"

## Build and run

```
cargo build --release
target\release\wahoo-desktop.exe
```

The app opens its own native window (WebView2, no web server or open port). Maps use the OpenFreeMap basemap (needs internet).
Connect the ELEMNT with a data cable, switch it on and click **Sync device**. Files are mirrored to `%LOCALAPPDATA%\WahooDesktop\cache`, so the UI is instant afterwards and works offline.

## CLI

```
wahoo_cli status | sync | info | battery | storage | maps | sensors
wahoo_cli routes | add-route <file.gpx|file.fit> | plans | alerts | history -n 20 | activity <id> | stats | debug
wahoo_cli pull <device path> <local file>
wahoo_cli ls [prefix]
```

Global flags: `--json`, `--device <name>`.

Full documentation with all commands and examples: **[docs/CLI.md](docs/CLI.md)**.

## Map data: source and format

Summary (reverse-read from the device, not official documentation):

- Maps are stored as one Mapsforge map per zoom-8 slippy tile: `maps/tiles/8/<x>/<y>.map.lzma` (LZMA-compressed, 2-15 MB typical), with a zero-byte version marker next to it.
- Wahoo's CDN (`https://cdn.wahooligan.com/wahoo-maps`) returns 403 for anonymous requests, so maps cannot be downloaded over plain HTTPS.
- Community tools show that tiles can also be built from OpenStreetMap and copied to the device over USB.

Full details (layout, formats, tile maths, versions, building your own maps): **[docs/MAPS.md](docs/MAPS.md)**.

## Limitations

- **Downloading maps is not possible.** Over USB the device does not accept map downloads (it fetches tile packs over WiFi, triggered by the phone app over BLE), and the HTTPS source requires credentials (see above).
- Alerts are derived (low storage/battery, staged firmware, crash files, leftover maps); the device does not store alert definitions in readable form.
- Live battery/sensor data over Bluetooth is not implemented.

## Future ideas and extensions

- macOS/Linux backends (e.g. the `mtp-rs` crate)
- Bluetooth (btleplug) for live battery and sensor status
- Wahoo Cloud API integration
- Firmware update check is implemented; a future step is a one-click download of the update package
- GPX/FIT export of rides; deleting files from the device
- Segment analysis (FIT `segment_lap`), heatmap of all rides
- Sensor battery alerts, multi-device comparison
- Reclaiming space from `maps/temp`
- Ride selector for the sensor charts, decoding of manufacturer FIT messages
- Real map downloads once the CDN protocol is understood

## Building the installer and releases

The Windows setup is built with [Inno Setup](https://jrsoftware.org/isinfo.php) (`installer/wahoo-desktop.iss`). The GitHub Actions workflow `.github/workflows/release.yml` runs when a GitHub release is published: it takes the version from the tag (e.g. `v1.2.3`), stamps it into `Cargo.toml`, runs the tests, builds in release mode, builds `WahooDesktop-Setup-1.2.3.exe` and attaches it to the release. It can also be started manually (workflow_dispatch) to build an installer artifact without publishing.

To build locally: `cargo build --release`, then `ISCC /DAppVersion=0.1.0 installer\wahoo-desktop.iss` (output in `dist\`). The icon source is `assets/icon.svg`; `assets/make-icon.cjs` regenerates `icon.ico`/`icon.png`.


