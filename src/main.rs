//! 图墙 PicLine —— Rust 等价内核（Stage 1）
//!
//! 当前交付范围是"可严格等价的核心逻辑 + 无 Electron/PowerShell 的系统交互层"：
//! 配置、收件箱、截图捕获去重状态机、全屏判定、内存取样、TQ_* 环境变量兼容。
//! **不包含 GUI 渲染层**（透明窗口 / 晾衣绳绘制 / 手势 / 托盘 / 拖放），
//! 原因与待确认项见 ACCEPTANCE.md。请勿把它当作可直接替换的完整应用发布。
//!
//! 用法：
//!   picline-core --once                 只读扫描一次，输出配置/收件箱/全屏/内存
//!   picline-core --watch                常驻守护（默认），跑完整捕获管线
//!   picline-core --probe <out.json>     12 秒后写出 TQ_MEMPROBE 兼容 JSON 并退出
//!   picline-core --help

// 发布版以 GUI 子系统链接：双击启动不再弹出黑色控制台窗口（用户可见副作用）。
// Debug 构建保留 Console，方便 --once / --watch / --probe 直接看诊断输出。
// 注意：命令行模式从已有终端或管道启动时 stdout 仍可用，不影响验收脚本。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Stage 1 有意保留若干 Stage 2（GUI 层）才会接线的 API 与常量；
// 它们由单元测试覆盖，但在当前二进制里尚未被调用，故允许 dead_code。
#![allow(dead_code)]

mod api;
mod capture;
mod drag;
mod fullscreen;
mod gdi;
mod gui;
mod inbox;
mod memprobe;
mod png;
mod settings;
mod trace;
mod visual;
mod win32;

use capture::{CaptureCore, ClipDecision};
use fullscreen::{is_fullscreen, Display};
use inbox::{is_image_name, items_to_json, list_inbox};
use memprobe::MemTracker;
use settings::Settings;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PANEL_H: i32 = 210;
const REVEAL_DELAY_MS: i64 = 250;
const BAND_H: i32 = 4;
const PEEK_MS: i64 = 2_500;
const HIDE_DELAY_MS: i64 = 300;
const CURSOR_POLL_MS: i64 = 60;

#[derive(Debug)]
enum Mode {
    /// 完整 GUI（默认），Option 为 12 秒内存探针输出路径
    Gui(Option<PathBuf>),
    Once,
    /// 无窗口守护内核，仅用于逻辑层验收
    Watch,
    /// 无窗口内核的 12 秒探针（Stage 1 对照用）
    ProbeCore(PathBuf),
    Help,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mode = parse_mode(&args);
    if matches!(mode, Mode::Help) {
        print_help();
        return;
    }

    // 等价 main.js：先抢单实例锁，再初始化数据目录
    if !win32::single_instance_lock() {
        return; // 已在运行 -> app.quit()
    }
    let (data_dir, inbox_dir) = init_data_dirs();
    let cfg = Settings::load(&data_dir);

    // GDI+ 全局一次；win32::decode_file_visual 与 gui 共用
    gdi::startup();
    match mode {
        Mode::Gui(probe) => gui::run(gui::GuiOptions {
            data_dir: data_dir.clone(),
            inbox_dir: inbox_dir.clone(),
            cfg: cfg.clone(),
            pictures: resolve_pictures_dir(),
            shots_override: env::var("TQ_SHOTS_DIR").ok().map(PathBuf::from),
            probe_out: probe,
        }),
        Mode::Once => run_once(&data_dir, &inbox_dir, &cfg),
        Mode::Watch => run_watch(&inbox_dir, &cfg, None),
        Mode::ProbeCore(p) => run_watch(&inbox_dir, &cfg, Some(p)),
        Mode::Help => unreachable!(),
    }
    gdi::shutdown();
}

fn parse_mode(args: &[String]) -> Mode {
    let probe_env = env::var("TQ_MEMPROBE").ok().map(PathBuf::from);
    match args.first().map(|s| s.as_str()) {
        None | Some("--gui") => Mode::Gui(probe_env),
        Some("--probe") => {
            let p = args.get(1).map(PathBuf::from).or(probe_env);
            match p {
                Some(p) => Mode::Gui(Some(p)),
                None => Mode::Help,
            }
        }
        Some("--probe-core") => {
            let p = args.get(1).map(PathBuf::from).or(probe_env);
            match p {
                Some(p) => Mode::ProbeCore(p),
                None => Mode::Help,
            }
        }
        Some("--once") => Mode::Once,
        Some("--watch") | Some("--headless") => Mode::Watch,
        Some("--help") | Some("-h") => Mode::Help,
        _ => Mode::Help,
    }
}

fn print_help() {
    println!("图墙 PicLine · Rust / Win32 + GDI+ 自绘版");
    println!("  (无参数) / --gui       启动完整 GUI（默认）");
    println!("  --probe <out.json>     启动完整 GUI，12 秒后写内存探针 JSON 并退出");
    println!("  --once                 只读扫描一次并输出状态");
    println!("  --watch / --headless   无窗口守护内核（逻辑层验收）");
    println!("  --probe-core <out>     无窗口内核的 12 秒探针（Stage 1 对照）");
    println!("环境变量：TQ_DATA_DIR、TQ_SHOTS_DIR（隔离截图源）、TQ_MEMPROBE");
    println!(
        "行为常量：panel={}px reveal={}ms band={}px peek={}ms hide={}ms cursor-poll={}ms",
        PANEL_H, REVEAL_DELAY_MS, BAND_H, PEEK_MS, HIDE_DELAY_MS, CURSOR_POLL_MS
    );
}

/// 等价 lib/paths.js 的 initDataDirs()：
///   TQ_DATA_DIR 优先；否则 %APPDATA%\图墙；inbox 目录递归创建。
fn init_data_dirs() -> (PathBuf, PathBuf) {
    let data_dir = env::var("TQ_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let base = env::var("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| env::temp_dir());
            base.join("图墙")
        });
    let inbox_dir = data_dir.join("inbox");
    let _ = fs::create_dir_all(&inbox_dir); // 原版 mkdirSync(recursive) 未捕获，这里静默降级
    (data_dir, inbox_dir)
}

/// 等价 lib/paths.js 的 resolvePicturesDir()：PowerShell 失败时回落 ~/Pictures
fn resolve_pictures_dir() -> PathBuf {
    win32::pictures_dir().unwrap_or_else(|| {
        env::var("USERPROFILE")
            .map(|h| PathBuf::from(h).join("Pictures"))
            .unwrap_or_else(|_| PathBuf::from("Pictures"))
    })
}

fn run_once(data_dir: &Path, inbox_dir: &Path, cfg: &Settings) {
    println!("data-dir : {}", data_dir.display());
    println!("inbox    : {}", inbox_dir.display());
    println!("pictures : {}", resolve_pictures_dir().display());
    println!("config   : {}", cfg.to_json().replace('\n', " "));
    let items = list_inbox(inbox_dir);
    println!("items    : {}", items_to_json(&items));
    let d: Display = win32::primary_display();
    let fg = win32::foreground_info();
    println!(
        "display  : {}x{} @{}  fullscreen={}",
        d.w,
        d.h,
        d.scale_factor,
        is_fullscreen(fg.as_ref(), &d)
    );
    println!(
        "foreground: {}",
        fg.map(|f| format!("{},{},{},{} class={} title={}", f.l, f.t, f.r, f.b, f.class, f.title))
            .unwrap_or_else(|| "none".into())
    );
    println!("cursor   : {:?}（60ms 轮询的输入，Stage 1 只读取不驱动窗口）", win32::cursor_pos());
    println!("memory   : {}", memprobe::sample().one_line());
}

struct WatchState {
    core: CaptureCore,
    tracker: MemTracker,
    /// 等待写完成：path -> (上次大小, 连续稳定次数)
    pending: HashMap<PathBuf, (u64, u8)>,
    last_items_sig: String,
    last_fullscreen: bool,
}

fn run_watch(inbox_dir: &Path, cfg: &Settings, probe: Option<PathBuf>) {
    let started = SystemTime::now();
    let now = capture::now_ms();
    // TQ_SHOTS_DIR 是 Stage 1 新增的测试隔离钩子（默认不设置、行为不变）：
    // 自动化/内存取样时把截图源指向临时目录，确保不碰用户真实的 图片\Screenshots。
    let screenshots = env::var("TQ_SHOTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| resolve_pictures_dir().join("Screenshots"));
    let _ = fs::create_dir_all(&screenshots); // 等价 watchDir 的 mkdirSync(recursive)
    let display = win32::primary_display();

    let mut st = WatchState {
        core: CaptureCore::start(now),
        tracker: MemTracker::default(),
        pending: HashMap::new(),
        last_items_sig: String::new(),
        last_fullscreen: false,
    };
    st.core
        .set_options(Some(cfg.take_over), Some(cfg.clipboard_watch));

    // 启动基线：把当前剪贴板记为"已知"，不进去重库
    if let Some(snap) = win32::clipboard_snapshot() {
        let key = visual::visual_key(&snap.quantized, snap.width, snap.height);
        st.core.set_baseline_key(Some(key));
    }

    eprintln!(
        "[picline-core] watch 启动 | inbox={} | screenshots={} | {}",
        inbox_dir.display(),
        screenshots.display(),
        st.tracker.report()
    );

    let mut last = [0i64; 5]; // fg / clipboard / folder / prune / memory
    let tick_ms = 50i64;
    loop {
        let t = capture::now_ms();

        if t - last[3] >= 10_000 {
            last[3] = t;
            st.core.prune(t);
            eprintln!("[picline-core] {}", st.tracker.report());
        }
        if t - last[4] >= 250 {
            last[4] = t;
            st.tracker.tick();
        }
        if t - last[0] >= 350 {
            last[0] = t;
            let fs_now = is_fullscreen(win32::foreground_info().as_ref(), &display);
            if fs_now != st.last_fullscreen {
                st.last_fullscreen = fs_now;
                eprintln!("[picline-core] 全屏状态 -> {}", fs_now);
            }
        }
        if t - last[2] >= capture::SETTLE_INTERVAL_MS as i64 {
            last[2] = t;
            tick_folder(inbox_dir, &screenshots, &mut st);
        }
        if t - last[1] >= 800 {
            last[1] = t;
            tick_clipboard(inbox_dir, &mut st);
        }

        if let Some(p) = &probe {
            if started.elapsed().unwrap_or_default() >= Duration::from_secs(12) {
                let s = memprobe::sample();
                let _ = fs::write(p, s.to_probe_json());
                eprintln!(
                    "[picline-core] probe 写入 {} | ws={}KB peak={}KB",
                    p.display(),
                    s.working_set_kb,
                    s.peak_working_set_kb
                );
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(tick_ms as u64));
    }
}

fn mtime_ms(md: &fs::Metadata) -> f64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// 截图文件夹：只收启动之后的新文件；连续两次（间隔 250ms）大小一致才动它。
fn tick_folder(inbox_dir: &Path, screenshots: &Path, st: &mut WatchState) {
    let rd = match fs::read_dir(screenshots) {
        Ok(r) => r,
        Err(_) => {
            let _ = fs::create_dir_all(screenshots);
            return;
        }
    };
    let mut ready: Vec<PathBuf> = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_image_name(&name) {
            continue;
        }
        let path = entry.path();
        let md = match entry.metadata() {
            Ok(m) if m.is_file() => m,
            _ => continue,
        };
        if !st.core.is_fresh(mtime_ms(&md)) {
            continue;
        }
        let size = md.len();
        if size == 0 {
            continue;
        }
        match st.pending.get_mut(&path) {
            Some((prev, n)) if *prev == size => {
                *n = n.saturating_add(1);
                if *n >= 2 {
                    ready.push(path.clone());
                }
            }
            _ => {
                st.pending.insert(path, (size, 1));
            }
        }
    }
    for path in ready {
        st.pending.remove(&path);
        handle_incoming_file(inbox_dir, &path, st);
    }
}

fn handle_incoming_file(inbox_dir: &Path, src: &Path, st: &mut WatchState) {
    let ext = src
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_ascii_lowercase()))
        .unwrap_or_else(|| ".png".into());
    let (q, w, h) = match win32::decode_file_visual(src) {
        Some(v) => v,
        None => return, // 解不开的文件放着不动（等价 fileVisualInfo -> null）
    };
    let _ = q; // 文件通道不做内容去重，取键只为拿尺寸（原实现同样如此）
    let dims = visual::dims(w, h);
    let now = capture::now_ms();

    if !st.core.take_over {
        return; // 未接管：让截图留在原地
    }
    let dest = capture::unique_name(inbox_dir, "shot", &ext, now, &win32::random_hex3());
    let moved = fs::rename(src, &dest).is_ok()
        || (fs::copy(src, &dest).is_ok() && fs::remove_file(src).is_ok());
    if !moved {
        return;
    }
    if let Some(clip_file) = st.core.on_file_capture(&dims, now) {
        let _ = fs::remove_file(clip_file); // 换回原始文件，保住画质
    }
    emit_items(inbox_dir, st);
}

fn tick_clipboard(inbox_dir: &Path, st: &mut WatchState) {
    let snap = match win32::clipboard_snapshot() {
        Some(s) => s,
        None => return,
    };
    let key = visual::visual_key(&snap.quantized, snap.width, snap.height);
    let dims = visual::dims(snap.width, snap.height);
    let now = capture::now_ms();
    if st.core.poll_clipboard(&key, &dims, now) == ClipDecision::Accept {
        let bytes = png::encode_rows(snap.width, snap.height, &snap.png_rows);
        if bytes.is_empty() {
            return;
        }
        let dest = capture::unique_name(inbox_dir, "clip", ".png", now, &win32::random_hex3());
        if fs::write(&dest, bytes).is_ok() {
            st.core.on_clip_capture(&dims, dest, now);
            emit_items(inbox_dir, st);
        }
    }
}

fn emit_items(inbox_dir: &Path, st: &mut WatchState) {
    let json = items_to_json(&list_inbox(inbox_dir));
    if json != st.last_items_sig {
        st.last_items_sig = json.clone();
        eprintln!("[picline-core] items {}", json);
    }
}
