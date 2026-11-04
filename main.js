// 图墙 —— 截图,晾起来。(Windows 版,创意来自 macOS 应用 Tendedero)
// 主进程:多显示器晾衣绳窗口、托盘、全局快捷键、截图捕获、全屏检测。
//
// 移植自原版的行为参数:
//   - 指针在顶边停留 250ms 呼出(AppDelegate.revealDelay)
//   - 新截图到达时若线未展开,探头展示 2.5s(peekFor)
//   - 全屏应用前台时自动隐藏(FullScreen.swift)
//   - Ctrl+Alt+T 显示/隐藏(原版 ⌃⌥T)
'use strict';

const { app, BrowserWindow, Tray, Menu, globalShortcut, ipcMain, screen,
  clipboard, nativeImage, dialog, shell } = require('electron');
const path = require('path');
const fs = require('fs');
const { spawn } = require('child_process');

const { initDataDirs, getInboxDir, resolvePicturesDir } = require('./lib/paths');
const settings = require('./lib/settings');
const { Capture } = require('./lib/capture');
const { FullscreenMonitor } = require('./lib/fullscreen');

const PANEL_H = 210;              // 晾衣绳窗口高度(原版 Layout.panelHeight)
const REVEAL_DELAY = 250;         // 顶边停留呼出延时(ms,原版 0.25s)
const BAND_H = 4;                 // 顶边触发带厚度(px)
const PEEK_MS = 2500;             // 新截图探头展示时长
const HIDE_DELAY = 300;           // 指针离开后收起延时

// ---------- 全局状态 ----------
let tray = null;
let capture = null;
let fullscreen = null;
let lineWindows = new Map();      // display.id -> {win, shown, pinned, hideTimer, peekTimer, sinceInBand}
let picturesDir = null;           // 图片库(延迟解析)
let quitting = false;

// ---------- 单实例 ----------
if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => { /* 已在运行:忽略(托盘图标常在) */ });
}

app.setName('图墙');
initDataDirs();

// ---------- 晾衣绳窗口 ----------
function createLineWindows() {
  for (const { win } of lineWindows.values()) {
    try { win.destroy(); } catch {}
  }
  lineWindows = new Map();
  for (const d of screen.getAllDisplays()) {
    const win = new BrowserWindow({
      x: d.workArea.x,
      y: d.workArea.y,
      width: d.workArea.width,
      height: PANEL_H,
      frame: false,
      transparent: true,
      resizable: false,
      movable: false,
      minimizable: false,
      maximizable: false,
      fullscreenable: false,
      show: false,
      skipTaskbar: true,
      focusable: false,          // 不抢焦点:悬停/点击不打断当前输入
      hasShadow: false,
      thickFrame: false,
      webPreferences: {
        nodeIntegration: true,
        contextIsolation: false,
        backgroundThrottling: false,
        spellcheck: false,
      },
    });
    win.setAlwaysOnTop(true, 'screen-saver');
    win.setIgnoreMouseEvents(true);
    win.loadFile(path.join(__dirname, 'src', 'line.html'));
    lineWindows.set(d.id, { win, shown: false, pinned: false, hideTimer: null, peekTimer: null, sinceInBand: 0 });
  }
}

function broadcast(channel, ...args) {
  for (const { win } of lineWindows.values()) {
    if (!win.isDestroyed()) win.webContents.send(channel, ...args);
  }
}

function revealLine(entry, { pinned = false, peek = false } = {}) {
  if (!settings.get('showLine')) return;
  const { win } = entry;
  if (win.isDestroyed()) return;
  clearTimeout(entry.hideTimer);
  clearTimeout(entry.peekTimer);
  entry.shown = true;
  entry.pinned = pinned;
  try {
    win.setIgnoreMouseEvents(false);
    win.showInactive();
    win.webContents.send('reveal');
  } catch {}
  if (peek) {
    entry.peekTimer = setTimeout(() => {
      if (!entry.shown) return;
      // 探头期间指针没进来就收回
      const pt = screen.getCursorScreenPoint();
      const b = win.getBounds();
      const inside = pt.x >= b.x && pt.x <= b.x + b.width && pt.y >= b.y && pt.y <= b.y + b.height;
      if (!inside) hideLine(entry);
    }, PEEK_MS);
  }
}

function hideLine(entry) {
  const { win } = entry;
  if (!entry.shown || win.isDestroyed()) return;
  entry.shown = false;
  entry.pinned = false;
  clearTimeout(entry.hideTimer);
  clearTimeout(entry.peekTimer);
  try {
    win.webContents.send('hide');
  } catch {}
  // 等收起动画播完再恢复点击穿透
  setTimeout(() => { if (!entry.shown && !win.isDestroyed()) win.setIgnoreMouseEvents(true); }, 260);
}

function togglePinned() {
  for (const entry of lineWindows.values()) {
    if (entry.shown && entry.pinned) hideLine(entry);
    else revealLine(entry, { pinned: true });
  }
}

function hideAll() { for (const e of lineWindows.values()) hideLine(e); }

// ---------- 顶边悬停轮询(原版 mouseTimer 逻辑) ----------
function startCursorPolling() {
  setInterval(() => {
    const pt = screen.getCursorScreenPoint();
    const now = Date.now();
    const d = screen.getDisplayNearestPoint(pt);
    if (!d) return;

    for (const [displayId, entry] of lineWindows) {
      if (entry.win.isDestroyed()) continue;
      const disp = screen.getAllDisplays().find(x => x.id === displayId);
      if (!disp) continue;

      if (displayId === d.id) {
        const inBand = pt.y >= disp.workArea.y && pt.y <= disp.workArea.y + BAND_H
          && pt.x >= disp.workArea.x + disp.workArea.width * 0.22
          && pt.x <= disp.workArea.x + disp.workArea.width * 0.78;
        const inStrip = (() => {
          const b = entry.win.getBounds();
          return pt.x >= b.x - 4 && pt.x <= b.x + b.width + 4 && pt.y >= b.y - 4 && pt.y <= b.y + b.height;
        })();

        if (entry.shown) {
          // 已展开:指针在条带内 → 留着;离开 → 延时收起(pinned 时不自动收)
          if (inStrip) {
            clearTimeout(entry.hideTimer);
          } else if (!entry.pinned) {
            if (!entry.hideTimer) {
              entry.hideTimer = setTimeout(() => hideLine(entry), HIDE_DELAY);
            }
          }
          entry.sinceInBand = 0;
        } else if (inBand && settings.get('showLine') && !fullscreen.isFullscreen(disp)) {
          // 停留 250ms 才呼出(原版 revealDelay)
          if (!entry.sinceInBand) entry.sinceInBand = now;
          else if (now - entry.sinceInBand >= REVEAL_DELAY) revealLine(entry);
        } else {
          entry.sinceInBand = 0;
        }
      } else if (entry.shown && !entry.pinned) {
        // 指针去了别的显示器:那边收起
        if (!entry.hideTimer) entry.hideTimer = setTimeout(() => hideLine(entry), HIDE_DELAY);
      }
    }
  }, 120);
}

// ---------- 捕获 ----------
function startCapture() {
  capture = new Capture({
    inboxDir: getInboxDir(),
    screenshotsDir: path.join(picturesDir, 'Screenshots'),
  });
  capture.setOptions({
    takeOver: settings.get('takeOver'),
    clipboardWatch: settings.get('clipboardWatch'),
  });
  let first = true;
  capture.on('items', (items) => {
    broadcast('items', items, settings.get('maxPegs'));
    if (!first && items.length > 0) {
      // 新截图到达:若线收着,探头 2.5s(原版 peekFor)
      for (const [displayId, entry] of lineWindows) {
        if (entry.win.isDestroyed() || entry.shown) continue;
        const disp = screen.getAllDisplays().find(x => x.id === displayId);
        if (!disp || fullscreen.isFullscreen(disp)) continue;
        revealLine(entry, { peek: true });
      }
    }
    first = false;
  });
  capture.start();
}

// ---------- 托盘 ----------
function createTray() {
  tray = new Tray(path.join(__dirname, 'assets', 'icon-32.png'));
  tray.setToolTip('图墙 —— 截图,晾起来');
  tray.setContextMenu(buildTrayMenu());
  tray.on('click', () => tray.popUpContextMenu());
}

function buildTrayMenu() {
  const s = settings.load();
  const login = app.getLoginItemSettings();
  return Menu.buildFromTemplate([
    { label: '显示 / 收起晾衣绳\tCtrl+Alt+T', click: () => togglePinned() },
    { type: 'separator' },
    { label: '接管截图文件夹(新截图移入收件箱)',
      type: 'checkbox', checked: s.takeOver,
      click: (mi) => { settings.set('takeOver', mi.checked); capture.setOptions({ takeOver: mi.checked }); } },
    { label: '监听剪贴板截图(Win+Shift+S 未落盘时)',
      type: 'checkbox', checked: s.clipboardWatch,
      click: (mi) => { settings.set('clipboardWatch', mi.checked); capture.setOptions({ clipboardWatch: mi.checked }); } },
    { type: 'separator' },
    { label: '打开收件箱', click: () => shell.openPath(getInboxDir()) },
    { label: '开机自启', type: 'checkbox', checked: login.openAtLogin,
      click: (mi) => app.setLoginItemSettings({ openAtLogin: mi.checked }) },
    { type: 'separator' },
    { label: '关于 图墙', click: () => dialog.showMessageBox({
        type: 'info', title: '关于 图墙', message: '图墙 v' + app.getVersion(),
        detail: '截图,晾起来。\n\n灵感来自 Alejandro Buján 的 Tendedero(macOS)。\n本作品为其 Windows 移植,代码以 MIT 许可发布。' }) },
    { label: '退出', click: () => { quitting = true; app.quit(); } },
  ]);
}

// ---------- IPC(渲染端手势) ----------
function findItem(id) {
  return capture.listInbox().find(it => it.id === id) || null;
}

function registerIpc() {
  ipcMain.handle('items:get', () => capture.listInbox());

  ipcMain.on('item:copy', (e, id) => {
    const it = findItem(id);
    if (!it) return;
    try {
      clipboard.writeImage(nativeImage.createFromPath(it.file));
      capture.suppressCopy();         // 读取刚写入的剪贴板取签名,抑制轮询把自己再挂上去
    } catch {}
  });

  ipcMain.on('item:open', (e, id) => {
    const it = findItem(id);
    if (it) shell.openPath(it.file);
  });

  ipcMain.on('item:edit', (e, id) => {
    const it = findItem(id);
    if (!it) return;
    try { spawn('mspaint.exe', [it.file], { detached: true, stdio: 'ignore' }).unref(); } catch {}
  });

  ipcMain.handle('item:save-as', async (e, id) => {
    const it = findItem(id);
    if (!it) return false;
    const { canceled, filePath } = await dialog.showSaveDialog({
      defaultPath: it.id,
      filters: [{ name: '图片', extensions: [path.extname(it.id).replace('.', '') || 'png'] }],
    });
    if (canceled || !filePath) return false;
    try { fs.copyFileSync(it.file, filePath); return true; } catch { return false; }
  });

  // 取下:移回系统截图文件夹,离开晾衣绳
  ipcMain.handle('item:take-down', async (e, id) => {
    const it = findItem(id);
    if (!it) return false;
    const dest = path.join(picturesDir, 'Screenshots', it.id);
    try {
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      fs.renameSync(it.file, dest);
      capture.rescan();
      return true;
    } catch {
      try { fs.copyFileSync(it.file, dest); fs.unlinkSync(it.file); capture.rescan(); return true; }
      catch { return false; }
    }
  });

  // 删除:送回收站(原版拖到废纸篓 / 点 ×)
  // 用 invoke 返回真实结果:失败时渲染端会把相片放回绳上并提示,不再静默吞掉异常
  ipcMain.handle('item:discard', async (e, id) => {
    const it = findItem(id);
    if (!it) { capture.rescan(); return { ok: true }; }   // 已经不在收件箱:UI 与磁盘一致
    if (!path.isAbsolute(it.file)) return { ok: false, error: '路径无效:' + it.file };
    if (!fs.existsSync(it.file)) { capture.rescan(); return { ok: true }; }
    try {
      // Windows 上 shell.trashItem 会因文件被占用 / 路径过长 / 网络盘等原因 reject
      await shell.trashItem(it.file);
    } catch (err) {
      capture.rescan();
      return { ok: false, error: (err && err.message) || String(err) };
    }
    capture.rescan();
    return { ok: true };
  });

  // 拖出:交给系统拖放(拖进应用=发送副本;拖进文件夹=保存一份,留在绳上)
  ipcMain.on('item:drag', (e, id, dataUrl) => {
    const it = findItem(id);
    if (!it) return;
    try {
      let icon = nativeImage.createFromPath(path.join(__dirname, 'assets', 'icon-32.png'));
      try {
        const fromData = nativeImage.createFromDataURL(dataUrl || '');
        if (!fromData.isEmpty()) icon = fromData;
      } catch {}
      e.sender.startDrag({ files: [it.file], icon });
    } catch {}
  });

  ipcMain.on('line:hide-blank', () => hideAll());
  ipcMain.on('line:drag-active', (e, active) => {
    // 拖放进行中:抑制自动收起 3.5s,防止拖到一半线消失
    for (const entry of lineWindows.values()) {
      clearTimeout(entry.hideTimer);
      if (active) {
        entry.hideTimer = setTimeout(() => { entry.hideTimer = null; }, 3500);
      }
    }
  });
}

// ---------- 生命周期 ----------
app.whenReady().then(async () => {
  picturesDir = await resolvePicturesDir();
  createLineWindows();
  fullscreen = new FullscreenMonitor();
  fullscreen.start();
  startCapture();
  createTray();
  registerIpc();
  startCursorPolling();

  globalShortcut.register('Ctrl+Alt+T', () => togglePinned());

  screen.on('display-added', createLineWindows);
  screen.on('display-removed', createLineWindows);
  screen.on('display-metrics-changed', () => setTimeout(createLineWindows, 200));

  // 首次运行:展开晾衣绳 4 秒,让人知道它在那儿
  if (!settings.get('firstRunDone')) {
    settings.set('firstRunDone', true);
    setTimeout(() => {
      let any = false;
      for (const entry of lineWindows.values()) { revealLine(entry, { peek: true }); any = true; }
    }, 800);
  }

  app.on('quit', () => {
    try { capture && capture.stop(); } catch {}
    try { fullscreen && fullscreen.stop(); } catch {}
    try { globalShortcut.unregisterAll(); } catch {}
  });
});

app.on('window-all-closed', () => { /* 托盘常驻,不退出 */ });
app.on('before-quit', () => { quitting = true; });
