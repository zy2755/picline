//! 全屏检测判定逻辑（等价 lib/fullscreen.js 的 isFullscreen）
//!
//! 原实现靠一个常驻 powershell.exe 子进程每 350ms 输出一行 JSON；
//! Rust 版直接在进程内调 Win32（见 win32.rs），省掉整个 PowerShell 运行时。
//! 但判定规则本身逐条保留，包括 2px 容差和 JS Math.round 的语义。

pub const WS_CAPTION: i32 = 0x00C0_0000;
pub const SHELL_CLASSES: [&str; 3] = ["Progman", "WorkerW", "Shell_TrayWnd"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FgInfo {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
    pub style: i32,
    pub class: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    /// 逻辑像素（Electron display.bounds）
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub scale_factor: f64,
}

/// JS Math.round(x) = floor(x + 0.5)，负数语义与 Rust f64::round 不同。
/// 副屏位于主屏左侧时 bounds.x 为负，这里必须严格对齐，否则多显示器会误判。
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

pub fn is_fullscreen(fg: Option<&FgInfo>, d: &Display) -> bool {
    let fg = match fg {
        Some(v) => v,
        None => return false,
    };
    if fg.class.is_empty() {
        return false; // 等价 fg.c === undefined
    }
    if SHELL_CLASSES.contains(&fg.class.as_str()) {
        return false;
    }
    if fg.title.starts_with("图墙") {
        return false; // 我们自己的窗口
    }
    if fg.style & WS_CAPTION != 0 {
        return false; // 有标题栏就不是全屏
    }
    let sf = d.scale_factor;
    let ml = js_round(d.x as f64 * sf);
    let mt = js_round(d.y as f64 * sf);
    let mr = js_round((d.x + d.w) as f64 * sf);
    let mb = js_round((d.y + d.h) as f64 * sf);
    fg.l as f64 <= ml + 2.0
        && fg.t as f64 <= mt + 2.0
        && fg.r as f64 >= mr - 2.0
        && fg.b as f64 >= mb - 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display() -> Display {
        Display { x: 0, y: 0, w: 1920, h: 1080, scale_factor: 1.0 }
    }
    fn covering(class: &str, style: i32, title: &str) -> FgInfo {
        FgInfo { l: 0, t: 0, r: 1920, b: 1080, style, class: class.into(), title: title.into() }
    }

    #[test]
    fn js_round_matches_negative_half() {
        assert_eq!(js_round(-2.5), -2.0); // Math.round(-2.5) === -2
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!((-2.5f64).round(), -3.0); // 说明为何不能直接 f64::round
    }

    #[test]
    fn shell_and_own_windows_are_never_fullscreen() {
        let d = display();
        assert!(!is_fullscreen(None, &d));
        assert!(!is_fullscreen(Some(&covering("Progman", 0, "")), &d));
        assert!(!is_fullscreen(Some(&covering("Shell_TrayWnd", 0, "")), &d));
        assert!(!is_fullscreen(Some(&covering("Chrome_WidgetWin_1", 0, "图墙 - 设置")), &d));
    }

    #[test]
    fn captioned_window_is_not_fullscreen() {
        assert!(!is_fullscreen(Some(&covering("Notepad", WS_CAPTION, "")), &display()));
    }

    #[test]
    fn full_cover_with_2px_tolerance() {
        let d = display();
        assert!(is_fullscreen(Some(&covering("Game", 0, "")), &d));
        let mut almost = covering("Game", 0, "");
        almost.l = 2;
        almost.t = 2;
        assert!(is_fullscreen(Some(&almost), &d));
        almost.l = 3;
        assert!(!is_fullscreen(Some(&almost), &d));
    }

    #[test]
    fn dpi_scaled_display_uses_physical_pixels() {
        let d = Display { x: 0, y: 0, w: 1280, h: 720, scale_factor: 1.5 };
        let fg = FgInfo { l: 0, t: 0, r: 1920, b: 1080, style: 0, class: "Game".into(), title: "".into() };
        assert!(is_fullscreen(Some(&fg), &d));
        let logical = FgInfo { r: 1280, b: 720, ..fg.clone() };
        assert!(!is_fullscreen(Some(&logical), &d));
    }

    #[test]
    fn negative_origin_display_uses_js_round() {
        // 副屏在左：x=-1920，150% DPI -> 物理 -2880
        let d = Display { x: -1920, y: 0, w: 1920, h: 1080, scale_factor: 1.5 };
        let fg = FgInfo { l: -2880, t: 0, r: 0, b: 1620, style: 0, class: "Game".into(), title: "".into() };
        assert!(is_fullscreen(Some(&fg), &d));
    }
}
