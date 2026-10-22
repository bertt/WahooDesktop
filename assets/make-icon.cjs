// Regenerates assets/icon.ico and icon.png from icon.svg.
// Usage (in a folder with `sharp` and `png-to-ico` installed): node make-icon.cjs <assets dir>
const sharp = require("sharp");
const pngToIco = (m=>m.default||m)(require("png-to-ico"));
const fs = require("fs");
const path = require("path");

(async () => {
  const dir = process.argv[2];
  const svg = fs.readFileSync(path.join(dir, "icon.svg"));
  const sizes = [16, 24, 32, 48, 64, 128, 256];
  const bufs = [];
  for (const s of sizes) bufs.push(await sharp(svg, { density: 384 }).resize(s, s).png().toBuffer());
  fs.writeFileSync(path.join(dir, "icon.png"), bufs[bufs.length - 1]);
  fs.writeFileSync(path.join(dir, "icon.ico"), await pngToIco(bufs));
})();
