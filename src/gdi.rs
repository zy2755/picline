//! GDI+ 绘制封装：把内容画到 32bpp ARGB 内存位图，再转成 UpdateLayeredWindow
//! 需要的**预乘 alpha BGRA**写进 DIBSection。
//!
//! 为什么不用"GDI+ 直接从 HDC 创建 Graphics"：HDC 上的 DIBSection 不保证
//! GDI+ 写回 alpha 通道，半透明区域会变黑或全透。走 Bitmap + LockBits 最稳。

#![allow(dead_code)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    pub fn GdipSaveGraphics(g: *mut c_void, state: *mut u32) -> i32;
    pub fn GdipRestoreGraphics(g: *mut c_void, state: u32) -> i32;
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
    let t = TOKEN.swap(0, Ordering::Relaxed);
    if t != 0 {
        unsafe { GdiplusShutdown(t) };
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
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
        unsafe {
            let mut b: *mut c_void = std::ptr::null_mut();
            if GdipCreateSolidFill(color, &mut b) == 0 {
                GdipFillRectangleI(self.g, b, x, y, w, h);
                GdipDeleteBrush(b);
            }
        }
    }

    /// 圆角矩形 = 两个矩形 + 四个角圆（避免引入 GraphicsPath FFI）
    pub fn round_rect(&self, x: i32, y: i32, w: i32, h: i32, r: i32, color: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = r.min(w / 2).min(h / 2).max(0);
        self.fill_rect(x + r, y, w - 2 * r, h, color);
        self.fill_rect(x, y + r, w, h - 2 * r, color);
        let d = 2 * r;
        unsafe {
            let mut b: *mut c_void = std::ptr::null_mut();
            if GdipCreateSolidFill(color, &mut b) == 0 {
                GdipFillEllipseI(self.g, b, x, y, d, d);
                GdipFillEllipseI(self.g, b, x + w - d, y, d, d);
                GdipFillEllipseI(self.g, b, x, y + h - d, d, d);
                GdipFillEllipseI(self.g, b, x + w - d, y + h - d, d, d);
                GdipDeleteBrush(b);
            }
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
        unsafe {
            let family_name = wide("Microsoft YaHei UI");
            let mut family: *mut c_void = std::ptr::null_mut();
            if GdipCreateFontFamilyFromName(family_name.as_ptr(), std::ptr::null_mut(), &mut family) != 0 {
                return;
            }
            let mut font: *mut c_void = std::ptr::null_mut();
            if GdipCreateFont(family, size, FONT_REGULAR, UNIT_PIXEL, &mut font) != 0 {
                GdipDeleteFontFamily(family);
                return;
            }
            let mut fmt: *mut c_void = std::ptr::null_mut();
            GdipCreateStringFormat(0, 0, &mut fmt);
            GdipSetStringFormatAlign(fmt, if center { ALIGN_CENTER } else { ALIGN_NEAR });
            GdipSetStringFormatLineAlign(fmt, ALIGN_CENTER);
            let mut brush: *mut c_void = std::ptr::null_mut();
            GdipCreateSolidFill(color, &mut brush);
            let t = wide(s);
            let rect = RectF { x, y, w, h };
            GdipDrawString(self.g, t.as_ptr(), -1, font, &rect, fmt, brush);
            GdipDeleteBrush(brush);
            GdipDeleteStringFormat(fmt);
            GdipDeleteFont(font);
            GdipDeleteFontFamily(family);
        }
    }

    /// 转成预乘 alpha 的 BGRA 写入 DIBSection 的像素缓冲
    pub fn blit_premultiplied(&self, dst: *mut u8, dst_stride: i32) {
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
                    let a = (argb >> 24) & 0xFF;
                    let r = (((argb >> 16) & 0xFF) * a + 127) / 255;
                    let g = (((argb >> 8) & 0xFF) * a + 127) / 255;
                    let b = ((argb & 0xFF) * a + 127) / 255;
                    let o = x * 4;
                    *drow.add(o) = b as u8;
                    *drow.add(o + 1) = g as u8;
                    *drow.add(o + 2) = r as u8;
                    *drow.add(o + 3) = a as u8;
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
}
