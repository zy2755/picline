// 全屏检测:维护一个常驻 PowerShell 子进程,解析前台窗口,
// 判定各显示器是否被全屏应用占据(全屏时晾衣绳自动藏起,和原版一致)。
const { spawn } = require('child_process');
const path = require('path');
const readline = require('readline');
const { EventEmitter } = require('events');

const WS_CAPTION = 0x00C00000;
const SHELL_CLASSES = new Set(['Progman', 'WorkerW', 'Shell_TrayWnd']);

class FullscreenMonitor extends EventEmitter {
  constructor() {
    super();
    this.child = null;
    this.fg = null;              // 最近一次前台窗口信息(物理像素)
    this.restartTimer = null;
    this.stopped = false;
  }

  start() {
    this.stopped = false;
    this.spawnChild();
  }

  spawnChild() {
    if (this.stopped) return;
    try {
      this.child = spawn('powershell.exe',
        ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
          '-File', path.join(__dirname, 'fullscreen.ps1')],
        { windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'] });
    } catch { return; }
    const rl = readline.createInterface({ input: this.child.stdout });
    rl.on('line', (line) => {
      const s = line.trim();
      if (!s.startsWith('{')) return;
      try { this.fg = JSON.parse(s); this.emit('fg', this.fg); } catch {}
    });
    this.child.on('exit', () => {
      this.fg = null;
      if (!this.stopped) {
        clearTimeout(this.restartTimer);
        this.restartTimer = setTimeout(() => this.spawnChild(), 3000);
      }
    });
    this.child.on('error', () => { /* powershell 不可用时静默降级:全屏检测失效 */ });
  }

  stop() {
    this.stopped = true;
    clearTimeout(this.restartTimer);
    if (this.child) { try { this.child.kill(); } catch {} }
    this.fg = null;
  }

  // 某显示器(传入 Electron display)当前是否全屏
  isFullscreen(display) {
    const fg = this.fg;
    if (!fg || fg.c === undefined) return false;
    if (SHELL_CLASSES.has(fg.c)) return false;
    if ((fg.w || '').startsWith('图墙')) return false;         // 我们自己的窗口
    if (fg.st & WS_CAPTION) return false;                      // 有标题栏就不是全屏
    const sf = display.scaleFactor || 1;
    const mL = Math.round(display.bounds.x * sf);
    const mT = Math.round(display.bounds.y * sf);
    const mR = Math.round((display.bounds.x + display.bounds.width) * sf);
    const mB = Math.round((display.bounds.y + display.bounds.height) * sf);
    // 前台窗口完整覆盖该显示器(留 2px 容差)→ 全屏
    return fg.l <= mL + 2 && fg.t <= mT + 2 && fg.r >= mR - 2 && fg.b >= mB - 2;
  }
}

module.exports = { FullscreenMonitor };
