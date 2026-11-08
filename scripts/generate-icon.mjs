// Generates a 1024x1024 PNG icon for ZZMM without any image dependencies.
// Run with `node scripts/generate-icon.mjs`, then `pnpm tauri icon <output>`.
import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

const SIZE = 1024;
const SS = 3; // supersampling factor for smooth edges
const OUT = "src-tauri/app-icon.png";

function roundedRect(x, y, x0, y0, x1, y1, r) {
  if (x < x0 || x > x1 || y < y0 || y > y1) return false;
  const cx = Math.min(Math.max(x, x0 + r), x1 - r);
  const cy = Math.min(Math.max(y, y0 + r), y1 - r);
  const dx = x - cx;
  const dy = y - cy;
  return dx * dx + dy * dy <= r * r;
}

function segmentDistance(px, py, x0, y0, x1, y1) {
  const dx = x1 - x0;
  const dy = y1 - y0;
  const len2 = dx * dx + dy * dy;
  let t = len2 === 0 ? 0 : ((px - x0) * dx + (py - y0) * dy) / len2;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(px - (x0 + t * dx), py - (y0 + t * dy));
}

function inZ(x, y) {
  // Top bar
  if (x >= 0.245 && x <= 0.755 && y >= 0.265 && y <= 0.37) return true;
  // Bottom bar
  if (x >= 0.245 && x <= 0.755 && y >= 0.63 && y <= 0.735) return true;
  // Diagonal stroke
  return segmentDistance(x, y, 0.7, 0.36, 0.3, 0.64) <= 0.068;
}

function sample(x, y) {
  if (!roundedRect(x, y, 0.03, 0.03, 0.97, 0.97, 0.2)) {
    return [0, 0, 0, 0];
  }

  // Dark asphalt gradient.
  let r = Math.round(12 - 7 * y);
  let g = Math.round(15 - 9 * y);
  let b = Math.round(24 - 14 * y);

  // Thin acid frame.
  const frameOuter = roundedRect(x, y, 0.055, 0.055, 0.945, 0.945, 0.175);
  const frameInner = roundedRect(x, y, 0.072, 0.072, 0.928, 0.928, 0.16);
  if (frameOuter && !frameInner) {
    r = 190;
    g = 255;
    b = 70;
  }

  // Hot-pink offset shadow, then the acid Z on top.
  if (inZ(x + 0.018, y - 0.022)) {
    r = 255;
    g = 31;
    b = 111;
  }
  if (inZ(x, y)) {
    r = 226;
    g = 255;
    b = 90;
  }

  return [r, g, b, 255];
}

const stride = SIZE * 4;
const raw = Buffer.alloc((stride + 1) * SIZE);

for (let py = 0; py < SIZE; py++) {
  raw[py * (stride + 1)] = 0; // PNG filter: none
  for (let px = 0; px < SIZE; px++) {
    let r = 0;
    let g = 0;
    let b = 0;
    let a = 0;

    for (let sy = 0; sy < SS; sy++) {
      for (let sx = 0; sx < SS; sx++) {
        const [sr, sg, sb, sa] = sample(
          (px + (sx + 1 / (SS + 1)) / SS) / SIZE,
          (py + (sy + 1 / (SS + 1)) / SS) / SIZE,
        );
        r += sr * sa;
        g += sg * sa;
        b += sb * sa;
        a += sa;
      }
    }

    const offset = py * (stride + 1) + 1 + px * 4;
    if (a === 0) {
      raw[offset] = 0;
      raw[offset + 1] = 0;
      raw[offset + 2] = 0;
      raw[offset + 3] = 0;
    } else {
      raw[offset] = Math.round(r / a);
      raw[offset + 1] = Math.round(g / a);
      raw[offset + 2] = Math.round(b / a);
      raw[offset + 3] = Math.round(a / (SS * SS));
    }
  }
}

const crcTable = new Uint32Array(256);
for (let n = 0; n < 256; n++) {
  let c = n;
  for (let k = 0; k < 8; k++) {
    c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  }
  crcTable[n] = c >>> 0;
}

function crc32(buffer) {
  let c = 0xffffffff;
  for (const byte of buffer) {
    c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  }
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const typeBuffer = Buffer.from(type, "ascii");
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([typeBuffer, data])));
  return Buffer.concat([length, typeBuffer, data, crc]);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA
ihdr[10] = 0;
ihdr[11] = 0;
ihdr[12] = 0;

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, png);
console.log(`Wrote ${OUT} (${SIZE}x${SIZE}, ${Math.round(png.length / 1024)} KiB)`);
