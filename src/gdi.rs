//! GDI+ 绘制封装：把内容画到 32bpp ARGB 内存位图，再转成 UpdateLayeredWindow
//! 需要的**预乘 alpha BGRA**写进 DIBSection。
//!
//! 为什么不用"GDI+ 直接从 HDC 创建 Graphics"：HDC 上的 DIBSection 不保证
//! GDI+ 写回 alpha 通道，半透明区域会变黑或全透。走 Bitmap + LockBits 最稳。

#![allow(dead_code)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

pub const PIXEL_ARGB: i32 = 0x0026_200A; // PixelFormat32bppARGB
pub const UNIT_PIXEL: i32 = 2;
pub const SMOOTH_ANTIALIAS: i32 = 4;
pub const TEXT_ANTIALIAS: i32 = 3;
pub const MATRIX_PREPEND: i32 = 0;
pub const FONT_REGULAR: i32 = 0;
pub const ALIGN_CENTER: i32 = 1;
pub const ALIGN_NEAR: i32 = 0;

#[repr(C)]
pub struct GpRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
#[repr(C)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
#[repr(C)]
pub struct GpPoint {
    pub x: i32,
    pub y: i32,
}
#[repr(C)]
pub struct BitmapData {
    pub width: u32,
    pub height: u32,
    pub stride: i32,
    pub pixel_format: i32,
    pub scan0: *mut u8,
    pub reserved: usize,
}
#[repr(C)]
struct StartupInput {
    version: u32,
    debug_event_callback: *mut c_void,
    suppress_background_thread: i32,
    suppress_external_codecs: i32,
}

#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(
        token: *mut usize,
        input: *const StartupInput,
        output: *mut c_void,
    ) -> i32;
    pub fn GdiplusShutdown(token: usize);
    pub fn GdipCreateBitmapFromScan0(
        w: i32,
        h: i32,
        stride: i32,
        format: i32,
        scan0: *mut u8,
        bitmap: *mut *mut c_void,
    ) -> i32;
    pub fn GdipGetImageGraphicsContext(image: *mut c_void, g: *mut *mut c_void) -> i32;
    pub fn GdipDeleteGraphics(g: *mut c_void) -> i32;
    pub fn GdipDisposeImage(image: *mut c_void) -> i32;
    pub fn GdipGraphicsClear(g: *mut c_void, color: u32) -> i32;
    pub fn GdipSetSmoothingMode(g: *mut c_void, mode: i32) -> i32;
    pub fn GdipSetTextRenderingHint(g: *mut c_void, hint: i32) -> i32;
    pub fn GdipCreatePen1(color: u32, width: f32, unit: i32, pen: *mut *mut c_void) -> i32;
    pub fn GdipDeletePen(pen: *mut c_void) -> i32;
    pub fn GdipCreateSolidFill(color: u32, brush: *mut *mut c_void) -> i32;
    pub fn GdipDeleteBrush(brush: *mut c_void) -> i32;
    pub fn GdipFillRectangleI(g: *mut c_void, brush: *mut c_void, x: i32, y: i32, w: i32, h: i32) -> i32;
    pub fn GdipFillEllipseI(g: *mut c_void, brush: *mut c_void, x: i32, y: i32, w: i32, h: i32) -> i32;
    pub fn GdipDrawLineI(g: *mut c_void, pen: *mut c_void, x1: i32, y1: i32, x2: i32, y2: i32) -> i32;
    pub fn GdipDrawBezierI(
        g: *mut c_void,
        pen: *mut c_void,
        x1: i32, y1: i32, x2: i32, y2: i32,
        x3: i32, y3: i32, x4: i32, y4: i32,
    ) -> i32;
    pub fn GdipDrawImageRectI(g: *mut c_void, image: *mut c_void, x: i32, y: i32, w: i32, h: i32) -> i32;
    pub fn GdipDrawString(
        g: *mut c_void,
        text: *const u16,
        len: i32,
        font: *mut c_void,
        layout: *const RectF,
        format: *mut c_void,
        brush: *mut c_void,
    ) -> i32;
    pub fn GdipCreateFontFamilyFromName(
        name: *const u16,
        collection: *mut c_void,
        family: *mut *mut c_void,
    ) -> i32;
    pub fn GdipCreateFont(
        family: *mut c_void,
        size: f32,
        style: i32,
        unit: i32,
        font: *mut *mut c_void,
    ) -> i32;
    pub fn GdipDeleteFont(font: *mut c_void) -> i32;
    pub fn GdipDeleteFontFamily(family: *mut c_void) -> i32;
    pub fn GdipCreateStringFormat(attrs: i32, lang: u16, fmt: *mut *mut c_void) -> i32;
    pub fn GdipSetStringFormatAlign(fmt: *mut c_void, align: i32) -> i32;
    pub fn GdipSetStringFormatLineAlign(fmt: *mut c_void, align: i32) -> i32;
    pub fn GdipDeleteStringFormat(fmt: *mut c_void) -> i32;
    pub fn GdipCreatePath(fill_mode: i32, path: *mut *mut c_void) -> i32;
    pub fn GdipAddPathArc(
        path: *mut c_void, x: f32, y: f32, w: f32, h: f32, start: f32, sweep: f32,
    ) -> i32;
    pub fn GdipClosePathFigure(path: *mut c_void) -> i32;
    pub fn GdipFillPath(g: *mut c_void, brush: *mut c_void, path: *mut c_void) -> i32;
    pub fn GdipDeletePath(path: *mut c_void) -> i32;
    pub fn GdipSaveGraphics(g: *mut c_void, state: *mut u32) -> i32;
    pub fn GdipRestoreGraphics(g: *mut c_void, state: u32) -> i32;
    pub fn GdipResetWorldTransform(g: *mut c_void) -> i32;
    pub fn GdipTranslateWorldTransform(g: *mut c_void, dx: f32, dy: f32, order: i32) -> i32;
    pub fn GdipRotateWorldTransform(g: *mut c_void, angle: f32, order: i32) -> i32;
    pub fn GdipScaleWorldTransform(g: *mut c_void, sx: f32, sy: f32, order: i32) -> i32;
    pub fn GdipBitmapLockBits(
        bitmap: *mut c_void,
        rect: *const GpRect,
        flags: u32,
        format: i32,
        data: *mut BitmapData,
    ) -> i32;
    pub fn GdipBitmapUnlockBits(bitmap: *mut c_void, data: *mut BitmapData) -> i32;
    pub fn GdipCreateBitmapFromFile(file: *const u16, bitmap: *mut *mut c_void) -> i32;
    pub fn GdipGetImageWidth(image: *mut c_void, w: *mut u32) -> i32;
    pub fn GdipGetImageHeight(image: *mut c_void, h: *mut u32) -> i32;
    pub fn GdipCreateBitmapFromScan0WithPal(
        w: i32, h: i32, stride: i32, format: i32, scan0: *mut u8, bmp: *mut *mut c_void,
    ) -> i32;
}

static TOKEN: AtomicUsize = AtomicUsize::new(0);

pub fn startup() {
    if TOKEN.load(Ordering::Relaxed) != 0 {
        return;
    }
    unsafe {
        let input = StartupInput {
            version: 1,
            debug_event_callback: std::ptr::null_mut(),
            suppress_background_thread: 0,
            suppress_external_codecs: 0,
        };
        let mut token: usize = 0;
        if GdiplusStartup(&mut token, &input, std::ptr::null_mut()) == 0 {
            TOKEN.store(token, Ordering::Relaxed);
        }
    }
}

pub fn shutdown() {
    release_caches();
    let t = TOKEN.swap(0, Ordering::Relaxed);
    if t != 0 {
        unsafe { GdiplusShutdown(t) };
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------------- 资源缓存 ----------------
// 字体族/字体/排版格式/画刷原本是"每次调用都建、用完就删"，每帧要做几十次
// 同参数的创建销毁，纯重复劳动。这些对象与具体 Graphics 无关，可以跨帧、
// 跨窗口复用，进程退出前统一释放（必须在 GdiplusShutdown 之前）。

struct TextCache {
    /// 字体族（Microsoft YaHei UI），只创建一次
    family: *mut c_void,
    /// (字号×100 取整, GpFont)：同字号复用同一个字体对象
    fonts: Vec<(i32, *mut c_void)>,
    /// 左对齐 / 居中两种 StringFormat
    fmt_near: *mut c_void,
    fmt_center: *mut c_void,
}
// 裸指针由本模块独占管理，且只在消息循环线程使用
unsafe impl Send for TextCache {}

static TEXT_CACHE: Mutex<TextCache> = Mutex::new(TextCache {
    family: std::ptr::null_mut(),
    fonts: Vec::new(),
    fmt_near: std::ptr::null_mut(),
    fmt_center: std::ptr::null_mut(),
});

struct BrushCache {
    items: Vec<(u32, *mut c_void)>,
}
unsafe impl Send for BrushCache {}

static BRUSH_CACHE: Mutex<BrushCache> = Mutex::new(BrushCache { items: Vec::new() });
/// 颜色种类上限：超出就整体重建，防止动态色把缓存撑成无界内存
const BRUSH_CACHE_MAX: usize = 512;

fn cached_brush(color: u32) -> *mut c_void {
    let mut c = match BRUSH_CACHE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(&(_, b)) = c.items.iter().find(|(col, _)| *col == color) {
        return b;
    }
    unsafe {
        let mut b: *mut c_void = std::ptr::null_mut();
        if GdipCreateSolidFill(color, &mut b) != 0 {
            return std::ptr::null_mut();
        }
        if c.items.len() >= BRUSH_CACHE_MAX {
            for (_, old) in c.items.drain(..) {
                GdipDeleteBrush(old);
            }
        }
        c.items.push((color, b));
        b
    }
}

/// 预乘 alpha 查表：lut[a * 256 + c] = (c * a + 127) / 255
/// 每帧要算 40~80 万次三个通道的乘除，查表把它降成一次内存读。
fn premul_lut() -> &'static [u8] {
    static LUT: OnceLock<Vec<u8>> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut t = vec![0u8; 256 * 256];
        for a in 0..256usize {
            for c in 0..256usize {
                t[a * 256 + c] = ((c * a + 127) / 255) as u8;
            }
        }
        t
    })
}

/// 释放缓存对象：必须在 GdiplusShutdown 之前调用，否则会删到已失效的句柄
fn release_caches() {
    unsafe {
        let mut t = match TEXT_CACHE.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        for (_, f) in t.fonts.drain(..) {
            GdipDeleteFont(f);
        }
        for fmt in [t.fmt_near, t.fmt_center] {
            if !fmt.is_null() {
                GdipDeleteStringFormat(fmt);
            }
        }
        t.fmt_near = std::ptr::null_mut();
        t.fmt_center = std::ptr::null_mut();
        if !t.family.is_null() {
            GdipDeleteFontFamily(t.family);
            t.family = std::ptr::null_mut();
        }
        drop(t);

        let mut b = match BRUSH_CACHE.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        for (_, br) in b.items.drain(..) {
            GdipDeleteBrush(br);
        }
    }
}

/// 加载图片文件为 GpImage，失败返回 null（等价"解不开就放着不动"）
pub fn load_image(path: &std::path::Path) -> *mut c_void {
    unsafe {
        let w = wide(&path.to_string_lossy());
        let mut img: *mut c_void = std::ptr::null_mut();
        if GdipCreateBitmapFromFile(w.as_ptr(), &mut img) == 0 {
            img
        } else {
            std::ptr::null_mut()
        }
    }
}

/// 生成一张缩放到 max_w x max_h 的缓存位图（只保留显示所需像素，降低常驻内存）
pub fn make_thumb(src: *mut c_void, max_w: i32, max_h: i32) -> *mut c_void {
    unsafe {
        if src.is_null() {
            return std::ptr::null_mut();
        }
        let mut w = 0u32;
        let mut h = 0u32;
        GdipGetImageWidth(src, &mut w);
        GdipGetImageHeight(src, &mut h);
        if w == 0 || h == 0 {
            return std::ptr::null_mut();
        }
        let scale = (max_w as f32 / w as f32).min(max_h as f32 / h as f32).min(1.0);
        let tw = (w as f32 * scale).max(1.0) as i32;
        let th = (h as f32 * scale).max(1.0) as i32;
        let mut bmp: *mut c_void = std::ptr::null_mut();
        if GdipCreateBitmapFromScan0(tw, th, 0, PIXEL_ARGB, std::ptr::null_mut(), &mut bmp) != 0 {
            return std::ptr::null_mut();
        }
        let mut g: *mut c_void = std::ptr::null_mut();
        if GdipGetImageGraphicsContext(bmp, &mut g) == 0 {
            GdipSetSmoothingMode(g, SMOOTH_ANTIALIAS);
            GdipDrawImageRectI(g, src, 0, 0, tw, th);
            GdipDeleteGraphics(g);
        }
        bmp
    }
}

/// 单个像素：ARGB（非预乘）→ 写进 DIBSection 的 u32。
/// ⚠️ 约定：该 u32 按小端写进内存后是 **B,G,R,A**，也就是**低字节是 B**。
/// 所以合成结果必须是 `A<<24 | R<<16 | G<<8 | B`，R 绝不能挪到低位
/// —— 2026-11-27 这里曾把 `(R)>>16` 当成"换序"，结果所有不透明像素红蓝互换
/// （文字发蓝、相片偏色），靠 unit test 才抓住。
#[inline]
fn premul_pixel(argb: u32, lut: &[u8]) -> u32 {
    let a = (argb >> 24) as usize;
    if a == 0 {
        return 0;
    }
    if a == 255 {
        // 免乘快路径：通道位置本来就是对的，只需把 alpha 拉满
        return 0xFF00_0000 | (argb & 0x00FF_FFFF);
    }
    let base = a * 256;
    ((a as u32) << 24)
        | ((lut[base + ((argb >> 16) & 0xFF) as usize] as u32) << 16)
        | ((lut[base + ((argb >> 8) & 0xFF) as usize] as u32) << 8)
        | lut[base + (argb & 0xFF) as usize] as u32
}

pub struct Canvas {
    pub bmp: *mut c_void,
    pub g: *mut c_void,
    pub w: i32,
    pub h: i32,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Option<Self> {
        unsafe {
            let mut bmp: *mut c_void = std::ptr::null_mut();
            if GdipCreateBitmapFromScan0(w, h, 0, PIXEL_ARGB, std::ptr::null_mut(), &mut bmp) != 0 {
                return None;
            }
            let mut g: *mut c_void = std::ptr::null_mut();
            if GdipGetImageGraphicsContext(bmp, &mut g) != 0 {
                GdipDisposeImage(bmp);
                return None;
            }
            GdipGraphicsClear(g, 0);
            GdipSetSmoothingMode(g, SMOOTH_ANTIALIAS);
            GdipSetTextRenderingHint(g, TEXT_ANTIALIAS);
            Some(Self { bmp, g, w, h })
        }
    }

    /// 复用同一块位图开新一帧：清空 + 复位世界变换。
    /// scale()/translate() 都是 PREPEND 累加的，不复位会一帧帧叠上去。
    /// 平滑模式与文字渲染提示挂在 Graphics 上，创建时设一次即可，无需重复。
    pub fn begin_frame(&self) {
        unsafe {
            GdipResetWorldTransform(self.g);
            GdipGraphicsClear(self.g, 0);
        }
    }

    pub fn save(&self) -> u32 {
        let mut s = 0u32;
        unsafe { GdipSaveGraphics(self.g, &mut s) };
        s
    }
    pub fn restore(&self, s: u32) {
        unsafe { GdipRestoreGraphics(self.g, s) };
    }
    pub fn rotate_around(&self, cx: f32, cy: f32, deg: f32) {
        unsafe {
            GdipTranslateWorldTransform(self.g, cx, cy, MATRIX_PREPEND);
            GdipRotateWorldTransform(self.g, deg, MATRIX_PREPEND);
            GdipTranslateWorldTransform(self.g, -cx, -cy, MATRIX_PREPEND);
        }
    }
    pub fn translate(&self, dx: f32, dy: f32) {
        unsafe { GdipTranslateWorldTransform(self.g, dx, dy, MATRIX_PREPEND) };
    }
    /// 一次性把逻辑 CSS 像素映射到物理像素（高 DPI），线宽与圆角同步缩放
    pub fn scale(&self, s: f32) {
        unsafe { GdipScaleWorldTransform(self.g, s, s, MATRIX_PREPEND) };
    }

    /// ARGB = 0xAARRGGBB
    pub fn fill_rect(&self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let b = cached_brush(color);
        if b.is_null() {
            return;
        }
        unsafe { GdipFillRectangleI(self.g, b, x, y, w, h) };
    }

    /// 圆角矩形：走 GraphicsPath 一次填充。
    /// 早期用"两个矩形 + 四个圆"拼，接缝处会被叠加两次（90% 透明 → 99%），
    /// 在深色壁纸上看得出一条更亮的竖缝；胶囊（h == 2r）时两端更是整块发亮。
    pub fn round_rect(&self, x: i32, y: i32, w: i32, h: i32, r: i32, color: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2).max(0);
        if r == 0 {
            self.fill_rect(x, y, w, h, color);
            return;
        }
        let b = cached_brush(color);
        if b.is_null() {
            return;
        }
        let (d, (fx, fy, fw, fh)) = (2 * r, (x as f32, y as f32, w as f32, h as f32));
        let df = d as f32;
        unsafe {
            let mut path: *mut c_void = std::ptr::null_mut();
            if GdipCreatePath(0, &mut path) != 0 {
                return;
            }
            // 顺时针四段 90° 弧，AddPathArc 会自动从上一点连线过来，闭合即成圆角矩形
            GdipAddPathArc(path, fx, fy, df, df, 180.0, 90.0);
            GdipAddPathArc(path, fx + fw - df, fy, df, df, 270.0, 90.0);
            GdipAddPathArc(path, fx + fw - df, fy + fh - df, df, df, 0.0, 90.0);
            GdipAddPathArc(path, fx, fy + fh - df, df, df, 90.0, 90.0);
            GdipClosePathFigure(path);
            GdipFillPath(self.g, b, path);
            GdipDeletePath(path);
        }
    }

    pub fn line(&self, x1: i32, y1: i32, x2: i32, y2: i32, color: u32, width: f32) {
        unsafe {
            let mut p: *mut c_void = std::ptr::null_mut();
            if GdipCreatePen1(color, width, UNIT_PIXEL, &mut p) == 0 {
                GdipDrawLineI(self.g, p, x1, y1, x2, y2);
                GdipDeletePen(p);
            }
        }
    }

    /// 二次贝塞尔用三次贝塞尔等价控制点：C1=P0+2/3(Q-P0), C2=P2+2/3(Q-P2)
    pub fn quad(&self, x0: i32, y0: i32, cx: i32, cy: i32, x2: i32, y2: i32, color: u32, width: f32) {
        let c1x = x0 as f32 + 2.0 / 3.0 * (cx - x0) as f32;
        let c1y = y0 as f32 + 2.0 / 3.0 * (cy - y0) as f32;
        let c2x = x2 as f32 + 2.0 / 3.0 * (cx - x2) as f32;
        let c2y = y2 as f32 + 2.0 / 3.0 * (cy - y2) as f32;
        unsafe {
            let mut p: *mut c_void = std::ptr::null_mut();
            if GdipCreatePen1(color, width, UNIT_PIXEL, &mut p) == 0 {
                GdipDrawBezierI(
                    self.g, p, x0, y0, c1x as i32, c1y as i32, c2x as i32, c2y as i32, x2, y2,
                );
                GdipDeletePen(p);
            }
        }
    }

    pub fn image(&self, img: *mut c_void, x: i32, y: i32, w: i32, h: i32) {
        if !img.is_null() && w > 0 && h > 0 {
            unsafe { GdipDrawImageRectI(self.g, img, x, y, w, h) };
        }
    }

    pub fn text(&self, s: &str, x: f32, y: f32, w: f32, h: f32, size: f32, color: u32, center: bool) {
        let brush = cached_brush(color);
        if brush.is_null() {
            return;
        }
        let mut cache = match TEXT_CACHE.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        unsafe {
            if cache.family.is_null() {
                let family_name = wide("Microsoft YaHei UI");
                let mut family: *mut c_void = std::ptr::null_mut();
                if GdipCreateFontFamilyFromName(family_name.as_ptr(), std::ptr::null_mut(), &mut family) != 0 {
                    return;
                }
                cache.family = family;
            }
            // 字号按 0.01px 取整做键：同一字号跨帧、跨元素只建一次字体对象
            let key = (size * 100.0).round() as i32;
            let font = match cache.fonts.iter().find(|(k, _)| *k == key) {
                Some(&(_, f)) => f,
                None => {
                    let mut f: *mut c_void = std::ptr::null_mut();
                    if GdipCreateFont(cache.family, size, FONT_REGULAR, UNIT_PIXEL, &mut f) != 0 {
                        return;
                    }
                    cache.fonts.push((key, f));
                    f
                }
            };
            let slot = if center { &mut cache.fmt_center } else { &mut cache.fmt_near };
            if slot.is_null() {
                let mut fmt: *mut c_void = std::ptr::null_mut();
                if GdipCreateStringFormat(0, 0, &mut fmt) != 0 {
                    return;
                }
                GdipSetStringFormatAlign(fmt, if center { ALIGN_CENTER } else { ALIGN_NEAR });
                GdipSetStringFormatLineAlign(fmt, ALIGN_CENTER);
                *slot = fmt;
            }
            let fmt = *slot;
            if fmt.is_null() {
                return;
            }
            let t = wide(s);
            let rect = RectF { x, y, w, h };
            GdipDrawString(self.g, t.as_ptr(), -1, font, &rect, fmt, brush);
        }
    }

    /// 转成预乘 alpha 的 BGRA 写入 DIBSection 的像素缓冲。
    /// `floor_alpha` 为真时把全透明像素的 alpha 抬到 1/255（等价原先的二次扫描），
    /// 合并进这一趟可以省掉对整个表面再遍历一遍。
    pub fn blit_premultiplied(&self, dst: *mut u8, dst_stride: i32, floor_alpha: bool) {
        let lut = premul_lut();
        unsafe {
            let rect = GpRect { x: 0, y: 0, w: self.w, h: self.h };
            let mut data: BitmapData = std::mem::zeroed();
            if GdipBitmapLockBits(self.bmp, &rect, 1, PIXEL_ARGB, &mut data) != 0 || data.scan0.is_null() {
                return;
            }
            let src_stride = data.stride;
            for y in 0..self.h as usize {
                let srow = data.scan0.add(y * src_stride as usize);
                let drow = dst.add(y * dst_stride as usize);
                for x in 0..self.w as usize {
                    let argb = std::ptr::read_unaligned(srow.add(x * 4) as *const u32);
                    let mut out = premul_pixel(argb, lut);
                    if floor_alpha && out & 0xFF00_0000 == 0 {
                        out = 0x0100_0000;
                    }
                    std::ptr::write_unaligned(drow.add(x * 4) as *mut u32, out);
                }
            }
            GdipBitmapUnlockBits(self.bmp, &mut data);
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            GdipDeleteGraphics(self.g);
            GdipDisposeImage(self.bmp);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn argb_helper() {
        // 0xAARRGGBB -> 预乘 BGRA 的分量计算（与 blit_premultiplied 相同公式）
        let argb = 0x80FF_8040u32;
        let a = (argb >> 24) & 0xFF;
        let r = (((argb >> 16) & 0xFF) * a + 127) / 255;
        assert_eq!(a, 128);
        assert_eq!(r, 128); // 255*128/255 = 128
    }

    #[test]
    fn premul_lut_matches_formula() {
        // 查表必须与逐像素公式逐位一致，否则半透明边缘会出现 ±1 的色差
        let lut = super::premul_lut();
        assert_eq!(lut.len(), 256 * 256);
        for a in 0..256usize {
            for c in 0..256usize {
                assert_eq!(lut[a * 256 + c] as usize, (c * a + 127) / 255, "a={a} c={c}");
            }
        }
    }

    #[test]
    fn premul_pixel_preserves_channel_order() {
        // 写进 DIB 的 u32 小端展开就是 [B, G, R, A]；用 to_le_bytes 反解，
        // 与"预乘后的期望通道"逐一比对。曾因把 R 右移 16 位导致红蓝互换，
        // 全不透明像素（文字、相片）全部偏色 —— 这条就是防它回潮的。
        let lut = super::premul_lut();
        let cases = [0xFFFF_0000u32, 0xFF00_FF00, 0xFF00_00FF, 0xFFFF_FFFF,
                     0x80FF_0000, 0xE6F6_E7CE, 0x01_FF_FF_FF, 0x0000_0000];
        for argb in cases {
            let [b, g, r, a] = super::premul_pixel(argb, lut).to_le_bytes();
            let a_in = (argb >> 24) as u32;
            let exp = |c: u32| if a_in == 0 { 0 } else { (c * a_in + 127) / 255 };
            assert_eq!(
                (r as u32, g as u32, b as u32, a as u32),
                (exp((argb >> 16) & 0xFF), exp((argb >> 8) & 0xFF), exp(argb & 0xFF), a_in),
                "argb={:08X} 通道序错了", argb
            );
        }
    }

    #[test]
    fn premul_fast_paths_match_lut() {
        // 短路分支（a=0 / a=255）与查表结果必须等价
        let lut = super::premul_lut();
        assert_eq!(lut[0], 0);
        for c in 0..256usize {
            assert_eq!(lut[255 * 256 + c] as usize, c);
        }
    }
}
