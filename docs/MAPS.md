# Wahoo ELEMNT maps: source, format and custom maps

What is known about how an ELEMNT stores and receives maps. Most of this is reverse-read from the device, not from official documentation. Back to the [README](../README.md).

## Contents

- [On-device layout](#on-device-layout)
- [File formats](#file-formats)
- [Tiles and versions](#tiles-and-versions)
- [Where Wahoo downloads maps from](#where-wahoo-downloads-maps-from) (CDN layout, map versions, update check, GeoJSON of all tiles)
- [Routing data (Valhalla)](#routing-data-valhalla)
- [Building your own maps (community approach)](#building-your-own-maps-community-approach)
- [What Wahoo Desktop does and does not do](#what-wahoo-desktop-does-and-does-not-do)
- [Sources](#sources)

## On-device layout

| Path | Meaning |
| --- | --- |
| `maps/tiles/<z>/<x>/<y>.map.lzma` | One map tile. `z` is always `8` (slippy-map numbering, about 156 km wide at the equator, roughly 150 x 110 km in the Netherlands). |
| `maps/tiles/<z>/<x>/<y>.map.lzma.<version>` | Zero-byte version marker next to the tile (e.g. `.12`, `.18`). |
| `maps/tilepack-versions.json` | `[{"<tile pack id>": <version>}, ...]` |
| `maps/tiles_md5`, `maps/routing_md5` | Checksum lists used for integrity checks. |
| `maps/temp/z8x<x>y<y>.map` | Decompressed copy the device actually renders from. It is removed when a tile is replaced so the new tile is used; leftovers can be deleted. |
| `maps/routing/<level>/<aaa>/<bbb>/<ccc>.gph.lzma` | Valhalla routing graph tiles (level 0 = highways, 1 = arterial roads, 2 = local roads). See [Routing data (Valhalla)](#routing-data-valhalla). |
| `maps/routing/routing-config.json` | Valhalla service configuration (loki / meili / thor). |
| `maps/routing_md5` | `<md5>  <path>` per graph tile; the device's list of its routing files (about 3.3 MB, roughly 40,000 lines). |

## File formats

- `*.map.lzma` is an LZMA-alone stream (header `5d 00 00 00 04` followed by an unknown-size marker `ff ff ff ff ff ff ff ff`, 64 MB dictionary). Decompressed it is a [Mapsforge binary map](https://github.com/mapsforge/mapsforge/blob/master/docs/Specification-Binary-Map-File.md); the header starts with `mapsforge binary OSM`. The firmware also references `mapsforge.org/renderTheme`.
- `*.gph.lzma` is an LZMA-compressed Valhalla graph tile.
- Tiles are about 0.3 kB (almost empty ocean) up to roughly 45 MB; a typical land tile is 2-15 MB.

### Tile maths

Zoom-8 slippy tile for a coordinate:

```
n = 2^8 = 256
x = floor((lon + 180) / 360 * n)
y = floor((1 - asinh(tan(lat_rad)) / pi) / 2 * n)
```

A route is covered by every tile it touches. Adding a margin (e.g. 10 km around the track) avoids blank edges when you ride near a tile border.

## Tiles and versions

- Versions are kept per Wahoo **tile pack** in `tilepack-versions.json` (`[{"<pack id>": <map version>}]`), not per tile. Newer map versions exist on the CDN (see [Map versions](#map-versions)); the ELEMNT app marks packs below the newest version as stale.
- The version number in the marker file (`<y>.map.lzma.<version>`) should be kept when replacing a tile; the community tooling reuses the existing marker, or `18` if there is none.
- Keep Wahoo's original tile before overwriting it, so it can be restored.
- **When were the maps made?** The device exposes no build date, but every tile file has a modification time. The Maps tab shows, per map version, the date range of its tiles (e.g. `v18: 3000 (Mar 2025 – Jun 2025)`) and the date per tile in the coverage map popup. These dates come from the files' timestamps (tile creation or install time), so treat them as indicative.

## Where Wahoo downloads maps from

The firmware package staged on the device (`system_update_elemnt/<build>/BoltApp`, an Android APK; fetch it with `wahoo_cli pull`) contains these endpoints:

| Purpose | URL |
| --- | --- |
| Map CDN base | `https://cdn.wahooligan.com/wahoo-maps` |
| Firmware manifest (BOLT2) | `https://bolt.wahoofitness.com/boltapp/version.json-bolt2` (also `-bolt`, `-roam`, `-roam2`, `-ace`) |

```
curl -i https://bolt.wahoofitness.com/boltapp/version.json-bolt2     # 200, JSON with std-version / std-url
```

The bucket root is not listable, but the objects below it **are readable without credentials** (plain `curl`, no login).

### CDN layout

| What | URL |
| --- | --- |
| Tile pack list of a map version (gzip JSON) | `https://cdn.wahooligan.com/wahoo-maps/<version>/mappack-gzip.json.gz` |
| One base-map tile (zoom 8) | `https://cdn.wahooligan.com/wahoo-maps/<version>/tiles/8/<x>/<y>.map.lzma` |

```
curl -O https://cdn.wahooligan.com/wahoo-maps/23/mappack-gzip.json.gz           # 200, pack list of v23 (~0.7 MB)
curl -O https://cdn.wahooligan.com/wahoo-maps/18/mappack-gzip.json.gz           # 200, pack list of v18
curl -I https://cdn.wahooligan.com/wahoo-maps/18/tiles/8/131/83.map.lzma        # 200, Content-Length, Last-Modified, ETag
curl -I https://cdn.wahooligan.com/wahoo-maps/23/tiles/8/131/83.map.lzma        # 200, newest build of the same tile
curl -O https://cdn.wahooligan.com/wahoo-maps/23/tiles/8/131/83.map.lzma        # 200, downloads the tile (~7.6 MB)
```

The tile on the device is the same file as on the CDN: tile 131/83 on a BOLT2 (version 18) is 6,200,765 bytes, exactly the `Content-Length` of `.../18/tiles/8/131/83.map.lzma`.

The ELEMNT app downloads the pack list as `<version>/mappack-gzip.json.gz` for the map version the device reports, and ships a copy of the newest list in its assets (`assets/maps/mappack-gzip.json`).

### Map versions

Versions are whole-world releases. Probing `<version>/mappack-gzip.json.gz` (October 2026) found these (the `created_date` is inside the JSON):

| Version | Created | Notes |
| --- | --- | --- |
| 1-4 | 2015-07-22 | |
| 5-6 | 2018-11-29 | |
| 7 | 2019-04-27 | |
| 8 | 2019-07-30 | |
| 9, 12 | 2019-08-02 | v12 is what older packs on a BOLT2 report |
| 17 | 2022-06-08 | |
| 18 | 2023-08-10 | bundled with app 1.77.5.2 |
| 20 | 2026-04-28 | |
| 21 | 2025-11-24 | |
| 23 | 2026-10-21 | newest found |

Other version numbers are not published. The dates of 20 and 21 are out of order as published; treat the numbers, not the dates, as the ordering.

Each tile can differ per version. Example, NL tile 131/83:

| Version | Size | Last-Modified |
| --- | --- | --- |
| 12 | 4.5 MB | 2021-01-30 |
| 17 | 5.9 MB | 2022-05-24 |
| 18 | 6.2 MB | 2023-08-10 |
| 20 | 6.7 MB | 2024-09-09 |
| 21 | 7.3 MB | 2025-11-24 |
| 23 | 7.6 MB | 2026-10-21 |

### Tile pack list format

`mappack-gzip.json` is an object with `version`, `created_date`, `size`, `routing_size`, `routing_elevation_size`, `enhanced_size` and `tile_packs`. Packs are nested (continent, country, region) through `sub_packs`. Each pack has `id`, `name`, `size`, `routing_size`, `routing_elevation_size`, `enhanced_size`, `hidden`, `allow_download_all` and `tile_sets`; a tile set has `zoom` (8), `tile_coordinates` (`[x, y]` pairs) and `routing_coordinates`. Version 23 holds 407 packs.

The ids in the device's `tilepack-versions.json` resolve through this list. On the tested BOLT2: 127 = Europe > Belgium, 156 = Europe > Netherlands, 166 = Europe > Spain, 251 = United States > Pennsylvania, and 372-384 = the Canadian provinces and territories (Alberta to Yukon).

### Is there an update for a tile or pack?

Yes: compare the version on the device with the newest version on the CDN.

- **Pack level:** `tilepack-versions.json` gives the installed version per pack; any pack below the newest published version has a newer build.
- **Tile level:** a `HEAD` request on `<newest>/tiles/8/<x>/<y>.map.lzma` returns the size and `Last-Modified`; if the size equals the file on the device, it is the same build.

Wahoo Desktop does this: the Maps tab has a **Map updates** card, and `wahoo_cli map-updates [--tile x/y]` prints the same information (needs internet). See [docs/CLI.md](CLI.md). The newest version is found by probing the next 25 version numbers after the newest installed one.

Caveats: the check shows that a newer build *exists*; whether the device would pick it up automatically is up to the ELEMNT app. A new version may be larger (v23 packs are about 70 % bigger than v12), so check free space first.

The device itself fetches tile packs over WiFi, triggered by the phone app over Bluetooth LE.

Do not hammer the CDN: the check issues a few dozen small requests at most.

### GeoJSON of all tiles with download links

`tools/geojson.js` builds a GeoJSON of the whole tile grid, one square polygon per zoom-8 tile, with the CDN download links as attributes. It downloads the pack lists of the chosen versions (6 small requests) and needs only Node 18+, not a device:

```
node tools/geojson.js wahoo-map-tiles.geojson            # versions 12,17,18,20,21,23 (default)
node tools/geojson.js tiles.geojson 18,23                # choose versions
```

The default output is about 29,000 features and 24 MB, so it is not committed (`*.geojson` is git-ignored). Attributes per tile:

| Attribute | Example |
| --- | --- |
| `x`, `y`, `zoom` | `131`, `83`, `8` |
| `latest_version`, `versions` | `23`, `12,17,18,20,21,23` |
| `packs` | `Europe > Netherlands` (packs of the latest version that contain the tile) |
| `url_v<version>` | `https://cdn.wahooligan.com/wahoo-maps/18/tiles/8/131/83.map.lzma` |
| `url_latest` | same as the `url_v` of the latest version |

A `url_v<version>` is only present when that version's pack list contains the tile; the links themselves are not individually checked. Open the file in QGIS, geojson.io or any GIS tool and click a square to get its links.

### Additional findings from the community

- **Firmware manifests:** a community gist ([Intyre](https://gist.github.com/Intyre/2c0a8e337671ed6f523950ef08e3ca3f)) describes `bolt.wahoofitness.com/boltapp/` version files; these cover firmware only (see the firmware check in the app).

## Routing data (Valhalla)

Besides the Mapsforge tiles used for drawing, the ELEMNT carries routing data for calculating and re-calculating routes on the device. It uses an embedded [Valhalla](https://valhalla.github.io/valhalla/) engine, configured in `maps/routing/routing-config.json` (services `httpd`, `loki` for locating, `meili` for map matching, `thor` for routing; the local service listens on `tcp://*:8002`).

- **Graph tiles:** `maps/routing/<level>/<aaa>/<bbb>/<ccc>.gph.lzma`, each an LZMA-compressed Valhalla graph tile. The levels are Valhalla's hierarchy: 0 = highways (4° tiles), 1 = arterial roads (1° tiles), 2 = local roads (0.25° tiles). On a BOLT2 only level 2 is present.
- **Tile id and position:** `<aaa><bbb><ccc>` are the digits of the Valhalla tile id, grouped by three. On level 2 the grid has 1440 columns, so `row = id / 1440`, `col = id % 1440`, `lat = -90 + row * 0.25`, `lon = -180 + col * 0.25` (the south-west corner of the tile).
- **Checksum list:** `maps/routing_md5` has one line per graph tile: `<md5>  /data/media/0/maps/routing/2/000/729/042.gph.lzma`. The ELEMNT uses it for integrity checks. On a BOLT2 (firmware 180.01) it lists 39,642 level-2 tiles spread over the whole world; the Netherlands and Spain are complete (16 tiles per 1° cell).
- **Not walked by the sync:** listing the roughly 40,000 graph files over MTP is too slow, so the sync stops at `maps/routing/<level>/<aaa>`. Wahoo Desktop instead copies `maps/routing_md5` to its cache and derives tile counts and coverage from it. This is the device's own list, so it shows which tiles are expected, not a verification that every file is present (`WAHOO_PROBE=maps/routing` makes the sync walk deeper, slowly).
- **Per pack on the CDN:** the pack list has `routing_size` and `routing_coordinates` (tile ids) per pack, shown in the Routing tab for the installed packs. For v23 the world total is 20.3 GB of routing data (24.7 GB with elevation).
- **Temp files:** `maps/routing/temp` can hold leftovers; they are reported as a warning.

Wahoo Desktop shows this in the **Routing** tab and with `wahoo_cli routing` (see [docs/CLI.md](CLI.md)). It only reads routing data; it does not download or write graph tiles.

## Building your own maps (community approach)

Wahoo's servers are not required. The tile format is plain Mapsforge, so tiles can be generated from OpenStreetMap data and copied to the device. The [Sommet `wahoo_maps.py`](https://github.com/skinnie/sommet/blob/main/tools/wahoo_maps.py) tool documents a working pipeline (observed on an ELEMNT running BoltApp 1.77.10.1, over `adb`):

1. **Pick tiles.** Compute the zoom-8 tiles a GPX route needs (plus a margin).
2. **Build tiles** with [wahooMapsCreator](https://github.com/treee111/wahooMapsCreator) (GPL-3.0), an external tool:
   Geofabrik OSM extracts and land polygons, then `osmium` tag filtering, then `osmosis` with the Mapsforge map writer and Wahoo's tag mapping, then LZMA compression. Needs GDAL (`ogr2ogr`), `lzma`, Java and several GB of data.
3. **Install on the device:**
   - Copy the built `<y>.map.lzma` to a temporary file on the same filesystem.
   - Move Wahoo's original tile aside once (e.g. `maps/<backup>/8/<x>/<y>.map.lzma`) so it can be restored.
   - Rename the new tile into `maps/tiles/8/<x>/<y>.map.lzma` (a rename on one filesystem is instant).
   - Remove the old marker (`<y>.map.lzma.*`), recreate it with the previous version number, and delete `maps/temp/z8x<x>y<y>.map` so the device re-extracts the new tile.
   - Verify the file size on the device equals the local file.
4. **Restore** by moving the original tile back and deleting the temp copy again.

Notes from that work:

- A tile is 2-15 MB, which is too slow over the Bluetooth file channel; **use the USB cable**.
- The result is fresh OpenStreetMap data instead of the 2019 packs.
- Always keep a backup of the original tile.

## What Wahoo Desktop does and does not do

- **Does:** read `maps/` over USB (MTP); list installed tiles with their coverage, sizes, dates and versions; show the Valhalla routing graph tile counts and coverage from `maps/routing_md5` (Routing tab, `wahoo_cli routing`); resolve pack ids to names; check the public CDN for newer map versions per pack (Maps tab, `wahoo_cli map-updates`) and compare a single tile (`--tile x/y`); show the pack tree with installed packs marked and download a pack's tiles to `%LOCALAPPDATA%\WahooDesktop\maps\<version>\tiles\8\<x>\<y>.map.lzma` (Maps tab, `wahoo_cli map-packs`, `wahoo_cli map-download <pack>`).
- **Does not (yet):** write downloaded tiles to the device or build maps from OpenStreetMap.
- **Possible future work:** download a newer tile from the CDN and write it to `maps/tiles/8/<x>/<y>.map.lzma` over USB/MTP, following the install procedure above (keep the original and update the version marker). This is untested with Wahoo Desktop; try it only with a backup of the original tile, and expect that the device may need a restart to pick up changes.

## Sources

- Sommet, `tools/wahoo_maps.py`: <https://github.com/skinnie/sommet/blob/main/tools/wahoo_maps.py>
- wahooMapsCreator: <https://github.com/treee111/wahooMapsCreator>
- Mapsforge binary map format: <https://github.com/mapsforge/mapsforge/blob/master/docs/Specification-Binary-Map-File.md>
- Own inspection of a BOLT2 (firmware 180.01) and the staged BoltApp APK, plus `curl` checks of the CDN.
