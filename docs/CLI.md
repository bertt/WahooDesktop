# wahoo_cli

`wahoo_cli` is the command-line companion of Wahoo Desktop. It uses the same backend (USB/MTP reader, FIT parser and local cache) as the desktop app, which makes it handy for scripting, quick checks and bug reports. Windows only for now.

## Getting it

- **Installer:** `wahoo_cli.exe` is installed next to `wahoo-desktop.exe` (default `%LOCALAPPDATA%\Programs\Wahoo Desktop`). Add that folder to your `PATH` to call it from anywhere.
- **From source:** `cargo build --release`, then run `target\release\wahoo_cli.exe`. During development you can also use `cargo run --release --bin wahoo_cli -- <command>`.

## How it works

1. Connect the ELEMNT with a data cable and switch it on.
2. Run `wahoo_cli sync` once. This reads the device over USB and mirrors it to the local cache (`%LOCALAPPDATA%\WahooDesktop\cache`). The cache is shared with the desktop app.
3. All read commands (`info`, `battery`, `history`, ...) work from the cache, so they are instant and work without the device attached. Only `status`, `sync`, `add-route`, `pull` and `ls` talk to the device.

Do not run a sync from the CLI and the desktop app at the same time.

## Usage

```
wahoo_cli [--json] [--device <name>] <command>
```

Global flags:

| Flag | Meaning |
| --- | --- |
| `-j`, `--json` | Print raw JSON instead of formatted text (ideal for scripts and `jq`) |
| `-d`, `--device <name>` | Device cache key; defaults to the first synced device |
| `-h`, `--help` / `-V`, `--version` | Help and version |

Exit code is `0` on success and `1` on error (message on stderr).

## Commands

| Command | Talks to device | What it does |
| --- | :---: | --- |
| `status` | yes | Connection state: online, USB attached but device off / charge-only, or absent |
| `sync` | yes | Read the device over USB and refresh the local cache |
| `add-route <file>` | yes | Upload a `.fit` course, or a `.gpx` (converted to a FIT course), to `/routes` on the device |
| `pull <device path> <local file>` | yes | Copy one raw file from the device to your computer |
| `ls [prefix]` | yes | List raw device files, optionally filtered by path prefix |
| `info` | no | Model, firmware, hardware, serial and factory info |
| `firmware` | no | Checks the public Wahoo manifest: up to date, update available or newer (needs internet) |
| `battery` | no | Battery level, history and drain statistics |
| `storage` | no | Capacity and usage breakdown per folder |
| `maps` | no | Installed map tiles, regions, versions and warnings |
| `map-updates` | no | Checks Wahoo's public map CDN for newer map versions per tile pack (needs internet). `--tile 131/83` compares one installed tile with the newest version (size, last-modified, download URL) |
| `map-packs [--all]` | no | Lists the map pack tree (continent > country > region) of the newest map version, marking the packs installed on the device. Without `--all` only installed packs and their parents are shown (needs internet) |
| `map-download <pack> [--version <n>]` | no | Downloads all tiles of one pack from Wahoo's CDN to `%LOCALAPPDATA%\WahooDesktop\maps\<version>\tiles\8\<x>\<y>.map.lzma`. `<pack>` is an id, a name (`Netherlands`) or a full name (`"Europe > Netherlands"`). Existing tiles of the same size are skipped. This only downloads to your computer; nothing is written to the device yet (needs internet) |
| `routing` | no | Valhalla routing data: graph tile count per level and coverage (from the device's `maps/routing_md5` list, cached by `sync`), service config and leftovers |
| `sensors` | no | Sensor readings (temperature, altitude, ...) and sensors seen by the device |
| `routes` | no | Saved routes |
| `plans` | no | Planned workouts |
| `alerts` | no | Derived alerts and warnings, newest first |
| `history [-n <count>]` | no | Ride history, newest first (default 20) |
| `activity <id>` | no | Details of one activity; get the id from `history` |
| `stats` | no | Overall statistics: totals, per month/year, personal records |
| `debug` | no | Technical details: sync timings, system files, tile packs |

## Examples

```
wahoo_cli status
wahoo_cli sync
wahoo_cli battery
wahoo_cli history -n 5
wahoo_cli activity 2026-10-22-144634-ELEMNT BOLT F9F1-421-0
wahoo_cli --json stats > stats.json
wahoo_cli add-route my-ride.gpx
wahoo_cli ls maps/tiles/8/
wahoo_cli map-packs
wahoo_cli map-download Netherlands          # use case: update the Netherlands pack (about 170 MB)
wahoo_cli pull system_update_elemnt/18001/BoltApp BoltApp.apk
```

## Using wahoo_cli with AI agents and skills

The CLI is designed to be easy for coding agents (GitHub Copilot CLI, Claude Code, and similar) and for agent skills to call: it is non-interactive, returns a non-zero exit code on failure, and `--json` gives structured output that is the same data the desktop UI shows.

Guidelines for an agent or skill:

1. Run `wahoo_cli status --json` first. If the status is not online, ask the user to connect and switch on the ELEMNT instead of retrying.
2. Run `wahoo_cli sync` only when fresh data is needed. Every read command works from the local cache and is instant.
3. Always pass `--json` and parse the output rather than scraping the text format.
4. Treat `add-route` as a write action: confirm with the user first. Do not use `pull` or `ls` on large folders such as `maps/tiles` unless asked; they are slow and large.
5. Do not paste serial numbers or the output of `debug` into public places without the user's consent.

Example prompts that work well with an agent that has shell access:

- "Sync my Wahoo and tell me how much storage the maps use."
- "Show my last 10 rides as a table and flag any over 100 km."
- "Add `route.gpx` to my Wahoo."
- "Why is my battery draining faster this month? Use `wahoo_cli --json battery`."

A minimal skill or instruction file (for example `SKILL.md` or `AGENTS.md`) could look like this:

```markdown
---
name: wahoo-cli
description: Read a Wahoo ELEMNT over USB and answer questions about its battery, storage, maps, routes and rides using wahoo_cli.
---

- Check the connection: `wahoo_cli status --json`
- Refresh data when needed: `wahoo_cli sync`
- Query with JSON: `wahoo_cli --json battery | storage | maps | sensors | routes | plans | alerts | stats`
- Rides: `wahoo_cli --json history -n 20`, then `wahoo_cli --json activity "<id>"`
- Ask before running `wahoo_cli add-route <file>`.
```

## Tips

- Wrap an activity id that contains spaces in quotes.
- `--json` output is the same data the desktop UI uses, so it is a convenient way to attach diagnostics to an issue (`wahoo_cli --json debug`).
- `add-route` accepts up to about 10,000 track points per GPX file. The device may need a moment (or a restart) to show a new route.
- If `status` reports that USB is attached but the device is off, switch the ELEMNT on (not just charging) and try again.
- Downloading maps to the device is not possible over USB yet; `map-download` only saves tiles on your computer. See [Map data](../README.md#map-data-source-and-format) in the README.
