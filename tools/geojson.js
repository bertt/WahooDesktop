// Builds a GeoJSON of the Wahoo map tile grid (zoom 8) with CDN download links.
// Usage: node tools/geojson.js [output.geojson] [version,version,...]
// Needs Node 18+ and internet access. It downloads <version>/mappack-gzip.json.gz
// from the public Wahoo map CDN for each version and does not need a device.
const fs = require("fs");
const zlib = require("zlib");

const CDN = "https://cdn.wahooligan.com/wahoo-maps";
const out = process.argv[2] || "wahoo-map-tiles.geojson";
const versions = (process.argv[3] || "12,17,18,20,21,23").split(",").map(Number);

async function mappack(v) {
  const res = await fetch(`${CDN}/${v}/mappack-gzip.json.gz`);
  if (!res.ok) throw new Error(`v${v}: HTTP ${res.status}`);
  return JSON.parse(zlib.gunzipSync(Buffer.from(await res.arrayBuffer())).toString("utf8"));
}

// "x/y" -> { version -> Set of pack paths }
const tiles = new Map();
function walk(p, path, v) {
  const name = [...path, p.name];
  for (const ts of p.tile_sets || []) {
    if (ts.zoom !== 8) continue;
    for (const [x, y] of ts.tile_coordinates) {
      const k = x + "/" + y;
      if (!tiles.has(k)) tiles.set(k, {});
      (tiles.get(k)[v] ||= new Set()).add(name.join(" > "));
    }
  }
  for (const s of p.sub_packs || []) walk(s, name, v);
}

const lon = (x) => (x / 256) * 360 - 180;
const lat = (y) => {
  const n = Math.PI - (2 * Math.PI * y) / 256;
  return (180 / Math.PI) * Math.atan(0.5 * (Math.exp(n) - Math.exp(-n)));
};
const r = (n) => Math.round(n * 1e5) / 1e5;

(async () => {
  for (const v of versions) {
    const j = await mappack(v);
    for (const p of j.tile_packs) walk(p, [], v);
    console.log(`v${v}: ${j.created_date}`);
  }

  const features = [...tiles.entries()]
    .map(([k, byVer]) => {
      const [x, y] = k.split("/").map(Number);
      const vs = Object.keys(byVer).map(Number).sort((a, b) => a - b);
      const latest = vs[vs.length - 1];
      const props = { x, y, zoom: 8, latest_version: latest, versions: vs.join(","), packs: [...byVer[latest]].join("; ") };
      for (const v of vs) props[`url_v${v}`] = `${CDN}/${v}/tiles/8/${x}/${y}.map.lzma`;
      props.url_latest = props[`url_v${latest}`];
      const [w, e, n, s] = [r(lon(x)), r(lon(x + 1)), r(lat(y)), r(lat(y + 1))];
      return {
        type: "Feature",
        properties: props,
        geometry: { type: "Polygon", coordinates: [[[w, n], [e, n], [e, s], [w, s], [w, n]]] },
      };
    })
    .sort((a, b) => a.properties.y - b.properties.y || a.properties.x - b.properties.x);

  fs.writeFileSync(out, JSON.stringify({ type: "FeatureCollection", features }));
  console.log(`${features.length} features, ${(fs.statSync(out).size / 1e6).toFixed(1)} MB -> ${out}`);
})().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
