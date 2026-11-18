import fs from 'node:fs';
import { PNG } from 'pngjs';

function seamDelta(data, width, height) {
  const y = Math.round(height * (560 / 1024));
  const x = Math.floor(width * 0.12);
  const at = (yy) => {
    const i = (yy * width + x) * 4;
    return data[i] + data[i + 1] + data[i + 2];
  };
  return at(y) - (at(y - 1) + at(y + 1)) / 2;
}

function check(label, png) {
  const d = seamDelta(png.data, png.width, png.height);
  console.log(`${label.padEnd(22)} ${png.width}x${png.height} seam delta = ${d.toFixed(1)} ${Math.abs(d) < 12 ? 'OK' : 'SEAM!'}`);
}

check('icon.png', PNG.sync.read(fs.readFileSync('src-tauri/icons/icon.png')));
check('icon-source.png', PNG.sync.read(fs.readFileSync('src-tauri/icons/icon-source.png')));
check('256x256.png', PNG.sync.read(fs.readFileSync('src-tauri/icons/256x256.png')));
check('128x128.png', PNG.sync.read(fs.readFileSync('src-tauri/icons/128x128.png')));
check('64x64.png', PNG.sync.read(fs.readFileSync('src-tauri/icons/64x64.png')));
check('48x48.png', PNG.sync.read(fs.readFileSync('src-tauri/icons/48x48.png')));

// ICO entries
const ico = fs.readFileSync('src-tauri/icons/icon.ico');
for (let i = 0; i < ico.readUInt16LE(4); i++) {
  const off = 6 + i * 16;
  const w = ico[off] || 256, h = ico[off + 1] || 256;
  const base = ico.readUInt32LE(off + 12) + 40;
  const y = Math.round(h * (560 / 1024));
  const x = Math.floor(w * 0.12);
  const px = (yy) => {
    const p = base + ((h - 1 - yy) * w + x) * 4;
    return ico[p] + ico[p + 1] + ico[p + 2];
  };
  const d = px(y) - (px(y - 1) + px(y + 1)) / 2;
  console.log(`ico ${w}x${h}`.padEnd(22), `seam delta = ${d.toFixed(1)} ${Math.abs(d) < 20 ? 'OK' : 'SEAM!'}`);
}

// ICNS entries
const icns = fs.readFileSync('src-tauri/icons/icon.icns');
let p = 8;
while (p + 8 <= icns.length) {
  const type = icns.subarray(p, p + 4).toString('latin1');
  const len = icns.readUInt32BE(p + 4);
  const png = PNG.sync.read(icns.subarray(p + 8, p + len));
  check(`icns ${type}`, png);
  p += len;
}
