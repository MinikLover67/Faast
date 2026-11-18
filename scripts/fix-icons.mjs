// Repairs the 1px bright horizontal seam that runs across every Faast icon.
// The seam sits at ~54.7% of icon height (e.g. row 560 of the 1024px source).
// We replace only blue-background pixels in that band with an interpolated
// value from clean rows above/below, leaving the white arrow untouched.
import fs from 'node:fs';
import path from 'node:path';
import { PNG } from 'pngjs';

const ICON_DIR = 'src-tauri/icons';
const SEAM_RATIO = 560 / 1024;

function repairPixels(get, set, width, height) {
  const center = Math.round(height * SEAM_RATIO);
  const lo = Math.max(0, center - 2);
  const hi = Math.min(height - 1, center + 2);
  const above = Math.max(0, lo - 1);
  const below = Math.min(height - 1, hi + 1);
  let fixed = 0;

  for (let y = lo; y <= hi; y++) {
    const t = below === above ? 0 : (y - above) / (below - above);
    for (let x = 0; x < width; x++) {
      const a = get(above, x);
      const b = get(below, x);
      const cur = get(y, x);
      if (!a || !b || !cur) continue;
      // Only touch blue (background) pixels: white arrow pixels are neutral.
      if (cur[2] - cur[0] <= 60 || cur[2] - cur[1] <= 40) continue;
      const exp = [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
      ];
      const delta = cur[0] + cur[1] + cur[2] - (exp[0] + exp[1] + exp[2]);
      if (delta <= 12) continue;
      set(y, x, [
        Math.round(exp[0]),
        Math.round(exp[1]),
        Math.round(exp[2]),
        cur[3] ?? 255,
      ]);
      fixed++;
    }
  }
  return fixed;
}

function repairPng(file) {
  const png = PNG.sync.read(fs.readFileSync(file));
  const { width, height, data } = png;
  const get = (y, x) => {
    const i = (y * width + x) * 4;
    return [data[i], data[i + 1], data[i + 2], data[i + 3]];
  };
  const set = (y, x, [r, g, b, a]) => {
    const i = (y * width + x) * 4;
    data[i] = r;
    data[i + 1] = g;
    data[i + 2] = b;
    data[i + 3] = a;
  };
  const fixed = repairPixels(get, set, width, height);
  if (fixed) {
    fs.writeFileSync(file, PNG.sync.write(png));
    console.log(`  ${path.relative('.', file)}: fixed ${fixed} px`);
  }
  return fixed;
}

function walk(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    return e.isDirectory() ? walk(p) : [p];
  });
}

console.log('Repairing PNG icons...');
for (const file of walk(ICON_DIR).filter((f) => f.endsWith('.png'))) repairPng(file);

// ---- ICO (32bpp BMP entries, bottom-up rows) ----
console.log('Repairing icon.ico...');
{
  const file = path.join(ICON_DIR, 'icon.ico');
  const buf = fs.readFileSync(file);
  const n = buf.readUInt16LE(4);
  for (let i = 0; i < n; i++) {
    const off = 6 + i * 16;
    const w = buf[off] || 256;
    const h = buf[off + 1] || 256;
    const dataOff = buf.readUInt32LE(off + 12);
    const base = dataOff + 40; // BITMAPINFOHEADER
    const get = (y, x) => {
      const i = base + ((h - 1 - y) * w + x) * 4;
      return [buf[i + 2], buf[i + 1], buf[i], 255]; // BGRA -> RGBA
    };
    const set = (y, x, [r, g, b]) => {
      const i = base + ((h - 1 - y) * w + x) * 4;
      buf[i] = b;
      buf[i + 1] = g;
      buf[i + 2] = r;
    };
    const fixed = repairPixels(get, set, w, h);
    if (fixed) console.log(`  entry ${w}x${h}: fixed ${fixed} px`);
  }
  fs.writeFileSync(file, buf);
}

// ---- ICNS (PNG chunks) ----
console.log('Repairing icon.icns...');
{
  const file = path.join(ICON_DIR, 'icon.icns');
  const buf = fs.readFileSync(file);
  const out = [Buffer.alloc(8)];
  let p = 8;
  while (p + 8 <= buf.length) {
    const type = buf.subarray(p, p + 4).toString('latin1');
    const len = buf.readUInt32BE(p + 4);
    let payload = Buffer.from(buf.subarray(p + 8, p + 8 + len));
    if (payload.subarray(0, 8).toString('hex') === '89504e470d0a1a0a') {
      // ICNS entries here carry 8 stray bytes after IEND (the next chunk's
      // header). Trim to the real PNG stream before handing it to pngjs.
      let iend = -1;
      for (let i = 0; i + 12 <= payload.length; i++) {
        if (payload.subarray(i + 4, i + 8).toString('latin1') === 'IEND') {
          iend = i;
          break;
        }
      }
      if (iend >= 0) payload = payload.subarray(0, iend + 12);
      const png = PNG.sync.read(payload);
      const { width, height, data } = png;
      const get = (y, x) => {
        const i = (y * width + x) * 4;
        return [data[i], data[i + 1], data[i + 2], data[i + 3]];
      };
      const set = (y, x, [r, g, b, a]) => {
        const i = (y * width + x) * 4;
        data[i] = r;
        data[i + 1] = g;
        data[i + 2] = b;
        data[i + 3] = a;
      };
      const fixed = repairPixels(get, set, width, height);
      if (fixed) {
        payload = PNG.sync.write(png);
        console.log(`  ${type}: fixed ${fixed} px`);
      }
    }
    const hdr = Buffer.alloc(8);
    hdr.write(type, 0, 4, 'latin1');
    // ICNS length includes this 8-byte header (matches build-icon.mjs).
    hdr.writeUInt32BE(payload.length + 8, 4);
    out.push(hdr, payload);
    p += len;
  }
  const total = 8 + out.slice(1).reduce((n, b) => n + b.length, 0);
  out[0].write('icns', 0, 4, 'latin1');
  out[0].writeUInt32BE(total, 4);
  fs.writeFileSync(file, Buffer.concat(out));
}
console.log('Done.');
