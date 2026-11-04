#!/usr/bin/env python3
# 录制演示 GIF 的前半段:按时间线移动真实光标,并逐帧截取屏幕顶部条带。
#
# 为什么不用 PowerShell / Pillow:本机 PowerShell 的 Add-Type 与反射加载被安全策略
# 拦掉,pip 也连不上网。于是只用标准库 ctypes 调 GDI/user32 —— 截屏 + 画光标 + 移动指针。
#
# 背景由 scripts/demo-backdrop.js 提供(一块干净的渐变窗口),所以这里截到的
# 只有"舞台 + 晾衣绳",不会有桌面壁纸、图标或别的窗口。
#
# 用法: python scripts/record-demo.py --out <帧目录> [--fps 12] [--height 0]
import argparse
import ctypes
import ctypes.wintypes as wt
import os
import time

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

try:                                  # 物理像素:否则高 DPI 下截到的图会被系统缩放
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception:
    user32.SetProcessDPIAware()

SRCCOPY = 0x00CC0020
DIB_RGB_COLORS = 0
BI_RGB = 0


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [("biSize", wt.DWORD), ("biWidth", wt.LONG), ("biHeight", wt.LONG),
                ("biPlanes", wt.WORD), ("biBitCount", wt.WORD), ("biCompression", wt.DWORD),
                ("biSizeImage", wt.DWORD), ("biXPelsPerMeter", wt.LONG),
                ("biYPelsPerMeter", wt.LONG), ("biClrUsed", wt.DWORD),
                ("biClrImportant", wt.DWORD)]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wt.DWORD * 3)]


def cursor_pos():
    p = wt.POINT()
    user32.GetCursorPos(ctypes.byref(p))
    return p.x, p.y


def set_cursor(x, y):
    user32.SetCursorPos(int(round(x)), int(round(y)))


def click():
    """左键单击(按下 + 抬起)。"""
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.02)
    user32.mouse_event(0x0004, 0, 0, 0, 0)


ARROW = [(0, 0), (0, 16), (4, 12), (6.6, 19), (9.2, 17.8), (6.6, 11), (11, 11)]


def draw_cursor(hdc, x, y, scale=1.25):
    """在内存 DC 上画一个白色箭头光标(带黑边),让 GIF 里有指针感。
    CopyFromScreen/BitBlt 不带光标,不画的话动画会像自己动的。"""
    pts = [(int(x + px * scale), int(y + py * scale)) for px, py in ARROW]
    arr = (wt.POINT * len(pts))(*[wt.POINT(a, b) for a, b in pts])
    white = gdi32.CreateSolidBrush(0x00FFFFFF)
    black = gdi32.CreateSolidBrush(0x00000000)
    pen = gdi32.CreatePen(0, 1, 0x00000000)          # PS_SOLID, 1px 黑
    old_pen = gdi32.SelectObject(hdc, pen)

    gdi32.SelectObject(hdc, black)                   # 先画一圈稍大的黑边
    big = (wt.POINT * len(pts))(*[wt.POINT(int(x + px * scale) - 1, int(y + py * scale) - 1)
                                  for px, py in pts])
    gdi32.Polygon(hdc, big, len(pts))
    gdi32.SelectObject(hdc, white)                   # 再填白色
    gdi32.Polygon(hdc, arr, len(pts))

    gdi32.SelectObject(hdc, old_pen)
    gdi32.DeleteObject(pen)
    gdi32.DeleteObject(white)
    gdi32.DeleteObject(black)


def write_bmp(path, w, h, data):
    """24 位 BMP。GetDIBits 拿的是自上而下的行,所以头里写负高度,
    否则整张图会被上下颠倒。"""
    row = (w * 3 + 3) & ~3
    size = row * h
    hdr = bytearray()
    hdr += b"BM"
    hdr += (14 + 40 + size).to_bytes(4, "little")
    hdr += (0).to_bytes(4, "little")
    hdr += (54).to_bytes(4, "little")
    hdr += (40).to_bytes(4, "little")
    hdr += w.to_bytes(4, "little", signed=True)
    hdr += (-h).to_bytes(4, "little", signed=True)
    hdr += (1).to_bytes(2, "little")
    hdr += (24).to_bytes(2, "little")
    hdr += (BI_RGB).to_bytes(4, "little")
    hdr += size.to_bytes(4, "little")
    hdr += bytes(16)
    with open(path, "wb") as f:
        f.write(hdr)
        f.write(data)


# 时间线用逻辑像素(和 CSS 一致);写光标前再乘 DPI 缩放。
# 应用里:相片间距 174、面板高 210、顶边触发带 4px、toast 贴在底部。
# 这一段讲的故事:指针上移→顶边停一下→绳子滑下来→悬停一张相片→单击(复制)
#              →指针移开→绳子收回。
def default_timeline(w_logical):
    cx = w_logical // 2
    return (
        [(0.00, cx, 420), (0.85, cx, 420), (1.25, cx, 2), (2.25, cx, 2),
         (2.90, cx - 174, 100), (3.45, cx - 174, 100), (4.35, cx - 174, 100),
         (4.95, cx - 174, 520), (6.00, cx - 174, 520)],
        [3.55],                       # 这一次单击:相片 → 复制到剪贴板(底部 toast 反馈)
    )


def pos_at(keys, t):
    if t <= keys[0][0]:
        return keys[0][1], keys[0][2]
    for (t0, x0, y0), (t1, x1, y1) in zip(keys, keys[1:]):
        if t0 <= t <= t1:
            k = 0 if t1 == t0 else (t - t0) / (t1 - t0)
            e = k * k * (3 - 2 * k)          # smoothstep:像人手的加减速
            return x0 + (x1 - x0) * e, y0 + (y1 - y0) * e
    return keys[-1][1], keys[-1][2]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--fps", type=float, default=12)
    ap.add_argument("--height", type=int, default=0, help="截取高度(物理像素);0 = 面板高 + 余量")
    args = ap.parse_args()

    os.makedirs(args.out, exist_ok=True)
    w = user32.GetSystemMetrics(0)
    scale = 1.0
    try:
        scale = user32.GetDpiForSystem() / 96.0     # 高 DPI 下界面按逻辑像素布局
    except Exception:
        pass
    w_logical = int(w / scale)
    h = args.height or int(210 * scale) + 16     # 面板 210 逻辑像素 + 一点余量
    h = min(h, user32.GetSystemMetrics(1))
    row = (w * 3 + 3) & ~3

    hdc_screen = user32.GetDC(0)
    hdc_mem = gdi32.CreateCompatibleDC(hdc_screen)
    bits = gdi32.CreateCompatibleBitmap(hdc_screen, w, h)
    gdi32.SelectObject(hdc_mem, bits)

    bmi = BITMAPINFO()
    bmi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bmi.bmiHeader.biWidth = w
    bmi.bmiHeader.biHeight = -h                 # 负数 = 自上而下
    bmi.bmiHeader.biPlanes = 1
    bmi.bmiHeader.biBitCount = 24
    bmi.bmiHeader.biCompression = BI_RGB

    buf = ctypes.create_string_buffer(row * h)

    keys, clicks = default_timeline(w_logical)
    duration = keys[-1][0]
    frames = int(duration * args.fps)
    print(f"屏幕 {w}x{user32.GetSystemMetrics(1)} @ {scale}x,截取 {w}x{h},共 {frames} 帧 @ {args.fps}fps")

    saved = cursor_pos()                        # 录完还原指针
    try:
        t_start = time.perf_counter()
        fired = set()
        for i in range(frames):
            t = i / args.fps
            lx, ly = pos_at(keys, t)
            x, y = lx * scale, ly * scale       # 逻辑 → 物理
            set_cursor(x, y)

            for j, ct in enumerate(clicks):
                if ct <= t < ct + 1.0 / args.fps and j not in fired:
                    fired.add(j)
                    click()

            gdi32.BitBlt(hdc_mem, 0, 0, w, h, hdc_screen, 0, 0, SRCCOPY)
            draw_cursor(hdc_mem, x, y)
            gdi32.GetDIBits(hdc_mem, bits, 0, h, buf, ctypes.byref(bmi), DIB_RGB_COLORS)
            write_bmp(os.path.join(args.out, f"f{i:03d}.bmp"), w, h, buf.raw)

            # 按帧率节流,避免截图耗时把节奏拖乱
            target = t_start + (i + 1) / args.fps
            lag = target - time.perf_counter()
            if lag > 0:
                time.sleep(lag)
    finally:
        set_cursor(*saved)
        gdi32.DeleteObject(bits)
        gdi32.DeleteDC(hdc_mem)
        user32.ReleaseDC(0, hdc_screen)
    print(f"已写出 {frames} 帧到 {args.out}")


if __name__ == "__main__":
    main()
