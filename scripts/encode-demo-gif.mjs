import fs from 'node:fs';
import { PNG } from 'pngjs';
import GIFEncoder from 'gif-encoder-2';

const frames = ['01-list.png', '02-modal.png', '03-files.png'].map((f) =>
  PNG.sync.read(fs.readFileSync(`scripts/gif/${f}`))
);
const { width, height } = frames[0];

const encoder = new GIFEncoder(width, height, 'neuquant', false, 10);
encoder.setDelay(1400);
encoder.setRepeat(0);
encoder.setQuality(12);
encoder.start();
for (const frame of frames) encoder.addFrame(frame.data);
encoder.finish();

const out = 'docs/assets/faast-demo.gif';
fs.writeFileSync(out, encoder.out.getData());
console.log(`wrote ${out} (${width}x${height}, ${frames.length} frames)`);
