// 生成固定尺寸的测试截图（1920x1080 PNG，零外部依赖，只用 node 内置 zlib）。
// 用法: node scripts/make-test-photos.js <目录> [张数]
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const dir = process.argv[2];
const count = Number(process.argv[3] || 10);
const w = 1920;
const h = 1080;
fs.mkdirSync(dir, { recursive: true });

let table = null;
function crc32(buf) {
  if (!table) {
    table = new Int32Array(256);
    for (let i = 0; i < 256; i++) {
      let c = i;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      table[i] = c;
    }
  }
  let c = -1;
  for (const b of buf) c = table[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const t = Buffer.from(type, 'ascii');
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([t, data])), 0);
  return Buffer.concat([len, t, data, crc]);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(w, 0);
ihdr.writeUInt32BE(h, 4);
ihdr[8] = 8;  // bit depth
ihdr[9] = 2;  // truecolor RGB

for (let k = 0; k < count; k++) {
  const raw = Buffer.alloc((1 + w * 3) * h);
  for (let y = 0; y < h; y++) {
    const off = y * (1 + w * 3);
    raw[off] = 0; // filter: None
    for (let x = 0; x < w; x++) {
      const o = off + 1 + x * 3;
      raw[o] = (x + k * 17) & 255;
      raw[o + 1] = (y + k * 29) & 255;
      raw[o + 2] = 128;
    }
  }
  const png = Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', ihdr),
    chunk('IDAT', zlib.deflateSync(raw, { level: 1 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
  fs.writeFileSync(path.join(dir, `test-${String(k).padStart(2, '0')}.png`), png);
}
console.log(`generated ${count} x ${w}x${h} PNG in ${dir}`);
