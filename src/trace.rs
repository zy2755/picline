//! 真机验收可观测钩子（默认完全关闭，不改变正常行为与内存口径）。
//!
//! - `TQ_TRACE=<path>`：事件日志，每行一个 JSON 对象，追加写。
//! - `TQ_STATE=<path>`：状态快照，原子覆写（tmp + rename），供注入器读内部状态。
//!
//! 两个变量都不设置时，`ev()` / `snapshot()` 只做一次 OnceLock 查询后直接返回，
//! 不产生任何文件 IO、不分配堆内存（除调用方已构造好的字符串）。
//! 因此正常启动、--probe 内存取样与 S0/S1 对比的口径不受影响。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

static TRACE: OnceLock<Option<PathBuf>> = OnceLock::new();
static STATE: OnceLock<Option<PathBuf>> = OnceLock::new();

fn trace_path() -> Option<&'static PathBuf> {
    TRACE
        .get_or_init(|| std::env::var_os("TQ_TRACE").map(PathBuf::from))
        .as_ref()
}
fn state_path() -> Option<&'static PathBuf> {
    STATE
        .get_or_init(|| std::env::var_os("TQ_STATE").map(PathBuf::from))
        .as_ref()
}

pub fn enabled() -> bool {
    trace_path().is_some() || state_path().is_some()
}

pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// 追加一条事件。kind 用短 ASCII 标识，detail 为可选说明（路径/toast/参数）。
pub fn ev(kind: &str, detail: &str) {
    let Some(p) = trace_path() else { return };
    let t = crate::capture::now_ms();
    let line = format!(
        "{{\"t\":{},\"ev\":\"{}\",\"detail\":\"{}\"}}\n",
        t,
        esc(kind),
        esc(detail)
    );
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// 原子覆写状态快照 JSON。
pub fn snapshot(json: &str) {
    let Some(p) = state_path() else { return };
    let mut tmp = p.clone();
    tmp.set_extension("tmp");
    if fs::write(&tmp, json.as_bytes()).is_ok() {
        if fs::rename(&tmp, p).is_err() {
            // 极少数情况（目标被占用）退化为直接写，读侧有重试
            let _ = fs::write(p, json.as_bytes());
            let _ = fs::remove_file(&tmp);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn esc_handles_path_and_quotes() {
        assert_eq!(super::esc("C:\\a\\b"), "C:\\\\a\\\\b");
        assert_eq!(super::esc("a\"b"), "a\\\"b");
        assert_eq!(super::esc("已复制到剪贴板"), "已复制到剪贴板");
    }
}
