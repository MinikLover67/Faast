// Builds src-tauri/icons/icon.ico with classic BMP (DIB) entries — the most
// compatible format for GDI+, the Windows shell, NSIS and resource editors.
// Run after scripts/generate-icons.ps1 creates the PNG sizes.
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PNG } from 'pngjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const dir = join(root, 'src-tauri', 'icons');

function dibEntry(name, size) {
  const img = PNG.sync.read(readFileSync(join(dir, name)));
  if (img.width !== size || img.height !== size) {
    throw new Error(`${name} is ${img.width}x${img.height}, expected ${size}`);
  }

  const header = Buffer.alloc(40);
  header.writeUInt32LE(40, 0); // biSize
  header.writeInt32LE(size, 4); // biWidth
  header.writeInt32LE(size * 2, 8); // biHeight (XOR + AND)
  header.writeUInt16LE(1, 12); // biPlanes
  header.writeUInt16LE(32, 14); // biBitCount
  header.writeUInt32LE(0, 16); // BI_RGB
  header.writeUInt32LE(size * size * 4, 20); // biSizeImage

  // BGRA, bottom-up
  const pixels = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y++) {
    const sy = size - 1 - y;
    for (let x = 0; x < size; x++) {
      const si = (sy * size + x) * 4;
      const di = (y * size + x) * 4;
      pixels[di] = img.data[si + 2];
      pixels[di + 1] = img.data[si + 1];
      pixels[di + 2] = img.data[si];
      pixels[di + 3] = img.data[si + 3];
    }
  }

  // AND mask: all zero = fully opaque (alpha channel takes precedence)
  const maskRow = Math.ceil(size / 32) * 4;
  const mask = Buffer.alloc(maskRow * size);

  return Buffer.concat([header, pixels, mask]);
}

const sizes = [
  [16, '16x16.png'],
  [24, '24x24.png'],
  [32, '32x32.png'],
  [48, '48x48.png'],
  [64, '64x64.png'],
  [128, '128x128.png'],
  [256, '256x256.png'],
];

const entries = sizes.map(([size, file]) => ({ size, data: dibEntry(file, size) }));

const header = Buffer.alloc(6);
header.writeUInt16LE(0, 0);
header.writeUInt16LE(1, 2);
header.writeUInt16LE(entries.length, 4);

const dirBuf = Buffer.alloc(16 * entries.length);
let offset = 6 + dirBuf.length;
entries.forEach((e, i) => {
  const b = i * 16;
  dirBuf.writeUInt8(e.size >= 256 ? 0 : e.size, b);
  dirBuf.writeUInt8(e.size >= 256 ? 0 : e.size, b + 1);
  dirBuf.writeUInt8(0, b + 2);
  dirBuf.writeUInt8(0, b + 3);
  dirBuf.writeUInt16LE(1, b + 4);
  dirBuf.writeUInt16LE(32, b + 6);
  dirBuf.writeUInt32LE(e.data.length, b + 8);
  dirBuf.writeUInt32LE(offset, b + 12);
  offset += e.data.length;
});

const ico = Buffer.concat([header, dirBuf, ...entries.map((e) => e.data)]);
writeFileSync(join(dir, 'icon.ico'), ico);
console.log('wrote DIB icon.ico', ico.length, 'bytes,', entries.length, 'sizes');

// ICNS (PNG entries are native to ICNS)
function icnsChunk(type, file) {
  const data = readFileSync(join(dir, file));
  const head = Buffer.alloc(8);
  head.write(type, 0, 4, 'ascii');
  head.writeUInt32BE(8 + data.length, 4);
  return Buffer.concat([head, data]);
}

const chunks = [
  icnsChunk('ic07', '128x128.png'),
  icnsChunk('ic08', '256x256.png'),
  icnsChunk('ic09', 'icon.png'),
  icnsChunk('ic10', 'icon-source.png'),
];
const total = 8 + chunks.reduce((n, c) => n + c.length, 0);
const icnsHead = Buffer.alloc(8);
icnsHead.write('icns', 0, 4, 'ascii');
icnsHead.writeUInt32BE(total, 4);
writeFileSync(join(dir, 'icon.icns'), Buffer.concat([icnsHead, ...chunks]));
console.log('wrote icon.icns', total, 'bytes');
