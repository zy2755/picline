//! Win32 API 声明与常量（GUI / 托盘 / 剪贴板 / Shell / 显示器 / 对话框）。
//! 只声明本项目实际用到的符号，避免引入 windows-rs 这类大依赖。

#![allow(dead_code, non_snake_case)]

use std::ffi::c_void;

pub type Hwnd = isize;
pub type Wparam = usize;
pub type Lparam = isize;

pub const WM_DESTROY: u32 = 0x0002;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_MOUSELEAVE: u32 = 0x02A3;
pub const WM_SETCURSOR: u32 = 0x0020;
pub const WM_HOTKEY: u32 = 0x0312;
pub const WM_DISPLAYCHANGE: u32 = 0x007E;
pub const WM_APP: u32 = 0x8000;
pub const WM_APP_TRAY: u32 = WM_APP + 1;

pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
pub const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
pub const GWL_EXSTYLE: i32 = -20;
pub const GWLP_USERDATA: i32 = -21;

pub const SW_HIDE: i32 = 0;
pub const SW_SHOWNOACTIVATE: i32 = 4;
pub const SW_SHOWNORMAL: i32 = 1;
pub const HWND_TOPMOST: isize = -1;
pub const SWP_NOACTIVATE: u32 = 0x0010;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOZORDER: u32 = 0x0004;

pub const ULW_ALPHA: u32 = 0x0000_0002;
pub const AC_SRC_OVER: u8 = 0;
pub const AC_SRC_ALPHA: u8 = 1;

pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_NOREPEAT: u32 = 0x4000;
pub const VK_T: u32 = 0x54;

pub const NIM_ADD: u32 = 0;
pub const NIM_DELETE: u32 = 2;
pub const NIF_ICON: u32 = 0x0002;
pub const NIF_MESSAGE: u32 = 0x0001;
pub const NIF_TIP: u32 = 0x0004;
pub const IDI_APPLICATION: u32 = 32512;
pub const IMAGE_ICON: u32 = 1;
pub const LR_SHARED: u32 = 0x0000_8000;
pub const LR_DEFAULTSIZE: u32 = 0x0000_0040;

pub const MF_STRING: u32 = 0x0000;
pub const MF_SEPARATOR: u32 = 0x0800;
pub const MF_CHECKED: u32 = 0x0008;
pub const TPM_RIGHTBUTTON: u32 = 0x0002;

pub const CF_DIB: u32 = 8;
pub const GMEM_MOVEABLE: u32 = 0x0002;
pub const GMEM_ZEROINIT: u32 = 0x0040;

pub const FO_DELETE: u32 = 3;
pub const FOF_SILENT: u16 = 0x0004;
pub const FOF_NOCONFIRMATION: u16 = 0x0010;
pub const FOF_ALLOWUNDO: u16 = 0x0040;
pub const FOF_NOERRORUI: u16 = 0x0400;

pub const HKEY_CURRENT_USER: isize = 0x8000_0001;
pub const KEY_QUERY_VALUE: u32 = 0x0001;
pub const KEY_SET_VALUE: u32 = 0x0002;
pub const REG_SZ: u32 = 1;

pub const MB_ICONINFORMATION: u32 = 0x0040;
pub const MB_OK: u32 = 0;

#[repr(C)]
pub struct WndClassExW {
    pub cb_size: u32,
    pub style: u32,
    pub lpfn_wnd_proc: *const c_void,
    pub cb_cls_extra: i32,
    pub cb_wnd_extra: i32,
    pub h_instance: isize,
    pub h_icon: isize,
    pub h_cursor: isize,
    pub h_br_background: isize,
    pub lpsz_menu_name: *const u16,
    pub lpsz_class_name: *const u16,
    pub h_icon_sm: isize,
}

#[repr(C)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct Msg {
    pub hwnd: isize,
    pub message: u32,
    pub w_param: usize,
    pub l_param: isize,
    pub time: u32,
    pub pt: Point,
    pub l_private: u32,
}

#[repr(C)]
pub struct Size {
    pub cx: i32,
    pub cy: i32,
}

#[repr(C)]
pub struct BlendFunction {
    pub blend_op: u8,
    pub blend_flags: u8,
    pub source_constant_alpha: u8,
    pub alpha_format: u8,
}

#[repr(C)]
pub struct BitmapInfoHeader {
    pub bi_size: u32,
    pub bi_width: i32,
    pub bi_height: i32,
    pub bi_planes: u16,
    pub bi_bit_count: u16,
    pub bi_compression: u32,
    pub bi_size_image: u32,
    pub bi_x_pels_per_meter: i32,
    pub bi_y_pels_per_meter: i32,
    pub bi_clr_used: u32,
    pub bi_clr_important: u32,
}

#[repr(C)]
pub struct BitmapInfo {
    pub header: BitmapInfoHeader,
    pub colors: [u8; 4],
}

#[repr(C)]
pub struct Rect {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

#[repr(C)]
pub struct MonitorInfoExW {
    pub cb_size: u32,
    pub rc_monitor: Rect,
    pub rc_work: Rect,
    pub dw_flags: u32,
    pub sz_device: [u16; 32],
}

#[repr(C)]
pub struct NotifyIconDataW {
    pub cb_size: u32,
    pub h_wnd: isize,
    pub u_id: u32,
    pub u_flags: u32,
    pub u_callback_message: u32,
    pub h_icon: isize,
    pub sz_tip: [u16; 128],
    pub dw_state: u32,
    pub dw_state_mask: u32,
    pub sz_info: [u16; 256],
    pub u_version: u32,
    pub sz_info_title: [u16; 64],
    pub dw_info_flags: u32,
    pub guid_item: [u8; 16],
    pub h_balloon_icon: isize,
}

#[repr(C)]
pub struct ShFileOpStructW {
    pub hwnd: isize,
    pub w_func: u32,
    pub p_from: *const u16,
    pub p_to: *const u16,
    pub f_flags: u16,
    pub f_any_operations_aborted: i32,
    pub h_name_mappings: *mut c_void,
    pub lpsz_progress_title: *const u16,
}

#[repr(C)]
pub struct OpenFileNameW {
    pub l_struct_size: u32,
    pub hwnd_owner: isize,
    pub h_instance: isize,
    pub lpstr_filter: *const u16,
    pub lpstr_custom_filter: *mut u16,
    pub n_max_cust_filter: u32,
    pub n_filter_index: u32,
    pub lpstr_file: *mut u16,
    pub n_max_file: u32,
    pub lpstr_file_title: *mut u16,
    pub n_max_file_title: u32,
    pub lpstr_initial_dir: *const u16,
    pub lpstr_title: *const u16,
    pub flags: u32,
    pub n_file_offset: u16,
    pub n_file_extension: u16,
    pub lpstr_def_ext: *const u16,
    pub l_cust_data: isize,
    pub lpfn_hook: *mut c_void,
    pub lp_template_name: *const u16,
    pub pv_reserved: *mut c_void,
    pub dw_reserved: u32,
    pub flags_ex: u32,
}

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassExW(cls: *const WndClassExW) -> u16;
    pub fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32, y: i32, w: i32, h: i32,
        parent: isize, menu: isize, instance: isize, param: *mut c_void,
    ) -> isize;
    pub fn DestroyWindow(h: isize) -> i32;
    pub fn DefWindowProcW(h: isize, msg: u32, wp: usize, lp: isize) -> isize;
    pub fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
    pub fn TranslateMessage(msg: *const Msg) -> i32;
    pub fn DispatchMessageW(msg: *const Msg) -> isize;
    pub fn PostQuitMessage(code: i32);
    pub fn ShowWindow(h: isize, cmd: i32) -> i32;
    pub fn SetWindowPos(h: isize, after: isize, x: i32, y: i32, w: i32, h: i32, flags: u32) -> i32;
    pub fn SetTimer(h: isize, id: usize, elapse: u32, cb: *mut c_void) -> usize;
    pub fn KillTimer(h: isize, id: usize) -> i32;
    pub fn GetWindowLongW(h: isize, index: i32) -> i32;
    pub fn SetWindowLongW(h: isize, index: i32, value: i32) -> i32;
    pub fn GetCursorPos(p: *mut Point) -> i32;
    pub fn LoadCursorW(instance: isize, name: *const u16) -> isize;
    pub fn SetCursor(cursor: isize) -> isize;
    pub fn SendMessageW(h: isize, msg: u32, wp: usize, lp: isize) -> isize;
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn SetProcessDPIAware() -> i32;
    pub fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    pub fn GetDpiForSystem() -> u32;
    pub fn UpdateLayeredWindow(
        hwnd: isize, dst: isize, ppt_dst: *const Point, psize: *const Size,
        src: isize, ppt_src: *const Point, key: u32, blend: *const BlendFunction, flags: u32,
    ) -> i32;
    pub fn RegisterHotKey(h: isize, id: i32, mods: u32, vk: u32) -> i32;
    pub fn UnregisterHotKey(h: isize, id: i32) -> i32;
    pub fn CreatePopupMenu() -> isize;
    pub fn AppendMenuW(menu: isize, flags: u32, id: usize, text: *const u16) -> i32;
    pub fn TrackPopupMenuEx(menu: isize, flags: u32, x: i32, y: i32, hwnd: isize, p: *mut c_void) -> i32;
    pub fn DestroyMenu(menu: isize) -> i32;
    pub fn SetForegroundWindow(h: isize) -> i32;
    pub fn GetDC(h: isize) -> isize;
    pub fn ReleaseDC(h: isize, dc: isize) -> i32;
    pub fn LoadIconW(instance: isize, name: *const u16) -> isize;
    pub fn LoadImageW(
        instance: isize, name: *const u16, kind: u32, w: i32, h: i32, flags: u32,
    ) -> isize;
    pub fn MessageBoxW(h: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
    pub fn EnumDisplayMonitors(
        dc: isize, clip: *mut c_void, cb: *const c_void, data: isize,
    ) -> i32;
    pub fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfoExW) -> i32;
    pub fn GetForegroundWindow() -> isize;
    pub fn GetWindowRect(h: isize, r: *mut Rect) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateCompatibleDC(dc: isize) -> isize;
    pub fn DeleteDC(dc: isize) -> i32;
    pub fn CreateDIBSection(
        dc: isize, info: *const BitmapInfo, usage: u32,
        bits: *mut *mut c_void, section: isize, offset: u32,
    ) -> isize;
    pub fn SelectObject(dc: isize, obj: isize) -> isize;
    pub fn DeleteObject(obj: isize) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> isize;
    pub fn GlobalAlloc(flags: u32, size: usize) -> isize;
    pub fn GlobalLock(h: isize) -> *mut c_void;
    pub fn GlobalUnlock(h: isize) -> i32;
    pub fn GlobalFree(h: isize) -> isize;
    pub fn GetTickCount64() -> u64;
    pub fn GetCurrentProcessId() -> u32;
}

#[link(name = "shell32")]
extern "system" {
    pub fn Shell_NotifyIconW(action: u32, data: *mut NotifyIconDataW) -> i32;
    pub fn SHFileOperationW(op: *mut ShFileOpStructW) -> i32;
    pub fn ShellExecuteW(
        hwnd: isize, op: *const u16, file: *const u16,
        params: *const u16, dir: *const u16, show: i32,
    ) -> isize;
}

#[link(name = "advapi32")]
extern "system" {
    pub fn RegOpenKeyExW(root: isize, sub: *const u16, options: u32, access: u32, out: *mut isize) -> i32;
    pub fn RegSetValueExW(
        key: isize, name: *const u16, reserved: u32, kind: u32, data: *const u8, len: u32,
    ) -> i32;
    pub fn RegDeleteValueW(key: isize, name: *const u16) -> i32;
    pub fn RegQueryValueExW(
        key: isize, name: *const u16, reserved: *mut u32, kind: *mut u32,
        data: *mut u8, len: *mut u32,
    ) -> i32;
    pub fn RegCloseKey(key: isize) -> i32;
}

#[link(name = "comdlg32")]
extern "system" {
    pub fn GetSaveFileNameW(ofn: *mut OpenFileNameW) -> i32;
    pub fn CommDlgExtendedError() -> u32;
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 双 NUL 结尾的宽字符串（SHFileOperationW 的 pFrom 要求）
pub fn wide_double(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v.push(0);
    v
}

pub fn makelparam(x: i32, y: i32) -> isize {
    ((x as u16 as u32) | ((y as u16 as u32) << 16)) as i32 as isize
}

pub fn loword(l: isize) -> i32 {
    (l & 0xFFFF) as i16 as i32
}
pub fn hiword(l: isize) -> i32 {
    ((l >> 16) & 0xFFFF) as i16 as i32
}
