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

| Level | Name | Tile size | Columns x rows | North-south | East-west at 52 N (Utrecht) |
|---|---|---|---|---|---|
| 0 | highway | 4° | 90 x 45 | about 445 km | about 274 km |
| 1 | arterial | 1° | 360 x 180 | about 111 km | about 68 km |
| 2 | local | 0.25° | 1440 x 720 | about 28 km | about 17 km |

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

**Node info (32 bytes, from Valhalla 3.1.4 `nodeinfo.h`, decoded on all three tiles).** Four 8-byte words; bit positions count from the least significant bit:

| Word (offset) | Bits | Field | Notes |
|---|---|---|---|
| 0 (+0) | 0-21 | `lat_offset` | 1e-6 degree from the base latitude |
| | 22-25 | `lat_offset7` | extra 1e-7 digit |
| | 26-47 | `lon_offset` | 1e-6 degree from the base longitude |
| | 48-51 | `lon_offset7` | extra 1e-7 digit |
| | 52-63 | `access` | mode bit mask: 1 auto, 2 pedestrian, 4 bicycle, 8 truck, 16 emergency, 32 taxi, 64 bus, 128 HOV, 256 wheelchair, 512 moped, 1024 motorcycle |
| 1 (+8) | 0-20 | `edge_index` | index of the first outbound directed edge |
| | 21-27 | `edge_count` | outbound edges on this level |
| | 28-39 | `admin_index` | index into the tile's admin list (country, region) |
| | 40-48 | `timezone` | time zone index |
| | 49-52 | `intersection` | 0 regular, 1 false, 2 dead end, 3 fork (and others) |
| | 53-56 | `type` | node type: 0 street intersection, 1 gate, 2 bollard, 3 toll booth, 9 toll gantry, 12 elevator, among others |
| | 57-60 | `density` | relative road density 0-15: how dense the road network is around the node (0 rural, 15 dense urban); derived by Valhalla's graph builder and used by the costing models to tell urban from rural areas. In these tiles: 12-14 in central Utrecht (818660), 4-6 in the quieter 818661 |
| | 61, 62, 63 | `traffic_signal`, `mode_change`, `named` | flags |
| 2 (+16) | 0-20 | `transition_index` | 21 bits |
| | 21-23 | `transition_count` | 3 bits |
| | 24-39 | `local_driveability` | 2 bits for each of 8 local edges |
| | 40-42 | `local_edge_count` | stored minus 1 |
| | 43-46 | `drive_on_right`, `tagged_access`, `private_access`, `cash_only_toll` | flags |
| | 47-63 | spare | 0 in all nodes |
| 3 (+24) | 0-63 | `headings` | 8 local edges, 8 bits each, degrees x 255/359 |

There is no elevation field (see [Elevation data](#elevation-data)). Example nodes (first nodes of each tile and one from the middle; `ei` = edge index, `ec` = edge count):

| Tile | Node | Lat, lon | Access | ec / ei | Admin | Intersection | Type | Density | Headings (raw, local edges) |
|---|---|---|---|---|---|---|---|---|---|
| 818659 | 0 | 52.0807100, 4.8572866 | 2047 | 3 / 0 | 2 | regular | street | 6 | 184, 56, 121 |
| 818659 | 1 | 52.0774454, 4.8567883 | 2047 | 4 / 3 | 2 | regular | street | 5 | 242, 180, 116, 49 |
| 818659 | 21762 | 52.2388882, 4.7863334 | 2047 | 3 / 58270 | 1 | regular | street | 5 | 70, 200, 131 |
| 818660 | 0 | 52.0604793, 5.1115687 | 2047 | 3 / 0 | 2 | regular | street | 12 | 13, 207, 142 |
| 818660 | 1 | 52.0779972, 5.1154650 | 2047 | 3 / 3 | 2 | regular | street | 14 | 242, 149, 28 |
| 818660 | 58564 | 52.0660512, 5.1217001 | 2047 | 3 / 171323 | 2 | regular | street | 12 | 175, 75, 203 |
| 818661 | 0 | 52.0827994, 5.4181685 | 2047 | 3 / 0 | 3 | regular | street | 4 | 137, 9, 201 |
| 818661 | 1 | 52.0817915, 5.4045964 | 2047 | 4 / 3 | 3 | regular | street | 4 | 113, 175, 50, 244 |
| 818661 | 34759 | 52.0813581, 5.4132088 | 2047 | 1 / 104596 | 3 | dead end | street | 4 | 180 |

All sampled nodes have `drive_on_right` = 1, time zone index 292 and no transitions. Over a sample of about 20,000 nodes per tile: about 98% have access 2047 (all modes) and the rest are mostly 262 or 258 (pedestrian and bicycle, plus wheelchair for 262); about 64-78% are regular intersections, 12-20% false intersections and 9-16% dead ends; node type 0 (street intersection) covers about 96%, with a few gates, bollards and toll gantries; 0-7 nodes per tile have a traffic signal; edge counts are mostly 1-4. Nodes lie within their tile (818660: 52.0-52.25 N, 5.0-5.25 E).

**Directed edge info (48 bytes, from Valhalla 3.1.4 `directededge.h`, decoded on all three tiles).** Six 8-byte words, bit positions from the least significant bit. Only the fields read below are listed; the rest (restrictions, truck/predicted speeds, turn types, stop impact, shortcuts, local edge indexes) are not decoded:

| Word (offset) | Bits | Field | Notes |
|---|---|---|---|
| 0 (+0) | 0-45 | `endnode` | graph id of the end node: level (3 bits), tile id (22), node index (21) |
| | 54-60 | `opp_index` | index of the opposing edge at the end node |
| | 61 | `forward` | 1 if the edge-info shape runs in this edge's direction |
| | 62 | `leaves_tile` | end node is in another tile |
| 1 (+8) | 0-24 | `edgeinfo_offset` | offset into the edge-info section (shape, names, way id, mean elevation) |
| 2 (+16) | 0-7 | `speed` | km/h |
| | 40-45 | `use` | 0 road, 1 ramp, 3 track, 4 driveway, 10 living street, 11 service road, 20 cycleway, 25 footway, 27 path, among others |
| | 46-49 | `lanecount` | |
| | 54-56 | `classification` | 0 motorway, 1 trunk, 2 primary, 3 secondary, 4 tertiary, 5 unclassified, 6 residential, 7 service/other |
| | 57-59 | `surface` | 0 paved smooth, 1 paved, 2 paved rough, 3 compacted, 4 dirt, 5 gravel, 6 path, 7 impassable |
| | 60, 61 | `toll`, `roundabout` | flags |
| 3 (+24) | 0-11 | `forwardaccess` | mode mask, same bits as the node `access` |
| | 12-23 | `reverseaccess` | access against the edge direction |
| | 24-28, 29-33 | `max_up_slope`, `max_down_slope` | 5 bits each |
| | 39 | `bike_network` | |
| | 49, 50 | `tunnel`, `bridge` | flags |
| 4 (+32) | 32-55 | `length` | metres |
| | 56-59 | `weighted_grade` | 4 bits; 6 is level ground, higher is uphill, lower is downhill |
| | 60-63 | `curvature` | |
| 5 (+40) | 62, 63 | `named`, `link` | has names; ramp or turn channel |

Example edges (`mean elev` comes from the edge info, `name` from the text list; access masks: 2031 = all road and path modes except emergency, 262 = pedestrian, bicycle and wheelchair, 258 = pedestrian and wheelchair, 64 = bus):

| Tile | Edge | Name | Class / use | km/h | Length m | Surface | Access fwd / rev | Up / down slope | Weighted grade | Mean elev m | Opp edge | Forward |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 818659 | 0 | Vaartuigenlaan | residential / road | 30 | 20 | paved smooth | 2031 / 2031 | 2 / 0 | 7 | 0 | 1 | 0 |
| 818659 | 2 | Boterbloemweide | service / footway | 12 | 98 | compacted | 258 / 258 | 0 / 1 | 6 | -2 | 1 | 0 |
| 818660 | 0 | (none) | service / cycleway | 20 | 113 | paved smooth | 262 / 262 | 1 / 0 | 6 | 2 | 0 | 1 |
| 818660 | 106 | Tolsteegsingel | tertiary / road | 30 | 8 | paved smooth | 2031 / 2031 | 0 / 0 | 6 | 4 | 1 | 1 |
| 818660 | 689 | Van Zijstweg | service / road | 50 | 14 | paved smooth | 0 / 64 | 18 / 0 | 15 | 6 | 3 | 0 |
| 818660 | 848 | Krikkelaan | residential / road | 30 | 7 | paved smooth | 262 / 2031 | 0 / 0 | 6 | 4 | 0 | 0 |
| 818660 | 1378 | Ingenieur F E D Enschedeweg | unclassified / road | 60 | 9 | paved smooth | 2031 / 2031 | 0 / 0 | 6 | -4 | 1 | 0 |
| 818660 | 1643 | Lekstraat | unclassified / road | 30 | 40 | paved | 2031 / 2031 | 3 / 0 | 8 | 4 | 1 | 0 |
| 818660 | 1908 | Zinniastraat | residential / road | 30 | 19 | paved | 2031 / 2031 | 0 / 4 | 4 | 4 | 0 | 1 |
| 818661 | 0 | Koningin Emmastraat | residential / road | 30 | 19 | paved smooth | 2031 / 2031 | 0 / 2 | 5 | 2 | 1 | 0 |
| 818661 | 2 | Prins Clausstraat | service / footway | 25 | 42 | paved smooth | 258 / 258 | 1 / 0 | 7 | 2 | 1 | 1 |

Observations: the end node of edge 0 of tile 818659 is `2/818659/2868` (level 2, same tile, node 2868). A one-way street shows as different forward and reverse access (Krikkelaan: only pedestrians and bikes in the forward direction; Van Zijstweg: only buses against the edge direction). The weighted grade follows the slopes (grade 15 for an 18 up slope, 4 for a 4 down slope), and `forward` = 0 edges point to a shape stored in the opposite direction. Over a sample of about 30,000 edges per tile: 63-65% are `service_other` class, 18-20% residential and 6-9% unclassified; about 98% have 1 lane; paved smooth is the most common surface (49-53%); mean edge length is 48-63 m with a maximum of about 2.9 km; 0.2-0.5% of the edges end in another tile; and about half have a street name.

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
- **On the device:** the elevation is stored in the graph tiles, on the edges: a mean elevation per road segment (edge info) and slopes per directed edge. The nodes and shape points have no elevation. No separate terrain files (such as the AWS terrain tiles Valhalla can use to build or serve elevation) are on the device.
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
