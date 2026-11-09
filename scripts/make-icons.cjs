// Generates app icons (PNG, ICO, ICNS) from an inline SVG.
// Usage: node scripts/make-icons.cjs   (needs `sharp`; or use `npx tauri icon app-icon.png`)
const fs = require('fs');
const path = require('path');
const sharp = require('sharp');

const out = path.join(__dirname, '..', 'src-tauri', 'icons');
fs.mkdirSync(out, { recursive: true });

// Ring gauge at ~30% with a pace tick, on a dark rounded square.
const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <defs><linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#1f2a37"/><stop offset="1" stop-color="#0d1117"/></linearGradient></defs>
  <rect x="64" y="64" width="896" height="896" rx="200" fill="url(#bg)"/>
  <circle cx="512" cy="512" r="270" fill="none" stroke="#3a4552" stroke-width="110"/>
  <path d="M512 242 A270 270 0 0 1 768 595" fill="none" stroke="#2ea043" stroke-width="110" stroke-linecap="butt"/>
  <line x1="512" y1="512" x2="512" y2="512" />
  <g transform="rotate(130 512 512)"><rect x="494" y="170" width="36" height="210" rx="10" fill="#c9d1d9"/></g>
</svg>`;

(async () => {
  const base = sharp(Buffer.from(svg));
  const png = (s) => base.clone().resize(s, s).png().toBuffer();
  const sizes = { '32x32.png': 32, '128x128.png': 128, '128x128@2x.png': 256, 'icon.png': 512 };
  for (const [name, s] of Object.entries(sizes)) fs.writeFileSync(path.join(out, name), await png(s));

  // ICO with embedded PNGs.
  const icoSizes = [16, 24, 32, 48, 64, 128, 256];
  const imgs = await Promise.all(icoSizes.map(png));
  const header = Buffer.alloc(6 + 16 * imgs.length);
  header.writeUInt16LE(0, 0); header.writeUInt16LE(1, 2); header.writeUInt16LE(imgs.length, 4);
  let offset = header.length;
  imgs.forEach((buf, i) => {
    const s = icoSizes[i], o = 6 + 16 * i;
    header.writeUInt8(s >= 256 ? 0 : s, o); header.writeUInt8(s >= 256 ? 0 : s, o + 1);
    header.writeUInt8(0, o + 2); header.writeUInt8(0, o + 3);
    header.writeUInt16LE(1, o + 4); header.writeUInt16LE(32, o + 6);
    header.writeUInt32LE(buf.length, o + 8); header.writeUInt32LE(offset, o + 12);
    offset += buf.length;
  });
  fs.writeFileSync(path.join(out, 'icon.ico'), Buffer.concat([header, ...imgs]));

  // ICNS with PNG payloads.
  const icns = [['icp4', 16], ['icp5', 32], ['ic11', 32], ['ic12', 64], ['ic07', 128], ['ic13', 256], ['ic08', 256], ['ic14', 512], ['ic09', 512], ['ic10', 1024]];
  const chunks = [];
  for (const [type, s] of icns) {
    const data = await png(s);
    const h = Buffer.alloc(8); h.write(type, 0, 'ascii'); h.writeUInt32BE(data.length + 8, 4);
    chunks.push(h, data);
  }
  const body = Buffer.concat(chunks);
  const h = Buffer.alloc(8); h.write('icns', 0, 'ascii'); h.writeUInt32BE(body.length + 8, 4);
  fs.writeFileSync(path.join(out, 'icon.icns'), Buffer.concat([h, body]));
  console.log('icons written to', out);
})();
