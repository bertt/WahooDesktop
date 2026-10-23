# Wahoo ELEMNT routing data (Valhalla)

Reverse-engineered from an ELEMNT BOLT2 (firmware 180.01); not official documentation. For the drawing maps see [MAPS.md](MAPS.md).

## Contents

- [Overview](#overview)
- [On-device files](#on-device-files)
- [Tile ids and positions](#tile-ids-and-positions)
- [The checksum list `routing_md5`](#the-checksum-list-routing_md5)
- [Graph tile format](#graph-tile-format)
- [Routing data per map pack (CDN)](#routing-data-per-map-pack-cdn)
- [Elevation data](#elevation-data)
- [What Wahoo Desktop does](#what-wahoo-desktop-does)
- [Limitations](#limitations)

## Overview

Besides the Mapsforge tiles used for drawing, the ELEMNT carries routing data for calculating and re-calculating routes on the device. It uses an embedded [Valhalla](https://valhalla.github.io/valhalla/) engine. Routing data is separate from the drawing tiles: installed packs have both, with their own sizes and files.

## On-device files

| Path | Purpose |
|---|---|
| `maps/routing/<level>/<aaa>/<bbb>/<ccc>.gph.lzma` | LZMA-compressed Valhalla graph tile |
| `maps/routing/routing-config.json` | Valhalla configuration: `httpd`, `loki` (locating), `meili` (map matching), `thor` (routing); the local service listens on `tcp://*:8002` |
| `maps/routing_md5` | One line per graph tile: `<md5>  /data/media/0/maps/routing/2/000/729/042.gph.lzma` |
| `maps/routing/temp` | Can hold leftovers; reported as a warning |

Valhalla hierarchy levels: 0 = highways (4° tiles), 1 = arterial roads (1° tiles), 2 = local roads (0.25° tiles). On a BOLT2 only level 2 is present.

## Tile ids and positions

Valhalla uses its own regular latitude/longitude grid (not Web Mercator or XYZ), with three levels:

| Level | Name | Tile size | Columns x rows | North-south | East-west at the equator | East-west at 52 N (Utrecht) |
|---|---|---|---|---|---|---|
| 0 | highway | 4° | 90 x 45 | about 445 km | about 445 km | about 274 km |
| 1 | arterial | 1° | 360 x 180 | about 111 km | about 111 km | about 68 km |
| 2 | local | 0.25° | 1440 x 720 | about 28 km | about 28 km | about 17 km |

The origin is the south-west corner (-180°, -90°). Tiles are numbered row by row, west to east, then south to north, so `id = row * columns + col` with `row = floor((lat + 90) / size)` and `col = floor((lon + 180) / size)`. Tiles are fixed in degrees, so they get narrower towards the poles. The level 0 and 1 sizes come from Valhalla's tile hierarchy; only level 2 is present on the BOLT2.

Example: Utrecht (52.09 N, 5.12 E) gives `row = 568`, `col = 740`, `id = 568 * 1440 + 740 = 818660`, which is tile `2/000/818/660`.

A full graph id (64 bits) is `level` (3 bits), `tile id` (22 bits) and the node or edge index within the tile (21 bits). The tile's own graph id is in the first 8 bytes of its header, and the header also holds the south-west corner (see [Graph tile format](#graph-tile-format)); the north-east corner is that plus the tile size.

`<aaa><bbb><ccc>` in the path are the digits of the tile id, grouped by three (`2/000/729/042` is id 729042). On level 2 the grid has 1440 columns:

```
row = id / 1440      lat = -90  + row * 0.25
col = id % 1440      lon = -180 + col * 0.25     (south-west corner of the tile)
```

Four times four level-2 tiles make one 1° cell; the Netherlands and Spain are complete (16 tiles per cell). Level 0 and 1 use the same scheme with 4° and 1° tiles, but are not drawn on the coverage map.

## The checksum list `routing_md5`

The ELEMNT uses it for integrity checks. On a BOLT2 it is 3.3 MB and lists 39,642 level-2 tiles spread over the whole world (3,598 one-degree cells). It is the device's own list: it shows which tiles are expected, not that every file is actually present.

## Graph tile format

Checked on three level-2 tiles around Utrecht (`2/000/818/659`, `660`, `661`, fetched by the sync).

**Container.** A `.gph.lzma` file is an LZMA-alone stream (header `5d 00 00 80 00`, unknown-size marker, 8 MB dictionary) around a raw Valhalla graph tile. Compressed sizes are 2.6, 7.7 and 5.0 MB; decompressed 9.5, 27.5 and 16.9 MB (about 3.6x). The tile is Valhalla's native binary format (little-endian structs, memory-mapped by the router), version string `3.1.4`. The layout follows `valhalla/baldr/graphtileheader.h` of that release.

**Header (272 bytes).** Verified fields:

| Offset | Size | Field | Example (tile 818660) |
|---|---|---|---|
| 0 | 8 | bit field: graph id (level 3 bits, tile id 22 bits), quality values (density, name, speed, exit; 4 bits each, all 0 here), `has_elevation` (bit 62), `has_ext_directededge` (bit 63) | level 2, id 818660, `has_elevation` = 1 |
| 8 | 8 | base longitude, latitude (two `float32`): south-west corner | 5.0, 52.0 |
| 16 | 16 | version string | `3.1.4` |
| 32 | 8 | dataset id (OSM changeset) | 137495824 (same in all three tiles) |
| 40 | 8 | bit field: node count (21 bits), directed edge count (21), predicted speed count (21) | 117,128 nodes, 325,463 edges, 0 predicted speeds |
| 48 | 4 | transition count (22 bits) | 0 |
| 52 | 4 | turn lane count (21 bits) | 1,549 |
| 96, 100, 104, 108 | 4 each | byte offsets: complex restrictions forward, reverse, edge info, text list | |
| 112 | 4 | creation date, days since 2014-01-01 | 3501 (about 2023-08) |
| 116 | 100 | 25 `uint32` offsets: end of each bin in the 5x5 edge-bin grid (row-major, cumulative) | |
| 216, 220, 224 | 4 each | offsets: lane connectivity, predicted speeds, tile size | tile size = decompressed file size |

Other header words (transit and restriction counts) are not decoded.

**Body.** Fixed-size records follow the header back to back:

| Section | Record | Notes |
|---|---|---|
| Nodes | 32 bytes, from offset 272 | Intersections. The first 8 bytes hold the latitude and longitude as offsets from the base corner (22 bits each, 1e-6 degree, plus a 4-bit 1e-7 refinement). Decoding the first nodes gives coordinates inside the tile (for example 52.0605 N, 5.1116 E in tile 818660) |
| Node transitions | 8 bytes | None in these tiles |
| Directed edges | 48 bytes | One per travel direction of a road segment: speed, road class, use, surface, access, grade and length, plus the end node id. The end of this section is `272 + 32 * nodes + 8 * transitions + 48 * edges` |
| Other fixed records | various | Access restrictions, signs, admin areas and the edge bins (graph ids of the edges that touch each of the 25 grid cells, 8 bytes each) |
| Complex restrictions | variable | Starts at the forward-restriction offset |
| Edge info | variable | Starts at the edge-info offset: per road segment the shape (encoded polyline) and name indexes |
| Text list | variable | Starts at the text-list offset: NUL-terminated strings (street names, refs) |
| Lane connectivity | variable | Starts at the lane-connectivity offset, up to the end of the tile |

Observed sizes: tile 818659 has 43,524 nodes and 109,870 edges (about 2.5 edges per node), 818660 has 117,128 and 325,463, 818661 has 69,518 and 198,560. Tile 818660 covers the city of Utrecht and is the largest. The edge section is about 56-57% of each tile.

Not verified: the exact record layout of the other fixed sections, and the decoding of the other directed edge bit fields.

**Edge info (verified by `routing-export`).** A directed edge's second 8-byte word holds (low 25 bits) the offset into the edge-info section. Both directions of a road segment point to the same edge info, so a tile has about half as many segments as directed edges (tile 818660: 325,463 directed edges, 163,036 segments). Each edge-info record is:

| Bytes | Content |
|---|---|
| 4 | OSM way id (low 32 bits) |
| 4 | mean elevation (12 bits, 2 m steps, `value * 2 - 500` metres), bike network (4), speed limit (8), way id bits 32-39 (8) |
| 4 | name count (4 bits), encoded shape size (16), way id bits 40-47 (8), extra way id bytes (2) |
| 4 x names | name info: offset into the text list (24 bits), flags (route number, tagged) |
| shape size | the shape: zig-zag varints (7 bits per byte, high bit = continues), `lat, lon` deltas in 1e-6 degree, starting from 0,0 |

Street names are NUL-terminated strings at `text-list offset + name offset`. The decoded shapes start at the node coordinates and stay within the tile, and the mean elevations fit the Utrecht area (-10 to +34 m), which confirms the layout and the elevation encoding. The tiles contain all modes (cars, bicycles and pedestrians): for example 106,408 of 163,036 segments in tile 818660 are `service_other` (cycleways, footways, service roads). `routing-export` writes these as GeoJSON lines with `road_class`, `use`, `surface`, `speed_kph`, `length_m`, `mean_elevation_m`, slopes and the OSM way id. Valhalla's [`directededge.h`](https://github.com/valhalla/valhalla/blob/3.1.4/valhalla/baldr/directededge.h), [`nodeinfo.h`](https://github.com/valhalla/valhalla/blob/3.1.4/valhalla/baldr/nodeinfo.h) and [`graphtile.h`](https://github.com/valhalla/valhalla/blob/3.1.4/valhalla/baldr/graphtile.h) are the reference. Wahoo's build may differ in details.

## Routing data per map pack (CDN)

The pack list on Wahoo's CDN (`<version>/mappack-gzip.json.gz`, see [MAPS.md](MAPS.md#tile-pack-list-format)) has per pack:

- `routing_size`: bytes of routing data (v23: 20.3 GB for the whole world, 24.7 GB with elevation)
- `routing_coordinates`: the routing tile ids of the pack

The Routing tab shows, for each installed pack, its installed version and date, the number of routing tiles and the routing size. The size is that of the newest map version, not of the installed one.

## Elevation data

Elevation (terrain) data is a separate, optional layer in Wahoo's pack list. Verified against the v23 pack list:

- **Sizes:** the top level has `routing_size` and `routing_elevation_size`, and each pack has `routing_size` and `routing_elevation_size`. `routing_elevation_size` is the routing data *including* elevation, so it is larger than `routing_size`. Example, Netherlands: routing 162,952,479 bytes, routing with elevation 184,762,737 bytes (about 13% more). World total: 20.3 GB versus 24.7 GB.
- **Terrain:** there are also `terrain_size` (top level and per pack) and `terrain_tiles` in each tile set, next to `tile_coordinates` and `routing_coordinates`. `enhanced_size` (Netherlands: 288,400,460 bytes) is a third, larger variant that Wahoo's app offers; what it contains is not documented.
- **Is it installed on the device?** The tile headers say yes: all three sampled Utrecht tiles have the `has_elevation` flag set (see [Graph tile format](#graph-tile-format)). `maps/routing_md5` lists only level-2 `.gph.lzma` graph tiles and no separate elevation files, so the elevation data appears to be embedded in the graph tiles themselves (edge slopes). This is based on three sampled tiles; the flag was read from the header, the slope values were not decoded.
- **Can it be installed?** Unknown. The URL layout of elevation or terrain files on the CDN has not been found: guessed paths such as `routing_elevation/2/<aaa>/<bbb>/<ccc>.gph.lzma` and `elevation/...` return HTTP 403. The `routing_coordinates` ids in the pack list (for example 6503146 for the Netherlands) also do not follow the id scheme above, so the id-to-path mapping for pack downloads is still open. Wahoo Desktop therefore neither downloads nor installs elevation data. To find out, capture what the Wahoo iPhone app requests when downloading a pack with elevation enabled.

## What Wahoo Desktop does

- **Routing tab:** tile count per level, coverage map (from `routing_md5`), service config, leftovers, and a table of installed packs with version, routing tiles and routing size.
- **CLI:** `wahoo_cli routing` (cache only, see [CLI.md](CLI.md)) and `wahoo_cli routing-export <tile>`, which converts a graph tile to GeoJSON for QGIS or geojson.io (see [Graph tile format](#graph-tile-format)).
- **Sync:** copies `maps/routing_md5` to the cache. A sync only copies it when its size changed, so run a sync once after updating the app. It also fetches three sample level-2 graph tiles around Utrecht (`2/000/818/659`-`661`) by path, when present on the device.
- It only **reads** routing data. It does not download or write graph tiles.

## Limitations

- **Graph tiles are not walked.** Listing the roughly 40,000 graph files over MTP is too slow, so the sync stops at `maps/routing/<level>/<aaa>` (depth 3). Tile counts and coverage come from `routing_md5`. `WAHOO_PROBE=maps/routing` makes the sync walk deeper, slowly.
- Without a cached `routing_md5` the Routing tab shows a warning to run a sync.
- Graph tile contents are only partly decoded: `routing-export` reads the header, directed edges and edge info, not turn restrictions, signs, admin areas, lane connectivity or the node graph (see [Graph tile format](#graph-tile-format)).

Code: `views::routing` / `views::routing_listing` in `src/views.rs`, pack routing data in `src/mapupdates.rs` (`routing_bytes`, `routing_tiles`), the Routing view in `static/app.js`.
