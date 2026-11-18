const fs = require('fs');
const { PNG } = require('pngjs');
const img = PNG.sync.read(fs.readFileSync('src-tauri/icons/icon.png'));
console.log('size', img.width, img.height);
// Check top/bottom/left/right borders for non-transparent white-ish pixels
function isWhite(r,g,b,a){ return a>200 && r>240 && g>240 && b>240; }
let topWhite=0,bottomWhite=0,leftWhite=0,rightWhite=0;
for(let x=0;x<img.width;x++){ if(isWhite(img.data[(0*img.width+x)*4],img.data[(0*img.width+x)*4+1],img.data[(0*img.width+x)*4+2],img.data[(0*img.width+x)*4+3])) topWhite++; }
for(let x=0;x<img.width;x++){ const y=img.height-1; if(isWhite(img.data[(y*img.width+x)*4],img.data[(y*img.width+x)*4+1],img.data[(y*img.width+x)*4+2],img.data[(y*img.width+x)*4+3])) bottomWhite++; }
for(let y=0;y<img.height;y++){ if(isWhite(img.data[(y*img.width+0)*4],img.data[(y*img.width+0)*4+1],img.data[(y*img.width+0)*4+2],img.data[(y*img.width+0)*4+3])) leftWhite++; }
for(let y=0;y<img.height;y++){ const x=img.width-1; if(isWhite(img.data[(y*img.width+x)*4],img.data[(y*img.width+x)*4+1],img.data[(y*img.width+x)*4+2],img.data[(y*img.width+x)*4+3])) rightWhite++; }
console.log('top white px:',topWhite,'/',img.width);
console.log('bottom white px:',bottomWhite,'/',img.width);
console.log('left white px:',leftWhite,'/',img.height);
console.log('right white px:',rightWhite,'/',img.height);
// Find any fully horizontal white line (all pixels white) in middle
for(let y=0;y<img.height;y++){
  let all=true;
  for(let x=0;x<img.width;x++){ const i=(y*img.width+x)*4; if(!isWhite(img.data[i],img.data[i+1],img.data[i+2],img.data[i+3])){all=false;break;} }
  if(all) console.log('horizontal white line at y=',y);
}
for(let x=0;x<img.width;x++){
  let all=true;
  for(let y=0;y<img.height;y++){ const i=(y*img.width+x)*4; if(!isWhite(img.data[i],img.data[i+1],img.data[i+2],img.data[i+3])){all=false;break;} }
  if(all) console.log('vertical white line at x=',x);
}
