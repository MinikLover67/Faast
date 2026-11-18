import fs from 'node:fs';
import { PNG } from 'pngjs';

const data = fs.readFileSync('src-tauri/icons/icon-source.png');
const png = PNG.sync.read(data);

let r = 0, g = 0, b = 0, count = 0;
for (let y = 0; y < png.height; y++) {
  for (let x = 0; x < png.width; x++) {
    const idx = (png.width * y + x) << 2;
    const a = png.data[idx + 3];
    if (a > 128) {
      r += png.data[idx];
      g += png.data[idx + 1];
      b += png.data[idx + 2];
      count++;
    }
  }
}
const avgR = Math.round(r / count);
const avgG = Math.round(g / count);
const avgB = Math.round(b / count);
console.log(`Average color: rgb(${avgR}, ${avgG}, ${avgB}) = #${avgR.toString(16).padStart(2,'0')}${avgG.toString(16).padStart(2,'0')}${avgB.toString(16).padStart(2,'0')}`);

// Sample grid for dominant colors
const samples = [];
const stepX = Math.max(1, Math.floor(png.width / 20));
const stepY = Math.max(1, Math.floor(png.height / 20));
for (let y = 0; y < png.height; y += stepY) {
  for (let x = 0; x < png.width; x += stepX) {
    const idx = (png.width * y + x) << 2;
    const a = png.data[idx + 3];
    if (a > 128) {
      samples.push({r: png.data[idx], g: png.data[idx+1], b: png.data[idx+2]});
    }
  }
}
const buckets = {};
for (const s of samples) {
  const key = `${Math.round(s.r/32)*32},${Math.round(s.g/32)*32},${Math.round(s.b/32)*32}`;
  buckets[key] = (buckets[key] || 0) + 1;
}
const top = Object.entries(buckets).sort((a,b) => b[1]-a[1]).slice(0, 5);
console.log('Top dominant buckets:');
for (const [k, v] of top) {
  const [rr, gg, bb] = k.split(',').map(Number);
  console.log(`  RGB(${rr}, ${gg}, ${bb}) = ${v} samples  ->  #${rr.toString(16).padStart(2,'0')}${gg.toString(16).padStart(2,'0')}${bb.toString(16).padStart(2,'0')}`);
}