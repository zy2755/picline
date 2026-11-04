// 生成演示用"截图":纯代码画的抽象图,角度、配色各不相同。
// 录 GIF 时它们会被放进临时收件箱,不动用户真实的截图。
// 用法: node scripts/make-demo-photos.js <输出目录> [数量]
'use strict';
const fs = require('fs');
const path = require('path');
const { PNG } = require('pngjs');

const W = 900, H = 620;

const THEMES = [
  { bg: [[232, 216, 190], [190, 158, 122]], a: [72, 104, 132], b: [246, 206, 138] },  // 沙丘
  { bg: [[186, 214, 224], [108, 148, 168]], a: [28, 56, 78], b: [238, 246, 252] },    // 海雾
  { bg: [[238, 204, 194], [188, 120, 112]], a: [104, 44, 50], b: [252, 232, 212] },   // 落日
  { bg: [[204, 224, 198], [132, 168, 138]], a: [40, 74, 54], b: [244, 250, 236] },    // 苔原
  { bg: [[214, 208, 232], [146, 138, 186]], a: [56, 48, 92], b: [244, 238, 254] },    // 暮紫
];

function lerp(a, b, t) { return a + (b - a) * t; }

function make(i) {
  const th = THEMES[i % THEMES.length];
  const png = new PNG({ width: W, height: H });
  const cx = W * (0.28 + 0.18 * ((i * 7) % 5) / 4);
  const cy = H * (0.30 + 0.14 * ((i * 3) % 4) / 3);
  const r0 = W * (0.34 + 0.06 * (i % 3));

  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      const t = (x / W * 0.55 + y / H * 0.45);
      let r = lerp(th.bg[0][0], th.bg[1][0], t);
      let g = lerp(th.bg[0][1], th.bg[1][1], t);
      let b = lerp(th.bg[0][2], th.bg[1][2], t);

      // 一大一小两个柔和的光斑
      const d1 = Math.hypot(x - cx, y - cy) / r0;
      const k1 = Math.max(0, 1 - d1) ** 2 * 0.85;
      r = lerp(r, th.b[0], k1); g = lerp(g, th.b[1], k1); b = lerp(b, th.b[2], k1);

      const d2 = Math.hypot(x - W * 0.78, y - H * 0.74) / (W * 0.30);
      const k2 = Math.max(0, 1 - d2) ** 2 * 0.6;
      r = lerp(r, th.a[0], k2); g = lerp(g, th.a[1], k2); b = lerp(b, th.a[2], k2);

      // 细颗粒,避免大面积渐变出现色带
      const n = (Math.sin(x * 12.9898 + y * 78.233) * 43758.5453) % 1;
      const grain = (n - Math.floor(n) - 0.5) * 6;

      const o = (W * y + x) << 2;
      png.data[o] = Math.max(0, Math.min(255, r + grain));
      png.data[o + 1] = Math.max(0, Math.min(255, g + grain));
      png.data[o + 2] = Math.max(0, Math.min(255, b + grain));
      png.data[o + 3] = 255;
    }
  }
  return png;
}

function main() {
  const outDir = process.argv[2];
  const count = Number(process.argv[3] || 5);
  if (!outDir) { console.error('用法: node scripts/make-demo-photos.js <输出目录> [数量]'); process.exit(1); }
  fs.mkdirSync(outDir, { recursive: true });
  for (let i = 0; i < count; i++) {
    const png = make(i);
    const file = path.join(outDir, `demo-${String(i + 1).padStart(2, '0')}.png`);
    fs.writeFileSync(file, PNG.sync.write(png));
    console.log('wrote', file);
  }
}

main();
