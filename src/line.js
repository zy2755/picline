// 图墙 · 晾衣绳渲染端
// 几何与物理参数移植自原版:
//   tilt ±2.5°(Line.swift)· 微风 nudge 1.6–3.4°、随机延迟 0–0.35s(PeggedView.breeze)
//   到位 spring(0.42, 0.72)· 回摆 interpolatingSpring(38, 2.4)· 长按 0.45s(GrabArea)
'use strict';
const { ipcRenderer } = require('electron');

// ---------- 常量 ----------
const CARD_W = 150;
const SPACING = CARD_W + 24;          // Layout.spacing
const ROPE_TOP = 10;
const EDGE_PAD = 24;

const els = {
  line: document.getElementById('line'),
  rope: document.getElementById('rope'),
  pegs: document.getElementById('pegs'),
  empty: document.getElementById('empty'),
  hint: document.getElementById('hint'),
  toast: document.getElementById('toast'),
  menu: document.getElementById('menu'),
};

let items = [];                       // 全量(旧→新)
let maxPegs = 10;
const pegs = new Map();               // id -> {el, swing, card, x, ropey, tilt, a, v, arriveAt}
let shown = false;

// ---------- 工具 ----------
const rand = (lo, hi) => lo + Math.random() * (hi - lo);

function ropeY(x, width) {
  const sag = Math.min(30, width * 0.018);
  const f = width > 0 ? x / width : 0;
  return ROPE_TOP + 4 * sag * f * (1 - f);
}

function toast(text) {
  els.toast.textContent = text;
  els.toast.hidden = false;
  els.toast.classList.add('on');
  clearTimeout(toast.t);
  toast.t = setTimeout(() => els.toast.classList.remove('on'), 1200);
}

// ---------- 绳子 ----------
function drawRope() {
  const w = window.innerWidth;
  const sag = Math.min(30, w * 0.018);
  els.rope.setAttribute('viewBox', `0 0 ${w} 56`);
  els.rope.innerHTML =
    `<path d="M 0 ${ROPE_TOP} Q ${w / 2} ${ROPE_TOP + 2 * sag} ${w} ${ROPE_TOP}" fill="none"
       stroke="rgba(255,252,244,.28)" stroke-width="1.1" transform="translate(0 1.6)"/>
     <path d="M 0 ${ROPE_TOP} Q ${w / 2} ${ROPE_TOP + 2 * sag} ${w} ${ROPE_TOP}" fill="none"
       stroke="#3e342a" stroke-width="2.4" stroke-linecap="round"/>`;
}

// ---------- 布局 ----------
function visibleItems() {
  const fit = Math.max(3, Math.floor((window.innerWidth - EDGE_PAD * 2) / SPACING));
  const n = Math.min(maxPegs, fit, items.length);
  return items.slice(items.length - n);
}

function layout() {
  const w = window.innerWidth;
  const vis = visibleItems();
  const n = vis.length;
  const spacing = n > 1 ? Math.min(SPACING, (w - EDGE_PAD * 2 - CARD_W) / (n - 1)) : 0;
  const cx = w / 2;

  const keep = new Set(vis.map(it => it.id));
  for (const [id, peg] of pegs) {
    if (!keep.has(id)) {
      peg.el.remove();
      pegs.delete(id);
    }
  }

  vis.forEach((it, i) => {
    const x = n > 1 ? cx + (i - (n - 1) / 2) * spacing : cx;
    let peg = pegs.get(it.id);
    if (!peg) {
      peg = makePeg(it);
      pegs.set(it.id, peg);
    }
    const ry = ropeY(x, w);
    peg.x = x;
    peg.ropey = ry;
    peg.el.style.left = `${x - CARD_W / 2}px`;
    peg.el.style.setProperty('--ropey', `${ry}px`);
    peg.swing.style.transformOrigin = `50% ${ry}px`;
  });

  els.empty.hidden = items.length > 0;
  reportRects();                        // 位置变了,把新的可交互区域告诉主进程
}

function makePeg(it) {
  const el = document.createElement('div');
  el.className = 'peg';
  el.innerHTML =
    `<div class="swing">
       <div class="clip"></div>
       <div class="card">
         <img class="photo" draggable="false">
         <div class="glass"></div>
         <button class="xbtn" title="取下并删除(回收站)">✕</button>
       </div>
     </div>`;
  const img = el.querySelector('img');
  img.src = 'file:///' + it.file.replace(/\\/g, '/');
  const peg = {
    el, swing: el.querySelector('.swing'), card: el.querySelector('.card'),
    x: 0, ropey: ROPE_TOP,
    tilt: rand(-2.5, 2.5),              // 原版 Line.swift:每张图随机静置倾角
    a: 0, v: 0,                         // 摆角(°)与角速度(°/s)
  };
  els.pegs.appendChild(el);
  bindGestures(peg, it);
  // 到位动画 + 初始一摆
  el.classList.add('arrive');
  peg.v = rand(14, 22);
  setTimeout(() => el.classList.remove('arrive'), 500);
  return peg;
}

// ---------- 物理:阻尼弹簧摆动 ----------
// 只在展开时跑。收起状态下没有任何可见动画,让 rAF 停掉可以省 CPU。
let lastT = 0;
let rafId = 0;
function tick(t) {
  const dt = Math.min(0.032, lastT ? (t - lastT) / 1000 : 0.016);
  lastT = t;
  const K = 30, C = 2.6;                // ≈ 原版 interpolatingSpring(38, 2.4) 的手感
  for (const peg of pegs.values()) {
    if (Math.abs(peg.a) < 0.004 && Math.abs(peg.v) < 0.02) {
      if (peg.a !== 0) { peg.a = 0; peg.v = 0; peg.swing.style.transform = `rotate(${peg.tilt}deg)`; }
      continue;
    }
    peg.v += (-K * peg.a - C * peg.v) * dt;
    peg.a += peg.v * dt;
    peg.swing.style.transform = `rotate(${(peg.tilt + peg.a).toFixed(3)}deg)`;
  }
  rafId = requestAnimationFrame(tick);
}
function startTick() { if (!rafId) { lastT = 0; rafId = requestAnimationFrame(tick); } }
function stopTick() { if (rafId) { cancelAnimationFrame(rafId); rafId = 0; } }

// 微风:周期性一阵,每张错开随机延迟(原版 breeze())
function gust() {
  if (!shown) { setTimeout(gust, rand(3800, 8500)); return; }   // 收着就空转,不白算
  const strength = rand(1.6, 3.4);
  for (const peg of pegs.values()) {
    setTimeout(() => { peg.v += strength * Math.sqrt(30) * (Math.random() < 0.5 ? -1 : 1); },
      rand(0, 350));
  }
  setTimeout(gust, rand(3800, 8500));
}
setTimeout(gust, 1500);

// ---------- 手势(原版 GrabArea) ----------
function bindGestures(peg, it) {
  const HOLD_MS = 450;
  let downX = 0, downY = 0, downT = 0, holdTimer = null, held = false, dragging = false;

  const clearHold = () => { clearTimeout(holdTimer); holdTimer = null; };

  peg.card.addEventListener('mousedown', (e) => {
    if (e.button === 2) return;
    downX = e.screenX; downY = e.screenY; downT = Date.now(); held = false; dragging = false;
    holdTimer = setTimeout(() => {
      held = true;                        // 长按 → 用画图编辑(对应原版 Markup)
      ipcRenderer.send('item:edit', it.id);
    }, HOLD_MS);
    e.preventDefault();
  });

  peg.card.addEventListener('mousemove', (e) => {
    if (!downT || dragging) return;
    if (Math.hypot(e.screenX - downX, e.screenY - downY) < 7) return;
    clearHold();
    dragging = true;
    dragOut = true;                     // 拖出期间不放开鼠标接管,否则系统拖放会断
    ipcRenderer.send('line:drag-active', true);
    ipcRenderer.send('item:drag', it.id, thumbnailDataUrl(peg));
    setTimeout(() => { dragOut = false; ipcRenderer.send('line:drag-active', false); }, 3500);
  });

  peg.card.addEventListener('mouseup', (e) => {
    if (e.button === 2) return;
    clearHold();
    dragOut = false;
    if (held || dragging) { downT = 0; return; }
    downT = 0;
    const now = Date.now();
    if (now - (bindGestures.lastClick || 0) < 350 && bindGestures.lastId === it.id) {
      ipcRenderer.send('item:open', it.id);          // 双击 → 系统照片查看器
      bindGestures.lastClick = 0;
      return;
    }
    bindGestures.lastClick = now; bindGestures.lastId = it.id;
    ipcRenderer.send('item:copy', it.id);            // 单击 → 复制(原版 Click = Copy)
    toast('已复制到剪贴板');
  });

  peg.card.addEventListener('mouseleave', clearHold);

  peg.card.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    openMenu(e.clientX, e.clientY, it);
  });

  const xbtn = peg.el.querySelector('.xbtn');
  // ✕ 上的按下/抬起都不许冒泡到 card 的手势层(否则会顺带触发"单击=复制")
  xbtn.addEventListener('mousedown', (e) => e.stopPropagation());
  xbtn.addEventListener('mouseup', (e) => e.stopPropagation());
  xbtn.addEventListener('click', async (e) => {
    e.stopPropagation();
    if (xbtn.disabled) return;
    xbtn.disabled = true;
    peg.el.classList.add('falling');                 // 坠落动画
    const res = await ipcRenderer.invoke('item:discard', it.id);
    if (res && res.ok) return;                       // 已移入回收站,主进程 rescan 后节点会移除
    peg.el.classList.remove('falling');              // 删除失败:相片回到绳上
    xbtn.disabled = false;
    toast('删除失败: ' + ((res && res.error) || '未知原因'));
  });
}

function thumbnailDataUrl(peg) {
  try {
    const img = peg.card.querySelector('img');
    const c = document.createElement('canvas');
    const scale = Math.min(160 / img.naturalWidth || 1, 120 / img.naturalHeight || 1, 1);
    c.width = Math.max(8, Math.round((img.naturalWidth || 160) * scale));
    c.height = Math.max(8, Math.round((img.naturalHeight || 120) * scale));
    c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
    return c.toDataURL('image/png');
  } catch { return ''; }
}

// ---------- 右键菜单 ----------
function openMenu(x, y, it) {
  els.menu.innerHTML = '';
  const mk = (label, fn, cls = '') => {
    const d = document.createElement('div');
    d.className = 'mi ' + cls;
    d.textContent = label;
    d.addEventListener('click', () => { closeMenu(); fn(); });
    els.menu.appendChild(d);
  };
  const sep = () => {
    const d = document.createElement('div');
    d.className = 'sep';
    els.menu.appendChild(d);
  };
  mk('复制图片', () => { ipcRenderer.send('item:copy', it.id); toast('已复制到剪贴板'); });
  mk('用画图编辑', () => ipcRenderer.send('item:edit', it.id));
  mk('打开', () => ipcRenderer.send('item:open', it.id));
  mk('另存为…', async () => { const ok = await ipcRenderer.invoke('item:save-as', it.id); if (ok) toast('已保存'); });
  sep();
  mk('取下并保存到截图文件夹', async () => { await ipcRenderer.invoke('item:take-down', it.id); });
  mk('删除(移入回收站)', async () => {
    const res = await ipcRenderer.invoke('item:discard', it.id);
    if (res && res.ok) toast('已移入回收站');
    else toast('删除失败: ' + ((res && res.error) || '未知原因'));
  }, 'danger');

  els.menu.hidden = false;
  const mw = els.menu.offsetWidth, mh = els.menu.offsetHeight;
  els.menu.style.left = Math.min(x, window.innerWidth - mw - 6) + 'px';
  els.menu.style.top = Math.min(y, window.innerHeight - mh - 6) + 'px';
  reportRects();                        // 菜单也要算进可交互区域

  // 只在"点在菜单外面"时收起。若在 mousedown 就无条件收起,菜单会被
  // display:none 隐藏,mouseup/click 便落不到菜单项上——菜单点了等于没点。
  window.addEventListener('mousedown', onMenuOutside, true);
}
function onMenuOutside(e) {
  if (!els.menu.contains(e.target)) closeMenu();
}
function closeMenu() {
  els.menu.hidden = true;
  window.removeEventListener('mousedown', onMenuOutside, true);
  reportRects();
}

// ---------- 空白穿透:只有相片上才接管鼠标 ----------
// 窗口默认穿透。主进程按光标位置轮询命中检测,依据就是这里上报的矩形;
// 真收到 mousemove(说明已被接管)时,这里再做一次更精确的复核。
// 于是空白处不再吃点击 —— 整条 210px 的窗口不会把屏幕顶边占死。
let interactive = false;
let dragOut = false;                 // 拖出相片的过程中:必须保持接管
let lastMoveAt = 0;

function setInteractive(next) {
  if (next === interactive) return;
  interactive = next;
  ipcRenderer.send('line:passthrough', !next);
}

function hitsPeg(x, y) {
  if (!els.menu.hidden) return true;        // 菜单开着:整条接管,才能"点别处收起"
  const el = document.elementFromPoint(x, y);
  return !!(el && el.closest('.card, .xbtn'));
}

// 上报可交互区域。getBoundingClientRect 会带上 #line 的滑入位移,
// 减掉它,统一换算成"完全展开"时的坐标。
function reportRects() {
  const dy = els.line.getBoundingClientRect().top;
  const rects = [];
  for (const el of document.querySelectorAll('.card')) {
    const r = el.getBoundingClientRect();
    if (!r.width || !r.height) continue;
    rects.push({ x: r.left, y: r.top - dy, w: r.width, h: r.height });
  }
  if (!els.menu.hidden) {
    const m = els.menu.getBoundingClientRect();
    if (m.width && m.height) rects.push({ x: m.left, y: m.top - dy, w: m.width, h: m.height });
  }
  ipcRenderer.send('line:rects', rects);
}

window.addEventListener('mousemove', (e) => {
  lastMoveAt = performance.now();
  if (dragOut) return;
  setInteractive(hitsPeg(e.clientX, e.clientY));
}, { passive: true });

window.addEventListener('mouseleave', () => { if (!dragOut) setInteractive(false); });

// ---------- IPC ----------
ipcRenderer.on('items', (e, list, max) => {
  maxPegs = max || 10;
  items = list || [];
  layout();
});

ipcRenderer.on('reveal', () => {
  shown = true;
  // 主进程在展开瞬间把窗口切回"穿透 + 转发",这里同步状态,
  // 否则本地记录为已接管时,下一次命中相片就不会再发请求
  interactive = false;
  els.line.classList.remove('hidden');
  els.line.classList.add('shown');
  startTick();
  gust();
  reportRects();
  setTimeout(reportRects, 460);         // 滑入动画结束后再校准一次
});

ipcRenderer.on('hide', () => {
  shown = false;
  dragOut = false;
  if (interactive) { interactive = false; ipcRenderer.send('line:passthrough', true); }
  els.line.classList.remove('shown');
  els.line.classList.add('hidden');
  stopTick();
  closeMenu();
});

// ---------- 启动 ----------
window.addEventListener('resize', () => { drawRope(); layout(); });
drawRope();
ipcRenderer.invoke('items:get').then((list) => {
  items = list || [];
  layout();
  // 开屏若直接可见,来一阵风
  if (shown) gust();
});
