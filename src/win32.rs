//! Win32 原生调用层：把原实现里由 Electron / PowerShell 提供的能力搬进单进程。
//!
//! 关键收益：原版为全屏检测常驻一个 powershell.exe（每 350ms 输出 JSON），
//! 这里直接用 GetForegroundWindow/GetWindowRect/GetClassNameW 在进程内完成，
//! 省掉整个 PowerShell 运行时及其工作集；判定规则仍由 fullscreen.rs 逐条复刻。
//!
//! 所有失败路径都返回 None/false（等价原实现的 try{}catch{} 静默降级）。

use crate::fullscreen::{Display, FgInfo};
#[allow(unused_imports)]
use crate::visual::{quantize_bgra, PIXELS, THUMB};

#[cfg(windows)]
mod sys {
    #[repr(C)]
    pub struct Rect {
        pub l: i32,
        pub t: i32,
        pub r: i32,
        pub b: i32,
    }

    #[repr(C)]
    pub struct Point {
        pub x: i32,
        pub y: i32,
    }

    /// PROCESS_MEMORY_COUNTERS_EX（x64 / x86 均按 usize 对齐）
    #[repr(C)]
    pub struct MemoryCountersEx {
        pub cb: u32,
        pub page_fault_count: u32,
        pub peak_working_set_size: usize,
        pub working_set_size: usize,
        pub quota_peak_paged_pool_usage: usize,
        pub quota_paged_pool_usage: usize,
        pub quota_peak_non_paged_pool_usage: usize,
        pub quota_non_paged_pool_usage: usize,
        pub pagefile_usage: usize,
        pub peak_pagefile_usage: usize,
        pub private_usage: usize,
    }

    #[repr(C)]
    pub struct GdiplusStartupInput {
        pub version: u32,
        pub debug_event_callback: *mut core::ffi::c_void,
        pub suppress_background_thread: i32,
        pub suppress_external_codecs: i32,
    }

    #[link(name = "user32")]
    extern "system" {
        pub fn GetForegroundWindow() -> isize;
        pub fn GetWindowRect(h: isize, r: *mut Rect) -> i32;
        pub fn GetWindowLongW(h: isize, index: i32) -> i32;
        pub fn GetClassNameW(h: isize, out: *mut u16, max: i32) -> i32;
        pub fn GetWindowTextW(h: isize, out: *mut u16, max: i32) -> i32;
        pub fn GetCursorPos(p: *mut Point) -> i32;
        pub fn GetSystemMetrics(index: i32) -> i32;
        pub fn GetDpiForSystem() -> u32;
        pub fn OpenClipboard(h: isize) -> i32;
        pub fn CloseClipboard() -> i32;
        pub fn GetClipboardData(format: u32) -> isize;
        pub fn IsClipboardFormatAvailable(format: u32) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetCurrentProcess() -> isize;
        pub fn K32GetProcessMemoryInfo(
            h: isize,
            counters: *mut MemoryCountersEx,
            cb: u32,
        ) -> i32;
        pub fn GetLastError() -> u32;
        pub fn CreateMutexW(attr: *mut core::ffi::c_void, owner: i32, name: *const u16) -> isize;
        pub fn GlobalLock(h: isize) -> *mut core::ffi::c_void;
        pub fn GlobalUnlock(h: isize) -> i32;
        pub fn GlobalSize(h: isize) -> usize;
    }

    #[link(name = "shell32")]
    extern "system" {
        pub fn SHGetFolderPathW(
            owner: isize,
            csidl: i32,
            token: isize,
            flags: u32,
            path: *mut u16,
        ) -> i32;
    }

    #[link(name = "gdiplus")]
    extern "system" {
        pub fn GdiplusStartup(
            token: *mut usize,
            input: *const GdiplusStartupInput,
            output: *mut core::ffi::c_void,
        ) -> i32;
        pub fn GdiplusShutdown(token: usize);
        pub fn GdipCreateBitmapFromFile(file: *const u16, bitmap: *mut *mut core::ffi::c_void) -> i32;
        pub fn GdipDisposeImage(image: *mut core::ffi::c_void) -> i32;
        pub fn GdipGetImageWidth(image: *mut core::ffi::c_void, w: *mut u32) -> i32;
        pub fn GdipGetImageHeight(image: *mut core::ffi::c_void, h: *mut u32) -> i32;
        pub fn GdipGetImageThumbnail(
            image: *mut core::ffi::c_void,
            w: u32,
            h: u32,
            thumb: *mut *mut core::ffi::c_void,
            abort: *mut core::ffi::c_void,
            abort_data: *mut core::ffi::c_void,
        ) -> i32;
        pub fn GdipBitmapGetPixel(
            bitmap: *mut core::ffi::c_void,
            x: i32,
            y: i32,
            color: *mut u32,
        ) -> i32;
    }
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// 前台窗口信息，等价 fullscreen.ps1 输出的一行 JSON。
#[cfg(windows)]
pub fn foreground_info() -> Option<FgInfo> {
    use sys::*;
    unsafe {
        let h = GetForegroundWindow();
        if h == 0 {
            return None;
        }
        let mut r = Rect { l: 0, t: 0, r: 0, b: 0 };
        if GetWindowRect(h, &mut r) == 0 {
            return None;
        }
        let mut cls = [0u16; 512];
        let mut title = [0u16; 512];
        GetClassNameW(h, cls.as_mut_ptr(), 512);
        GetWindowTextW(h, title.as_mut_ptr(), 512);
        Some(FgInfo {
            l: r.l,
            t: r.t,
            r: r.r,
            b: r.b,
            style: GetWindowLongW(h, -16), // GWL_STYLE
            class: from_wide(&cls),
            title: from_wide(&title),
        })
    }
}

#[cfg(not(windows))]
pub fn foreground_info() -> Option<FgInfo> {
    None
}

/// "图片"已知文件夹。原版用 PowerShell 的 GetFolderPath('MyPictures') 处理
/// OneDrive 重定向；SHGetFolderPathW(CSIDL_MYPICTURES, SHGFP_TYPE_CURRENT)
/// 走同一套 KnownFolder 解析。
#[cfg(windows)]
pub fn pictures_dir() -> Option<std::path::PathBuf> {
    use sys::*;
    unsafe {
        let mut buf = [0u16; 260];
        let hr = SHGetFolderPathW(0, 0x0027, 0, 0, buf.as_mut_ptr());
        if hr != 0 {
            return None;
        }
        let s = from_wide(&buf);
        if s.is_empty() || !std::path::Path::new(&s).exists() {
            return None;
        }
        Some(std::path::PathBuf::from(s))
    }
}

#[cfg(not(windows))]
pub fn pictures_dir() -> Option<std::path::PathBuf> {
    None
}

/// 单实例锁，等价 app.requestSingleInstanceLock()。
/// 句柄不显式关闭：进程存活期间锁一直持有，退出时由内核回收。
#[cfg(windows)]
pub fn single_instance_lock() -> bool {
    use sys::*;
    unsafe {
        let name = wide("Local\\PicLineSingleInstance");
        let h = CreateMutexW(std::ptr::null_mut(), 1, name.as_ptr());
        h != 0 && GetLastError() != 183 // ERROR_ALREADY_EXISTS
    }
}

#[cfg(not(windows))]
pub fn single_instance_lock() -> bool {
    true
}

/// (工作集, 峰值工作集, 私有提交)，单位字节
#[cfg(windows)]
pub fn process_memory() -> (u64, u64, u64) {
    use sys::*;
    unsafe {
        let mut c: MemoryCountersEx = std::mem::zeroed();
        c.cb = std::mem::size_of::<MemoryCountersEx>() as u32;
        let ok = K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb);
        if ok == 0 {
            return (0, 0, 0);
        }
        (
            c.working_set_size as u64,
            c.peak_working_set_size as u64,
            c.private_usage as u64,
        )
    }
}

#[cfg(not(windows))]
pub fn process_memory() -> (u64, u64, u64) {
    (0, 0, 0)
}

#[cfg(windows)]
pub fn cursor_pos() -> (i32, i32) {
    use sys::*;
    unsafe {
        let mut p = Point { x: 0, y: 0 };
        if GetCursorPos(&mut p) == 0 {
            return (0, 0);
        }
        (p.x, p.y)
    }
}

#[cfg(not(windows))]
pub fn cursor_pos() -> (i32, i32) {
    (0, 0)
}

/// 主显示器（逻辑像素 + DPI 缩放），供全屏判定使用。
#[cfg(windows)]
pub fn primary_display() -> Display {
    use sys::*;
    unsafe {
        let dpi = GetDpiForSystem();
        let sf = if dpi > 0 { dpi as f64 / 96.0 } else { 1.0 };
        Display {
            x: 0,
            y: 0,
            w: GetSystemMetrics(0),  // SM_CXSCREEN
            h: GetSystemMetrics(1),  // SM_CYSCREEN
            scale_factor: sf,
        }
    }
}

#[cfg(not(windows))]
pub fn primary_display() -> Display {
    Display { x: 0, y: 0, w: 1920, h: 1080, scale_factor: 1.0 }
}

/// GDI+ 解码文件并生成 24x24 量化缓冲；返回 (量化缓冲, 原始宽, 原始高)。
/// 解码失败返回 None（等价 nativeImage.createFromPath(...).isEmpty() -> 放着不动）。
#[cfg(windows)]
pub fn decode_file_visual(path: &std::path::Path) -> Option<(Vec<u8>, u32, u32)> {
    // GDI+ 由 main() 统一 startup/shutdown，避免每次捕获都重新初始化运行时
    unsafe { decode_file_visual_inner(path) }
}

#[cfg(windows)]
unsafe fn decode_file_visual_inner(path: &std::path::Path) -> Option<(Vec<u8>, u32, u32)> {
    use sys::*;
    let wpath = wide(&path.to_string_lossy());
    let mut bmp: *mut core::ffi::c_void = std::ptr::null_mut();
    if GdipCreateBitmapFromFile(wpath.as_ptr(), &mut bmp) != 0 || bmp.is_null() {
        return None;
    }
    let mut w = 0u32;
    let mut h = 0u32;
    if GdipGetImageWidth(bmp, &mut w) != 0 || GdipGetImageHeight(bmp, &mut h) != 0 {
        GdipDisposeImage(bmp);
        return None;
    }
    let mut thumb: *mut core::ffi::c_void = std::ptr::null_mut();
    let ok = GdipGetImageThumbnail(
        bmp,
        THUMB as u32,
        THUMB as u32,
        &mut thumb,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    if ok != 0 || thumb.is_null() {
        GdipDisposeImage(bmp);
        return None;
    }
    let mut tw = 0u32;
    let mut th = 0u32;
    GdipGetImageWidth(thumb, &mut tw);
    GdipGetImageHeight(thumb, &mut th);
    let mut bgra = vec![0u8; PIXELS * 4];
    for y in 0..THUMB {
        for x in 0..THUMB {
            let sx = (x * tw.max(1) as usize / THUMB) as i32;
            let sy = (y * th.max(1) as usize / THUMB) as i32;
            let mut argb = 0u32;
            if GdipBitmapGetPixel(thumb, sx, sy, &mut argb) == 0 {
                let o = (y * THUMB + x) * 4;
                bgra[o] = (argb & 0xFF) as u8; // B
                bgra[o + 1] = ((argb >> 8) & 0xFF) as u8; // G
                bgra[o + 2] = ((argb >> 16) & 0xFF) as u8; // R
                bgra[o + 3] = ((argb >> 24) & 0xFF) as u8; // A
            }
        }
    }
    GdipDisposeImage(thumb);
    GdipDisposeImage(bmp);
    Some((quantize_bgra(&bgra), w, h))
}

#[cfg(not(windows))]
pub fn decode_file_visual(_path: &std::path::Path) -> Option<(Vec<u8>, u32, u32)> {
    None
}

/// 一次剪贴板读取同时产出：视觉哈希用的量化缓冲 + 落盘 PNG 用的原始行数据。
/// 避免保存整幅位图副本（峰值内存敏感），也避免两次读剪贴板造成内容不一致。
#[derive(Clone, Debug)]
pub struct ClipSnapshot {
    pub quantized: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// 每行 = 1 字节 filter(0) + width*3 字节 RGB
    pub png_rows: Vec<u8>,
}

/// 剪贴板 CF_DIB 读取，支持 BI_RGB / BI_BITFIELDS 的 24/32bpp。
/// 其他格式返回 None —— 等价原版 readImage() 取不到图时静默 return。
#[cfg(windows)]
pub fn clipboard_snapshot() -> Option<ClipSnapshot> {
    use sys::*;
    const CF_DIB: u32 = 8;
    unsafe {
        if IsClipboardFormatAvailable(CF_DIB) == 0 || OpenClipboard(0) == 0 {
            return None;
        }
        let handle = GetClipboardData(CF_DIB);
        if handle == 0 {
            CloseClipboard();
            return None;
        }
        let ptr = GlobalLock(handle);
        let size = GlobalSize(handle);
        let result = if ptr.is_null() || size < 40 {
            None
        } else {
            let bytes = std::slice::from_raw_parts(ptr as *const u8, size);
            decode_dib(bytes)
        };
        if !ptr.is_null() {
            GlobalUnlock(handle);
        }
        CloseClipboard();
        result
    }
}

#[cfg(not(windows))]
pub fn clipboard_snapshot() -> Option<ClipSnapshot> {
    None
}

#[cfg(windows)]
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

#[cfg(windows)]
fn i32le(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// BITMAPINFOHEADER -> (24x24 量化缓冲, 原始尺寸, PNG 行数据)
#[cfg(windows)]
fn decode_dib(b: &[u8]) -> Option<ClipSnapshot> {
    if b.len() < 40 {
        return None;
    }
    let header_size = u32le(b, 0) as usize;
    let width = i32le(b, 4);
    let height_raw = i32le(b, 8);
    let bit_count = u16::from_le_bytes([b[14], b[15]]) as usize;
    let compression = u32le(b, 16);
    if width <= 0 || height_raw == 0 || header_size < 40 {
        return None;
    }
    if !(bit_count == 24 || bit_count == 32) || compression > 3 {
        return None;
    }
    let height = height_raw.unsigned_abs() as usize;
    let bottom_up = height_raw > 0;
    let w = width as usize;
    let stride = ((w * bit_count + 31) / 32) * 4;
    let mut offset = header_size;
    if compression == 3 {
        offset += 12; // BI_BITFIELDS 的三个掩码
    }
    if offset + stride * height > b.len() {
        return None;
    }
    let bytes_per_px = bit_count / 8;
    let row_len = 1 + w * 3;
    let mut rows = Vec::with_capacity(row_len * height);
    for sy in 0..height {
        rows.push(0u8); // filter: None
        let row = if bottom_up { height - 1 - sy } else { sy };
        for sx in 0..w {
            let p = offset + row * stride + sx * bytes_per_px;
            if p + 2 >= b.len() {
                rows.extend_from_slice(&[0, 0, 0]);
                continue;
            }
            rows.push(b[p + 2]); // R
            rows.push(b[p + 1]); // G
            rows.push(b[p]); // B
        }
    }
    let mut bgra = vec![0u8; PIXELS * 4];
    for ty in 0..THUMB {
        for tx in 0..THUMB {
            let sx = tx * w / THUMB;
            let sy = ty * height / THUMB;
            let base = sy * row_len + 1 + sx * 3;
            let o = (ty * THUMB + tx) * 4;
            bgra[o] = rows[base + 2]; // B
            bgra[o + 1] = rows[base + 1]; // G
            bgra[o + 2] = rows[base]; // R
            bgra[o + 3] = 255;
        }
    }
    Some(ClipSnapshot {
        quantized: quantize_bgra(&bgra),
        width: w as u32,
        height: height as u32,
        png_rows: rows,
    })
}

/// 3 字节随机十六进制。原版用 crypto.randomBytes(3)；
/// 这里用高精度计数器 + 进程 ID 混合，只用于文件名防碰撞，不作安全用途。
pub fn random_hex3() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tick = crate::capture::now_ms() as u64 ^ (std::process::id() as u64) << 20;
    let v = tick
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(n.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    format!("{:06x}", (v ^ (v >> 32)) as u32 & 0xFF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_hex3_shape_and_variation() {
        let a = random_hex3();
        let b = random_hex3();
        assert_eq!(a.len(), 6);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[cfg(windows)]
    #[test]
    fn dib_32bpp_bottom_up_is_decodable() {
        let w: usize = 2;
        let h: usize = 2;
        let stride = ((w * 32 + 31) / 32) * 4;
        let mut b = vec![0u8; 40 + stride * h];
        b[0..4].copy_from_slice(&40u32.to_le_bytes());
        b[4..8].copy_from_slice(&(w as i32).to_le_bytes());
        b[8..12].copy_from_slice(&(h as i32).to_le_bytes());
        b[12..14].copy_from_slice(&1u16.to_le_bytes());
        b[14..16].copy_from_slice(&32u16.to_le_bytes());
        let snap = decode_dib(&b).unwrap();
        assert_eq!((snap.width, snap.height), (2, 2));
        assert_eq!(snap.quantized.len(), PIXELS * 3);
        assert_eq!(snap.png_rows.len(), 2 * (1 + 2 * 3));
    }

    #[cfg(windows)]
    #[test]
    fn dib_rejects_unsupported_layouts() {
        assert!(decode_dib(&[0u8; 8]).is_none());
        let mut b = vec![0u8; 40 + 64];
        b[0..4].copy_from_slice(&40u32.to_le_bytes());
        b[4..8].copy_from_slice(&2i32.to_le_bytes());
        b[8..12].copy_from_slice(&2i32.to_le_bytes());
        b[14..16].copy_from_slice(&8u16.to_le_bytes()); // 8bpp 调色板：不支持
        assert!(decode_dib(&b).is_none());
    }
}
