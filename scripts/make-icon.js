// 生成图墙应用图标:晾衣绳 + 木衣夹 + 相片。
// 纯 Node + pngjs 光栅化(SDF 形状 + 4x 超采样抗锯齿),无原生依赖。
// 用法: node scripts/make-icon.js
const fs = require('fs');
const path = require('path');
const { PNG } = require('pngjs');

const SS = 4;                 // 超采样倍数
const SIZE = 256;
const W = SIZE * SS;

function sdRoundRect(px, py, cx, cy, hw, hh, r) {
  const qx = Math.abs(px - cx) - hw + r;
  const qy = Math.abs(py - cy) - hh + r;
  const ox = Math.max(qx, 0), oy = Math.max(qy, 0);
  return Math.min(Math.max(qx, qy), 0) + Math.hypot(ox, oy) - r;
}
function sdSegment(px, py, ax, ay, bx, by, r) {
  const vx = bx - ax, vy = by - ay;
  const t = Math.max(0, Math.min(1, ((px - ax) * vx + (py - ay) * vy) / (vx * vx + vy * vy || 1)));
  return Math.hypot(px - (ax + vx * t), py - (ay + vy * t)) - r;
}
const cov = (d) => Math.max(0, Math.min(1, 0.5 - d));

function blend(img, x, y, col, a) {
  if (x < 0 || y < 0 || x >= W || y >= W || a <= 0) return;
  const i = (y * W + x) * 4;
  const da = img.data[i + 3] / 255;
  const oa = a + da * (1 - a);
  if (oa <= 0) return;
  img.data[i]     = (col[0] * a + img.data[i]     * da * (1 - a)) / oa;
  img.data[i + 1] = (col[1] * a + img.data[i + 1] * da * (1 - a)) / oa;
  img.data[i + 2] = (col[2] * a + img.data[i + 2] * da * (1 - a)) / oa;
  img.data[i + 3] = oa * 255;
}
// 对整个画布应用 SDF 着色:stroke=0 画实心,>0 画描边。
// SDF 约定:形状内部 d<0;实心覆盖度 = cov(d)(d=-∞ → 1,d=+∞ → 0)
function paint(img, sdf, col, stroke = 0) {
  for (let y = 0; y < W; y++) for (let x = 0; x < W; x++) {
    const d = sdf(x + 0.5, y + 0.5);
    const a = stroke > 0 ? cov(Math.abs(d) - stroke / 2) : cov(d);
    if (a > 0) blend(img, x, y, col, a);
  }
}

function makeIcon() {
  const img = new PNG({ width: W, height: W });
  const S = SS;
  const rr = (cx, cy, hw, hh, r) => (x, y) => sdRoundRect(x, y, cx * S, cy * S, hw * S, hh * S, r * S);
  const seg = (ax, ay, bx, by, r) => (x, y) => sdSegment(x, y, ax * S, ay * S, bx * S, by * S, r * S);

  // 1) 背景:奶油色圆角方块,深一圈外沿
  paint(img, rr(128, 128, 124, 124, 56), [214, 201, 178]);
  paint(img, rr(128, 128, 119, 119, 52), [246, 240, 228]);

  // 2) 晾衣绳:轻微下垂的弧线(分段胶囊)
  const sag = 12, steps = 24;
  let prev = null;
  for (let i = 0; i <= steps; i++) {
    const t = i / steps;
    const x = 30 + (226 - 30) * t;
    const y = 84 + sag * (4 * t * (1 - t));
    if (prev) paint(img, seg(prev[0], prev[1], x, y, 2.6), [74, 63, 51]);
    prev = [x, y];
  }

  // 3) 相片:投影 → 白框 → 内芯天空 + 太阳(裁剪在相片内)+ 玻璃高光
  paint(img, (x, y) => sdRoundRect(x, y, 128 * S, 167 * S, 45 * S, 52 * S, 9 * S), [206, 195, 175]);
  paint(img, rr(128, 163, 45, 52, 9), [255, 255, 255]);
  paint(img, rr(128, 156, 34, 27, 4), [156, 197, 218]);
  for (let y = 0; y < W; y++) for (let x = 0; x < W; x++) {           // 太阳
    if (sdRoundRect(x, y, 128 * S, 156 * S, 34 * S, 27 * S, 4 * S) >= 0) continue;
    const a = cov(Math.hypot(x - 116 * S, y - 148 * S) - 11 * S);
    if (a > 0) blend(img, x, y, [243, 210, 135], a);
  }
  for (let y = 0; y < W; y++) for (let x = 0; x < W; x++) {           // 高光
    if (sdRoundRect(x, y, 128 * S, 163 * S, 45 * S, 52 * S, 9 * S) >= 0) continue;
    const a = cov(sdSegment(x, y, 96 * S, 200 * S, 132 * S, 118 * S, 10 * S)) * 0.35;
    if (a > 0) blend(img, x, y, [255, 255, 255], a);
  }

  // 4) 衣夹:夹在绳子与相片之间
  paint(img, (x, y) => sdRoundRect(x, y, 128 * S, 105 * S, 10 * S, 26 * S, 7 * S), [176, 128, 82]); // 侧影
  paint(img, rr(126, 104, 9, 24, 6), [210, 162, 110]);                 // 木身
  paint(img, rr(126, 86, 11, 5, 3), [138, 107, 74]);                   // 弹簧

  return img;
}

function downsample(src, target) {
  const f = src.width / target;
  const out = new PNG({ width: target, height: target });
  for (let y = 0; y < target; y++) for (let x = 0; x < target; x++) {
    let r = 0, g = 0, b = 0, a = 0, n = 0;
    for (let sy = Math.floor(y * f); sy < Math.floor((y + 1) * f); sy++)
      for (let sx = Math.floor(x * f); sx < Math.floor((x + 1) * f); sx++) {
        const i = (sy * src.width + sx) * 4;
        const al = src.data[i + 3] / 255;
        r += src.data[i] * al; g += src.data[i + 1] * al; b += src.data[i + 2] * al; a += al; n++;
      }
    const i = (y * target + x) * 4;
    if (a > 0) { out.data[i] = r / a; out.data[i + 1] = g / a; out.data[i + 2] = b / a; }
    out.data[i + 3] = Math.round((a / n) * 255);
  }
  return out;
}

const outDir = path.join(__dirname, '..', 'assets');
fs.mkdirSync(outDir, { recursive: true });
const big = makeIcon();
for (const size of [256, 32, 16]) {
  fs.writeFileSync(path.join(outDir, `icon-${size}.png`), PNG.sync.write(downsample(big, size)));
  console.log(`assets/icon-${size}.png written`);
}
