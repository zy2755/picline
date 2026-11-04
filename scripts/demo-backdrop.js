// 录演示 GIF 时用的"舞台":一块无边框渐变窗口,铺满屏幕但当背景板。
//
// 为什么要它:真实桌面上有壁纸、图标、别的窗口,录进公开的 GIF 里既杂乱又涉隐私。
// 这块窗口只提供干净背景,图墙的晾衣绳照常浮在它上面 —— 顺便也演示了
// "它一直待在别的窗口之上"。
//
// 关键细节:高度故意少 8px,不让它盖满整块屏幕。否则图墙的全屏检测
// (前台窗口完整覆盖显示器且无标题栏)会把它当成全屏应用,晾衣绳就不出来了。
//
// 用法: electron scripts/demo-backdrop.js
'use strict';
const { app, BrowserWindow, screen } = require('electron');

app.disableHardwareAcceleration();          // 演示窗口不需要 GPU,省点启动开销

const PAGE = `data:text/html;charset=utf-8,${encodeURIComponent(`
<!doctype html><html><head><meta charset="utf-8"><style>
  html,body{height:100%;margin:0;overflow:hidden}
  body{
    background:
      radial-gradient(1200px 700px at 22% 8%, rgba(126,158,196,.34), transparent 62%),
      radial-gradient(900px 620px at 82% 2%, rgba(180,142,116,.22), transparent 60%),
      linear-gradient(158deg, #33445e 0%, #26313f 42%, #171d26 100%);
  }
  /* 极淡的斜向纹理,避免大面积纯渐变在 GIF 里出色带 */
  body::after{
    content:"";position:fixed;inset:0;
    background:repeating-linear-gradient(58deg, rgba(255,255,255,.016) 0 1px, transparent 1px 3px);
  }
</style></head><body></body></html>`)}`;

app.whenReady().then(() => {
  const b = screen.getPrimaryDisplay().bounds;
  const win = new BrowserWindow({
    x: b.x, y: b.y,
    width: b.width, height: Math.max(200, b.height - 8),
    frame: false,
    transparent: false,
    resizable: false,
    movable: false,
    minimizable: false,
    maximizable: false,
    fullscreenable: false,
    skipTaskbar: true,
    focusable: false,          // 永不抢焦点:前台窗口保持原样,全屏判定不受影响
    hasShadow: false,
    thickFrame: false,
    show: false,
  });
  win.setAlwaysOnTop(true, 'floating');   // 盖住普通窗口,但低于晾衣绳的 screen-saver 层级
  win.setMenuBarVisibility(false);
  win.loadURL(PAGE);
  win.once('ready-to-show', () => win.showInactive());
});

app.on('window-all-closed', () => app.quit());
