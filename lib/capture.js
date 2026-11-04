// 截图捕获管线:
//   1) 监视系统截图文件夹(Win+Shift+S / Win+PrintScreen 落盘处),新截图移入收件箱
//   2) 监听剪贴板图像(捕捉不落盘的截图,如关闭"自动保存"时的 Win+Shift+S)
//   3) 去重,防同一张图挂两次
// 收件箱即晾衣绳的状态:文件在 = 挂在绳上。
//
// 去重设计(踩过的坑):
//   - 文件解码与剪贴板 DIB 解码的像素字节不同(色彩管理),精确哈希无法跨通道统一;
//   - Electron 33 的 nativeImage 懒解码:与其他图像操作交错时 toBitmap 可能返回全零,
//     故文件通道的取键是同步原子段(无 await);
//   - 因此:各通道内部用"视觉哈希(24×24 RGB 量化)+尺寸"自去重;
//     跨通道用"同尺寸 + 时间邻近(10s)"判为同一张截图:
//     · 剪贴板图与 10s 内捕获的文件同尺寸 → 跳过(文件通道已挂);
//     · 文件与 10s 内捕获的剪贴板图同尺寸 → 用原始文件替换 clip(保住画质)。
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { EventEmitter } = require('events');
const { clipboard, nativeImage } = require('electron');

const IMAGE_EXT = new Set(['.png', '.jpg', '.jpeg', '.bmp', '.webp']);
const VISUAL_TTL = 90_000;      // 视觉键记忆时长(毫秒)
const CROSS_TTL = 10_000;       // 跨通道"同一张"的时间窗(毫秒)

class Capture extends EventEmitter {
  constructor({ inboxDir, screenshotsDir }) {
    super();
    this.inboxDir = inboxDir;
    this.screenshotsDir = screenshotsDir;
    this.takeOver = true;
    this.clipboardWatch = true;
    this.seenVisual = new Map();       // 视觉键 -> ts(各通道内部去重)
    this.copiedKeys = new Set();       // 我们自己写入剪贴板的图的视觉键
    this.lastClipboardKey = null;      // 剪贴板变化检测
    this.lastFileCapture = null;       // {dims, ts}  供剪贴板通道判"同一张"
    this.lastClipCapture = null;       // {dims, ts, file} 供文件通道替换 clip
    this.watchers = [];
    this.timers = [];
    this.stopped = false;
  }

  start() {
    this.startedAt = Date.now() - 5000;   // 防线:只收启动之后新出现的文件,绝不扫走历史截图
    this.suppressUntil = 0;
    // 基线:把当前剪贴板内容记为"已知",启动时不抓取残留的旧图。
    // 只记 lastClipboardKey(变化检测),不进去重库——用户之后再复制同一张图,应该照挂。
    try {
      const img = clipboard.readImage();
      if (!img.isEmpty()) this.lastClipboardKey = visualKey(img)?.key ?? null;
    } catch {}
    this.rescan();
    this.watchDir(this.screenshotsDir);
    this.watchDir(this.inboxDir);
    this.timers.push(setInterval(() => this.pollClipboard(), 800));
    this.timers.push(setInterval(() => this.pruneKeys(), 10_000));
  }

  stop() {
    this.stopped = true;
    for (const w of this.watchers) { try { w.close(); } catch {} }
    for (const t of this.timers) clearInterval(t);
    this.watchers = []; this.timers = [];
  }

  setOptions({ takeOver, clipboardWatch } = {}) {
    if (takeOver !== undefined) this.takeOver = takeOver;
    if (clipboardWatch !== undefined) this.clipboardWatch = clipboardWatch;
  }

  // ---------- 收件箱 ----------

  listInbox() {
    let entries;
    try { entries = fs.readdirSync(this.inboxDir); } catch { return []; }
    const items = [];
    for (const name of entries) {
      const ext = path.extname(name).toLowerCase();
      if (!IMAGE_EXT.has(ext)) continue;
      const file = path.join(this.inboxDir, name);
      try {
        const st = fs.statSync(file);
        if (!st.isFile()) continue;
        items.push({ id: name, file, mtime: st.mtimeMs, size: st.size });
      } catch {}
    }
    items.sort((a, b) => a.mtime - b.mtime);      // 旧 → 新,渲染端把新的挂右端
    return items.map((it, i) => ({ ...it, seq: i }));
  }

  rescan = () => {
    if (this.stopped) return;
    this.emit('items', this.listInbox());
  };

  // ---------- 文件夹监视 ----------

  watchDir(dir) {
    fs.mkdirSync(dir, { recursive: true });
    let pending = new Set();
    let flushTimer = null;
    const schedule = (name) => {
      if (!name || pending.has(name)) return;
      pending.add(name);
      clearTimeout(flushTimer);
      flushTimer = setTimeout(async () => {
        const names = [...pending];
        pending = new Set();
        for (const n of names) await this.handleIncoming(dir, n);
        if (dir === this.inboxDir) this.rescan();
      }, 350);
    };
    const watcher = fs.watch(dir, (event, name) => schedule(name));
    watcher.on('error', () => { /* 文件夹被删等;忽略,下轮事件会重建 */ });
    this.watchers.push(watcher);
  }

  async handleIncoming(dir, name) {
    const ext = path.extname(name).toLowerCase();
    if (!IMAGE_EXT.has(ext)) return;
    const src = path.join(dir, name);
    // 等文件写完:连续两次(间隔 250ms)大小一致才动它
    let size = -1;
    for (let i = 0; i < 12; i++) {
      let st;
      try { st = fs.statSync(src); } catch { return; }        // 已消失(可能是临时文件)
      if (!st.isFile()) return;
      if (st.size === size && st.size > 0) break;
      size = st.size;
      await sleep(250);
    }
    if (dir === this.inboxDir) return;                         // 收件箱自己的变化,由 watchDir 统一 rescan
    if (dir !== this.inboxDir) {
      // 启动前就存在的文件(以及云端同步等引起的元数据事件)一律不动
      try { if (fs.statSync(src).mtimeMs < this.startedAt) return; } catch { return; }
    }
    // 同步原子段:取键不许有 await(懒解码见文件头注释)。
    // 文件通道不做内容去重:每个新文件都是一次新截图(原版行为),重复的
    // watcher 事件已被 pending 集合折叠、文件移走后 statSync 也会自然失败。
    let info;
    try { info = fileVisualInfo(src); } catch { return; }
    if (!info) return;                                         // 解不开的文件,放着不动

    const sameAsClip = this.lastClipCapture
      && info.dims === this.lastClipCapture.dims
      && Date.now() - this.lastClipCapture.ts < CROSS_TTL;

    if (!this.takeOver) return;                                // 未接管:让截图留在原地

    let dest;
    try {
      dest = uniqueName(this.inboxDir, 'shot', ext);
      fs.renameSync(src, dest);                                // 同盘符 rename 原子
    } catch {
      try {
        dest = path.join(this.inboxDir, name);
        fs.copyFileSync(src, dest); fs.unlinkSync(src);
      } catch { return; }
    }
    if (sameAsClip) {
      // 剪贴板通道抢先挂了同一张(画质差的转码版):换回原始文件
      try { fs.unlinkSync(this.lastClipCapture.file); } catch {}
      this.lastClipCapture = null;
    }
    this.lastFileCapture = { dims: info.dims, ts: Date.now() };
    this.rescan();
  }

  // ---------- 剪贴板监听 ----------

  pollClipboard() {
    if (!this.clipboardWatch || this.stopped) return;
    const formats = clipboard.availableFormats();
    if (!formats.some(f => f.startsWith('image/'))) return;
    let img;
    try { img = clipboard.readImage(); } catch { return; }
    if (img.isEmpty()) return;
    const key = visualKey(img);
    if (!key || key.key === this.lastClipboardKey) return;     // 剪贴板没变
    this.lastClipboardKey = key.key;
    if (Date.now() < this.suppressUntil) return;               // 自己复制出去的(时窗保险)
    if (this.copiedKeys.has(key.key)) return;                  // 自己复制出去的
    // 跨通道:与刚捕获的文件同尺寸 → 同一张截图,文件通道已挂,跳过
    if (this.lastFileCapture
      && key.dims === this.lastFileCapture.dims
      && Date.now() - this.lastFileCapture.ts < CROSS_TTL) return;
    const dest = uniqueName(this.inboxDir, 'clip', '.png');
    try { fs.writeFileSync(dest, img.toPNG()); } catch { return; }
    this.lastClipCapture = { dims: key.dims, ts: Date.now(), file: dest };
    this.rescan();
  }

  // 点击复制后调用:读取刚写入剪贴板的图像,记下其视觉键,
  // 抑制轮询把自己再挂上去。
  suppressCopy() {
    try {
      const img = clipboard.readImage();
      if (img.isEmpty()) return;
      const key = visualKey(img);
      if (!key) return;
      this.copiedKeys.add(key.key);
      this.lastClipboardKey = key.key;
      this.suppressUntil = Date.now() + 1500;
    } catch {}
  }

  // 记录"见过";已在 → true。参数为视觉键对象或键字符串。(保留给扩展用)
  rememberVisual(x) {
    const k = (typeof x === 'string') ? x : x && x.key;
    if (!k) return false;
    const now = Date.now();
    if (this.seenVisual.has(k)) { this.seenVisual.set(k, now); return true; }
    this.seenVisual.set(k, now);
    return false;
  }

  pruneKeys() {
    const cutoff = Date.now() - VISUAL_TTL;
    for (const [k, ts] of this.seenVisual) if (ts < cutoff) this.seenVisual.delete(k);
    if (this.copiedKeys.size > 32) this.copiedKeys.clear();
  }
}

// ---------- 工具 ----------

const sleep = (ms) => new Promise(r => setTimeout(r, ms));

function quickHash(...parts) {
  const h = crypto.createHash('sha1');
  for (const p of parts) h.update(p);
  return h.digest('hex');
}

// 视觉哈希:缩到 24×24 → RGB 各 8 级量化。与编码格式无关。
// 注意:必须在无 await 的同步段内完成(resize 触发解码,toBitmap 取像素),
// 否则 Electron 33 的懒解码缓存可能被并发图像操作清掉,得到全零。
function visualHash(img) {
  try {
    const small = img.resize({ width: 24, height: 24 });
    const bmp = small.toBitmap();                        // BGRA
    const buf = Buffer.alloc(24 * 24 * 3);
    for (let i = 0; i < 24 * 24; i++) {
      buf[i * 3]     = bmp[i * 4 + 2] >> 5;              // R
      buf[i * 3 + 1] = bmp[i * 4 + 1] >> 5;              // G
      buf[i * 3 + 2] = bmp[i * 4] >> 5;                  // B
    }
    return quickHash(buf);
  } catch { return null; }
}

function visualKey(img) {
  try {
    const { width, height } = img.getSize();
    const vhash = visualHash(img);
    if (!vhash) return null;
    return { key: `${vhash}|${width}x${height}`, dims: `${width}x${height}` };
  } catch { return null; }
}

// 文件通道取键:同步、原子(内部无 await)。
function fileVisualInfo(file) {
  const img = nativeImage.createFromPath(file);
  if (img.isEmpty()) return null;
  return visualKey(img);
}

function uniqueName(dir, stem, ext) {
  const ts = new Date().toISOString().replace(/[-:T]/g, '').slice(0, 14);
  const rand = crypto.randomBytes(3).toString('hex');
  return path.join(dir, `${stem}-${ts}-${rand}${ext}`);
}

module.exports = { Capture };
