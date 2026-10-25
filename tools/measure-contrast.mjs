// Measures WCAG contrast of white text (given alpha) against what a rendered screenshot shows
// behind it. The backdrop under a region is its median colour (text covers a small part of a
// region that is a little larger than the text), so it is measured, not estimated.
//
//   node tools/measure-contrast.mjs <png> name:x,y,w,h:alpha[:rrggbb] [...]
// The ink is white at that alpha unless a hex colour is given.
//
// Prints one row per region: backdrop colour, text colour after compositing, ratio.
import fs from 'fs';
import zlib from 'zlib';

export function decodePng(file) {
  const buf = fs.readFileSync(file);
  let pos = 8;
  let width = 0, height = 0, depth = 0, ctype = 0;
  const idat = [];
  while (pos < buf.length) {
    const len = buf.readUInt32BE(pos);
    const type = buf.toString('ascii', pos + 4, pos + 8);
    const data = buf.subarray(pos + 8, pos + 8 + len);
    if (type === 'IHDR') {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      depth = data[8];
      ctype = data[9];
      if (data[12] !== 0) throw new Error('interlaced png not supported');
    } else if (type === 'IDAT') idat.push(data);
    pos += 12 + len;
  }
  if (depth !== 8 || (ctype !== 2 && ctype !== 6)) throw new Error(`unsupported png ${depth}/${ctype}`);
  const bpp = ctype === 6 ? 4 : 3;
  const raw = zlib.inflateSync(Buffer.concat(idat));
  const stride = width * bpp;
  const out = Buffer.alloc(height * stride);
  for (let y = 0; y < height; y++) {
    const f = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? out[y * stride + x - bpp] : 0;
      const b = y > 0 ? out[(y - 1) * stride + x] : 0;
      const c = x >= bpp && y > 0 ? out[(y - 1) * stride + x - bpp] : 0;
      let v = line[x];
      if (f === 1) v += a;
      else if (f === 2) v += b;
      else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      out[y * stride + x] = v & 255;
    }
  }
  return { width, height, bpp, data: out };
}

const lin = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
export const lum = ([r, g, b]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
export const ratio = (a, b) => { const la = lum(a), lb = lum(b); return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05); };

export function backdrop(img, x, y, w, h) {
  const px = [];
  for (let j = y; j < y + h; j++) for (let i = x; i < x + w; i++) {
    const o = (j * img.width + i) * img.bpp;
    px.push([img.data[o], img.data[o + 1], img.data[o + 2]]);
  }
  px.sort((p, q) => lum(p) - lum(q));
  return px[Math.floor(px.length / 2)];
}

if (process.argv[1] && process.argv[1].endsWith("measure-contrast.mjs")) {
  const img = decodePng(process.argv[2]);
  for (const spec of process.argv.slice(3)) {
    const [name, box, alpha, hex] = spec.split(':');
    const [x, y, w, h] = box.split(',').map(Number);
    const a = Number(alpha);
    const bg = backdrop(img, x, y, w, h);
    const ink = hex ? [0, 2, 4].map((i) => parseInt(hex.slice(i, i + 2), 16)) : [255, 255, 255];
    const fg = bg.map((c, i) => Math.round(ink[i] * a + c * (1 - a)));
    console.log(`${name.padEnd(14)} backdrop rgb(${bg.join(',')}) text rgb(${fg.join(',')}) ratio ${ratio(fg, bg).toFixed(2)}`);
  }
}
