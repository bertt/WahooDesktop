# Wahoo ELEMNT maps: source, format and custom maps

What is known about how an ELEMNT stores and receives maps. Most of this is reverse-read from the device, not from official documentation. Back to the [README](../README.md).

## Contents

- [On-device layout](#on-device-layout)
- [File formats](#file-formats)
- [Tiles and versions](#tiles-and-versions)
- [Where Wahoo downloads maps from](#where-wahoo-downloads-maps-from)
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
| `maps/routing/<level>/<aaa>/<bbb>/<ccc>.gph.lzma` | Valhalla routing graph tiles (level 2 = local roads). |
| `maps/routing/routing-config.json` | Valhalla service configuration (loki / meili / thor). |

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

- Versions are kept per Wahoo **tile pack** in `tilepack-versions.json`, not per tile. A tile you replaced yourself is therefore not re-downloaded unless Wahoo publishes a newer version of the pack it belongs to (the official packs date from 2019).
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
curl -i https://cdn.wahooligan.com/wahoo-maps                        # 403 Forbidden (AmazonS3)
curl -i https://cdn.wahooligan.com/wahoo-maps/tilepack-versions.json # 403 Forbidden
```

**Not yet known:** the object layout under the CDN base and how the device/app is authorised. The bucket rejects anonymous requests, so plain HTTPS downloads do not work without credentials (probably signed URLs obtained from the Wahoo cloud or phone app). The firmware only holds the names `tile_packs`, `tile_pack_id` and `tilepack-versions.json`. Capturing the traffic of the ELEMNT app during a map download (e.g. with mitmproxy) would reveal the real requests; this project does not do that.

The device itself fetches tile packs over WiFi, triggered by the phone app over Bluetooth LE.

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

- **Does:** read `maps/` over USB (MTP), list installed tiles with their coverage on a map, sizes, dates, versions and the map-source details above.
- **Does not (yet):** download maps from Wahoo (credentials are required) or build maps from OpenStreetMap.
- **Possible future work:** since tiles are ordinary files under `maps/tiles/8/`, writing a locally built tile over USB/MTP should be feasible, following the procedure above. This is untested with Wahoo Desktop; try it only with a backup of the original tile, and expect that the device may need a restart to pick up changes.

## Sources

- Sommet, `tools/wahoo_maps.py`: <https://github.com/skinnie/sommet/blob/main/tools/wahoo_maps.py>
- wahooMapsCreator: <https://github.com/treee111/wahooMapsCreator>
- Mapsforge binary map format: <https://github.com/mapsforge/mapsforge/blob/master/docs/Specification-Binary-Map-File.md>
- Own inspection of a BOLT2 (firmware 180.01) and the staged BoltApp APK.
