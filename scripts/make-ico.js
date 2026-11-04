// 把 assets/icon-*.png 打包成多尺寸 assets/icon.ico(Windows 应用图标)。
//
// 为什么不用 PNG 压缩的 ico:Vista 之后的 ico 允许内部塞 PNG,但部分工具
// (rcedit / 资源编辑器 / 老安装器)只认经典 BMP 格式。这里统一输出
// 32 位 BGRA + AND 掩码的 BMP 条目,兼容性最稳。
//
// 用法: node scripts/make-ico.js
'use strict';
const fs = require('fs');
const path = require('path');
const { PNG } = require('pngjs');

const ROOT = path.join(__dirname, '..');
const ASSETS = path.join(ROOT, 'assets');
const SIZES = [16, 32, 48, 64, 128, 256];   // 尽量凑齐 Windows 会挑的尺寸

// 从最近的大图做双线性缩放,补出缺失尺寸(图标本身是几何图形,缩放够用)
function resize(png, size) {
  if (png.width === size && png.height === size) return png;
  const out = new PNG({ width: size, height: size });
  const sw = png.width, sh = png.height;
  for (let y = 0; y < size; y++) {
    const fy = (y + 0.5) * sh / size - 0.5;
    const y0 = Math.max(0, Math.floor(fy)), y1 = Math.min(sh - 1, y0 + 1);
    const ty = Math.min(1, Math.max(0, fy - y0));
    for (let x = 0; x < size; x++) {
      const fx = (x + 0.5) * sw / size - 0.5;
      const x0 = Math.max(0, Math.floor(fx)), x1 = Math.min(sw - 1, x0 + 1);
      const tx = Math.min(1, Math.max(0, fx - x0));
      const o = (out.width * y + x) << 2;
      for (let c = 0; c < 4; c++) {
        const p00 = png.data[((sw * y0 + x0) << 2) + c];
        const p10 = png.data[((sw * y0 + x1) << 2) + c];
        const p01 = png.data[((sw * y1 + x0) << 2) + c];
        const p11 = png.data[((sw * y1 + x1) << 2) + c];
        const top = p00 + (p10 - p00) * tx;
        const bot = p01 + (p11 - p01) * tx;
        out.data[o + c] = Math.round(top + (bot - top) * ty);
      }
    }
  }
  return out;
}

// 单个尺寸 → ICO 内部的 BMP 位图块(BITMAPINFOHEADER + BGRA(自下而上) + AND 掩码)
function bmpBlock(png) {
  const size = png.width;
  const xorSize = size * size * 4;
  const maskRow = Math.ceil(size / 32) * 4;      // AND 掩码每行 4 字节对齐
  const andSize = maskRow * size;
  const buf = Buffer.alloc(40 + xorSize + andSize);

  buf.writeUInt32LE(40, 0);                     // biSize
  buf.writeInt32LE(size, 4);                    // biWidth
  buf.writeInt32LE(size * 2, 8);                // biHeight = 图像 + 掩码
  buf.writeUInt16LE(1, 12);                     // biPlanes
  buf.writeUInt16LE(32, 14);                    // biBitCount
  buf.writeUInt32LE(0, 16);                     // biCompression = BI_RGB
  buf.writeUInt32LE(xorSize + andSize, 20);     // biSizeImage
  buf.writeInt32LE(0, 24);
  buf.writeInt32LE(0, 28);
  buf.writeUInt32LE(0, 32);
  buf.writeUInt32LE(0, 36);

  // BGRA,自下而上
  let o = 40;
  for (let y = size - 1; y >= 0; y--) {
    for (let x = 0; x < size; x++) {
      const i = (y * size + x) << 2;
      buf[o++] = png.data[i + 2];   // B
      buf[o++] = png.data[i + 1];   // G
      buf[o++] = png.data[i];       // R
      buf[o++] = png.data[i + 3];   // A
    }
  }
  // AND 掩码全 0:透明度完全交给 alpha 通道
  return buf;
}

function main() {
  const images = [];
  for (const size of SIZES) {
    const file = path.join(ASSETS, `icon-${size}.png`);
    let png;
    if (fs.existsSync(file)) {
      png = PNG.sync.read(fs.readFileSync(file));
    } else {
      const base = SIZES.filter(s => s >= size && fs.existsSync(path.join(ASSETS, `icon-${s}.png`)))[0]
        || SIZES.slice().reverse().filter(s => fs.existsSync(path.join(ASSETS, `icon-${s}.png`)))[0];
      if (!base) continue;
      png = resize(PNG.sync.read(fs.readFileSync(path.join(ASSETS, `icon-${base}.png`))), size);
    }
    if (png.width !== size) png = resize(png, size);
    images.push({ size, png });
  }
  if (!images.length) throw new Error('assets/ 下找不到 icon-*.png');

  const blocks = images.map(img => ({ size: img.size, data: bmpBlock(img.png) }));

  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);                       // reserved
  header.writeUInt16LE(1, 2);                       // type = icon
  header.writeUInt16LE(blocks.length, 4);

  const entries = Buffer.alloc(16 * blocks.length);
  let offset = 6 + entries.length;
  blocks.forEach((b, i) => {
    const e = i * 16;
    entries[e]     = b.size >= 256 ? 0 : b.size;    // 0 表示 256
    entries[e + 1] = b.size >= 256 ? 0 : b.size;
    entries[e + 2] = 0;                             // 调色板数
    entries[e + 3] = 0;
    entries.writeUInt16LE(1, e + 4);                // planes
    entries.writeUInt16LE(32, e + 6);               // bpp
    entries.writeUInt32LE(b.data.length, e + 8);
    entries.writeUInt32LE(offset, e + 12);
    offset += b.data.length;
  });

  const out = path.join(ASSETS, 'icon.ico');
  fs.writeFileSync(out, Buffer.concat([header, entries, ...blocks.map(b => b.data)]));
  console.log(`已生成 ${path.relative(ROOT, out)}  (${blocks.map(b => b.size).join(', ')} px, ${(fs.statSync(out).size / 1024).toFixed(1)} KB)`);
}

main();
