#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""重录 README 顶部的演示 GIF（Rust 版，真实渲染 + 真实光标）。

为什么要重录：旧 GIF 是 960×124 的细条，首帧只有空背景，静态预览像一张空图。
这里改成 16:9 画幅、自绘一块"桌面"背景、首帧就是展开状态，且结尾回到展开
状态，循环播放不会跳。

做法（全程不碰用户真实数据）：
  1. 建一块 TOPMOST 的无边框窗口当"桌面"，用渐变位图当背景刷；
     图墙自己是 TOPMOST，会把自己抬到它上面，于是透明区域露出这块背景。
  2. 用 TQ_DATA_DIR / TQ_SHOTS_DIR 把数据目录和截图源指到临时目录，
     往里塞 4 张代码生成的假截图（启动时加载到绳上）。
  3. 用 SetCursorPos + mouse_event 走位：悬停相片 → 单击复制 → 移开收起
     → 顶边悬停呼出，边操作边 BitBlt 抓帧，直接灌给 ffmpeg 存成无损中间片，
     最后再合成调色板优化的 GIF。
  4. 结束还原光标位置、杀掉测试实例、删临时目录。

安全护栏：所有注入坐标先做围栏检查，单击前用 WindowFromPoint 确认落点属于
PicLineWindow，否则跳过并告警（2026-11 那次误注入的教训）。

用法：
  python scripts/record-demo.py --probe          # 只抓一帧，用来核构图/量坐标
  python scripts/record-demo.py                  # 正式录制，输出 docs/demo.gif
  python scripts/record-demo.py --exe <路径>     # 指定要录的 exe
"""

import ctypes
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import zlib
from ctypes import wintypes as wt

# ---------------------------------------------------------------- Win32 基础

user32 = ctypes.WinDLL("user32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

try:
    user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # PER_MONITOR_AWARE_V2
except Exception:
    user32.SetProcessDPIAware()

SW, SH = user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)
try:
    DPI = user32.GetDpiForSystem() or 96
except Exception:
    DPI = 96
SF = DPI / 96.0

# 画幅：抓 16:9 的一块，横向居中（绳子上的相片是绕屏幕中线排的）
CW, CH = 1200, 675
X0 = max(0, (SW - CW) // 2)
Y0 = 0

PANEL_H = int(210 * SF)  # 图墙面板的物理高度，和 gui.rs 的 PANEL_H * sf 对齐
# 成片只保留顶部这一条：相片 + 绳 + toast（逻辑 y=176..200 → 物理 264..300）都在 380 以内。
# 全抓 675 高再缩到 800x450 的话，下面 400px 全是空桌面，GIF 一半体积浪费在渐变上。
CROP_H = 380

# 与 gui.rs 保持一致（改那边记得同步这几个数）
CARD_W, CARD_H, SPACING, ROPE_TOP, EDGE_PAD = 150.0, 131.0, 174.0, 10.0, 24.0
N_PHOTOS = 4

SRCCOPY = 0x00CC0020
CAPTUREBLT = 0x40000000  # 不加它，BitBlt 抓不到 WS_EX_LAYERED 的分层窗口
DIB_RGB_COLORS = 0
CURSOR_SHOWING = 0x00000001
DI_NORMAL = 0x0003
WS_POPUP = 0x80000000
WS_VISIBLE = 0x10000000
WS_EX_TOPMOST = 0x00000008
WS_EX_TOOLWINDOW = 0x00000080
WS_EX_NOACTIVATE = 0x08000000
MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004


class WNDCLASSEXW(ctypes.Structure):
    _fields_ = [
        ("cbSize", wt.UINT), ("style", wt.UINT), ("lpfnWndProc", ctypes.c_void_p),
        ("cbClsExtra", ctypes.c_int), ("cbWndExtra", ctypes.c_int),
        ("hInstance", wt.HINSTANCE), ("hIcon", wt.HICON), ("hCursor", wt.HANDLE),
        ("hbrBackground", wt.HBRUSH), ("lpszMenuName", wt.LPCWSTR),
        ("lpszClassName", wt.LPCWSTR), ("hIconSm", wt.HICON),
    ]


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wt.DWORD), ("biWidth", ctypes.c_long), ("biHeight", ctypes.c_long),
        ("biPlanes", wt.WORD), ("biBitCount", wt.WORD), ("biCompression", wt.DWORD),
        ("biSizeImage", wt.DWORD), ("biXPelsPerMeter", ctypes.c_long),
        ("biYPelsPerMeter", ctypes.c_long), ("biClrUsed", wt.DWORD),
        ("biClrImportant", wt.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wt.DWORD * 3)]


class CURSORINFO(ctypes.Structure):
    _fields_ = [("cbSize", wt.DWORD), ("flags", wt.DWORD),
                ("hCursor", wt.HANDLE), ("ptScreenPos", wt.POINT)]


user32.DefWindowProcW.argtypes = [wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM]
user32.DefWindowProcW.restype = ctypes.c_ssize_t
user32.GetDC.restype = wt.HDC
user32.GetDC.argtypes = [wt.HWND]
gdi32.CreateCompatibleDC.restype = wt.HDC
gdi32.CreateCompatibleDC.argtypes = [wt.HDC]
gdi32.CreateCompatibleBitmap.restype = wt.HBITMAP
gdi32.CreateCompatibleBitmap.argtypes = [wt.HDC, ctypes.c_int, ctypes.c_int]
gdi32.SelectObject.restype = wt.HGDIOBJ
gdi32.SelectObject.argtypes = [wt.HDC, wt.HGDIOBJ]
gdi32.BitBlt.restype = wt.BOOL
gdi32.BitBlt.argtypes = [wt.HDC, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                         wt.HDC, ctypes.c_int, ctypes.c_int, wt.DWORD]
gdi32.GetDIBits.restype = ctypes.c_int
gdi32.GetDIBits.argtypes = [wt.HDC, wt.HBITMAP, wt.UINT, wt.UINT, ctypes.c_void_p,
                            ctypes.POINTER(BITMAPINFO), wt.UINT]

# 64 位下句柄是指针宽度，不声明 argtypes 会被截成 int（Overflow / 静默错句柄）
gdi32.CreateDIBSection.argtypes = [wt.HDC, ctypes.POINTER(BITMAPINFO), wt.UINT,
                                   ctypes.POINTER(ctypes.c_void_p), wt.HANDLE, wt.DWORD]
gdi32.CreateDIBSection.restype = wt.HBITMAP
gdi32.CreatePatternBrush.argtypes = [wt.HBITMAP]
gdi32.CreatePatternBrush.restype = wt.HBRUSH
user32.RegisterClassExW.argtypes = [ctypes.POINTER(WNDCLASSEXW)]
user32.RegisterClassExW.restype = wt.WORD
user32.CreateWindowExW.argtypes = [wt.DWORD, wt.LPCWSTR, wt.LPCWSTR, wt.DWORD,
                                   ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                   wt.HWND, wt.HMENU, wt.HINSTANCE, ctypes.c_void_p]
user32.CreateWindowExW.restype = wt.HWND
user32.ShowWindow.argtypes = [wt.HWND, ctypes.c_int]
user32.UpdateWindow.argtypes = [wt.HWND]
user32.DestroyWindow.argtypes = [wt.HWND]
user32.GetCursorPos.argtypes = [ctypes.POINTER(wt.POINT)]
user32.SetCursorPos.argtypes = [ctypes.c_int, ctypes.c_int]
user32.mouse_event.argtypes = [wt.DWORD, wt.DWORD, wt.DWORD, wt.DWORD, ctypes.c_void_p]
user32.DrawIconEx.argtypes = [wt.HDC, ctypes.c_int, ctypes.c_int, wt.HANDLE,
                              ctypes.c_int, ctypes.c_int, wt.UINT, wt.HANDLE, wt.UINT]
user32.GetCursorInfo.argtypes = [ctypes.POINTER(CURSORINFO)]
user32.WindowFromPoint.argtypes = [wt.POINT]
user32.WindowFromPoint.restype = wt.HWND
user32.GetClassNameW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
user32.PeekMessageW.argtypes = [ctypes.c_void_p, wt.HWND, wt.UINT, wt.UINT, wt.UINT]
kernel32.GetModuleHandleW.argtypes = [wt.LPCWSTR]
kernel32.GetModuleHandleW.restype = wt.HANDLE
user32.GetForegroundWindow.restype = wt.HWND
user32.FindWindowW.argtypes = [wt.LPCWSTR, wt.LPCWSTR]
user32.FindWindowW.restype = wt.HWND
user32.SetForegroundWindow.argtypes = [wt.HWND]
user32.BringWindowToTop.argtypes = [wt.HWND]
user32.SetActiveWindow.argtypes = [wt.HWND]
user32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.c_void_p]
user32.GetWindowThreadProcessId.restype = wt.DWORD
user32.AttachThreadInput.argtypes = [wt.DWORD, wt.DWORD, wt.BOOL]
user32.keybd_event.argtypes = [wt.BYTE, wt.BYTE, wt.DWORD, ctypes.c_void_p]
kernel32.GetCurrentThreadId.restype = wt.DWORD


def log(*a):
    print("[demo]", *a, flush=True)


# ---------------------------------------------------------------- 假截图


def _png(path, w, h, rows):
    raw = b"".join(b"\x00" + bytes(r) for r in rows)

    def chunk(tag, data):
        return (struct.pack(">I", len(data)) + tag + data +
                struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 6))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


def make_fake_screenshots(dst, n=4):
    """生成 n 张「像截图」的图：浅底 + 顶部色条 + 几行灰条 + 一个彩色按钮。"""
    themes = [
        ((36, 44, 54), (120, 160, 220)),   # 深色 IDE
        ((248, 249, 251), (232, 120, 90)),  # 浅色网页
        ((30, 46, 40), (120, 200, 160)),   # 终端
        ((250, 248, 244), (150, 120, 200)),  # 文档
    ]
    W, H = 720, 450
    os.makedirs(dst, exist_ok=True)
    paths = []
    for k in range(n):
        bg, accent = themes[k % len(themes)]
        dark = sum(bg) < 300
        rows = []
        for y in range(H):
            row = bytearray()
            for x in range(W):
                if y < 34:                                    # 标题栏
                    c = tuple(max(0, v - 12) for v in bg)
                elif 34 <= y < 38:                            # 分隔线
                    c = accent if k % 2 else tuple(max(0, v - 20) for v in bg)
                else:
                    c = bg
                row += bytes(c)
            rows.append(row)
        # 标题条上的三个小圆点
        for cx in (18, 36, 54):
            for y in range(12, 24):
                for x in range(cx - 6, cx + 6):
                    if (x - cx) ** 2 + (y - 18) ** 2 < 36:
                        rows[y][x * 3:x * 3 + 3] = bytes((200, 200, 205))
        # 正文假文字：长短不一的灰条
        ty = 74
        lengths = [0.62, 0.88, 0.45, 0.75, 0.58, 0.30]
        for i, frac in enumerate(lengths):
            yy = ty + i * 30
            if yy + 12 > H - 40:
                break
            wpx = int((W - 96) * frac)
            col = tuple(min(255, v + 26) if not dark else max(0, v + 22) for v in bg)
            for y in range(yy, yy + 11):
                rows[y][48 * 3:(48 + wpx) * 3] = bytes(col) * wpx
        # 一个强调色的"按钮"
        for y in range(H - 78, H - 40):
            for x in range(W - 200, W - 60):
                rows[y][x * 3:x * 3 + 3] = bytes(accent)
        p = os.path.join(dst, "%02d.png" % (k + 1))
        _png(p, W, H, rows)
        paths.append(p)
    return paths


# ---------------------------------------------------------------- 背景窗口


def force_foreground(hwnd):
    """把窗口顶到前台。Windows 会拦 SetForegroundWindow，先用一次 Alt 解锁。"""
    if user32.GetForegroundWindow() == hwnd:
        return True
    user32.keybd_event(0x12, 0, 0, None)   # VK_MENU down
    user32.keybd_event(0x12, 0, 2, None)   # VK_MENU up
    user32.BringWindowToTop(hwnd)
    user32.SetForegroundWindow(hwnd)
    if user32.GetForegroundWindow() == hwnd:
        return True
    fg = user32.GetForegroundWindow()
    tid_fg = user32.GetWindowThreadProcessId(fg, None) if fg else 0
    tid_me = kernel32.GetCurrentThreadId()
    if tid_fg and tid_fg != tid_me:
        user32.AttachThreadInput(tid_me, tid_fg, True)
        user32.SetForegroundWindow(hwnd)
        user32.AttachThreadInput(tid_me, tid_fg, False)
    return user32.GetForegroundWindow() == hwnd


def make_backdrop(pixels):
    """把一块渐变位图登记成窗口类的背景刷，窗口自己就把它铺满。"""
    hinst = kernel32.GetModuleHandleW(None)
    sdc = user32.GetDC(0)
    bi = BITMAPINFO()
    bi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bi.bmiHeader.biWidth = CW
    bi.bmiHeader.biHeight = -CH
    bi.bmiHeader.biPlanes = 1
    bi.bmiHeader.biBitCount = 32
    bi.bmiHeader.biCompression = 0
    bits = ctypes.c_void_p()
    hbm = gdi32.CreateDIBSection(sdc, ctypes.byref(bi), DIB_RGB_COLORS,
                                 ctypes.byref(bits), None, 0)
    ctypes.memmove(bits, pixels, len(pixels))
    brush = gdi32.CreatePatternBrush(hbm)

    proc = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM)(
        lambda h, m, wp, lp: user32.DefWindowProcW(h, m, wp, lp))

    wc = WNDCLASSEXW()
    wc.cbSize = ctypes.sizeof(WNDCLASSEXW)
    wc.style = 0
    wc.lpfnWndProc = ctypes.cast(proc, ctypes.c_void_p)
    wc.hInstance = hinst
    wc.hbrBackground = brush
    wc.lpszClassName = "PicLineDemoBackdrop"
    user32.RegisterClassExW(ctypes.byref(wc))

    hwnd = user32.CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        "PicLineDemoBackdrop", "demo", WS_POPUP | WS_VISIBLE,
        X0, Y0, CW, CH, 0, 0, hinst, 0)
    user32.ShowWindow(hwnd, 5)
    user32.UpdateWindow(hwnd)
    force_foreground(hwnd)
    return hwnd, proc


def gradient_pixels():
    """竖向渐变 + 上方一抹柔光，模拟深色壁纸。纯 Python 算 81 万像素，约 1 秒。"""
    top = (22, 30, 41)
    bot = (46, 62, 80)
    buf = bytearray(CW * CH * 4)
    cx, cy = CW / 2.0, -CH * 0.25
    r = CH * 1.15
    i = 0
    for y in range(CH):
        t = y / (CH - 1)
        br = top[0] + (bot[0] - top[0]) * t
        bg = top[1] + (bot[1] - top[1]) * t
        bb = top[2] + (bot[2] - top[2]) * t
        for x in range(CW):
            d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5 / r
            k = max(0.0, 1.0 - d) ** 2 * 26
            buf[i] = int(min(255, bb + k))
            buf[i + 1] = int(min(255, bg + k))
            buf[i + 2] = int(min(255, br + k))
            buf[i + 3] = 255
            i += 4
    return bytes(buf)


# ---------------------------------------------------------------- 抓帧


class Capturer:
    def __init__(self):
        self.sdc = user32.GetDC(0)
        self.mdc = gdi32.CreateCompatibleDC(self.sdc)
        self.hbmp = gdi32.CreateCompatibleBitmap(self.sdc, CW, CH)
        gdi32.SelectObject(self.mdc, self.hbmp)
        self.bi = BITMAPINFO()
        self.bi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
        self.bi.bmiHeader.biWidth = CW
        self.bi.bmiHeader.biHeight = -CH
        self.bi.bmiHeader.biPlanes = 1
        self.bi.bmiHeader.biBitCount = 32
        self.bi.bmiHeader.biCompression = 0
        self.buf = ctypes.create_string_buffer(CW * CH * 4)

    def grab(self):
        gdi32.BitBlt(self.mdc, 0, 0, CW, CH, self.sdc, X0, Y0, SRCCOPY | CAPTUREBLT)
        ci = CURSORINFO()
        ci.cbSize = ctypes.sizeof(CURSORINFO)
        if user32.GetCursorInfo(ctypes.byref(ci)) and (ci.flags & CURSOR_SHOWING):
            px = ci.ptScreenPos.x - X0
            py = ci.ptScreenPos.y - Y0
            if -32 <= px < CW and -32 <= py < CH:
                user32.DrawIconEx(self.mdc, px, py, ci.hCursor, 0, 0, 0, None, DI_NORMAL)
        gdi32.GetDIBits(self.mdc, self.hbmp, 0, CH, self.buf,
                        ctypes.byref(self.bi), DIB_RGB_COLORS)
        return self.buf.raw


# ---------------------------------------------------------------- 坐标与注入


def fence(x, y):
    if not (0 <= x < SW and 0 <= y < SH):
        raise RuntimeError("注入坐标越界: %d,%d" % (x, y))
    return x, y


def to_screen(x, y):
    """抓取坐标(Capturer 的像素) → 屏幕物理坐标。相片坐标全是抓取坐标，
    直接把 (208,172) 当屏幕坐标用，光标会跑到屏幕左边 680px 外的地方 —— 单击就点到别人窗口上了。"""
    return int(x) + X0, int(y) + Y0


def move(x, y):
    fence(x, y)
    user32.SetCursorPos(int(x), int(y))


def window_class_at(x, y):
    hwnd = user32.WindowFromPoint(wt.POINT(int(x), int(y)))
    if not hwnd:
        return ""
    buf = ctypes.create_unicode_buffer(256)
    user32.GetClassNameW(hwnd, buf, 256)
    return buf.value


def click(x, y, cap=None):
    """单击护栏：图墙的窗口是 WS_EX_TRANSPARENT，WindowFromPoint 永远穿过去，
    所以不能用"落点窗口类名"判定，改成 ①点必须在面板内 ②进程窗口存在
    ③抓一帧确认该位置确实有相框（不是背景）——三条都过才按下去。"""
    fence(x, y)
    if y >= PANEL_H:
        log("跳过单击：y=%d 在面板之外" % y)
        return False
    if not user32.FindWindowW("PicLineWindow", None):
        log("跳过单击：找不到图墙窗口")
        return False
    if cap is not None:
        f = cap.grab()
        q = (y - int(CARD_H / 2 * SF) + 8) * (CW * 4) + x * 4
        if q < 0 or q + 3 >= len(f) or f[q] + f[q + 1] + f[q + 2] < 500:
            log("跳过单击：(%d,%d) 处没看到相框" % (x, y))
            return False
    sx, sy = to_screen(x, y)
    user32.SetCursorPos(sx, sy)
    time.sleep(0.05)
    # 回读一次：远程桌面/远控客户端会持续把光标钉在它自己那边，
    # 这时 SetCursorPos 看着成功、实际位置已经被改掉 —— 按下去就点到别人窗口上了。
    now = wt.POINT()
    user32.GetCursorPos(ctypes.byref(now))
    if abs(now.x - sx) > 2 or abs(now.y - sy) > 2:
        log("跳过单击：光标没停在目标点（现在 %d,%d，期望 %d,%d）" % (now.x, now.y, sx, sy))
        return False
    user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0)
    time.sleep(0.04)
    user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0)
    return True


# ---------------------------------------------------------------- 主流程


def launch(exe, data_dir, shots_dir):
    env = dict(os.environ)
    env["TQ_DATA_DIR"] = data_dir
    env["TQ_SHOTS_DIR"] = shots_dir
    env.pop("TQ_FORCE_SHOW", None)
    env.pop("TQ_TRACE", None)
    return subprocess.Popen([exe], env=env, close_fds=True)


def prep_data(data_dir, shots_dir):
    os.makedirs(data_dir, exist_ok=True)
    os.makedirs(shots_dir, exist_ok=True)
    inbox = os.path.join(data_dir, "inbox")
    make_fake_screenshots(inbox, N_PHOTOS)
    # firstRunDone=false：启动 800ms 后会自己把绳子放下来，正好当首帧
    cfg = ('{"showLine": true, "takeOver": true, "clipboardWatch": true, '
           '"maxPegs": 10, "firstRunDone": false}')
    with open(os.path.join(data_dir, "config.json"), "w", encoding="utf-8") as f:
        f.write(cfg)


def layout_cards(n):
    """按 gui.rs 的 layout() 公式算出每张相片的屏幕坐标，再换算到抓取区域。"""
    w = SW / SF
    sag = min(30.0, w * 0.018)
    spacing = SPACING if n > 1 else 0.0
    out = []
    for i in range(n):
        x = w / 2.0 + (i - (n - 1) / 2.0) * spacing
        f = x / w
        top = ROPE_TOP + 4.0 * sag * f * (1.0 - f) + 12.0
        out.append((int(x * SF) - X0, int((top + CARD_H / 2.0) * SF), int(top * SF)))
    return out


def locate_cards(frame):
    """先信公式，用像素校验（相片位置不该是背景色），不行再退回视觉识别。"""
    step = CW * 4
    cards = layout_cards(N_PHOTOS)
    ok = True
    for x, y, top in cards:
        if not (0 <= x < CW and 0 <= y < CH):
            ok = False
            break
        # 相片内容有深有浅（深色编辑器截图就很暗），但**相框**一定是奶白色，
        # 所以拿卡片上沿那条边框做体检，而不是卡片中心。
        q = (top + 8) * step + x * 4
        if frame[q] + frame[q + 1] + frame[q + 2] < 500:
            ok = False
            break
    if ok:
        return [(x, y) for x, y, _t in cards]
    log("布局校验没过，改用视觉识别")
    return find_cards(frame)


def find_cards(frame):
    """在抓到的帧里找相片：面板区域里又白又高的那些列。"""
    stride = CW * 4
    lo, hi = 0, min(PANEL_H, CH)
    bright_cols = [0] * CW
    rows_hit = [0] * CH
    for y in range(lo, hi, 2):
        base = y * stride
        for x in range(0, CW, 2):
            p = base + x * 4
            if frame[p] + frame[p + 1] + frame[p + 2] > 570:  # 近白（相框）
                bright_cols[x] += 1
                rows_hit[y] += 1
    thresh = (hi - lo) / 2 * 0.25
    runs, cur = [], None
    for x in range(0, CW, 2):  # 采样时是隔列统计的，这里也必须隔列读，否则奇数列恒 0 会把区间切碎
        if bright_cols[x] > thresh:
            cur = [x, x] if cur is None else [cur[0], x]
        elif cur:
            runs.append(cur)
            cur = None
    if cur:
        runs.append(cur)
    runs = [r for r in runs if r[1] - r[0] > CW * 0.02]
    cards = []
    for a, b in runs:
        ys = [y for y in range(lo, hi) if rows_hit[y] > 0]
        cards.append(((a + b) // 2, (min(ys) + max(ys)) // 2 if ys else PANEL_H // 2))
    return cards


def record(exe, out_gif, probe=False):
    tmp = tempfile.mkdtemp(prefix="picline_demo_")
    data_dir = os.path.join(tmp, "data")
    shots_dir = os.path.join(tmp, "shots")
    orig = wt.POINT()
    user32.GetCursorPos(ctypes.byref(orig))
    proc = None
    backdrop = None
    cap = None
    ff = None
    try:
        prep_data(data_dir, shots_dir)
        log("屏 %dx%d  DPI %d  (%.0f%%)  抓取区域 %d,%d %dx%d  面板 %dpx"
            % (SW, SH, DPI, SF * 100, X0, Y0, CW, CH, PANEL_H))

        pixels = gradient_pixels()
        prev_fg = user32.GetForegroundWindow()
        backdrop, _wndproc = make_backdrop(pixels)
        log("前台已切到背景窗口：%s" % (user32.GetForegroundWindow() == backdrop))
        cap = Capturer()

        proc = launch(exe, data_dir, shots_dir)
        log("已启动 pid=%d，等首次呼出（800ms + 420ms 动画）" % proc.pid)
        # 图墙的规矩是「指针离开就收起」，800ms 首次呼出时人得已经在屏幕顶边等着，
        # 否则 350ms 后就自己缩回去了 —— 这里先把光标放到面板正中。
        move(SW // 2, max(40, PANEL_H // 2))
        t0 = time.perf_counter()
        while time.perf_counter() - t0 < 1.5:
            user32.PeekMessageW(None, 0, 0, 0, 1)
            time.sleep(0.02)

        frame = cap.grab()
        cards = locate_cards(frame)
        log("识别到 %d 张相片：%s" % (len(cards), cards))
        if len(cards) < 3 and not probe:
            raise RuntimeError("没找到相片，构图不对或窗口没展开")

        if probe:
            raw = os.path.join(tmp, "probe.bgra")
            with open(raw, "wb") as f:
                f.write(frame)
            png = os.path.abspath("target/demo-probe.png")
            os.makedirs(os.path.dirname(png), exist_ok=True)
            subprocess.run(["ffmpeg", "-y", "-v", "error", "-f", "rawvideo",
                            "-pix_fmt", "bgra", "-s", "%dx%d" % (CW, CH),
                            "-i", raw, "-frames:v", "1", png], check=True)
            log("探针帧 ->", png)
            return

        keep_mid = "--keep-mid" in sys.argv
        mid = os.path.join(tmp, "mid.mkv")
        if keep_mid:
            os.makedirs("target", exist_ok=True)
            mid = os.path.abspath("target/demo-mid.mkv")
        ff = subprocess.Popen(
            ["ffmpeg", "-y", "-v", "error", "-f", "rawvideo", "-pix_fmt", "bgra",
             "-s", "%dx%d" % (CW, CH), "-r", "15", "-i", "-",
             "-c:v", "ffv1", "-level", "3", "-pix_fmt", "bgr0", mid],
            stdin=subprocess.PIPE)

        # 时间线：起点已展开 → 悬停相片 → 单击复制 → 移开收起 → 顶边呼出
        # 节奏按「看的人不熟这个软件」来定：每步之间留 1s 以上，
        # 否则 5 秒跑完四个动作，观众还没认出这是截图就结束了。
        cx = (cards[0][0] + cards[-1][0]) // 2
        c1x, c1y = cards[0]
        c2x, c2y = cards[min(1, len(cards) - 1)]
        c3x, c3y = cards[min(2, len(cards) - 1)]
        # 相片三组坐标都是抓取坐标，注入前必须 to_screen()；cy 是相片中心，y 直接用即可。
        script = [
            (0.30, lambda: move(*to_screen(c1x, c1y))),
            (1.05, lambda: move(*to_screen(c3x, c3y))),
            (2.20, lambda: click(c3x, c3y, cap)),
            (3.60, lambda: move(*to_screen(cx, CROP_H + 80))),  # 往下走出画面 → 绳子收起
            (5.20, lambda: move(*to_screen(cx, 1))),            # 回到屏幕顶边 → 再滑出来
            (6.90, lambda: move(*to_screen(c2x, c2y))),
            (8.00, lambda: None),
        ]
        DURATION = 8.5
        n = 0
        start = time.perf_counter()
        idx = 0
        while True:
            now = time.perf_counter() - start
            if now >= DURATION:
                break
            while idx < len(script) and now >= script[idx][0]:
                script[idx][1]()
                idx += 1
            user32.PeekMessageW(None, 0, 0, 0, 1)
            ff.stdin.write(cap.grab())
            n += 1
            nxt = start + n / 15.0
            dt = nxt - time.perf_counter()
            if dt > 0:
                time.sleep(dt)
        log("录了 %d 帧（%.1fs）" % (n, DURATION))
        ff.stdin.close()
        ff.wait()

        os.makedirs(os.path.dirname(os.path.abspath(out_gif)), exist_ok=True)
        # 编码档位是量过的：15fps/192色/抖动 1.40MB，12fps/160色/无抖动 0.95MB，
        # 800px 宽下肉眼分不出差别，README 首屏值得省这 450KB。
        vf = ("fps=12,crop=%d:%d:0:0,scale=800:-2:flags=lanczos,split[a][b];" % (CW, CROP_H)
              + "[a]palettegen=max_colors=160:stats_mode=diff[p];"
              + "[b][p]paletteuse=dither=none:diff_mode=rectangle")
        subprocess.run(
            ["ffmpeg", "-y", "-v", "error", "-i", mid,
             "-filter_complex", vf,
             "-loop", "0", out_gif], check=True)
        size = os.path.getsize(out_gif)
        log("GIF -> %s  %.0f KB" % (out_gif, size / 1024))
    finally:
        user32.SetCursorPos(orig.x, orig.y)
        if ff and ff.poll() is None:
            ff.kill()
        if proc and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=3)
            except Exception:
                proc.kill()
        if backdrop:
            user32.DestroyWindow(backdrop)
        try:
            if prev_fg:
                user32.SetForegroundWindow(prev_fg)
        except Exception:
            pass
        shutil.rmtree(tmp, ignore_errors=True)


def main():
    args = sys.argv[1:]
    probe = "--probe" in args
    exe = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                       "dist", "PicLine-0.1.3-win-x64-portable.exe")
    if "--exe" in args:
        exe = args[args.index("--exe") + 1]
    if not os.path.isfile(exe):
        raise SystemExit("找不到 exe: %s" % exe)
    out = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                       "docs", "demo.gif")
    record(exe, out, probe=probe)


if __name__ == "__main__":
    main()
