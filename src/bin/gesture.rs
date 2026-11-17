//! 图墙 Rust GUI 的「真机手势验收注入器」（零外部 crate）。
//!
//! 与逻辑层单测的区别：这里通过 SetCursorPos + mouse_event / keybd_event 向系统
//! 投递**真实的输入事件**，再由 WindowFromPoint / GetWindowLongW / 剪贴板 /
//! 进程窗口枚举 / 状态快照 / 事件日志采集可观测结果，逐项断言。
//!
//! 它会自己启动一个隔离的 GUI 实例（TQ_DATA_DIR / TQ_SHOTS_DIR 全指向临时目录），
//! 结束后恢复鼠标位置与剪贴板、关闭测试期间弹出的外部窗口、让 GUI 干净退出。
//!
//! 用法：
//!   gesture.exe --exe <picline-core.exe> --work <临时目录> [--keep]

#![allow(dead_code, non_snake_case)]

#[path = "../png.rs"]
mod png;

use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

// ---------------- Win32 ----------------

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point { x: i32, y: i32 }
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect { l: i32, t: i32, r: i32, b: i32 }
#[repr(C)]
struct WndClassW {
    style: u32,
    wnd_proc: *const c_void,
    cls_extra: i32,
    wnd_extra: i32,
    instance: isize,
    icon: isize,
    cursor: isize,
    background: isize,
    menu_name: *const u16,
    class_name: *const u16,
}

#[link(name = "user32")]
extern "system" {
    fn SetProcessDpiAwarenessContext(v: isize) -> i32;
    fn SetCursorPos(x: i32, y: i32) -> i32;
    fn GetCursorPos(p: *mut Point) -> i32;
    fn mouse_event(flags: u32, dx: u32, dy: u32, data: u32, extra: usize);
    fn keybd_event(vk: u8, scan: u8, flags: u32, extra: usize);
    fn WindowFromPoint(p: Point) -> isize;
    fn GetWindowLongW(h: isize, i: i32) -> i32;
    fn GetClassNameW(h: isize, b: *mut u16, n: i32) -> i32;
    fn GetWindowTextW(h: isize, b: *mut u16, n: i32) -> i32;
    fn GetWindowRect(h: isize, r: *mut Rect) -> i32;
    fn IsWindowVisible(h: isize) -> i32;
    fn EnumWindows(cb: extern "system" fn(isize, isize) -> i32, p: isize) -> i32;
    fn GetForegroundWindow() -> isize;
    fn SetForegroundWindow(h: isize) -> i32;
    fn GetTopWindow(h: isize) -> isize;
    fn GetWindow(h: isize, cmd: u32) -> isize;
    fn PostMessageW(h: isize, msg: u32, wp: usize, lp: isize) -> i32;
    fn RegisterClassW(c: *const WndClassW) -> u16;
    fn CreateWindowExW(ex: u32, cls: *const u16, name: *const u16, style: u32,
        x: i32, y: i32, w: i32, h: i32, parent: isize, menu: isize,
        inst: isize, param: *mut c_void) -> isize;
    fn DestroyWindow(h: isize) -> i32;
    fn DefWindowProcW(h: isize, msg: u32, wp: usize, lp: isize) -> isize;
    fn GetSystemMetrics(i: i32) -> i32;
    fn SetLayeredWindowAttributes(h: isize, key: u32, alpha: u8, flags: u32) -> i32;
    fn GetLayeredWindowAttributes(h: isize, key: *mut u32, alpha: *mut u8, flags: *mut u32) -> i32;
    fn IsClipboardFormatAvailable(fmt: u32) -> i32;
    fn OpenClipboard(h: isize) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn GetClipboardData(fmt: u32) -> isize;
    fn SetClipboardData(fmt: u32, h: isize) -> isize;
}
#[link(name = "kernel32")]
extern "system" {
    fn GlobalSize(h: isize) -> usize;
    fn GlobalLock(h: isize) -> *mut u8;
    fn GlobalUnlock(h: isize) -> i32;
    fn GlobalAlloc(flags: u32, n: usize) -> isize;
    fn GetModuleHandleW(p: *const u16) -> isize;
}
#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteW(h: isize, op: *const u16, file: *const u16, params: *const u16,
        dir: *const u16, show: i32) -> isize;
}

const WM_CLOSE: u32 = 0x0010;
const WM_COMMAND: u32 = 0x0111;
const WM_APP_QUIT: u32 = 0x8000 + 2;
const WM_APP_TRAY: u32 = 0x8000 + 1;
const GWL_EXSTYLE: i32 = -20;
const WS_EX_TRANSPARENT: u32 = 0x20;
const WS_POPUP: u32 = 0x8000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_EX_TOPMOST: u32 = 0x8;
const WS_EX_LAYERED: u32 = 0x0008_0000;
const LWA_ALPHA: u32 = 0x0000_0002;
const SM_XVIRTUALSCREEN: i32 = 76;
const SM_YVIRTUALSCREEN: i32 = 77;
const SM_CXVIRTUALSCREEN: i32 = 78;
const SM_CYVIRTUALSCREEN: i32 = 79;
const LEFTDOWN: u32 = 0x0002;
const LEFTUP: u32 = 0x0004;
const RIGHTDOWN: u32 = 0x0008;
const RIGHTUP: u32 = 0x0010;
const WHEEL: u32 = 0x0800;
const KEYUP: u32 = 0x0002;
const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;
const GMEM_MOVEABLE: u32 = 0x0002;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
fn from_wide(b: &[u16]) -> String {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf16_lossy(&b[..n])
}
// ---------------- 输入安全护栏 ----------------
// 真机验收会把系统级输入投递到真实桌面。一旦 GUI 退出/卡死或坐标算错，
// 未加保护的注入就会点到用户桌面图标（实测曾误右击微信图标）。
// 每次注入前强制校验：未熔断 + 健康心跳新鲜 + 坐标在虚拟屏幕内；
// 按钮「按下」还额外要求落点命中本程序分层窗口。任一失败立刻全局熔断，
// 把鼠标弹回测试前位置，此后所有输入函数变成 no-op。
static ABORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static HEALTH_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
static HOME_XY: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
static ABORT_REASON: std::sync::OnceLock<std::sync::Mutex<String>> = std::sync::OnceLock::new();
static REJECTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static SKIP_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const HEARTBEAT_TIMEOUT_MS: i64 = 8_000;
use std::sync::atomic::Ordering;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_millis() as i64
}
fn set_home(x: i32, y: i32) {
    HOME_XY.store(((x as i64) << 32) | ((y as i64) & 0xFFFF_FFFF), Ordering::Relaxed);
}
fn home_xy() -> (i32, i32) {
    let v = HOME_XY.load(Ordering::Relaxed);
    ((v >> 32) as i32, v as i32)
}
fn blocked() -> bool { ABORTED.load(Ordering::SeqCst) }
fn abort_reason() -> String {
    ABORT_REASON.get().and_then(|m| m.lock().ok().map(|s| s.clone())).unwrap_or_default()
}
fn abort(reason: &str) {
    if !ABORTED.swap(true, Ordering::SeqCst) {
        let m = ABORT_REASON.get_or_init(|| std::sync::Mutex::new(String::new()));
        if let Ok(mut s) = m.lock() { *s = reason.to_string(); }
        eprintln!("[ABORT] 停止全部输入注入：{}", reason);
        let (hx, hy) = home_xy();
        unsafe { SetCursorPos(hx, hy) }; // 立刻弹回原位，别把光标留在桌面图标上
    }
}
fn heartbeat_ok() -> bool {
    let h = HEALTH_MS.load(Ordering::Relaxed);
    h > 0 && now_ms() - h < HEARTBEAT_TIMEOUT_MS
}
/// 每个用例前调用：GUI 进程还在、且状态快照仍在刷新，才允许继续注入。
fn health_check(child: &mut Option<Child>, state: &Path) -> bool {
    if blocked() { return false; }
    if let Some(c) = child.as_mut() {
        match c.try_wait() {
            Ok(Some(st)) => { abort(&format!("GUI 进程已退出（{:?}）", st)); return false; }
            Err(e) => { abort(&format!("无法查询 GUI 进程状态：{}", e)); return false; }
            Ok(None) => {}
        }
    }
    let fresh = fs::metadata(state).and_then(|m| m.modified()).ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| (now_ms() - d.as_millis() as i64).abs() < 3_000)
        .unwrap_or(false);
    if !fresh {
        abort("GUI 状态快照停止刷新（窗口已关闭或消息循环卡死）");
        return false;
    }
    HEALTH_MS.store(now_ms(), Ordering::Relaxed);
    true
}
fn virtual_screen() -> (i32, i32, i32, i32) {
    unsafe { (
        GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN),
        GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN),
    ) }
}
/// 启动前的环境自检：本机是否允许系统级输入注入。
/// 实测曾出现 SetCursorPos 返回 0、光标恒定不动（游戏覆盖层 / GameInput /
/// BlockInput 残留 / 远程会话），此时继续跑只会得到一堆假的 FAIL，
/// 还会把键盘鼠标事件打到用户桌面。探测只移动 3px 并立刻还原。
fn input_available() -> bool {
    let mut p = Point::default();
    unsafe { GetCursorPos(&mut p) };
    let (vx, vy, vw, vh) = virtual_screen();
    let (tx, ty) = (p.x + 3, p.y + 3);
    if tx < vx || ty < vy || tx >= vx + vw || ty >= vy + vh {
        return false;
    }
    let ok = unsafe { SetCursorPos(tx, ty) } != 0;
    let mut q = Point::default();
    unsafe {
        GetCursorPos(&mut q);
        SetCursorPos(p.x, p.y);
    }
    ok && (q.x - tx).abs() <= 2 && (q.y - ty).abs() <= 2
}

fn sleep(ms: u64) { std::thread::sleep(Duration::from_millis(ms)); }
fn move_to(x: i32, y: i32) -> bool {
    if blocked() { return false; }
    if !heartbeat_ok() { abort("输入心跳超时，GUI 健康状态未知"); return false; }
    let (vx, vy, vw, vh) = virtual_screen();
    if x < vx || y < vy || x >= vx + vw || y >= vy + vh {
        abort(&format!("目标坐标 ({},{}) 越出虚拟屏幕 ({},{}) {}x{}，拒绝把鼠标移出安全区",
            x, y, vx, vy, vw, vh));
        return false;
    }
    unsafe { SetCursorPos(x, y) };
    sleep(35); // 等 GUI 的 60ms 光标轮询跟上
    let mut p = Point::default();
    unsafe { GetCursorPos(&mut p) };
    if (p.x - x).abs() > 2 || (p.y - y).abs() > 2 {
        eprintln!("[GUARD] SetCursorPos 未落到目标：期望 ({},{})，回读 ({},{})", x, y, p.x, p.y);
    }
    true
}
fn mouse(flags: u32, data: u32) {
    if blocked() { return; }
    if !heartbeat_ok() { abort("输入心跳超时，GUI 健康状态未知"); return; }
    if flags == LEFTDOWN || flags == RIGHTDOWN {
        let mut p = Point::default();
        unsafe { GetCursorPos(&mut p) };
        let h = wfp(p.x, p.y);
        if !is_our_hwnd(h) {
            let mut c = [0u16; 64];
            unsafe { GetClassNameW(h, c.as_mut_ptr(), 64) };
            // 单次落点不命中：只拒绝这一次按下（该用例记 FAIL），不全局熔断，
            // 否则一个用例的时序抖动会让后面十项全跑不到。任何一次注入仍然
            // 都经过这道校验，所以不会误点桌面；连续 3 次说明坐标系整体失效才熔断。
            let n = REJECTS.fetch_add(1, Ordering::SeqCst) + 1;
            eprintln!("[GUARD] 拦截按下：落点 ({},{}) 不是图墙窗口（hwnd={} class={}），第 {} 次 | zorder=[{}]",
                p.x, p.y, h, from_wide(&c), n, zorder_dump());
            if n >= 3 {
                abort("连续 3 次按钮落点不命中图墙窗口，判定坐标系整体失效");
            }
            // 按下被吞掉，同一次手势的抬起也必须吞掉，不能往系统投递孤立的 UP。
            SKIP_UP.store(true, Ordering::SeqCst);
            return;
        }
        REJECTS.store(0, Ordering::SeqCst);
    }
    if flags == LEFTUP || flags == RIGHTUP {
        if SKIP_UP.swap(false, Ordering::SeqCst) {
            return;
        }
    }
    unsafe { mouse_event(flags, 0, 0, data, 0) };
}
fn lclick() { mouse(LEFTDOWN, 0); sleep(55); mouse(LEFTUP, 0); }
fn rclick() { mouse(RIGHTDOWN, 0); sleep(55); mouse(RIGHTUP, 0); }
fn esc_key() {
    if blocked() { return; }
    if !heartbeat_ok() { abort("输入心跳超时，拒绝注入 ESC"); return; }
    unsafe { keybd_event(0x1B, 0, 0, 0); keybd_event(0x1B, 0, KEYUP, 0) };
}
fn hotkey_ctrl_alt_t() {
    if blocked() { return; }
    if !heartbeat_ok() { abort("输入心跳超时，拒绝注入热键"); return; }
    unsafe {
        keybd_event(0x11, 0, 0, 0); // Ctrl
        keybd_event(0x12, 0, 0, 0); // Alt
        keybd_event(0x54, 0, 0, 0); // T
        sleep(50);
        keybd_event(0x54, 0, KEYUP, 0);
        keybd_event(0x12, 0, KEYUP, 0);
        keybd_event(0x11, 0, KEYUP, 0);
    }
}

// ---------------- 极简 JSON 读取（状态与日志格式由本项目自己生成） ----------------

fn key_at(s: &str, key: &str) -> Option<usize> {
    let pat = format!("\"{}\":", key);
    s.find(&pat).map(|i| i + pat.len())
}
fn jstr(s: &str, key: &str) -> Option<String> {
    let i = key_at(s, key)?;
    let b = s.as_bytes();
    if *b.get(i)? != b'"' { return None; }
    let mut out = String::new();
    let mut j = i + 1;
    let mut seg = j;
    while j < b.len() {
        match b[j] {
            b'\\' => {
                out.push_str(&s[seg..j]);
                j += 1;
                match *b.get(j)? {
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => { j += 4; out.push('\u{FFFD}'); }
                    x => out.push(x as char),
                }
                j += 1;
                seg = j;
            }
            b'"' => { out.push_str(&s[seg..j]); return Some(out); }
            _ => j += 1,
        }
    }
    None
}
fn jnum(s: &str, key: &str) -> Option<f64> {
    let i = key_at(s, key)?;
    let rest = &s[i..];
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}
fn jbool(s: &str, key: &str) -> Option<bool> {
    let i = key_at(s, key)?;
    Some(s[i..].starts_with("true"))
}
fn jobjs(s: &str, key: &str) -> Vec<String> {
    let pat = format!("\"{}\":[", key);
    let Some(i) = s.find(&pat) else { return Vec::new() };
    let b = s.as_bytes();
    let mut j = i + pat.len();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut out = Vec::new();
    while j < b.len() {
        match b[j] {
            b'{' => { if depth == 0 { start = j; } depth += 1; }
            b'}' => { depth -= 1; if depth == 0 { out.push(s[start..=j].to_string()); } }
            b']' if depth == 0 => break,
            _ => {}
        }
        j += 1;
    }
    out
}

// ---------------- 状态 / 日志 ----------------

#[derive(Clone, Default)]
struct Disp { hwnd: isize, x: i32, y: i32, w: i32, h: i32, sf: f64, transparent: bool }
#[derive(Clone, Default)]
struct Peg { id: String, sx: i32, sy: i32, bx: i32, by: i32, x: f64, ropey: f64 }
#[derive(Clone, Default)]
struct State {
    raw: String,
    t: f64,
    shown: bool,
    pinned: bool,
    offset: f64,
    animating: bool,
    capturing: bool,
    menu_open: bool,
    menu_x: f64,
    menu_y: f64,
    fg_fullscreen: bool,
    item_count: usize,
    toast: String,
    controller: isize,
    cx: i32,
    cy: i32,
    displays: Vec<Disp>,
    pegs: Vec<Peg>,
}
fn parse_state(s: &str) -> State {
    let mut st = State { raw: s.to_string(), ..Default::default() };
    st.t = jnum(s, "t").unwrap_or(0.0);
    st.shown = jbool(s, "shown").unwrap_or(false);
    st.pinned = jbool(s, "pinned").unwrap_or(false);
    st.offset = jnum(s, "offset").unwrap_or(-222.0);
    st.animating = jbool(s, "animating").unwrap_or(false);
    st.capturing = jbool(s, "capturing").unwrap_or(false);
    st.menu_open = jbool(s, "menu_open").unwrap_or(false);
    st.menu_x = jnum(s, "menu_x").unwrap_or(0.0);
    st.menu_y = jnum(s, "menu_y").unwrap_or(0.0);
    st.fg_fullscreen = jbool(s, "fg_fullscreen").unwrap_or(false);
    st.item_count = jnum(s, "item_count").unwrap_or(0.0) as usize;
    st.toast = jstr(s, "toast").unwrap_or_default();
    st.controller = jnum(s, "controller").unwrap_or(0.0) as isize;
    st.cx = jnum(s, "cx").unwrap_or(0.0) as i32;
    st.cy = jnum(s, "cy").unwrap_or(0.0) as i32;
    for o in jobjs(s, "displays") {
        st.displays.push(Disp {
            hwnd: jnum(&o, "hwnd").unwrap_or(0.0) as isize,
            x: jnum(&o, "x").unwrap_or(0.0) as i32,
            y: jnum(&o, "y").unwrap_or(0.0) as i32,
            w: jnum(&o, "w").unwrap_or(0.0) as i32,
            h: jnum(&o, "h").unwrap_or(0.0) as i32,
            sf: jnum(&o, "sf").unwrap_or(1.0),
            transparent: jbool(&o, "transparent").unwrap_or(false),
        });
    }
    for o in jobjs(s, "pegs") {
        st.pegs.push(Peg {
            id: jstr(&o, "id").unwrap_or_default(),
            sx: jnum(&o, "sx").unwrap_or(0.0) as i32,
            sy: jnum(&o, "sy").unwrap_or(0.0) as i32,
            bx: jnum(&o, "bx").unwrap_or(0.0) as i32,
            by: jnum(&o, "by").unwrap_or(0.0) as i32,
            x: jnum(&o, "x").unwrap_or(0.0),
            ropey: jnum(&o, "ropey").unwrap_or(0.0),
        });
    }
    st
}
fn read_state(p: &Path) -> State {
    for _ in 0..6 {
        if let Ok(s) = fs::read_to_string(p) {
            if s.starts_with('{') && s.contains("\"pegs\"") {
                return parse_state(&s);
            }
        }
        sleep(30);
    }
    State::default()
}
fn wait_state(p: &Path, timeout_ms: u64) -> Option<State> {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(timeout_ms) {
        if let Ok(s) = fs::read_to_string(p) {
            if s.contains("\"pegs\"") && jnum(&s, "controller").unwrap_or(0.0) > 0.0 {
                return Some(parse_state(&s));
            }
        }
        sleep(80);
    }
    None
}
/// 等状态里的 t 前进，确认 GUI 消息循环没被卡住
fn wait_fresh(p: &Path, after_t: f64, timeout_ms: u64) -> State {
    let t0 = Instant::now();
    let mut last = State::default();
    while t0.elapsed() < Duration::from_millis(timeout_ms) {
        last = read_state(p);
        if last.t > after_t + 80.0 { return last; }
        sleep(60);
    }
    last
}

struct Log { lines: Vec<(String, String)> }
impl Log {
    fn has(&self, ev: &str) -> bool { self.lines.iter().any(|(e, _)| e == ev) }
    fn has_detail(&self, ev: &str, sub: &str) -> bool {
        self.lines.iter().any(|(e, d)| e == ev && d.contains(sub))
    }
    fn count(&self, ev: &str) -> usize { self.lines.iter().filter(|(e, _)| e == ev).count() }
    fn first_detail(&self, ev: &str) -> String {
        self.lines.iter().find(|(e, _)| e == ev).map(|(_, d)| d.clone()).unwrap_or_default()
    }
    fn summary(&self) -> String {
        let mut v: Vec<String> = self.lines.iter().map(|(e, d)| if d.is_empty() { e.clone() } else { format!("{}({})", e, d) }).collect();
        v.truncate(10);
        v.join(",")
    }
}
fn log_len(p: &Path) -> usize {
    fs::read_to_string(p).map(|s| s.lines().count()).unwrap_or(0)
}
fn log_since(p: &Path, from: usize) -> Log {
    let s = fs::read_to_string(p).unwrap_or_default();
    let lines = s.lines().skip(from).map(|line| {
        (jstr(line, "ev").unwrap_or_default(), jstr(line, "detail").unwrap_or_default())
    }).collect();
    Log { lines }
}

// ---------------- 窗口 / 剪贴板辅助 ----------------

extern "system" fn enum_cb(h: isize, p: isize) -> i32 {
    let v = unsafe { &mut *(p as *mut Vec<(isize, String, String)>) };
    unsafe {
        if IsWindowVisible(h) != 0 {
            let mut c = [0u16; 256];
            let n = GetClassNameW(h, c.as_mut_ptr(), 256);
            let mut t = [0u16; 256];
            let m = GetWindowTextW(h, t.as_mut_ptr(), 256);
            v.push((h, from_wide(&c[..n.max(0) as usize]), from_wide(&t[..m.max(0) as usize])));
        }
    }
    1
}
fn visible_windows() -> Vec<(isize, String, String)> {
    let mut v = Vec::new();
    unsafe { EnumWindows(enum_cb, &mut v as *mut _ as isize) };
    v
}
const KEEP_CLASSES: [&str; 7] = [
    "PicLineWindow", "TQFullscreenProbe", "Shell_TrayWnd", "Progman", "WorkerW",
    "Windows.UI.Core.CoreWindow", "Shell_SecondaryTrayWnd",
];
/// 关闭测试动作新拉起的窗口（看图器 / 画图 / 资源管理器），返回被关窗口描述
fn close_new(before: &[(isize, String, String)]) -> Vec<String> {
    let after = visible_windows();
    let mut closed = Vec::new();
    for (h, cls, title) in &after {
        if before.iter().any(|x| x.0 == *h) || KEEP_CLASSES.contains(&cls.as_str()) {
            continue;
        }
        // 只关本次才出现的、且不是桌面上无标题的宿主窗口
        if title.is_empty() && cls.is_empty() { continue; }
        unsafe { PostMessageW(*h, WM_CLOSE, 0, 0) };
        closed.push(format!("{}|{}", cls, title));
    }
    closed
}

struct ClipBackup { items: Vec<(u32, Vec<u8>)> }
impl ClipBackup {
    fn save() -> Self {
        let mut items = Vec::new();
        unsafe {
            if OpenClipboard(0) != 0 {
                for fmt in [CF_UNICODETEXT, CF_DIB] {
                    let h = GetClipboardData(fmt);
                    if h == 0 { continue; }
                    let n = GlobalSize(h);
                    let p = GlobalLock(h);
                    if !p.is_null() && n > 0 {
                        items.push((fmt, std::slice::from_raw_parts(p, n).to_vec()));
                    }
                    if !p.is_null() { GlobalUnlock(h); }
                }
                CloseClipboard();
            }
        }
        ClipBackup { items }
    }
    fn restore(&self) {
        if self.items.is_empty() { return; }
        unsafe {
            if OpenClipboard(0) == 0 { return; }
            EmptyClipboard();
            for (fmt, data) in &self.items {
                let h = GlobalAlloc(GMEM_MOVEABLE, data.len());
                if h == 0 { continue; }
                let p = GlobalLock(h);
                if !p.is_null() {
                    std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
                    GlobalUnlock(h);
                    SetClipboardData(*fmt, h);
                }
            }
            CloseClipboard();
        }
    }
}
fn clipboard_dib_size() -> usize {
    unsafe {
        if IsClipboardFormatAvailable(CF_DIB) == 0 { return 0; }
        if OpenClipboard(0) == 0 { return 0; }
        let h = GetClipboardData(CF_DIB);
        let n = if h == 0 { 0 } else { GlobalSize(h) };
        CloseClipboard();
        n
    }
}

extern "system" fn probe_wnd(h: isize, msg: u32, wp: usize, lp: isize) -> isize {
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}
/// 造一个无标题栏的满屏窗口，模拟游戏/放映，验证全屏自动隐藏。
///
/// 安全约束：这个窗口**必须完全不可见**。早期版本用了系统白色背景画刷 +
/// WS_EX_TOPMOST + 铺满整屏，实测会让用户看到约 2 秒「全屏白屏」，
/// 且鼠标事件全被它吃掉。现在改成 layered + alpha=0 + WS_EX_TRANSPARENT：
/// 仍是可见的前台窗口（IsWindowVisible=true、rect 覆盖全屏，全屏判定照常通过），
/// 但视觉上零像素、鼠标零拦截，不会干扰桌面和其它窗口。
fn make_fullscreen_probe() -> isize {
    let cls = wide("TQFullscreenProbe");
    let title = wide("TQ 全屏探测窗口");
    unsafe {
        let wc = WndClassW {
            style: 0, wnd_proc: probe_wnd as *const c_void, cls_extra: 0, wnd_extra: 0,
            instance: GetModuleHandleW(std::ptr::null()), icon: 0, cursor: 0,
            background: 0, // 不注册背景画刷，杜绝白色填充
            menu_name: std::ptr::null(), class_name: cls.as_ptr(),
        };
        RegisterClassW(&wc);
        let sw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let sh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let h = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            cls.as_ptr(), title.as_ptr(),
            WS_POPUP | WS_VISIBLE, vx, vy, sw, sh, 0, 0, wc.instance, std::ptr::null_mut());
        if h != 0 {
            SetLayeredWindowAttributes(h, 0, 0, LWA_ALPHA); // alpha=0：整窗完全透明
            SetForegroundWindow(h);
        }
        h
    }
}

/// 结构性地确认探测窗口完全透明：读回 layered alpha 必须为 0。
/// 这比屏幕取色可靠（GetDC(NULL) 在 DWM 合成下不保证能采到 layered 窗口）。
fn probe_is_transparent(h: isize) -> (bool, u8, u32) {
    unsafe {
        let mut key = 0u32;
        let mut alpha = 255u8;
        let mut flags = 0u32;
        let ok = GetLayeredWindowAttributes(h, &mut key, &mut alpha, &mut flags);
        (ok != 0 && alpha == 0 && (flags & LWA_ALPHA) != 0, alpha, flags)
    }
}

fn make_png(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let row_len = 1 + w as usize * 3;
    let mut rows = vec![0u8; row_len * h as usize];
    for y in 0..h as usize {
        let off = y * row_len;
        for x in 0..w as usize {
            let o = off + 1 + x * 3;
            rows[o] = (x as u8).wrapping_add(seed.wrapping_mul(17));
            rows[o + 1] = (y as u8).wrapping_mul(3).wrapping_add(seed.wrapping_mul(29));
            rows[o + 2] = 128;
        }
    }
    png::encode_rows(w, h, &rows)
}

// ---------------- 报告 ----------------

struct Res { id: String, name: String, pass: bool, detail: String }
struct Runner { results: Vec<Res> }
impl Runner {
    fn check(&mut self, id: &str, name: &str, ok: bool, detail: String) {
        println!("[{}] {:<4} {} :: {}", if ok { "PASS" } else { "FAIL" }, id, name, detail);
        self.results.push(Res { id: id.into(), name: name.into(), pass: ok, detail });
    }
    fn to_json(&self) -> String {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        let rows: Vec<String> = self.results.iter().map(|r| format!(
            "{{\"id\":\"{}\",\"name\":\"{}\",\"pass\":{},\"detail\":\"{}\"}}",
            esc(&r.id), esc(&r.name), r.pass, esc(&r.detail)
        )).collect();
        let pass = self.results.iter().filter(|r| r.pass).count();
        format!("{{\"pass\":{},\"total\":{},\"results\":[{}]}}", pass, self.results.len(), rows.join(","))
    }
}

fn is_our_hwnd(h: isize) -> bool {
    let mut c = [0u16; 64];
    unsafe { GetClassNameW(h, c.as_mut_ptr(), 64) };
    from_wide(&c) == "PicLineWindow"
}
fn wfp(x: i32, y: i32) -> isize {
    unsafe { WindowFromPoint(Point { x, y }) }
}
/// 拦截时打印 Z 序链，便于确认是否外部窗口压在了分层窗口之上。
fn zorder_dump() -> String {
    let mut out = String::new();
    unsafe {
        let mut h = GetTopWindow(0);
        for _ in 0..8 {
            if h == 0 { break; }
            let mut c = [0u16; 64];
            GetClassNameW(h, c.as_mut_ptr(), 64);
            let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
            if !out.is_empty() { out.push_str(" > "); }
            out.push_str(&format!("{}:{}:{}", h, from_wide(&c),
                if ex & WS_EX_TOPMOST != 0 { "top" } else { "-" }));
            h = GetWindow(h, 2); // GW_HWNDNEXT
        }
    }
    out
}

// ---------------- 主流程 ----------------

fn main() {
    unsafe { SetProcessDpiAwarenessContext(-4) }; // PER_MONITOR_AWARE_V2：坐标必须是物理像素
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut exe = PathBuf::from("target/release/picline-core.exe");
    let mut work = PathBuf::from("gesture-work");
    let mut i = 0;
    while i + 1 < args.len() {
        match args[i].as_str() {
            "--exe" => exe = PathBuf::from(&args[i + 1]),
            "--work" => work = PathBuf::from(&args[i + 1]),
            _ => {}
        }
        i += 2;
    }
    let data = work.join("data");
    let inbox = data.join("inbox");
    let shots = work.join("shots");
    let drop_dir = work.join("drop");
    for d in [&inbox, &shots, &drop_dir] { let _ = fs::create_dir_all(d); }
    let state_path = work.join("state.json");
    let trace_path = work.join("trace.jsonl");
    let report_path = work.join("report.json");
    let _ = fs::remove_file(&state_path);
    let _ = fs::remove_file(&trace_path);

    if !input_available() {
        println!("[SKIP] 本机系统级输入注入当前不可用（SetCursorPos 失败或光标无法移动）。");
        println!("       常见原因：NVIDIA/游戏覆盖层或 GameInput 服务激活、BlockInput 未解除、");
        println!("       远程会话输入断开、锁屏。请关闭相关程序后重新运行本命令。");
        let _ = fs::write(&report_path,
            "{\"aborted\":true,\"abort_reason\":\"input_unavailable\",\"guard_rejects\":0,\"pass\":0,\"total\":0,\"results\":[]}");
        return;
    }

    // 固定配置：关掉剪贴板监听，避免"单击复制"被自己再挂一次干扰；
    // firstRunDone=true 避免 800ms 自动 peek 打乱时序。
    let _ = fs::write(data.join("config.json"),
        "{\"showLine\":true,\"takeOver\":true,\"clipboardWatch\":false,\"maxPegs\":10,\"firstRunDone\":true}");
    for k in 0..3u8 {
        let _ = fs::write(inbox.join(format!("test-{:02}.png", k)), make_png(320, 240, k + 1));
        sleep(25); // 拉开 mtime，保证 peg 顺序稳定
    }

    let mut home = Point::default();
    unsafe { GetCursorPos(&mut home) };
    set_home(home.x, home.y);
    let clip_backup = ClipBackup::save();
    println!("真机手势验收 | exe={} | work={}", exe.display(), work.display());
    println!("鼠标原位置=({}, {})，剪贴板已备份 {} 种格式", home.x, home.y, clip_backup.items.len());

    let mut child: Option<Child> = None;
    let mut runner = Runner { results: Vec::new() };

    let _state0 = match (|| {
        let c = Command::new(&exe)
            .env("TQ_DATA_DIR", &data)
            .env("TQ_SHOTS_DIR", &shots)
            .env("TQ_TRACE", &trace_path)
            .env("TQ_STATE", &state_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn().ok()?;
        child = Some(c);
        wait_state(&state_path, 12_000)
    })() {
        Some(s) => s,
        None => {
            runner.check("T00", "GUI 启动并写出状态", false, "12 秒内未拿到状态快照".into());
            let _ = fs::write(&report_path, runner.to_json());
            return;
        }
    };
    sleep(700); // 等托盘/热键/首次渲染稳定
    let st = read_state(&state_path);
    let d = st.displays.first().cloned().unwrap_or_default();
    let band_x = d.x + d.w / 2;
    let band_y = d.y + 2;
    let outside_y = d.y + d.h + 90;
    let blank_x = d.x + 90;
    let blank_y = d.y + (d.h as f64 * 0.75) as i32;
    let controller = st.controller;

    // 每个用例组前做健康检查；一旦熔断立刻收尾，绝不在 GUI 失联后继续注入。
    macro_rules! healthy {
        () => {
            if !health_check(&mut child, &state_path) {
                finish(&mut child, &runner, &report_path, &state_path, controller, home, &clip_backup);
                return;
            }
        };
    }

    healthy!();
    // ---- T01 初始状态 ----
    runner.check("T01", "A1 透明置顶窗口 / 不抢焦点 / 收起态",
        !st.shown && st.displays.len() >= 1 && d.w > 0 && d.h > 0
            && d.transparent && !is_our_hwnd(unsafe { GetForegroundWindow() }),
        format!("displays={} {}x{} sf={} transparent={} shown={} items={} fg_is_ours={}",
            st.displays.len(), d.w, d.h, d.sf, d.transparent, st.shown, st.item_count,
            is_our_hwnd(unsafe { GetForegroundWindow() })));

    // ---- T02 空白穿透 ----
    let w0 = wfp(blank_x, blank_y);
    runner.check("T02", "A2 空白区域点击穿透（WindowFromPoint）",
        !is_our_hwnd(w0),
        format!("WindowFromPoint({},{}) hwnd={} class={}", blank_x, blank_y, w0,
            { let mut c = [0u16; 64]; unsafe { GetClassNameW(w0, c.as_mut_ptr(), 64) }; from_wide(&c) }));

    // ---- T21 空白滚轮不产生 GUI 事件 ----
    let base = log_len(&trace_path);
    move_to(blank_x, blank_y);
    sleep(140);
    mouse(WHEEL, 0xFFFF_FDE0u32); // -120 滚动
    sleep(260);
    let lg = log_since(&trace_path, base);
    runner.check("T21", "A2 空白滚轮穿透、不触发任何 GUI 事件",
        lg.lines.is_empty(),
        format!("新增事件 {} 条 [{}]", lg.lines.len(), lg.summary()));

    healthy!();
    // ---- T03 / T04 顶边 250ms 呼出 + 420ms 展开动画 ----
    let base = log_len(&trace_path);
    let t_band = Instant::now();
    move_to(band_x, band_y);
    // 轮询等 shown 翻真（状态快照 40ms 一写，固定 sleep 会踩到旧快照）
    let mut s_reveal = State::default();
    let mut reveal_ms = 0u128;
    while t_band.elapsed() < Duration::from_millis(1400) {
        s_reveal = read_state(&state_path);
        if s_reveal.shown { reveal_ms = t_band.elapsed().as_millis(); break; }
        sleep(30);
    }
    let lg = log_since(&trace_path, base);
    runner.check("T03", "B1 顶边触发带停留 250ms 才呼出",
        s_reveal.shown && lg.has("band_reveal") && lg.has("reveal") && reveal_ms >= 230,
        format!("shown={} 首帧展开耗时={}ms(应≥230) GUI读到光标=({},{}) events=[{}]",
            s_reveal.shown, reveal_ms, s_reveal.cx, s_reveal.cy, lg.summary()));
    sleep(450);
    let s_anim = read_state(&state_path);
    runner.check("T04", "A4 展开动画 420ms 后 offset 归零",
        (s_anim.offset - 0.0).abs() < 1.0 && !s_anim.animating,
        format!("offset={:.2} animating={}", s_anim.offset, s_anim.animating));

    healthy!();
    // ---- T05 移开 300ms 收起 + 收起动画结束才恢复穿透 ----
    let base = log_len(&trace_path);
    move_to(blank_x, outside_y);
    // 细采样收起动画 220ms 窗口内的 transparent 序列：
    // 期间必须出现「仍接管」的帧，动画结束后才恢复穿透。
    let mut seq: Vec<bool> = Vec::new();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(500) {
        let s = read_state(&state_path);
        seq.push(s.displays.first().map(|x| x.transparent).unwrap_or(true));
        sleep(25);
    }
    let held_during_anim = seq.iter().any(|x| !*x);
    sleep(350);
    let after = read_state(&state_path);
    let lg = log_since(&trace_path, base);
    runner.check("T05", "A4/B1 移开 300ms 收起，动画播完才恢复完全穿透",
        held_during_anim && !after.shown && lg.has("hide")
            && after.displays.first().map(|x| x.transparent).unwrap_or(false),
        format!("动画期间出现接管帧={} 序列={:?} hide={} 收起后 transparent={} offset={:.1}",
            held_during_anim, &seq[..seq.len().min(16)], lg.has("hide"),
            after.displays.first().map(|x| x.transparent).unwrap_or(false), after.offset));

    healthy!();
    // ---- T16 热键：展开并 pinned，作为后续相片测试的稳定基座 ----
    let base = log_len(&trace_path);
    hotkey_ctrl_alt_t();
    sleep(600);
    let s_hot = read_state(&state_path);
    let lg = log_since(&trace_path, base);
    runner.check("T16", "B6 Ctrl+Alt+T 展开并固定",
        s_hot.shown && s_hot.pinned && lg.has("hotkey"),
        format!("shown={} pinned={} hotkey_event={}", s_hot.shown, s_hot.pinned, lg.has("hotkey")));

    // 相片类用例必须以展开态为基座：收起时 offset=-222，state 里 peg 的屏幕 y
    // 会变成负数，任何输入都会落到桌面。先确保展开，再重新取 peg 坐标。
    for _ in 0..4 {
        let s = read_state(&state_path);
        if s.shown && s.offset.abs() < 1.0 { break; }
        hotkey_ctrl_alt_t();
        sleep(450);
    }
    healthy!();
    let st = read_state(&state_path);
    let pegs = st.pegs.clone();
    if pegs.is_empty() {
        runner.check("T06", "A3 相片命中接管", false, "状态里没有 peg，后续相片项全部跳过".into());
        finish(&mut child, &runner, &report_path, &state_path, controller, home, &clip_backup);
        return;
    }
    let p0 = pegs[0].clone();
    let p1 = pegs.get(1).cloned().unwrap_or_else(|| p0.clone());
    let p2 = pegs.get(2).cloned().unwrap_or_else(|| p0.clone());

    healthy!();
    // ---- T06 相片命中接管 ----
    let base = log_len(&trace_path);
    move_to(p0.sx, p0.sy);
    sleep(220);
    let s_hit = read_state(&state_path);
    let lg = log_since(&trace_path, base);
    let wp_hit = wfp(p0.sx, p0.sy);
    runner.check("T06", "A3 命中相片才接管（摘 WS_EX_TRANSPARENT）",
        s_hit.capturing && !s_hit.displays.first().map(|x| x.transparent).unwrap_or(true)
            && lg.has_detail("capture", "on") && is_our_hwnd(wp_hit),
        format!("capturing={} transparent={} capture_on={} WFP_is_ours={} at({},{})",
            s_hit.capturing, s_hit.displays.first().map(|x| x.transparent).unwrap_or(true),
            lg.has_detail("capture", "on"), is_our_hwnd(wp_hit), p0.sx, p0.sy));

    // ---- T07 移出相片恢复穿透 ----
    move_to(blank_x, blank_y);
    sleep(220);
    let s_blank = read_state(&state_path);
    runner.check("T07", "A3 离开相片立刻恢复穿透",
        !s_blank.capturing && s_blank.displays.first().map(|x| x.transparent).unwrap_or(false)
            && !is_our_hwnd(wfp(blank_x, blank_y)),
        format!("capturing={} transparent={} WFP_is_ours={}",
            s_blank.capturing, s_blank.displays.first().map(|x| x.transparent).unwrap_or(false),
            is_our_hwnd(wfp(blank_x, blank_y))));

    healthy!();
    // ---- T08 单击复制 + 不自挂 ----
    let base = log_len(&trace_path);
    move_to(p0.sx, p0.sy);
    sleep(200);
    let count_before = read_state(&state_path).item_count;
    lclick();
    sleep(350);
    let dib = clipboard_dib_size();
    sleep(1200); // 跨过 800ms 剪贴板轮询，验证不自挂
    let s_copy = read_state(&state_path);
    let lg = log_since(&trace_path, base);
    runner.check("T08", "B2 单击复制 CF_DIB / 提示 / 不自挂",
        lg.has_detail("click", "single") && lg.has_detail("copy", "ok")
            && lg.has_detail("toast", "已复制到剪贴板") && dib > 40
            && s_copy.item_count == count_before,
        format!("click={} copy={} toast=[{}] CF_DIB={}B items {}->{} events=[{}]",
            lg.has_detail("click", "single"), lg.has_detail("copy", "ok"),
            s_copy.toast, dib, count_before, s_copy.item_count, lg.summary()));

    healthy!();
    // ---- T09 双击打开 ----
    let before_wins = visible_windows();
    let base = log_len(&trace_path);
    move_to(p1.sx, p1.sy);
    sleep(200);
    lclick();
    sleep(95);
    lclick();
    sleep(1600);
    let lg = log_since(&trace_path, base);
    let opened = close_new(&before_wins);
    runner.check("T09", "B2 双击打开（系统默认看图器）",
        lg.has_detail("click", "double") && lg.has("shell_open"),
        format!("double={} shell_open=[{}] 关闭窗口={:?}",
            lg.has_detail("click", "double"), lg.first_detail("shell_open"), opened));
    sleep(250);

    healthy!();
    // ---- T10 长按 450ms 画图 ----
    let before_wins = visible_windows();
    let base = log_len(&trace_path);
    move_to(p2.sx, p2.sy);
    sleep(200);
    mouse(LEFTDOWN, 0);
    sleep(650);
    mouse(LEFTUP, 0);
    sleep(1500);
    let lg = log_since(&trace_path, base);
    let opened = close_new(&before_wins);
    runner.check("T10", "B2 长按 450ms 打开画图且不触发复制",
        lg.has("hold") && lg.has("shell_edit") && lg.has_detail("click", "held") && !lg.has("copy"),
        format!("hold={} shell_edit=[{}] click_held={} copy_count={} 关闭窗口={:?}",
            lg.has("hold"), lg.first_detail("shell_edit"), lg.has_detail("click", "held"),
            lg.count("copy"), opened));
    sleep(250);

    healthy!();
    // ---- T11 右键菜单 + B4 整条接管 ----
    let base = log_len(&trace_path);
    // 右键点在相片顶部，保证自绘菜单完整落在 210px 窗口内
    move_to(p0.sx, p0.by);
    sleep(200);
    rclick();
    sleep(300);
    let s_menu = read_state(&state_path);
    let lg = log_since(&trace_path, base);
    runner.check("T11", "B4 右键弹出 6 项菜单并接管",
        s_menu.menu_open && lg.has("menu_open") && s_menu.capturing,
        format!("menu_open={} capturing={} menu=({:.0},{:.0}) events=[{}]",
            s_menu.menu_open, s_menu.capturing, s_menu.menu_x, s_menu.menu_y, lg.summary()));

    // ---- T12 菜单开启整条接管 + 点外面才收 ----
    move_to(blank_x, blank_y);
    sleep(220);
    let s_out = read_state(&state_path);
    let wfp_out = is_our_hwnd(wfp(blank_x, blank_y));
    let base2 = log_len(&trace_path);
    lclick();
    sleep(300);
    let s_after = read_state(&state_path);
    let lg2 = log_since(&trace_path, base2);
    runner.check("T12", "B4 菜单开启整条接管；点菜单外才收起",
        s_out.capturing && wfp_out && !s_after.menu_open && lg2.has("menu_dismiss"),
        format!("菜单外 capturing={} WFP_is_ours={} 点击后 menu_open={} dismiss={} events=[{}]",
            s_out.capturing, wfp_out, s_after.menu_open, lg2.has("menu_dismiss"), lg2.summary()));

    healthy!();
    // ---- T13 菜单点第 0 项（复制图片）----
    let base = log_len(&trace_path);
    move_to(p0.sx, p0.by);
    sleep(200);
    rclick();
    sleep(250);
    let sm = read_state(&state_path);
    let sf = st.displays.first().map(|x| x.sf).unwrap_or(1.0);
    let mi_x = d.x + ((sm.menu_x + 90.0) * sf) as i32;
    let mi_y = d.y + ((sm.menu_y + 5.0 + 13.0 + sm.offset) * sf) as i32;
    move_to(mi_x, mi_y);
    sleep(180);
    let wfp_mi = is_our_hwnd(wfp(mi_x, mi_y));
    let cap_mi = read_state(&state_path).capturing;
    lclick();
    sleep(300);
    let s_mi = read_state(&state_path);
    let lg = log_since(&trace_path, base);
    runner.check("T13", "B4 点菜单「复制图片」生效且菜单关闭",
        lg.has_detail("menu_action", "0:") && lg.has_detail("copy", "ok") && !s_mi.menu_open,
        format!("menu_action=[{}] copy={} menu_open={} at({},{}) 命中前 capturing={} WFP_is_ours={} events=[{}]",
            lg.first_detail("menu_action"), lg.has_detail("copy", "ok"), s_mi.menu_open,
            mi_x, mi_y, cap_mi, wfp_mi, lg.summary()));

    healthy!();
    // ---- T14 ✕ / 回收站 ----
    let base = log_len(&trace_path);
    let before_files = fs::read_dir(&inbox).map(|r| r.flatten().count()).unwrap_or(0);
    let target = p0.id.clone();
    move_to(p0.bx, p0.by);
    sleep(200);
    lclick();
    sleep(500);
    let s_del = read_state(&state_path);
    let lg = log_since(&trace_path, base);
    let after_files = fs::read_dir(&inbox).map(|r| r.flatten().count()).unwrap_or(0);
    let gone = !inbox.join(&target).exists();
    runner.check("T14", "B5 ✕ 移入回收站、相片离绳、给出提示",
        lg.has_detail("down", "xbtn") && lg.has_detail("discard", "ok") && gone
            && after_files == before_files - 1 && lg.has_detail("toast", "已移入回收站"),
        format!("target={} xbtn={} discard=[{}] gone={} files {}->{} toast=[{}]",
            target, lg.has_detail("down", "xbtn"), lg.first_detail("discard"),
            gone, before_files, after_files, s_del.toast));

    healthy!();
    // ---- T19 新截图自动挂绳 + peek 2.5s ----
    // 原版 peekFor 只在「线收着」时触发（main.js: if (entry.shown) continue），
    // 所以先把 pinned 展开态收起，测完再恢复，供后续拖放使用。
    for _ in 0..3 {
        if !read_state(&state_path).shown { break; }
        hotkey_ctrl_alt_t();
        sleep(450);
    }
    let was_hidden = !read_state(&state_path).shown;
    let base = log_len(&trace_path);
    let before_items = read_state(&state_path).item_count;
    let _ = fs::write(shots.join("incoming.png"), make_png(320, 240, 9));
    sleep(1800);
    let s_new = read_state(&state_path);
    let lg = log_since(&trace_path, base);
    runner.check("T19", "B6/C1 新截图落盘后自动挂绳并探头 2.5s",
        was_hidden && lg.has("capture_file") && lg.has("peek")
            && s_new.item_count == before_items + 1 && s_new.shown,
        format!("收起态={} capture_file={} peek={} items {}->{} shown={} events=[{}]",
            was_hidden, lg.has("capture_file"), lg.has("peek"),
            before_items, s_new.item_count, s_new.shown, lg.summary()));
    for _ in 0..3 {
        let s = read_state(&state_path);
        if s.shown && s.pinned { break; }
        hotkey_ctrl_alt_t();
        sleep(450);
    }

    healthy!();
    // ---- T15 拖出：真机 DoDragDrop + CF_HDROP 落到资源管理器 ----
    let before_wins = visible_windows();
    let drop_w = wide(&drop_dir.to_string_lossy());
    let open_w = wide("open");
    unsafe { ShellExecuteW(0, open_w.as_ptr(), drop_w.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
    sleep(1700);
    let explorer = visible_windows().into_iter()
        .find(|(h, cls, _)| cls == "CabinetWClass" && !before_wins.iter().any(|b| b.0 == *h));
    let (tx, ty) = match explorer {
        Some((h, _, _)) => {
            let mut r = Rect::default();
            unsafe { GetWindowRect(h, &mut r) };
            (r.l + (r.r - r.l) / 2, r.t + (r.b - r.t) * 2 / 3)
        }
        None => (d.x + d.w / 2, d.y + d.h + 260),
    };
    let st_drag = read_state(&state_path);
    let peg = st_drag.pegs.first().cloned().unwrap_or_else(|| p1.clone());
    let base = log_len(&trace_path);
    move_to(peg.sx, peg.sy);
    sleep(200);
    let wfp_drag = is_our_hwnd(wfp(peg.sx, peg.sy));
    let cap_drag = read_state(&state_path).capturing;
    mouse(LEFTDOWN, 0);
    sleep(90);
    // 分几步移动，让 OLE 有 DragEnter/DragOver 的机会
    for k in 1..=8 {
        let x = peg.sx + (tx - peg.sx) * k / 8;
        let y = peg.sy + (ty - peg.sy) * k / 8;
        move_to(x, y);
        sleep(55);
    }
    sleep(320);
    mouse(LEFTUP, 0);
    sleep(900);
    let lg = log_since(&trace_path, base);
    let dropped: Vec<String> = fs::read_dir(&drop_dir).map(|r| r.flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    let closed = close_new(&before_wins);
    runner.check("T15", "B3 拖出到文件夹（OLE CF_HDROP）产生副本且相片留绳",
        lg.has("drag_start") && lg.has_detail("drag_end", "ok") && !dropped.is_empty(),
        format!("drag_start=[{}] drag_end=[{}] 起点=({},{}) 命中前 capturing={} WFP_is_ours={} 落点=({},{}) 副本={:?} 关闭窗口={:?} 事件=[{}]",
            lg.first_detail("drag_start"), lg.first_detail("drag_end"), peg.sx, peg.sy,
            cap_drag, wfp_drag, tx, ty, dropped, closed, lg.summary()));

    healthy!();
    // ---- T18 全屏自动隐藏 ----
    // 先收起（pinned 态热键/Ctrl+Alt+T 会 hide），再造全屏窗口
    let base = log_len(&trace_path);
    hotkey_ctrl_alt_t();
    sleep(500);
    let fs_probe = make_fullscreen_probe();
    sleep(800);
    let (transparent, alpha, flags) = if fs_probe != 0 {
        probe_is_transparent(fs_probe)
    } else {
        (false, 255, 0)
    };
    runner.check("T18b", "D1 全屏探测窗口完全透明（不会产生全屏白屏）",
        transparent,
        format!("hwnd={} layered_alpha={} flags=0x{:X} 背景画刷=NULL(不刷白)",
            fs_probe, alpha, flags));
    let s_fs = read_state(&state_path);
    move_to(band_x, band_y);
    sleep(600);
    let s_fs2 = read_state(&state_path);
    let lg = log_since(&trace_path, base);
    let band_blocked = !lg.has("band_reveal");
    runner.check("T18", "D1 全屏应用在前台时顶边不呼出、不抢焦点",
        fs_probe != 0 && s_fs.fg_fullscreen && lg.has_detail("fullscreen", "true")
            && band_blocked && !s_fs2.shown,
        format!("probe_hwnd={} fg_fullscreen={} fullscreen_event={} 顶边呼出被抑制={} shown={} events=[{}]",
            fs_probe, s_fs.fg_fullscreen, lg.has_detail("fullscreen", "true"),
            band_blocked, s_fs2.shown, lg.summary()));
    if fs_probe != 0 { unsafe { DestroyWindow(fs_probe) }; }
    sleep(600);
    move_to(blank_x, outside_y);

    healthy!();
    // ---- T17 托盘命令（WM_COMMAND 消息注入，非点击托盘图标；菜单弹出单独放最后）----
    let base = log_len(&trace_path);
    let t_before = read_state(&state_path).t;
    let cfg_before = fs::read_to_string(data.join("config.json")).unwrap_or_default();
    unsafe {
        PostMessageW(controller, WM_COMMAND, 2, 0); // 切换 takeOver
        PostMessageW(controller, WM_COMMAND, 3, 0); // 切换 clipboardWatch
    }
    sleep(450);
    let cfg_after = fs::read_to_string(data.join("config.json")).unwrap_or_default();
    let lg = log_since(&trace_path, base);
    let alive = wait_fresh(&state_path, t_before, 1500).t > t_before + 80.0;
    runner.check("T17", "C3 托盘两通道开关落盘（WM_COMMAND 消息注入）",
        lg.count("command") >= 2 && cfg_after != cfg_before && alive,
        format!("commands={} config_changed={} 消息循环存活={} events=[{}]",
            lg.count("command"), cfg_after != cfg_before, alive, lg.summary()));
    // 配置文件写回原值（GUI 内存里的开关保持切换后状态，后续不再依赖捕获）
    let _ = fs::write(data.join("config.json"),
        "{\"showLine\":true,\"takeOver\":true,\"clipboardWatch\":false,\"maxPegs\":10,\"firstRunDone\":true}");

    // ---- T20 单实例锁 ----
    let t0 = Instant::now();
    let second = Command::new(&exe)
        .env("TQ_DATA_DIR", &data)
        .env("TQ_SHOTS_DIR", &shots)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let elapsed = t0.elapsed().as_millis();
    let exit_ok = second.as_ref().map(|s| s.success()).unwrap_or(false);
    runner.check("T20", "E1 第二个实例立即退出（单实例锁）",
        exit_ok && elapsed < 3000,
        format!("exit_ok={} elapsed={}ms", exit_ok, elapsed));

    healthy!();
    // ---- T17b 托盘右键菜单真实弹出 + ESC 关闭 ----
    // 放在最后：TrackPopupMenuEx 是模态循环，万一 ESC 没解除也不至于污染其它判据。
    let base_tray = log_len(&trace_path);
    let t_tray = read_state(&state_path).t;
    unsafe { PostMessageW(controller, WM_APP_TRAY, 0, 0x0205isize) }; // 低 16 位 = WM_RBUTTONUP
    sleep(350);
    for _ in 0..2 {
        esc_key();
        sleep(160);
    }
    sleep(400);
    let lg_tray = log_since(&trace_path, base_tray);
    let alive_tray = wait_fresh(&state_path, t_tray, 2000).t > t_tray + 80.0;
    runner.check("T17b", "C3 托盘右键菜单可弹出、可关闭、不卡死消息循环",
        lg_tray.has("tray_open") && alive_tray,
        format!("tray_open={} 消息循环存活={} events=[{}]",
            lg_tray.has("tray_open"), alive_tray, lg_tray.summary()));

    finish(&mut child, &runner, &report_path, &state_path, controller, home, &clip_backup);
}

fn finish(child: &mut Option<Child>, runner: &Runner, report: &Path, state: &Path,
          controller: isize, home: Point, clip: &ClipBackup) {
    // 关掉仍残留的测试期新窗口由调用方负责；这里只做收尾
    unsafe { PostMessageW(controller, WM_APP_QUIT, 0, 0) };
    if let Some(c) = child.as_mut() {
        // 托盘菜单万一没被 ESC 关掉，GUI 会卡在模态循环里；带超时强制收尾，
        // 绝不把注入器自己挂死在这。
        let t0 = Instant::now();
        loop {
            match c.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if t0.elapsed() < Duration::from_secs(3) => sleep(100),
                _ => { let _ = c.kill(); let _ = c.wait(); break; }
            }
        }
    }
    unsafe { SetCursorPos(home.x, home.y) };
    clip.restore();
    let rejects = REJECTS.load(Ordering::SeqCst);
    let body = &runner.to_json()[1..];
    let json = if blocked() {
        let reason = abort_reason().replace('\\', "\\\\").replace('"', "\\\"");
        format!("{{\"aborted\":true,\"abort_reason\":\"{}\",\"guard_rejects\":{},{}",
            reason, rejects, body)
    } else {
        format!("{{\"guard_rejects\":{},{}", rejects, body)
    };
    let _ = fs::write(report, json);
    let pass = runner.results.iter().filter(|r| r.pass).count();
    println!("\n==== 真机手势验收：{}/{} PASS ====", pass, runner.results.len());
    println!("[GUARD] 拦截了 {} 次落点不安全的按钮按下（均未投递到系统）", rejects);
    if blocked() {
        println!("[ABORT] 测试提前熔断：{}（未执行的用例不代表功能失败）", abort_reason());
    }
    for r in runner.results.iter().filter(|r| !r.pass) {
        println!("  FAIL {} {} -> {}", r.id, r.name, r.detail);
    }
    println!("report: {}", report.display());
    let _ = state;
}
