//! 纯 Win32 / GDI+ 自绘 GUI（Q1 路线）。
//!
//! 与 Electron 版的行为对齐点：
//!   - 每显示器一个 workArea 宽 × 210px 的透明分层窗口，不抢焦点、置顶
//!   - 默认 WS_EX_TRANSPARENT（空白穿透），命中相片/菜单才摘掉该位
//!   - 顶边中部 22%~78%、4px 触发带、停留 250ms 呼出；离开 300ms 收起
//!   - 单击复制 / 双击打开 / 长按 450ms 画图 / 右键菜单 / ✕ 回收站 / Ctrl+Alt+T
//!   - 阻尼弹簧 K=30 C=2.6、微风 1.6~3.4°、tilt ±2.5°、绳垂度 min(30, w*0.018)
//!   - peek 2.5s、收起 easeIn 0.22s、展开 easeOutBack 0.42s

#![allow(dead_code)]

use crate::api::*;
use crate::capture::{CaptureCore, ClipDecision};
use crate::fullscreen::{is_fullscreen as fg_is_fullscreen, Display as FgDisplay};
use crate::inbox::{list_inbox, Item};
use crate::memprobe::MemTracker;
use crate::settings::Settings;
use crate::{gdi, png, trace, visual, win32};
use std::collections::HashMap;
use std::ffi::c_void;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

const PANEL_H: f32 = 210.0;
const CARD_W: f32 = 150.0;
const SPACING: f32 = 174.0;
const ROPE_TOP: f32 = 10.0;
const EDGE_PAD: f32 = 24.0;
const CARD_H: f32 = 131.0;
const HIDE_OFFSET: f32 = -222.0;
const REVEAL_DELAY_MS: i64 = 250;
const HIDE_DELAY_MS: i64 = 300;
const PEEK_MS: i64 = 2500;
const BAND_H: f32 = 4.0;
const HOLD_MS: i64 = 450;
const DRAG_PX: f32 = 7.0;
const DOUBLE_MS: i64 = 350;
const TOAST_MS: i64 = 1200;
const CROSS_TTL: i64 = 10_000;

const ID_TOGGLE: u16 = 1;
const ID_TAKEOVER: u16 = 2;
const ID_CLIPWATCH: u16 = 3;
const ID_INBOX: u16 = 4;
const ID_AUTOSTART: u16 = 5;
const ID_ABOUT: u16 = 6;
const ID_QUIT: u16 = 7;

/// 真机验收专用：让注入器能干净地结束 GUI（不改变正常退出路径）
const WM_APP_QUIT: u32 = WM_APP + 2;

// 原版 line.css：html/body 是 cursor:default，只有 .xbtn 与菜单项是 cursor:pointer。
// 窗口类必须显式给光标，否则 hCursor=0 时 DefWindowProc 不会重置系统残留的忙碌光标，
// 鼠标移到相片上会一直显示转圈。
const IDC_ARROW: u16 = 32512;
const IDC_HAND: u16 = 32649;
const HTCLIENT: u32 = 1;
fn sys_cursor(id: u16) -> isize {
    // MAKEINTRESOURCE(id)：预定义光标用整数 ID 当资源名，instance 传 0。
    unsafe { LoadCursorW(0, id as usize as *const u16) }
}

const MENU_ITEMS: [&str; 7] = [
    "复制图片",
    "用画图编辑",
    "打开",
    "另存为…",
    "",
    "取下并保存到截图文件夹",
    "删除(移入回收站)",
];
const MENU_W: f32 = 196.0;
const MENU_ROW: f32 = 26.0;
const MENU_SEP: f32 = 9.0;
const MENU_PAD: f32 = 5.0;

struct Surface {
    hdc: isize,
    dib: isize,
    old: isize,
    bits: *mut u8,
    w: i32,
    h: i32,
}

struct LineDisplay {
    hwnd: isize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    sf: f32,
    surface: Surface,
}

struct Peg {
    id: String,
    file: PathBuf,
    mtime: f64,
    thumb: *mut c_void,
    tw: i32,
    th: i32,
    x: f32,
    ropey: f32,
    tilt: f32,
    a: f32,
    v: f32,
    hover: bool,
    falling: bool,
    arrived_at: i64,
}

impl Drop for Peg {
    fn drop(&mut self) {
        if !self.thumb.is_null() {
            unsafe { gdi::GdipDisposeImage(self.thumb) };
        }
    }
}

#[derive(Default)]
struct Gesture {
    down: bool,
    down_x: i32,
    down_y: i32,
    down_at: i64,
    held: bool,
    dragging: bool,
    id: String,
    last_click_at: i64,
    last_click_id: String,
}

struct App {
    cfg: Settings,
    data_dir: PathBuf,
    inbox_dir: PathBuf,
    screenshots: PathBuf,
    pictures: PathBuf,
    displays: Vec<LineDisplay>,
    controller: isize,
    tray_added: bool,
    items: Vec<Item>,
    pegs: Vec<Peg>,
    core: CaptureCore,
    tracker: MemTracker,
    shown: bool,
    pinned: bool,
    offset: f32,
    anim_from: f32,
    anim_to: f32,
    anim_start: i64,
    anim_ms: i64,
    animating: bool,
    since_in_band: i64,
    hide_at: i64,
    peek_until: i64,
    capturing: bool,
    dragging_until: i64,
    gesture: Gesture,
    menu_open: bool,
    menu_id: String,
    menu_x: f32,
    menu_y: f32,
    toast: String,
    toast_until: i64,
    fg_fullscreen: bool,
    pending: HashMap<PathBuf, (u64, u8)>,
    last_tick: [i64; 6],
    next_gust: i64,
    gusts: Vec<(i64, usize, f32)>,
    rng: u64,
    timer_ms: u32,
    last_items_sig: String,
    probe_out: Option<PathBuf>,
    probe_start: std::time::Instant,
    first_run_show_at: i64,
    /// 测试钩子：TQ_FORCE_SHOW=1 时启动后强制展开并保持，用于 S1 峰值场景
    force_show: bool,
    last_mem: String,
}

static mut APP: Option<App> = None;
/// 消息循环单线程；用裸指针打破跨函数借用链（窗口过程会回调多个子系统）。
#[allow(static_mut_refs)]
fn app() -> &'static mut App {
    unsafe {
        match APP.as_mut() {
            Some(a) => &mut *(a as *mut App),
            None => panic!("App 未初始化"),
        }
    }
}

static RNG: AtomicU64 = AtomicU64::new(8_812_726_314_123_252);
fn rand01() -> f32 {
    let n = RNG
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |x| {
            Some(x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1))
        })
        .unwrap_or(1);
    ((n >> 33) as f32) / (u32::MAX as f32)
}
fn rand_range(lo: f32, hi: f32) -> f32 {
    lo + rand01() * (hi - lo)
}

fn now() -> i64 {
    crate::capture::now_ms()
}

/// 真机验收状态快照（仅 TQ_STATE 存在时写）。坐标同时给逻辑值与屏幕物理值，
/// 注入器据此换算 SendInput 落点，避免在测试脚本里重复实现布局公式。
static LAST_STATE: AtomicI64 = AtomicI64::new(0);
fn write_state(a: &App) {
    if !trace::enabled() {
        return;
    }
    let t = now();
    // 40ms：真机验收要在 220ms 的收起动画里采到中间帧（仅测试钩子开启时生效）
    if t - LAST_STATE.load(Ordering::Relaxed) < 40 {
        return;
    }
    LAST_STATE.store(t, Ordering::Relaxed);
    let (cpx, cpy) = win32::cursor_pos(); // 真机诊断：GUI 实际读到的光标
    let mut s = format!(
        concat!(
            "{{\"t\":{},\"shown\":{},\"pinned\":{},\"offset\":{:.2},\"animating\":{},",
            "\"capturing\":{},\"menu_open\":{},\"menu_x\":{:.1},\"menu_y\":{:.1},",
            "\"fg_fullscreen\":{},\"item_count\":{},\"toast\":\"{}\",\"controller\":{},",
            "\"cx\":{},\"cy\":{},\"displays\":["
        ),
        t, a.shown, a.pinned, a.offset, a.animating, a.capturing,
        a.menu_open, a.menu_x, a.menu_y, a.fg_fullscreen,
        a.items.len(), trace::esc(&a.toast), a.controller, cpx, cpy
    );
    for (i, d) in a.displays.iter().enumerate() {
        let ex = unsafe { GetWindowLongW(d.hwnd, GWL_EXSTYLE) as u32 };
        if i > 0 { s.push(','); }
        s.push_str(&format!(
            "{{\"hwnd\":{},\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"sf\":{:.3},\"exstyle\":{},\"transparent\":{}}}",
            d.hwnd, d.x, d.y, d.w, d.h, d.sf, ex, ex & WS_EX_TRANSPARENT != 0
        ));
    }
    s.push_str("],\"pegs\":[");
    let d = a.displays.first();
    for (i, p) in a.pegs.iter().enumerate() {
        if i > 0 { s.push(','); }
        let (sx, sy, bx, by) = match d {
            Some(d) => (
                d.x + (p.x * d.sf) as i32,
                d.y + ((p.ropey + 12.0 + CARD_H / 2.0 + a.offset) * d.sf) as i32,
                d.x + ((p.x + 75.0) * d.sf) as i32,
                d.y + ((p.ropey + 12.0 + a.offset) * d.sf) as i32,
            ),
            None => (0, 0, 0, 0),
        };
        s.push_str(&format!(
            "{{\"id\":\"{}\",\"x\":{:.1},\"ropey\":{:.1},\"sx\":{},\"sy\":{},\"bx\":{},\"by\":{}}}",
            trace::esc(&p.id), p.x, p.ropey, sx, sy, bx, by
        ));
    }
    s.push_str("]}");
    trace::snapshot(&s);
}

impl App {
    fn toast(&mut self, s: &str) {
        self.toast = s.to_string();
        self.toast_until = now() + TOAST_MS;
        trace::ev("toast", s);
    }

    fn visible_count(&self, w: f32) -> usize {
        let fit = ((w - EDGE_PAD * 2.0) / SPACING).floor().max(3.0) as usize;
        self.items.len().min(self.cfg.max_pegs.max(0) as usize).min(fit)
    }

    fn layout(&mut self, w: f32) {
        let n = self.visible_count(w);
        let start = self.items.len().saturating_sub(n);
        let spacing = if n > 1 {
            SPACING.min((w - EDGE_PAD * 2.0 - CARD_W) / (n as f32 - 1.0))
        } else {
            0.0
        };
        let sag = (30.0_f32).min(w * 0.018);
        let cx = w / 2.0;
        let mut keep: Vec<(&str, &PathBuf, f64)> = Vec::new();
        for it in self.items.iter().skip(start) {
            keep.push((&it.id, &it.file, it.mtime_ms));
        }
        // 删除不在可见集合里的 peg
        self.pegs.retain(|p| keep.iter().any(|(id, _, _)| *id == p.id));
        for (i, (id, file, mtime)) in keep.into_iter().enumerate() {
            let x = if n > 1 {
                cx + (i as f32 - (n - 1) as f32 / 2.0) * spacing
            } else {
                cx
            };
            let f = if w > 0.0 { x / w } else { 0.0 };
            let ropey = ROPE_TOP + 4.0 * sag * f * (1.0 - f);
            if let Some(p) = self.pegs.iter_mut().find(|p| p.id == *id) {
                p.x = x;
                p.ropey = ropey;
                continue;
            }
            let src = gdi::load_image(file);
            let thumb = gdi::make_thumb(src, 136, 104);
            let (tw, th) = if thumb.is_null() {
                (0, 0)
            } else {
                let mut a = 0u32;
                let mut b = 0u32;
                unsafe {
                    gdi::GdipGetImageWidth(thumb, &mut a);
                    gdi::GdipGetImageHeight(thumb, &mut b);
                }
                (a as i32, b as i32)
            };
            if !src.is_null() {
                unsafe { gdi::GdipDisposeImage(src) };
            }
            let tilt = rand_range(-2.5, 2.5);
            let v = rand_range(14.0, 22.0);
            self.pegs.push(Peg {
                id: id.to_string(),
                file: file.clone(),
                mtime,
                thumb,
                tw,
                th,
                x,
                ropey,
                tilt,
                a: 0.0,
                v,
                hover: false,
                falling: false,
                arrived_at: now(),
            });
        }
        self.pegs.sort_by(|a, b| a.mtime.partial_cmp(&b.mtime).unwrap_or(std::cmp::Ordering::Equal));
    }

    fn rescan(&mut self) {
        self.items = list_inbox(&self.inbox_dir);
        let w = self.displays.first().map(|d| d.w as f32 / d.sf).unwrap_or(1920.0);
        self.layout(w);
        let sig = crate::inbox::items_to_json(&self.items);
        self.last_items_sig = sig;
    }

    fn reveal(&mut self, pinned: bool, peek: bool) {
        if !self.cfg.show_line {
            return;
        }
        self.shown = true;
        self.pinned = pinned;
        self.hide_at = 0;
        self.peek_until = if peek { now() + PEEK_MS } else { 0 };
        self.start_anim(HIDE_OFFSET.max(self.offset), 0.0, 420);
        self.set_capture(false);
        self.next_gust = now() + 300;
        trace::ev("reveal", &format!("pinned={} peek={}", pinned, peek));
    }

    fn hide(&mut self) {
        if !self.shown {
            return;
        }
        self.shown = false;
        self.pinned = false;
        self.peek_until = 0;
        self.menu_open = false;
        self.start_anim(self.offset, HIDE_OFFSET, 220);
        // 等价 hideLine()：等收起动画播完（260ms）再恢复完全穿透，
        // 期间保持接管，避免收起过程中点击落空
        for p in &mut self.pegs {
            p.a = 0.0;
            p.v = 0.0;
        }
        trace::ev("hide", "");
    }

    fn start_anim(&mut self, from: f32, to: f32, ms: i64) {
        self.anim_from = from;
        self.anim_to = to;
        self.offset = from;
        self.anim_start = now();
        self.anim_ms = ms;
        self.animating = true;
    }

    fn set_capture(&mut self, on: bool) {
        if self.capturing == on {
            return;
        }
        self.capturing = on;
        trace::ev("capture", if on { "on" } else { "off" });
        // 真机验收诊断：记录判定时刻的光标与状态，用于定位"相片上却放开穿透"的误判。
        // 仅在 TQ_TRACE 存在时写，默认运行零开销。
        let (cpx, cpy) = win32::cursor_pos();
        trace::ev("capture_diag", &format!(
            "on={} px={} py={} shown={} off={:.1} drag={} pegs={}",
            on, cpx, cpy, self.shown, self.offset,
            now() < self.dragging_until, self.pegs.len()));
        for d in &self.displays {
            unsafe {
                let ex = GetWindowLongW(d.hwnd, GWL_EXSTYLE) as u32;
                let v = if on { ex & !WS_EX_TRANSPARENT } else { ex | WS_EX_TRANSPARENT };
                SetWindowLongW(d.hwnd, GWL_EXSTYLE, v as i32);
            }
        }
        if on {
            // 外部窗口（看图器/浏览器）被激活后可能压到分层窗口之前。
            // 等价 Electron alwaysOnTop('screen-saver')：接管瞬间重新钉回最顶层，
            // 且 NOACTIVATE 不抢焦点、不改 A1「不抢焦点」判据。
            for d in &self.displays {
                unsafe {
                    SetWindowPos(d.hwnd, HWND_TOPMOST, 0, 0, 0, 0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
            }
            // 接管瞬间合成一次 mousemove，否则"移进相片后停住"没有 hover
            let (px, py) = unsafe {
                let mut p = Point { x: 0, y: 0 };
                GetCursorPos(&mut p);
                (p.x, p.y)
            };
            if let Some(d) = self.displays.first() {
                unsafe {
                    // 异步派发，避免窗口过程重入造成状态别名
                    PostMessageW(
                        d.hwnd,
                        WM_MOUSEMOVE,
                        0,
                        makelparam(px - d.x, py - d.y),
                    );
                }
            }
        }
    }

    fn cursor_hits(&self, px: i32, py: i32) -> Option<usize> {
        if !self.shown || now() < self.dragging_until {
            return if now() < self.dragging_until { Some(usize::MAX) } else { None };
        }
        let d = self.displays.iter().find(|d| {
            px >= d.x && px <= d.x + d.w && py >= d.y && py <= d.y + d.h
        })?;
        let s = d.sf;
        let lx = (px - d.x) as f32 / s;
        let ly = (py - d.y) as f32 / s - self.offset;
        if self.menu_open {
            // 等价 line.js: hitsPeg() 的 `if (!els.menu.hidden) return true`
            // 菜单开启时整条接管，否则鼠标一移到菜单外的空白区就恢复穿透，
            // "点在菜单外面才收起"的 mousedown 会落到底下窗口，菜单永远关不掉。
            return Some(usize::MAX);
        }
        for (i, p) in self.pegs.iter().enumerate() {
            let left = p.x - CARD_W / 2.0 - 2.0;
            let top = p.ropey + 12.0 - 2.0;
            if lx >= left && lx <= left + CARD_W + 4.0 && ly >= top && ly <= top + CARD_H + 4.0 {
                return Some(i);
            }
        }
        None
    }

    fn hit_peg_id(&self, px: i32, py: i32) -> Option<String> {
        match self.cursor_hits(px, py) {
            Some(i) if i != usize::MAX => self.pegs.get(i).map(|p| p.id.clone()),
            _ => None,
        }
    }

    fn item_path(&self, id: &str) -> Option<PathBuf> {
        self.items.iter().find(|i| i.id == id).map(|i| i.file.clone())
    }

    fn remove_peg(&mut self, id: &str) {
        self.pegs.retain(|p| p.id != id);
    }
}

fn menu_index(mx: f32, my: f32, lx: f32, ly: f32) -> Option<usize> {
    if lx < mx || lx > mx + MENU_W {
        return None;
    }
    let mut yy = my + MENU_PAD;
    for (i, label) in MENU_ITEMS.iter().enumerate() {
        let h = if label.is_empty() { MENU_SEP } else { MENU_ROW };
        if ly >= yy && ly <= yy + h {
            return if label.is_empty() { None } else { Some(i) };
        }
        yy += h;
    }
    None
}

fn menu_height() -> f32 {
    MENU_PAD * 2.0 + MENU_SEP + MENU_ROW * 6.0
}

// ---------------- 绘制 ----------------

fn render_all() {
    let n = app().displays.len();
    for i in 0..n {
        render_one(i);
    }
}

fn render_one(idx: usize) {
    let a = app();
    if !a.shown && a.offset <= HIDE_OFFSET + 0.5 {
        let hwnd = a.displays[idx].hwnd;
        unsafe { ShowWindow(hwnd, SW_HIDE) };
        return;
    }
    let (hwnd, pw, ph, x, y, sf, bits, hdc) = {
        let d = &a.displays[idx];
        (d.hwnd, d.w, d.h, d.x, d.y, d.sf, d.surface.bits, d.surface.hdc)
    };
    if bits.is_null() {
        return;
    }
    let lw = pw as f32 / sf;
    let offset = a.offset;
    let canvas = match gdi::Canvas::new(pw, ph) {
        Some(c) => c,
        None => return,
    };
    canvas.scale(sf);
    canvas.translate(0.0, offset);

    let sag = 30.0_f32.min(lw * 0.018);
    // 绳：浅色偏移 + 深色主线
    canvas.quad(0, (ROPE_TOP + 1.6) as i32, (lw / 2.0) as i32, (ROPE_TOP + 1.6 + 2.0 * sag) as i32, lw as i32, (ROPE_TOP + 1.6) as i32, 0x48_FFF_CF4, 1.1);
    canvas.quad(0, ROPE_TOP as i32, (lw / 2.0) as i32, (ROPE_TOP + 2.0 * sag) as i32, lw as i32, ROPE_TOP as i32, 0xFF_3E342A, 2.4);

    let pegs: Vec<(f32, f32, f32, f32, f32, i32, i32, *mut c_void, bool, bool, f32)> = a
        .pegs
        .iter()
        .map(|p| (p.x, p.ropey, p.tilt, p.a, p.v, p.tw, p.th, p.thumb, p.hover, p.falling, (now() - p.arrived_at) as f32))
        .collect();
    let shown = a.shown;
    for (x, ropey, tilt, ang, _v, tw, th, thumb, hover, falling, age) in pegs {
        let state = canvas.save();
        let drop = if falling { (age / 300.0).min(1.0) * 130.0 } else { 0.0 };
        let arrive = if age < 460.0 { (1.0 - age / 460.0) * -34.0 } else { 0.0 };
        if drop > 0.0 {
            canvas.translate(0.0, drop);
        }
        if arrive != 0.0 {
            canvas.translate(0.0, arrive);
        }
        canvas.rotate_around(x, ropey, tilt + ang);
        let cx = x as i32;
        let cy = ropey as i32;
        // 衣夹
        canvas.round_rect(cx - 6, cy - 11, 12, 26, 4, 0xFF_C99A66);
        canvas.fill_rect(cx - 4, cy - 11, 3, 26, 0xFF_E3B981);
        canvas.fill_rect(cx - 8, cy - 5, 16, 7, 0xFF_8A6B4A);
        canvas.line(cx, cy + 3, cx, cy + 15, 0xA6_462D14, 1.6);
        // 相框阴影
        let top = cy + 12;
        for k in 0..3 {
            canvas.round_rect(cx - 75 + k * 2, top + k * 3, 150, 131, 9, 0x14_0F0C08);
        }
        // 白边拍立得
        canvas.round_rect(cx - 75, top, 150, 131, 9, 0xFF_FFFE_FB);
        canvas.fill_rect(cx - 68, top + 20, 136, 100, 0xFF_F6F2E9);
        // 照片（缩略图，保持比例居中）
        if !thumb.is_null() && tw > 0 && th > 0 {
            let pw2 = tw as f32;
            let ph2 = th as f32;
            let px = (x - pw2 / 2.0) as i32;
            let py = (top as f32 + 7.0 + (104.0 - ph2) / 2.0) as i32;
            canvas.image(thumb, px, py, tw, th);
        }
        // 玻璃斜光
        canvas.fill_rect(cx - 68, top + 7, 136, 12, 0x24_FFFFFF);
        // hover：卡片微放大用外框近似；✕ 按钮
        if hover && shown {
            canvas.round_rect(cx - 76, top - 1, 152, 133, 10, 0x22_FFFFFF);
            canvas.fill_rect(cx - 68 + 136 - 12, top + 7, 26, 4, 0x14_FFFFFF);
            canvas.fill_rect(cx + 66, top - 9, 18, 18, 0xD0_28221A);
            canvas.text("✕", (cx + 66) as f32, (top - 9) as f32, 18.0, 18.0, 11.0, 0xFF_F4EFE6, true);
        }
        canvas.restore(state);
    }

    // 空状态 / 提示 / toast / 菜单
    if a.items.is_empty() {
        canvas.round_rect((lw / 2.0 - 150.0) as i32, 96, 300, 30, 15, 0xE0_FCFAF5);
        canvas.text("还没有截图 · 按 Win+Shift+S 拍一张", lw / 2.0 - 150.0, 96.0, 300.0, 30.0, 12.5, 0xFF_6B5C48, true);
    }
    canvas.text("点击复制 · 双击打开 · 长按编辑 · 拖出保存 · 右键更多", 14.0, PANEL_H - 22.0, 460.0, 18.0, 11.0, 0x80_463A2C, false);
    if a.menu_open {
        draw_menu(&canvas, a.menu_x, a.menu_y, lw);
    }
    if now() < a.toast_until && !a.toast.is_empty() {
        let tw = a.toast.chars().count() as f32 * 12.0 + 28.0;
        canvas.round_rect((lw / 2.0 - tw / 2.0) as i32, PANEL_H as i32 - 34, tw as i32, 24, 12, 0xD9_201C16);
        canvas.text(&a.toast, lw / 2.0 - tw / 2.0, PANEL_H - 34.0, tw, 24.0, 12.0, 0xFF_F7F3EA, true);
    }

    canvas.blit_premultiplied(bits, pw * 4);
    drop(canvas);
    // UpdateLayeredWindow 的命中测试按逐像素 alpha：alpha=0 的空白区鼠标会直接穿透，
    // 哪怕 WS_EX_TRANSPARENT 已经摘掉。接管期间把整幅表面的 alpha 下限抬到 1
    // （1/255 的暗化肉眼不可见），这样"菜单整条接管 / 点外面才收起"和
    // "拖出时 mousemove 不断流"才有物理基础，等价 Electron 的 setIgnoreMouseEvents(false)。
    // 展开态常驻抬升：几何命中判定（cursor_hits）与"实际绘制像素"在相框边缘
    // 会有 1px 级误差，若只在 capturing 时抬升，鼠标第一次落到边缘点时
    // alpha 仍为 0、事件根本到不了窗口，形成"永远接管不上"的死锁。
    // 穿透与否由 WS_EX_TRANSPARENT 决定，抬到 1/255 不影响 A2 空白穿透。
    if a.shown || a.capturing || a.menu_open || a.gesture.down {
        let n = (pw as usize) * (ph as usize) * 4;
        let buf = unsafe { std::slice::from_raw_parts_mut(bits, n) };
        for px in buf.chunks_exact_mut(4) {
            if px[3] == 0 {
                px[3] = 1;
            }
        }
    }
    unsafe {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let blend = BlendFunction {
            blend_op: AC_SRC_OVER,
            blend_flags: 0,
            source_constant_alpha: 255,
            alpha_format: AC_SRC_ALPHA,
        };
        let dst = Point { x, y };
        let size = Size { cx: pw, cy: ph };
        let src = Point { x: 0, y: 0 };
        UpdateLayeredWindow(hwnd, 0, &dst, &size, hdc, &src, 0, &blend, ULW_ALPHA);
    }
}

fn draw_menu(c: &gdi::Canvas, mx: f32, my: f32, lw: f32) {
    let x = mx.min(lw - MENU_W - 6.0);
    let y = my.min(PANEL_H - menu_height() - 6.0);
    c.round_rect(x as i32, y as i32, MENU_W as i32, menu_height() as i32, 10, 0xDB_24201A);
    let mut yy = y + MENU_PAD;
    for (i, label) in MENU_ITEMS.iter().enumerate() {
        if label.is_empty() {
            c.fill_rect((x + 8.0) as i32, (yy + 4.0) as i32, (MENU_W - 16.0) as i32, 1, 0x21_FFFFFF);
            yy += MENU_SEP;
        } else {
            let color = if i == 6 { 0xFF_FF9D8F } else { 0xFF_F2EDE3 };
            c.text(label, x + 12.0, yy, MENU_W - 24.0, MENU_ROW, 12.5, color, false);
            yy += MENU_ROW;
        }
    }
}

// ---------------- 系统操作 ----------------

fn copy_to_clipboard(path: &PathBuf) -> bool {
    unsafe {
        let img = gdi::load_image(path);
        if img.is_null() {
            return false;
        }
        let mut w = 0u32;
        let mut h = 0u32;
        gdi::GdipGetImageWidth(img, &mut w);
        gdi::GdipGetImageHeight(img, &mut h);
        if w == 0 || h == 0 {
            gdi::GdipDisposeImage(img);
            return false;
        }
        let (wi, hi) = (w as i32, h as i32);
        let mut bmp: *mut c_void = std::ptr::null_mut();
        if gdi::GdipCreateBitmapFromScan0(wi, hi, 0, gdi::PIXEL_ARGB, std::ptr::null_mut(), &mut bmp) != 0 {
            gdi::GdipDisposeImage(img);
            return false;
        }
        let mut g: *mut c_void = std::ptr::null_mut();
        gdi::GdipGetImageGraphicsContext(bmp, &mut g);
        gdi::GdipDrawImageRectI(g, img, 0, 0, wi, hi);
        gdi::GdipDeleteGraphics(g);
        let mut data: gdi::BitmapData = std::mem::zeroed();
        let rect = gdi::GpRect { x: 0, y: 0, w: wi, h: hi };
        let ok = gdi::GdipBitmapLockBits(bmp, &rect, 1, gdi::PIXEL_ARGB, &mut data) == 0 && !data.scan0.is_null();
        let mut result = false;
        if ok {
            let stride = ((wi * 32 + 31) / 32) * 4;
            let total = 40 + stride * hi;
            let hg = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, total as usize);
            if hg != 0 {
                let p = GlobalLock(hg) as *mut u8;
                if !p.is_null() {
                    let hdr = p as *mut BitmapInfoHeader;
                    *hdr = BitmapInfoHeader {
                        bi_size: 40,
                        bi_width: wi,
                        bi_height: hi,
                        bi_planes: 1,
                        bi_bit_count: 32,
                        bi_compression: 0,
                        bi_size_image: (stride * hi) as u32,
                        bi_x_pels_per_meter: 0,
                        bi_y_pels_per_meter: 0,
                        bi_clr_used: 0,
                        bi_clr_important: 0,
                    };
                    // CF_DIB 自下而上，ARGB 位图自上而下
                    for yy in 0..hi as usize {
                        let srow = data.scan0.add(yy * data.stride as usize);
                        let drow = p.add(40 + (hi as usize - 1 - yy) * stride as usize);
                        for xx in 0..wi as usize {
                            let argb = std::ptr::read_unaligned(srow.add(xx * 4) as *const u32);
                            let o = xx * 4;
                            *drow.add(o) = (argb & 0xFF) as u8;
                            *drow.add(o + 1) = ((argb >> 8) & 0xFF) as u8;
                            *drow.add(o + 2) = ((argb >> 16) & 0xFF) as u8;
                            *drow.add(o + 3) = 255;
                        }
                    }
                    GlobalUnlock(hg);
                    if win32_open_clipboard() {
                        win32_empty_clipboard();
                        win32_set_clipboard(hg);
                        win32_close_clipboard();
                        result = true;
                    } else {
                        GlobalFree(hg);
                    }
                } else {
                    GlobalUnlock(hg);
                    GlobalFree(hg);
                }
            }
            gdi::GdipBitmapUnlockBits(bmp, &mut data);
        }
        gdi::GdipDisposeImage(bmp);
        gdi::GdipDisposeImage(img);
        result
    }
}

#[link(name = "user32")]
extern "system" {
    fn OpenClipboard(h: isize) -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(fmt: u32, h: isize) -> isize;
    fn CloseClipboard() -> i32;
    fn PostMessageW(h: isize, msg: u32, wp: usize, lp: isize) -> i32;
}
fn win32_open_clipboard() -> bool { unsafe { OpenClipboard(0) != 0 } }
fn win32_empty_clipboard() { unsafe { EmptyClipboard(); } }
fn win32_set_clipboard(h: isize) { unsafe { SetClipboardData(CF_DIB, h); } }
fn win32_close_clipboard() { unsafe { CloseClipboard(); } }

fn shell_open(path: &PathBuf) {
    trace::ev("shell_open", &path.to_string_lossy());
    let f = wide(&path.to_string_lossy());
    let op = wide("open");
    unsafe { ShellExecuteW(0, op.as_ptr(), f.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
}

fn shell_edit(path: &PathBuf) {
    trace::ev("shell_edit", &path.to_string_lossy());
    let exe = wide("mspaint.exe");
    let f = wide(&path.to_string_lossy());
    let op = wide("open");
    unsafe { ShellExecuteW(0, op.as_ptr(), exe.as_ptr(), f.as_ptr(), std::ptr::null(), SW_SHOWNORMAL) };
}

fn trash(path: &PathBuf) -> Result<(), String> {
    let from = wide_double(&path.to_string_lossy());
    let mut op = ShFileOpStructW {
        hwnd: 0,
        w_func: FO_DELETE,
        p_from: from.as_ptr(),
        p_to: std::ptr::null(),
        f_flags: FOF_SILENT | FOF_NOCONFIRMATION | FOF_ALLOWUNDO | FOF_NOERRORUI,
        f_any_operations_aborted: 0,
        h_name_mappings: std::ptr::null_mut(),
        lpsz_progress_title: std::ptr::null(),
    };
    let r = unsafe { SHFileOperationW(&mut op) };
    if r == 0 && op.f_any_operations_aborted == 0 {
        Ok(())
    } else {
        Err(format!("SHFileOperation 失败 (code {})", r))
    }
}

fn save_as(a: &mut App, id: &str) {
    let Some(path) = a.item_path(id) else { return };
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "png".into());
    let filter = wide(&format!("图片 (*.{})\0*.{}\0\0", ext, ext));
    let mut buf = [0u16; 260];
    let name = wide(id);
    for (i, c) in name.iter().take(259).enumerate() {
        buf[i] = *c;
    }
    let title = wide("另存为");
    let def = wide(&ext);
    let mut ofn: OpenFileNameW = unsafe { std::mem::zeroed() };
    ofn.l_struct_size = std::mem::size_of::<OpenFileNameW>() as u32;
    ofn.hwnd_owner = a.controller;
    ofn.lpstr_filter = filter.as_ptr();
    ofn.lpstr_file = buf.as_mut_ptr();
    ofn.n_max_file = 260;
    ofn.lpstr_title = title.as_ptr();
    ofn.lpstr_def_ext = def.as_ptr();
    let ok = unsafe { GetSaveFileNameW(&mut ofn) };
    if ok == 0 {
        return;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let dest = String::from_utf16_lossy(&buf[..len]);
    if fs::copy(&path, &dest).is_ok() {
        a.toast("已保存");
    }
}

fn take_down(a: &mut App, id: &str) {
    let Some(path) = a.item_path(id) else { return };
    let dest_dir = a.pictures.join("Screenshots");
    let _ = fs::create_dir_all(&dest_dir);
    let dest = dest_dir.join(id);
    let ok = fs::rename(&path, &dest).is_ok()
        || (fs::copy(&path, &dest).is_ok() && fs::remove_file(&path).is_ok());
    if ok {
        a.rescan();
    }
}

fn discard(a: &mut App, id: &str) {
    let Some(path) = a.item_path(id) else {
        a.rescan();
        return;
    };
    if !path.is_absolute() {
        a.toast("删除失败: 路径无效");
        return;
    }
    if !path.exists() {
        a.rescan();
        return;
    }
    match trash(&path) {
        Ok(()) => {
            trace::ev("discard", &format!("ok:{}", path.display()));
            a.remove_peg(id);
            a.rescan();
            a.toast("已移入回收站");
        }
        Err(e) => {
            trace::ev("discard", &format!("fail:{}", e));
            a.rescan();
            a.toast(&format!("删除失败: {}", e));
        }
    }
}

fn do_copy(a: &mut App, id: &str) {
    let Some(path) = a.item_path(id) else {
        trace::ev("copy", "fail:no-item");
        return;
    };
    let ok = copy_to_clipboard(&path);
    trace::ev("copy", if ok { "ok" } else { "fail:clipboard" });
    if ok {
        // 读回刚写入的图，抑制轮询把自己再挂一次
        if let Some(snap) = win32::clipboard_snapshot() {
            let key = visual::visual_key(&snap.quantized, snap.width, snap.height);
            a.core.suppress_copy(&key, now());
        }
        a.toast("已复制到剪贴板");
    }
}

fn autostart_enabled() -> bool {
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let name = wide("图墙");
    let mut hk: isize = 0;
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_QUERY_VALUE, &mut hk) != 0 {
            return false;
        }
        let mut len = 0u32;
        let mut kind = 0u32;
        let r = RegQueryValueExW(hk, name.as_ptr(), std::ptr::null_mut(), &mut kind, std::ptr::null_mut(), &mut len);
        RegCloseKey(hk);
        r == 0 && len > 0
    }
}

fn set_autostart(on: bool) {
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let name = wide("图墙");
    let mut hk: isize = 0;
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_SET_VALUE | KEY_QUERY_VALUE, &mut hk) != 0 {
            return;
        }
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let v = wide(&format!("\"{}\"", exe.to_string_lossy()));
                let bytes: Vec<u8> = v.iter().flat_map(|c| c.to_le_bytes()).collect();
                RegSetValueExW(hk, name.as_ptr(), 0, REG_SZ, bytes.as_ptr(), bytes.len() as u32);
            }
        } else {
            RegDeleteValueW(hk, name.as_ptr());
        }
        RegCloseKey(hk);
    }
}

// ---------------- 托盘 / 窗口 ----------------

fn tray_menu() -> isize {
    let a = app();
    unsafe {
        let m = CreatePopupMenu();
        let t = wide("显示 / 收起晾衣绳\tCtrl+Alt+T");
        AppendMenuW(m, MF_STRING, ID_TOGGLE as usize, t.as_ptr());
        AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
        let f1 = if a.cfg.take_over { MF_STRING | MF_CHECKED } else { MF_STRING };
        let t = wide("接管截图文件夹(新截图移入收件箱)");
        AppendMenuW(m, f1, ID_TAKEOVER as usize, t.as_ptr());
        let f2 = if a.cfg.clipboard_watch { MF_STRING | MF_CHECKED } else { MF_STRING };
        let t = wide("监听剪贴板截图(Win+Shift+S 未落盘时)");
        AppendMenuW(m, f2, ID_CLIPWATCH as usize, t.as_ptr());
        AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
        let t = wide("打开收件箱");
        AppendMenuW(m, MF_STRING, ID_INBOX as usize, t.as_ptr());
        let f3 = if autostart_enabled() { MF_STRING | MF_CHECKED } else { MF_STRING };
        let t = wide("开机自启");
        AppendMenuW(m, f3, ID_AUTOSTART as usize, t.as_ptr());
        AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
        let t = wide("关于 图墙");
        AppendMenuW(m, MF_STRING, ID_ABOUT as usize, t.as_ptr());
        let t = wide("退出");
        AppendMenuW(m, MF_STRING, ID_QUIT as usize, t.as_ptr());
        m
    }
}

fn add_tray(hwnd: isize) {
    unsafe {
        let mut n: NotifyIconDataW = std::mem::zeroed();
        n.cb_size = std::mem::size_of::<NotifyIconDataW>() as u32;
        n.h_wnd = hwnd;
        n.u_id = 1;
        n.u_flags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        n.u_callback_message = WM_APP_TRAY;
        n.h_icon = load_app_icon();
        let tip = wide("图墙 —— 截图,晾起来");
        for (i, c) in tip.iter().take(127).enumerate() {
            n.sz_tip[i] = *c;
        }
        Shell_NotifyIconW(NIM_ADD, &mut n);
    }
}

/// 与 installer/picline.rc 中的 `#define IDI_APP 101` 保持一致。
/// `MAKEINTRESOURCE(101)` 在 Win32 里就是「把整数当字符串指针传」。
const IDI_APP: u16 = 101;

fn load_app_icon() -> isize {
    // 1) exe 内嵌资源（build.rs 用 rc.exe 编进去的）。
    //    portable 单文件不依赖任何旁边的 assets/ 目录，这是唯一来源。
    unsafe {
        let inst = GetModuleHandleW(std::ptr::null());
        let h = LoadImageW(
            inst,
            IDI_APP as usize as *const u16,
            IMAGE_ICON,
            16,
            16,
            0, // LR_DEFAULTCOLOR
        );
        if h != 0 {
            crate::trace::ev("icon_source", "resource");
            return h;
        }
    }
    // 2) 开发期回退：还没编资源 / 从源码树直接跑时，读旁边或上级的 ico
    let candidates = [
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("assets/icon.ico"))),
        std::env::current_exe().ok().and_then(|p| p.parent().and_then(|d| d.parent()).and_then(|d| d.parent()).map(|d| d.join("assets/icon.ico"))),
    ];
    for c in candidates.into_iter().flatten() {
        if c.exists() {
            let p = wide(&c.to_string_lossy());
            let h = unsafe { LoadImageW(0, p.as_ptr(), IMAGE_ICON, 16, 16, 0x0010) }; // LR_LOADFROMFILE
            if h != 0 {
                crate::trace::ev("icon_source", "file");
                return h;
            }
        }
    }
    crate::trace::ev("icon_source", "fallback");
    unsafe { LoadIconW(0, IDI_APPLICATION as usize as *const u16) }
}

fn monitor_enum(hmon: isize, _dc: isize, _rect: *mut Rect, data: isize) -> i32 {
    unsafe {
        let v = &mut *(data as *mut Vec<(i32, i32, i32, i32)>);
        let mut info: MonitorInfoExW = std::mem::zeroed();
        info.cb_size = std::mem::size_of::<MonitorInfoExW>() as u32;
        if GetMonitorInfoW(hmon, &mut info) != 0 {
            let r = info.rc_work;
            v.push((r.l, r.t, r.r - r.l, r.b - r.t));
        }
        1
    }
}

fn create_surface(_hwnd: isize, w: i32, h: i32) -> Surface {
    unsafe {
        let screen = GetDC(0);
        let hdc = CreateCompatibleDC(screen);
        ReleaseDC(0, screen);
        let bmi = BitmapInfo {
            header: BitmapInfoHeader {
                bi_size: 40,
                bi_width: w,
                bi_height: -h,
                bi_planes: 1,
                bi_bit_count: 32,
                bi_compression: 0,
                bi_size_image: 0,
                bi_x_pels_per_meter: 0,
                bi_y_pels_per_meter: 0,
                bi_clr_used: 0,
                bi_clr_important: 0,
            },
            colors: [0; 4],
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(hdc, &bmi, 0, &mut bits, 0, 0);
        let old = SelectObject(hdc, dib);
        Surface { hdc, dib, old, bits: bits as *mut u8, w, h }
    }
}

fn destroy_surface(s: &Surface) {
    unsafe {
        SelectObject(s.hdc, s.old);
        DeleteObject(s.dib);
        DeleteDC(s.hdc);
    }
}

fn rebuild_displays(class: *const u16, instance: isize) {
    let a = app();
    for d in &a.displays {
        destroy_surface(&d.surface);
        unsafe { DestroyWindow(d.hwnd) };
    }
    a.displays.clear();
    let mut rects: Vec<(i32, i32, i32, i32)> = Vec::new();
    unsafe {
        EnumDisplayMonitors(0, std::ptr::null_mut(), monitor_enum as *const c_void, &mut rects as *mut _ as isize);
    }
    if rects.is_empty() {
        rects.push(unsafe { (0, 0, GetSystemMetrics(0), GetSystemMetrics(1)) });
    }
    let dpi = unsafe { GetDpiForSystem() };
    let sf = if dpi > 96 { dpi as f32 / 96.0 } else { 1.0 };
    let title = wide("图墙");
    for (x, y, w, _h) in rects {
        let ph = (PANEL_H * sf) as i32;
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                class,
                title.as_ptr(),
                WS_POPUP,
                x, y, w, ph,
                0, 0, instance, std::ptr::null_mut(),
            )
        };
        if hwnd == 0 {
            continue;
        }
        unsafe { SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
        let surface = create_surface(hwnd, w, ph);
        a.displays.push(LineDisplay { hwnd, x, y, w, h: ph, sf, surface });
    }
    let lw = a.displays.first().map(|d| d.w as f32 / d.sf).unwrap_or(1920.0);
    a.layout(lw);
}

// ---------------- 捕获管线 tick ----------------

fn tick_capture(a: &mut App, t: i64) {
    // 文件夹：250ms，连续两次大小稳定
    let rd = match fs::read_dir(&a.screenshots) {
        Ok(r) => r,
        Err(_) => {
            let _ = fs::create_dir_all(&a.screenshots);
            return;
        }
    };
    let mut ready = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !crate::inbox::is_image_name(&name) {
            continue;
        }
        let path = e.path();
        let md = match e.metadata() {
            Ok(m) if m.is_file() => m,
            _ => continue,
        };
        let mt = md.modified()
            .ok()
            .and_then(|x| x.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        if !a.core.is_fresh(mt) {
            continue;
        }
        let size = md.len();
        if size == 0 {
            continue;
        }
        match a.pending.get_mut(&path) {
            Some((prev, n)) if *prev == size => {
                *n += 1;
                if *n >= 2 {
                    ready.push(path.clone());
                }
            }
            _ => {
                a.pending.insert(path, (size, 1));
            }
        }
    }
    for src in ready {
        a.pending.remove(&src);
        let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase())).unwrap_or_else(|| ".png".into());
        let img = gdi::load_image(&src);
        if img.is_null() {
            continue;
        }
        let mut w = 0u32;
        let mut h = 0u32;
        unsafe {
            gdi::GdipGetImageWidth(img, &mut w);
            gdi::GdipGetImageHeight(img, &mut h);
            gdi::GdipDisposeImage(img);
        }
        if !a.core.take_over {
            continue;
        }
        let dest = crate::capture::unique_name(&a.inbox_dir, "shot", &ext, t, &win32::random_hex3());
        if fs::rename(&src, &dest).is_ok() || (fs::copy(&src, &dest).is_ok() && fs::remove_file(&src).is_ok()) {
            if let Some(clip) = a.core.on_file_capture(&visual::dims(w, h), t) {
                let _ = fs::remove_file(clip);
            }
            trace::ev("capture_file", &dest.to_string_lossy());
            a.rescan();
            peek_on_new(a);
        }
    }
}

/// 新截图到达：线收着且没有全屏应用时，探头展示 2.5s（原版 peekFor）
fn peek_on_new(a: &mut App) {
    if !a.shown && !a.fg_fullscreen && a.cfg.show_line {
        trace::ev("peek", "new-item");
        a.reveal(false, true);
    }
}

fn tick_clipboard(a: &mut App, t: i64) {
    let Some(snap) = win32::clipboard_snapshot() else { return };
    let key = visual::visual_key(&snap.quantized, snap.width, snap.height);
    let dims = visual::dims(snap.width, snap.height);
    if a.core.poll_clipboard(&key, &dims, t) == ClipDecision::Accept {
        let bytes = png::encode_rows(snap.width, snap.height, &snap.png_rows);
        if bytes.is_empty() {
            return;
        }
        let dest = crate::capture::unique_name(&a.inbox_dir, "clip", ".png", t, &win32::random_hex3());
        if fs::write(&dest, bytes).is_ok() {
            trace::ev("capture_clip", &dest.to_string_lossy());
            a.core.on_clip_capture(&dims, dest, t);
            a.rescan();
            peek_on_new(a);
        }
    }
}

// ---------------- tick / 动画 ----------------

/// 持续钉在最顶层，等价 Electron `setAlwaysOnTop(true,'screen-saver')`。
/// 外部窗口（看图器、浏览器）被激活关闭后，Windows 会把前台窗口提到
/// topmost 组之前；只在「接管瞬间」置顶不够，因为那时 capturing 可能早已是
/// true，set_capture 是 no-op。150ms 节流 + NOACTIVATE：不抢焦点、不改尺寸。
static LAST_TOP: AtomicI64 = AtomicI64::new(0);
fn keep_on_top(a: &App) {
    let t = now();
    if t - LAST_TOP.load(Ordering::Relaxed) < 150 {
        return;
    }
    LAST_TOP.store(t, Ordering::Relaxed);
    for d in &a.displays {
        unsafe {
            SetWindowPos(d.hwnd, HWND_TOPMOST, 0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

fn tick() {
    // DoDragDrop 期间由 Windows 泵送消息，跳过以免重入窗口状态
    if crate::drag::DRAGGING.load(Ordering::Relaxed) {
        return;
    }
    let t = now();
    let a = app();
    keep_on_top(a);
    if t - a.last_tick[0] >= 60 {
        a.last_tick[0] = t;
        poll_cursor(t);
    }
    if t - a.last_tick[1] >= 250 {
        a.last_tick[1] = t;
        tick_capture(a, t);
        a.tracker.tick();
    }
    if t - a.last_tick[2] >= 350 {
        a.last_tick[2] = t;
        let d = a.displays.first().map(|d| FgDisplay {
            x: d.x, y: d.y, w: (d.w as f32 / d.sf) as i32, h: (d.h as f32 / d.sf) as i32,
            scale_factor: d.sf as f64,
        });
        let fs = d.map(|d| fg_is_fullscreen(win32::foreground_info().as_ref(), &d)).unwrap_or(false);
        if fs != a.fg_fullscreen {
            trace::ev("fullscreen", if fs { "true" } else { "false" });
        }
        a.fg_fullscreen = fs;
    }
    if t - a.last_tick[3] >= 800 {
        a.last_tick[3] = t;
        tick_clipboard(a, t);
    }
    if t - a.last_tick[4] >= 10_000 {
        a.last_tick[4] = t;
        a.core.prune(t);
        a.last_mem = a.tracker.report();
    }
    if t - a.last_tick[5] >= 3500 {
        a.last_tick[5] = t;
        let _ = Settings::save(&a.data_dir, &a.cfg);
    }

    // peek 到期 / 延迟收起
    if a.peek_until > 0 && t > a.peek_until {
        a.peek_until = 0;
        let (px, py) = unsafe {
            let mut p = Point { x: 0, y: 0 };
            GetCursorPos(&mut p);
            (p.x, p.y)
        };
        let inside = a.displays.iter().any(|d| px >= d.x && px <= d.x + d.w && py >= d.y && py <= d.y + d.h);
        if !inside {
            a.hide();
        }
    }
    if a.hide_at > 0 && t > a.hide_at && !a.pinned {
        a.hide();
    }
    // 首次运行引导：800ms 后 peek 展示
    if a.first_run_show_at > 0 && t >= a.first_run_show_at {
        a.first_run_show_at = 0;
        if a.force_show {
            a.reveal(true, false); // pinned，不自动收起
        } else {
            a.reveal(false, true);
        }
    }

    // 长按
    if a.gesture.down && !a.gesture.held && !a.gesture.dragging && t - a.gesture.down_at >= HOLD_MS {
        a.gesture.held = true;
        let id = a.gesture.id.clone();
        trace::ev("hold", &id);
        if let Some(p) = a.item_path(&id) {
            shell_edit(&p);
        }
    }

    // 滑入/收起动画
    let mut need_render = false;
    if a.animating {
        let p = ((t - a.anim_start) as f32 / a.anim_ms as f32).clamp(0.0, 1.0);
        let target = a.anim_to;
        let e = if a.anim_to == 0.0 {
            // easeOutBack（近似 cubic-bezier(.22,1.28,.36,1) 的过冲）
            let c1 = 1.70158f32;
            let c3 = c1 + 1.0;
            let u = p - 1.0;
            1.0 + c3 * u * u * u + c1 * u * u
        } else {
            p * p * p // easeInCubic
        };
        a.offset = a.anim_from + (target - a.anim_from) * e;
        if p >= 1.0 {
            a.offset = target;
            a.animating = false;
            if !a.shown {
                a.set_capture(false);
            }
        }
        need_render = true;
    }

    // 弹簧摆动（只在展开时）
    if a.shown {
        let mut gusts = std::mem::take(&mut a.gusts);
        for (at, idx, dv) in gusts.drain(..) {
            if at <= t {
                if let Some(p) = a.pegs.get_mut(idx) {
                    p.v += dv;
                }
                need_render = true;
            } else {
                a.gusts.push((at, idx, dv));
            }
        }
        if !gusts.is_empty() {
            // drain 已清空，剩余项已放回 a.gusts；避免重复插入
        }
        if t >= a.next_gust {
            let strength = rand_range(1.6, 3.4);
            for i in 0..a.pegs.len() {
                let sign = if rand01() < 0.5 { -1.0 } else { 1.0 };
                a.gusts.push((t + (rand01() * 350.0) as i64, i, strength * 30.0_f32.sqrt() * sign));
            }
            a.next_gust = t + rand_range(3800.0, 8500.0) as i64;
        }
        let dt = 0.016f32;
        for p in &mut a.pegs {
            if p.a.abs() < 0.004 && p.v.abs() < 0.02 {
                p.a = 0.0;
                p.v = 0.0;
                continue;
            }
            p.v += (-30.0 * p.a - 2.6 * p.v) * dt;
            p.a += p.v * dt;
            need_render = true;
        }
        if !a.gusts.is_empty() {
            need_render = true;
        }
    }
    // hover 变化也触发重绘
    for p in &mut a.pegs {
        if p.hover {
            need_render = true;
        }
    }
    if now() < a.toast_until || a.menu_open {
        need_render = true;
    }
    // 接管期间持续重绘：保证 alpha 底噪与 capturing 状态同步，
    // 否则鼠标刚移进相片那一帧用的还是上一帧的 alpha=0 表面。
    if need_render || a.capturing || a.gesture.down {
        render_all();
    }
    write_state(a);
    update_timer_rate();
}

fn poll_cursor(_t: i64) {
    let (px, py) = unsafe {
        let mut p = Point { x: 0, y: 0 };
        GetCursorPos(&mut p);
        (p.x, p.y)
    };
    let a = app();
    let sf = a.displays.first().map(|d| d.sf).unwrap_or(1.0);
    let nearest = a.displays.iter().find(|d| px >= d.x && px <= d.x + d.w && py >= d.y && py <= d.y + d.h).map(|d| (d.x, d.y, d.w, d.h));
    let (dx, dy, dw, _dh) = nearest.unwrap_or((0, 0, 0, 0));
    let in_band = nearest.is_some()
        && (py - dy) as f32 <= BAND_H * sf
        && (px - dx) as f32 >= dw as f32 * 0.22
        && (px - dx) as f32 <= dw as f32 * 0.78;
    let in_strip = a.displays.iter().any(|d| {
        px >= d.x - 4 && px <= d.x + d.w + 4 && py >= d.y - 4 && py <= d.y + d.h
    });
    if a.shown {
        if in_strip {
            a.hide_at = 0;
        } else if !a.pinned && a.hide_at == 0 {
            a.hide_at = now() + HIDE_DELAY_MS;
        }
        a.since_in_band = 0;
    } else if in_band && !a.fg_fullscreen {
        if a.since_in_band == 0 {
            a.since_in_band = now();
        } else if now() - a.since_in_band >= REVEAL_DELAY_MS {
            trace::ev("band_reveal", "");
            a.reveal(false, false);
        }
    } else {
        a.since_in_band = 0;
    }
    let hit = a.cursor_hits(px, py).is_some();
    if now() < a.dragging_until || a.gesture.down {
        // 按住相片到抬起之间必须保持接管：拖出时鼠标会离开相片矩形，
        // 若此刻放开穿透，WM_MOUSEMOVE 就断流，7px 阈值永远触发不了。
        a.set_capture(true);
    } else if a.animating && !a.shown {
        // 等价 main.js hideLine() 的 setTimeout(260) hardIgnore：
        // 收起动画播完前保持接管，不能让 60ms 轮询提前把 WS_EX_TRANSPARENT 放回去。
        a.set_capture(true);
    } else {
        a.set_capture(hit);
    }
    // hover 状态
    let hover_id = a.hit_peg_id(px, py);
    for p in &mut a.pegs {
        p.hover = hover_id.as_deref() == Some(p.id.as_str());
    }
}

fn update_timer_rate() {
    let a = app();
    let fast = a.animating || a.shown || a.menu_open || now() < a.toast_until;
    let want = if fast { 16u32 } else { 60u32 };
    if want != a.timer_ms {
        a.timer_ms = want;
        unsafe { SetTimer(a.controller, 1, want, std::ptr::null_mut()) };
    }
}

// ---------------- 窗口过程 ----------------

#[allow(static_mut_refs)]
extern "system" fn wnd_proc(hwnd: isize, msg: u32, wp: usize, lp: isize) -> isize {
    // CreateWindowExW 会同步派发 WM_NCCREATE/WM_CREATE，此时 APP 尚未安装；
    // 这些消息必须直接交给默认过程，否则启动即崩。
    if unsafe { APP.is_none() } {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }
    let a = app();
    match msg {
        WM_SETCURSOR => {
            // 只接管客户区；非客户区交回系统默认处理。显式 SetCursor 是为了
            // 把启动时残留的忙碌蓝圈清掉，并复现原版 default / pointer 两种形状。
            if (lp as u32 & 0xFFFF) == HTCLIENT {
                let mut p = Point { x: 0, y: 0 };
                unsafe { GetCursorPos(&mut p) };
                let menu_open = a.menu_open;
                let hand = menu_open || hit_xbtn(p.x, p.y).is_some();
                unsafe { SetCursor(sys_cursor(if hand { IDC_HAND } else { IDC_ARROW })) };
                return 1;
            }
            return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
        }
        WM_TIMER => {
            tick();
            return 0;
        }
        WM_HOTKEY => {
            trace::ev("hotkey", "Ctrl+Alt+T");
            if a.shown && a.pinned {
                a.hide();
            } else {
                a.reveal(true, false);
            }
            return 0;
        }
        WM_DISPLAYCHANGE => {
            let cls = wide("PicLineWindow");
            let inst = unsafe { GetModuleHandleW(std::ptr::null()) };
            rebuild_displays(cls.as_ptr(), inst);
            return 0;
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            return 0;
        }
        WM_APP_TRAY => {
            let code = (lp as u32) & 0xFFFF;
            if code == WM_LBUTTONUP as u32 || code == WM_RBUTTONUP as u32 {
                trace::ev("tray_open", if code == WM_LBUTTONUP as u32 { "left" } else { "right" });
                let mut p = Point { x: 0, y: 0 };
                unsafe { GetCursorPos(&mut p) };
                let m = tray_menu();
                unsafe {
                    SetForegroundWindow(hwnd);
                    TrackPopupMenuEx(m, TPM_RIGHTBUTTON, p.x, p.y, hwnd, std::ptr::null_mut());
                    DestroyMenu(m);
                }
            }
            return 0;
        }
        WM_COMMAND => {
            let id = (wp & 0xFFFF) as u16;
            handle_command(id);
            return 0;
        }
        WM_APP_QUIT => {
            trace::ev("quit", "WM_APP_QUIT");
            unsafe { PostQuitMessage(0) };
            return 0;
        }
        WM_LBUTTONDOWN => {
            let (x, y) = (loword(lp), hiword(lp));
            let (px, py) = global_from(hwnd, x, y);
            // 菜单开启时先走菜单分支（等价原版 mousedown 捕获阶段的 onMenuOutside），
            // 否则点菜单项落到 ✕ 区域会顺手把相片删掉。
            if let Some(i) = if a.menu_open { None } else { hit_xbtn(px, py) } {
                trace::ev("down", &format!("xbtn:{}", i));
                let id = a.pegs.get(i).map(|p| p.id.clone());
                if let Some(id) = id {
                    if let Some(p) = a.pegs.get_mut(i) {
                        p.falling = true;
                    }
                    discard(a, &id);
                }
                return 0;
            }
            if a.menu_open {
                let (lx, ly) = local_from(hwnd, x, y);
                if let Some(mi) = menu_index(a.menu_x, a.menu_y, lx, ly - a.offset) {
                    let id = a.menu_id.clone();
                    a.menu_open = false;
                    run_menu_action(a, mi, &id);
                } else {
                    a.menu_open = false;
                    trace::ev("menu_dismiss", "outside");
                }
                render_all();
                return 0;
            }
            if let Some(id) = a.hit_peg_id(px, py) {
                trace::ev("down", &format!("peg:{}", id));
                a.gesture.down = true;
                a.gesture.down_x = px;
                a.gesture.down_y = py;
                a.gesture.down_at = now();
                a.gesture.held = false;
                a.gesture.dragging = false;
                a.gesture.id = id;
            } else {
                trace::ev("down", "empty");
            }
            return 0;
        }
        WM_MOUSEMOVE => {
            let (x, y) = (loword(lp), hiword(lp));
            let (px, py) = global_from(hwnd, x, y);
            if a.gesture.down && !a.gesture.dragging {
                let d = ((px - a.gesture.down_x).pow(2) + (py - a.gesture.down_y).pow(2)) as f32;
                if d.sqrt() >= DRAG_PX {
                    trace::ev("drag_trigger", &format!("{:.1}px", d.sqrt()));
                    a.gesture.dragging = true;
                    a.gesture.held = false;
                    a.dragging_until = now() + 3500;
                    let id = a.gesture.id.clone();
                    start_drag(&id);
                }
            }
            return 0;
        }
        WM_LBUTTONUP => {
            if !a.gesture.down {
                return 0;
            }
            let g_held = a.gesture.held;
            let g_drag = a.gesture.dragging;
            let id = a.gesture.id.clone();
            let last_at = a.gesture.last_click_at;
            let last_id = a.gesture.last_click_id.clone();
            a.gesture.down = false;
            a.dragging_until = 0;
            if g_held || g_drag {
                trace::ev("click", if g_held { "held" } else { "drag" });
                return 0;
            }
            if now() - last_at < DOUBLE_MS && last_id == id {
                a.gesture.last_click_at = 0;
                trace::ev("click", "double");
                if let Some(p) = a.item_path(&id) {
                    shell_open(&p);
                }
            } else {
                a.gesture.last_click_at = now();
                a.gesture.last_click_id = id.clone();
                trace::ev("click", "single");
                do_copy(a, &id);
            }
            return 0;
        }
        WM_RBUTTONUP => {
            let (x, y) = (loword(lp), hiword(lp));
            let (px, py) = global_from(hwnd, x, y);
            if let Some(id) = a.hit_peg_id(px, py) {
                let (lx, ly) = local_from(hwnd, x, y);
                trace::ev("menu_open", &id);
                // 等价 line.js openMenu(): Math.min(x, innerWidth - mw - 6) / (mh 同理)
                // 不钳制时，在相片偏下位置右键会让菜单下半截落到 210px 窗口之外。
                let (lw, lh) = a.displays.iter().find(|d| d.hwnd == hwnd)
                    .map(|d| (d.w as f32 / d.sf, d.h as f32 / d.sf))
                    .unwrap_or((1920.0, PANEL_H));
                a.menu_id = id;
                a.menu_x = lx.min(lw - MENU_W - 6.0).max(0.0);
                a.menu_y = (ly - a.offset).min(lh - menu_height() - 6.0).max(0.0);
                a.menu_open = true;
                a.set_capture(true);
                render_all();
            }
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn global_from(hwnd: isize, x: i32, y: i32) -> (i32, i32) {
    if let Some(d) = app().displays.iter().find(|d| d.hwnd == hwnd) {
        (d.x + x, d.y + y)
    } else {
        (x, y)
    }
}

fn local_from(hwnd: isize, x: i32, y: i32) -> (f32, f32) {
    let sf = app().displays.iter().find(|d| d.hwnd == hwnd).map(|d| d.sf).unwrap_or(1.0);
    (x as f32 / sf, y as f32 / sf)
}

fn hit_xbtn(px: i32, py: i32) -> Option<usize> {
    let a = app();
    let d = a.displays.iter().find(|d| px >= d.x && px <= d.x + d.w && py >= d.y && py <= d.y + d.h)?;
    let lx = (px - d.x) as f32 / d.sf;
    let ly = (py - d.y) as f32 / d.sf - a.offset;
    a.pegs.iter().position(|p| {
        lx >= p.x + 66.0 && lx <= p.x + 84.0 && ly >= p.ropey + 3.0 && ly <= p.ropey + 21.0
    })
}

fn run_menu_action(a: &mut App, mi: usize, id: &str) {
    trace::ev("menu_action", &format!("{}:{}", mi, id));
    match mi {
        0 => {
            do_copy(a, id);
        }
        1 => {
            if let Some(p) = a.item_path(id) {
                shell_edit(&p);
            }
        }
        2 => {
            if let Some(p) = a.item_path(id) {
                shell_open(&p);
            }
        }
        3 => save_as(a, id),
        5 => take_down(a, id),
        6 => discard(a, id),
        _ => {}
    }
}

fn handle_command(id: u16) {
    trace::ev("command", &id.to_string());
    let a = app();
    match id {
        ID_TOGGLE => {
            if a.shown && a.pinned {
                a.hide();
            } else {
                a.reveal(true, false);
            }
        }
        ID_TAKEOVER => {
            a.cfg.take_over = !a.cfg.take_over;
            a.core.set_options(Some(a.cfg.take_over), None);
            let _ = Settings::save(&a.data_dir, &a.cfg);
        }
        ID_CLIPWATCH => {
            a.cfg.clipboard_watch = !a.cfg.clipboard_watch;
            a.core.set_options(None, Some(a.cfg.clipboard_watch));
            let _ = Settings::save(&a.data_dir, &a.cfg);
        }
        ID_INBOX => {
            let p = a.inbox_dir.clone();
            shell_open(&p);
        }
        ID_AUTOSTART => set_autostart(!autostart_enabled()),
        ID_ABOUT => {
            let text = wide("图墙 v0.1.1\n\n截图,晾起来。\n\n灵感来自 Alejandro Buján 的 Tendedero(macOS)。\n本作品为其 Windows 移植,代码以 MIT 许可发布。");
            let cap = wide("关于 图墙");
            unsafe { MessageBoxW(0, text.as_ptr(), cap.as_ptr(), MB_ICONINFORMATION | MB_OK) };
        }
        ID_QUIT => unsafe { PostQuitMessage(0) },
        _ => {}
    }
}

/// 拖出：OLE CF_HDROP，交给系统决定落到应用（发副本）还是文件夹（存一份）。
/// 相片文件本身不动，仍留在绳上；3.5s 抑制收起由 dragging_until 保证。
fn start_drag(id: &str) {
    let path = app().item_path(id);
    if let Some(p) = path {
        trace::ev("drag_start", &p.to_string_lossy());
        let ok = crate::drag::drag_file(&p);
        trace::ev("drag_end", if ok { "ok" } else { "fail" });
        // DoDragDrop 会在 OLE 模态循环里消费掉 LBUTTONUP，窗口过程收不到抬起，
        // 手势状态会一直停在 down=true / dragging=true。这里显式复位，
        // dragging_until 仍保留 3.5s 抑制收起（等价原版 dragOut）。
        let a = app();
        a.gesture.down = false;
        a.gesture.held = false;
        a.gesture.dragging = false;
        a.gesture.last_click_at = 0;
    } else {
        trace::ev("drag_start", "fail:no-item");
    }
}

// ---------------- 入口 ----------------

pub struct GuiOptions {
    pub data_dir: PathBuf,
    pub inbox_dir: PathBuf,
    pub cfg: Settings,
    pub pictures: PathBuf,
    pub shots_override: Option<PathBuf>,
    pub probe_out: Option<PathBuf>,
}

pub fn run(opts: GuiOptions) {
    unsafe {
        SetProcessDpiAwarenessContext(-4); // PER_MONITOR_AWARE_V2
    }
    let t0 = now();
    let mut core = CaptureCore::start(t0);
    let cfg = opts.cfg.clone();
    core.set_options(Some(cfg.take_over), Some(cfg.clipboard_watch));
    if let Some(snap) = win32::clipboard_snapshot() {
        core.set_baseline_key(Some(visual::visual_key(&snap.quantized, snap.width, snap.height)));
    }
    let screenshots = opts.shots_override.unwrap_or_else(|| opts.pictures.join("Screenshots"));
    let _ = fs::create_dir_all(&screenshots);
    let first_run = !cfg.first_run_done;
    let force_show = std::env::var("TQ_FORCE_SHOW").is_ok();
    let mut cfg2 = cfg.clone();
    if first_run {
        cfg2.first_run_done = true;
        let _ = Settings::save(&opts.data_dir, &cfg2);
    }

    let class_name = wide("PicLineWindow");
    let title = wide("图墙");
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    unsafe {
        let cls = WndClassExW {
            cb_size: std::mem::size_of::<WndClassExW>() as u32,
            style: 0,
            lpfn_wnd_proc: wnd_proc as *const c_void,
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: instance,
            h_icon: 0,
            h_cursor: sys_cursor(IDC_ARROW),
            h_br_background: 0,
            lpsz_menu_name: std::ptr::null(),
            lpsz_class_name: class_name.as_ptr(),
            h_icon_sm: 0,
        };
        RegisterClassExW(&cls);
    }
    let controller = unsafe {
        CreateWindowExW(
            0, class_name.as_ptr(), title.as_ptr(), 0,
            0, 0, 0, 0, -3isize, 0, instance, std::ptr::null_mut(),
        )
    };

    unsafe {
        APP = Some(App {
            cfg: cfg2,
            data_dir: opts.data_dir,
            inbox_dir: opts.inbox_dir,
            screenshots,
            pictures: opts.pictures,
            displays: Vec::new(),
            controller,
            tray_added: false,
            items: Vec::new(),
            pegs: Vec::new(),
            core,
            tracker: MemTracker::default(),
            shown: false,
            pinned: false,
            offset: HIDE_OFFSET,
            anim_from: HIDE_OFFSET,
            anim_to: HIDE_OFFSET,
            anim_start: t0,
            anim_ms: 1,
            animating: false,
            since_in_band: 0,
            hide_at: 0,
            peek_until: 0,
            capturing: false,
            dragging_until: 0,
            gesture: Gesture::default(),
            menu_open: false,
            menu_id: String::new(),
            menu_x: 0.0,
            menu_y: 0.0,
            toast: String::new(),
            toast_until: 0,
            fg_fullscreen: false,
            pending: HashMap::new(),
            last_tick: [t0; 6],
            next_gust: t0 + 1500,
            gusts: Vec::new(),
            rng: 8_812_726_314_123_252,
            timer_ms: 60,
            last_items_sig: String::new(),
            probe_out: opts.probe_out,
            probe_start: std::time::Instant::now(),
            first_run_show_at: if first_run || force_show { t0 + 800 } else { 0 },
            force_show,
            last_mem: String::new(),
        });
    }

    rebuild_displays(class_name.as_ptr(), instance);
    app().rescan();
    add_tray(controller);
    app().tray_added = true;
    unsafe {
        RegisterHotKey(controller, 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, VK_T);
        SetTimer(controller, 1, 60, std::ptr::null_mut());
    }
    render_all();

    loop {
        let mut msg: Msg = unsafe { std::mem::zeroed() };
        let r = unsafe { GetMessageW(&mut msg, 0, 0, 0) };
        if r <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if let Some(out) = app().probe_out.clone() {
            if app().probe_start.elapsed() >= std::time::Duration::from_secs(12) {
                let s = crate::memprobe::sample();
                let _ = fs::write(&out, s.to_probe_json());
                app().probe_out = None;
                unsafe { PostQuitMessage(0) };
            }
        }
    }

    unsafe {
        UnregisterHotKey(controller, 1);
        let mut n: NotifyIconDataW = std::mem::zeroed();
        n.cb_size = std::mem::size_of::<NotifyIconDataW>() as u32;
        n.h_wnd = controller;
        n.u_id = 1;
        Shell_NotifyIconW(NIM_DELETE, &mut n);
    }
    for d in &app().displays {
        destroy_surface(&d.surface);
    }
    let _ = &app().last_mem;
}

#[cfg(test)]
mod tests {
    #[test]
    fn menu_hit_mapping() {
        // 分隔项是第 4 项，不响应点击
        assert_eq!(super::menu_index(10.0, 10.0, 20.0, 10.0 + 5.0 + 3.0), Some(0));
        assert_eq!(super::menu_index(10.0, 10.0, 20.0, 10.0 + 5.0 + 26.0 * 3.0 + 2.0), Some(3));
        assert_eq!(super::menu_index(10.0, 10.0, 5.0, 20.0), None);
        assert_eq!(super::menu_index(10.0, 10.0, 20.0, 10.0 + 5.0 + 26.0 * 4.0 + 4.0), None);
    }

    #[test]
    fn menu_geometry_matches_css() {
        assert_eq!(super::menu_height(), 10.0 + 9.0 + 156.0);
    }

    #[test]
    fn ease_out_back_overshoots() {
        let p = 0.7f32;
        let c1 = 1.70158f32;
        let c3 = c1 + 1.0;
        let u = p - 1.0;
        let e = 1.0 + c3 * u * u * u + c1 * u * u;
        assert!(e > 1.0, "展开曲线应有过冲, 实得 {}", e);
    }
}
