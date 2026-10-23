# Wahoo ELEMNT routing data (Valhalla)

Reverse-engineered from an ELEMNT BOLT2 (firmware 180.01); not official documentation. For the drawing maps see [MAPS.md](MAPS.md).

## Contents

- [Overview](#overview)
- [On-device files](#on-device-files)
- [Tile ids and positions](#tile-ids-and-positions)
- [The checksum list `routing_md5`](#the-checksum-list-routing_md5)
- [Routing data per map pack (CDN)](#routing-data-per-map-pack-cdn)
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

`<aaa><bbb><ccc>` are the digits of the Valhalla tile id, grouped by three (`2/000/729/042` is id 729042). On level 2 the grid has 1440 columns:

```
row = id / 1440      lat = -90  + row * 0.25
col = id % 1440      lon = -180 + col * 0.25     (south-west corner of the tile)
```

Four times four level-2 tiles make one 1° cell; the Netherlands and Spain are complete (16 tiles per cell). Level 0 and 1 use the same scheme with 4° and 1° tiles, but are not drawn on the coverage map.

## The checksum list `routing_md5`

The ELEMNT uses it for integrity checks. On a BOLT2 it is 3.3 MB and lists 39,642 level-2 tiles spread over the whole world (3,598 one-degree cells). It is the device's own list: it shows which tiles are expected, not that every file is actually present.

## Routing data per map pack (CDN)

The pack list on Wahoo's CDN (`<version>/mappack-gzip.json.gz`, see [MAPS.md](MAPS.md#tile-pack-list-format)) has per pack:

- `routing_size`: bytes of routing data (v23: 20.3 GB for the whole world, 24.7 GB with elevation)
- `routing_coordinates`: the routing tile ids of the pack

The Routing tab shows, for each installed pack, its installed version and date, the number of routing tiles and the routing size. The size is that of the newest map version, not of the installed one.

## What Wahoo Desktop does

- **Routing tab:** tile count per level, coverage map (from `routing_md5`), service config, leftovers, and a table of installed packs with version, routing tiles and routing size.
- **CLI:** `wahoo_cli routing` (cache only, see [CLI.md](CLI.md)).
- **Sync:** copies `maps/routing_md5` to the cache. A sync only copies it when its size changed, so run a sync once after updating the app.
- It only **reads** routing data. It does not download or write graph tiles.

## Limitations

- **Graph tiles are not walked.** Listing the roughly 40,000 graph files over MTP is too slow, so the sync stops at `maps/routing/<level>/<aaa>` (depth 3). Tile counts and coverage come from `routing_md5`. `WAHOO_PROBE=maps/routing` makes the sync walk deeper, slowly.
- Without a cached `routing_md5` the Routing tab shows a warning to run a sync.
- Graph tile contents (`.gph.lzma`) are not parsed.

Code: `views::routing` / `views::routing_listing` in `src/views.rs`, pack routing data in `src/mapupdates.rs` (`routing_bytes`, `routing_tiles`), the Routing view in `static/app.js`.
