# AGENTS.md

## Project

Wahoo Desktop: a local Rust app (Windows only, v0.1) that reads a Wahoo ELEMNT (tested on BOLT2) over USB/MTP and shows firmware, battery, storage, maps, sensors, routes, workouts, alerts, ride history and statistics. No cloud account needed; device files are mirrored to `%LOCALAPPDATA%\WahooDesktop\cache`. Only two features use the internet: the firmware and map update checks (they must fail gracefully offline). The map basemap (OpenFreeMap) also needs internet.

## Layout

- `src/main.rs`, `src/main_window.rs`: GUI binary `wahoo-desktop` (tao + wry/WebView2 native window; no open port).
- `src/bin/wahoo_cli.rs`: CLI binary `wahoo_cli` (clap; global flags `--json`, `--device`). Docs in `docs/CLI.md`.
- `src/lib.rs` (crate `wahoo_desktop`): shared library.
  - `mtp/` (`mod.rs`, `wpd.rs`): device access via the Windows WPD COM API.
  - `fit.rs`: FIT parsing (fitparser). `course.rs`: GPX/FIT route handling.
  - `firmware.rs`, `mapupdates.rs`: update checks against Wahoo's public manifest/CDN.
  - `app.rs`, `views.rs`, `server.rs`: app state, view models, and the internal axum server that serves the UI.
- `static/` (`index.html`, `app.js`, `style.css`): frontend UI.
- `docs/`: GitHub Pages site (`index.html`), `CLI.md`, `MAPS.md` (reverse-engineered map format), `ROUTING.md` (Valhalla routing data; keep in sync with the Routing tab and `wahoo_cli routing`).
- `installer/wahoo-desktop.iss`: Inno Setup installer. `.github/workflows/release.yml`: builds and attaches the installer on release.
- `assets/`: icon sources (`icon.svg`; `make-icon.cjs` regenerates `icon.ico`/`icon.png`). `tools/geojson.js`: helper script.

## Build, run, test

```
cargo build --release
target\release\wahoo-desktop.exe
cargo test
```

Installer: `ISCC /DAppVersion=0.1.3 installer\wahoo-desktop.iss` (output in `dist\`). The release workflow stamps the version from the git tag into `Cargo.toml`.

CLI for other platforms: `cargo build --release --no-default-features --bin wahoo_cli --target <triple>` (the default `gui` feature pulls in tao/wry and is Windows-only in practice; `main_window` is gated on it). The release workflow builds Windows installers for x64 and arm64 (`/DArch=x64|arm64 /DBinDir=<target dir>` for ISCC; output `WahooDesktop-Setup-<version>-<arch>.exe`) and CLI-only packages for macOS (`.pkg` via pkgbuild) and Linux (`.deb`/`.rpm` via nfpm) on x64 and arm64, attached to the release. Device access (`mtp/wpd.rs`) is Windows-only; other platforms use the `mtp/unsupported.rs` stub, so the CLI only has cache and internet commands.

## Map knowledge (details in `docs/MAPS.md`; reverse-engineered, not official)

- On device: `maps/tiles/8/<x>/<y>.map.lzma` (zoom-8 slippy tiles, always z=8), a zero-byte version marker `<y>.map.lzma.<version>` next to each, `maps/tilepack-versions.json` (`[{"<pack id>": <version>}]`, versions are per tile pack, not per tile), `maps/tiles_md5`, `maps/routing_md5`, `maps/temp/z8x<x>y<y>.map` (decompressed render copy; leftovers can be deleted), `maps/routing/<level>/.../*.gph.lzma` (Valhalla graph) and `maps/routing/routing-config.json`.
- Formats: `.map.lzma` is an LZMA-alone stream (header `5d 00 00 00 04`, unknown-size marker, 64 MB dictionary) holding a Mapsforge binary map; tiles range from ~0.3 kB (ocean) to ~45 MB, typically 2-15 MB.
- Tile maths: `n=256; x=floor((lon+180)/360*n); y=floor((1-asinh(tan(lat_rad))/pi)/2*n)`.
- Routing: `maps/routing/<level>/<aaa>/<bbb>/<ccc>.gph.lzma` Valhalla graph tiles are not walked by the sync (~40,000 files); `maps/routing_md5` (`<md5>  <path>` per line) lists them and is cached and parsed for the Routing tab. Tile id = digits of `<aaa><bbb><ccc>`; level 2 (0.25° tiles, 1440 columns): `row=id/1440`, `col=id%1440`, `lat=-90+row*0.25`, `lon=-180+col*0.25`. The pack list gives `routing_size`/`routing_coordinates` per pack. Details: `docs/ROUTING.md`.
- CDN `https://cdn.wahooligan.com/wahoo-maps` is not listable, but objects are public: `<version>/mappack-gzip.json.gz` (pack list) and `<version>/tiles/8/<x>/<y>.map.lzma`. Device tiles are byte-identical to the CDN tiles of the same version.
- Map versions are whole-world releases (known: 1-9, 12, 17, 18, 20, 21, 23); treat numbers, not `created_date`, as ordering. Newest is found by probing the next 25 versions after the newest installed one; keep CDN requests to a few dozen at most.
- Pack list JSON: `version`, `created_date`, `tile_packs` nested via `sub_packs`; each pack has `id`, `name`, `tile_sets` (`zoom`, `tile_coordinates` `[x, y]`). Pack ids in `tilepack-versions.json` resolve through it.
- Update check: pack level compares installed version vs newest; tile level does a `HEAD` and compares size with the device file. Tile dates in the UI come from file modification times (indicative only).
- Firmware manifests: `https://bolt.wahoofitness.com/boltapp/version.json-<model>` (`bolt`, `bolt2`, `roam`, `roam2`, `ace`).
- Installing/removing a pack (implemented in `packinstall.rs`, GUI Maps tab, `install-pack`/`remove-pack`): plan first, then tile by tile (download+validate, replace or write new tile, read back, delete `maps/temp` copy, set marker), stop at the first error, restore the previous tile on a failed replace, never remove tiles shared with another installed pack, and only after all tiles succeeded patch `maps/tilepack-versions.json` (local backup in `%LOCALAPPDATA%\WahooDesktop\maps\backup`). `tiles_md5` and routing data are not touched. Use USB, not Bluetooth (too slow).
- `tools/geojson.js [out.geojson] [versions]` (Node 18+) builds a GeoJSON of the tile grid with CDN links; output is git-ignored (`*.geojson`).

## CLI (`wahoo_cli`, details in `docs/CLI.md`)

- Usage: `wahoo_cli [--json|-j] [--device|-d <name>] <command>`. Exit code 0 on success, 1 on error (message on stderr). Run it with `cargo run --release --bin wahoo_cli -- <command>` during development.
- Flow: connect the ELEMNT, run `sync` once (mirrors the device to `%LOCALAPPDATA%\WahooDesktop\cache`, shared with the desktop app); read commands work from the cache, offline and instantly. Never sync from the CLI and the desktop app at the same time.
- Talk to the device: `status`, `sync`, `add-route <file.gpx|file.fit>` (write action), `pull <device path> <local file>`, `ls [prefix]`.
- Cache-only: `info`, `battery`, `storage`, `maps`, `routing`, `sensors`, `routes`, `plans`, `alerts`, `history [-n <count>]` (default 20), `activity <id>` (id from `history`; quote ids with spaces), `stats`, `debug`.
- Need internet: `firmware` (manifest check), `map-updates [--tile x/y]`, `map-packs [--all]` (pack tree, installed marked) and `map-download <pack|id> [--version n]` (saves a pack's tiles to `%LOCALAPPDATA%\WahooDesktop\maps\<version>`; local only, not written to the device; `map-download` is a large download, confirm with the user first). Write actions to the device (dry run without `--apply`; always show the plan and confirm with the user first; one pack at a time): `install-pack <pack|id> --version <n> [--apply]` (new pack or switch version, up or down) and `remove-pack <pack|id> [--apply]`; they re-sync the cache afterwards and the device must be restarted. `clean-maps [--apply]` deletes leftover files in `maps/temp` (dry run without `--apply`).
- Guidance for agents: start with `wahoo_cli status --json`; if not online, ask the user to connect/switch on the device instead of retrying. Sync only when fresh data is needed. Always use `--json`. Confirm with the user before `add-route`. Avoid `pull`/`ls` on large folders such as `maps/tiles`. Do not share serial numbers or `debug` output publicly.
- `add-route` accepts up to ~10,000 GPX track points; the device may need a moment or a restart to show the route.

## Conventions and notes

- Rust edition 2024. Keep the GUI and CLI sharing logic through the library crate.
- Windows-only code relies on the `windows` crate (WPD); keep platform-specific code inside `mtp/`.
- The app is read-only towards the device, except adding routes (`add-route`, drag and drop .gpx/.fit) and installing, switching or removing map packs/tiles (`install-pack`, `remove-pack`, `clean-maps`, `install-tile`, Maps tab), which always show a plan and ask for confirmation.
- Map and device folder layout (`maps/tiles/8/<x>/<y>.map.lzma`, `routes/`, `plans/`, `exports/`) is reverse-engineered; see `docs/MAPS.md`.
- When changing CLI commands or features, update `README.md` and `docs/CLI.md`.
- Only the ELEMNT BOLT2 (firmware 180.01) is verified; other models are untested.
