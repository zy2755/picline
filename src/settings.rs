//! 配置读写：%APPDATA%\图墙\config.json
//!
//! 等价目标：lib/settings.js
//!   - DEFAULTS 五个键及默认值完全一致
//!   - load() = {...DEFAULTS, ...JSON.parse(file)}；读失败静默回落到 DEFAULTS
//!   - save(patch) 合并后以 JSON.stringify(cache, null, 2) 写回；写失败静默吞掉
//!   - 未知键会被 spread 保留并原样写回（这点很容易被重写漏掉，这里显式保留）
//!
//! 2026-11-27 新增 `showHint`（底部操作提示是否显示，默认 true）、
//! `wind`（微风 0 关 / 1 轻 / 2 标准，默认 2）、`anim`（弹出收起动画，默认 true）。
//! 这三个都是原 JS 版没有的键：老配置里不存在时按默认值处理，
//! 写回时会补上，老版本读新配置也只会把它们当未知键原样保留 —— 双向兼容。

use std::fs;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub show_line: bool,
    /// 底部「点击复制 · 双击打开 …」提示条是否显示（托盘可切，写回 config.json）
    pub show_hint: bool,
    /// 微风强度：0 = 关（画面完全静止）、1 = 轻、2 = 标准。越界按边界夹紧
    pub wind: i64,
    /// 弹出 / 收起动画（false 时瞬间到位，给晕动敏感或追求极快的人）
    pub anim: bool,
    pub take_over: bool,
    pub clipboard_watch: bool,
    pub max_pegs: i64,
    pub first_run_done: bool,
    /// JSON 里出现、但不在 DEFAULTS 中的键，保留原始值文本，保证写回不变
    pub extra: Vec<(String, String)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_line: true,
            show_hint: true,
            wind: 2,
            anim: true,
            take_over: true,
            clipboard_watch: true,
            max_pegs: 10,
            first_run_done: false,
            extra: Vec::new(),
        }
    }
}

/// 扫描顶层 JSON 对象，返回 (键, 原始值文本) 列表。
/// 容错与 JS 的 JSON.parse 不同：解析失败时返回空列表（等价 catch -> DEFAULTS）。
fn scan_object(text: &str) -> Vec<(String, String)> {
    let b = text.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();
    skip_ws(b, &mut i);
    if b.get(i) != Some(&b'{') {
        return out;
    }
    i += 1;
    loop {
        skip_ws(b, &mut i);
        if b.get(i) == Some(&b'}') || i >= b.len() {
            break;
        }
        let key = match parse_string(b, &mut i) {
            Some(k) => k,
            None => return out,
        };
        skip_ws(b, &mut i);
        if b.get(i) != Some(&b':') {
            return out;
        }
        i += 1;
        skip_ws(b, &mut i);
        let start = i;
        if skip_value(b, &mut i).is_none() {
            return out;
        }
        let raw = text[start..i].trim().to_string();
        out.push((key, raw));
        skip_ws(b, &mut i);
        if b.get(i) == Some(&b',') {
            i += 1;
        }
    }
    out
}

fn skip_ws(b: &[u8], i: &mut usize) {
    while let Some(&c) = b.get(*i) {
        if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' {
            *i += 1;
        } else {
            break;
        }
    }
}

fn parse_string(b: &[u8], i: &mut usize) -> Option<String> {
    if b.get(*i) != Some(&b'"') {
        return None;
    }
    *i += 1;
    let mut s = String::new();
    while let Some(&c) = b.get(*i) {
        *i += 1;
        match c {
            b'"' => return Some(s),
            b'\\' => {
                let e = *b.get(*i)?;
                *i += 1;
                match e {
                    b'"' => s.push('"'),
                    b'\\' => s.push('\\'),
                    b'/' => s.push('/'),
                    b'n' => s.push('\n'),
                    b'r' => s.push('\r'),
                    b't' => s.push('\t'),
                    b'b' => s.push('\u{0008}'),
                    b'f' => s.push('\u{000C}'),
                    b'u' => {
                        // \uXXXX：保留码位（代理对按单个 u16 处理，对配置键名足够）
                        let hex = std::str::from_utf8(b.get(*i..*i + 4)?).ok()?;
                        let cp = u32::from_str_radix(hex, 16).ok()?;
                        *i += 4;
                        s.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                    }
                    _ => return None,
                }
            }
            _ => {
                // 逐字节推进，遇到 UTF-8 多字节时按 char 收集
                let start = *i - 1;
                let mut end = start + 1;
                while end < b.len() && (b[end] & 0xC0) == 0x80 {
                    end += 1;
                }
                s.push_str(std::str::from_utf8(&b[start..end]).ok()?);
                *i = end;
            }
        }
    }
    None
}

/// 跳过一个完整 JSON 值（字符串 / 对象 / 数组 / 字面量 / 数字）
fn skip_value(b: &[u8], i: &mut usize) -> Option<()> {
    match *b.get(*i)? {
        b'"' => {
            parse_string(b, i)?;
            Some(())
        }
        b'{' | b'[' => {
            let open = b[*i];
            let close = if open == b'{' { b'}' } else { b']' };
            *i += 1;
            loop {
                skip_ws(b, i);
                if *b.get(*i)? == close {
                    *i += 1;
                    return Some(());
                }
                if open == b'{' {
                    parse_string(b, i)?;
                    skip_ws(b, i);
                    if *b.get(*i)? != b':' {
                        return None;
                    }
                    *i += 1;
                    skip_ws(b, i);
                }
                skip_value(b, i)?;
                skip_ws(b, i);
                match b.get(*i) {
                    Some(&b',') => *i += 1,
                    Some(&c) if c == close => {
                        *i += 1;
                        return Some(());
                    }
                    _ => return None,
                }
            }
        }
        _ => {
            let start = *i;
            while let Some(&c) = b.get(*i) {
                if c == b',' || c == b'}' || c == b']' || c == b' ' || c == b'\n' || c == b'\r' {
                    break;
                }
                *i += 1;
            }
            if *i > start {
                Some(())
            } else {
                None
            }
        }
    }
}

impl Settings {
    /// 等价 load()：读不到 / 解析不了 -> DEFAULTS，不抛错
    pub fn load(dir: &Path) -> Self {
        let text = match fs::read_to_string(dir.join("config.json")) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for (k, raw) in scan_object(text) {
            match k.as_str() {
                "showLine" => s.show_line = raw == "true",
                "showHint" => s.show_hint = raw == "true",
                "wind" => {
                    if let Ok(n) = raw.parse::<i64>() {
                        s.wind = n.clamp(0, 2);
                    }
                }
                "anim" => s.anim = raw == "true",
                "takeOver" => s.take_over = raw == "true",
                "clipboardWatch" => s.clipboard_watch = raw == "true",
                "maxPegs" => {
                    if let Ok(n) = raw.parse::<i64>() {
                        s.max_pegs = n;
                    }
                }
                "firstRunDone" => s.first_run_done = raw == "true",
                _ => s.extra.push((k, raw)),
            }
        }
        s
    }

    /// 等价 JSON.stringify(cache, null, 2)：两空格缩进、DEFAULTS 键序在前、额外键原样追加
    pub fn to_json(&self) -> String {
        let mut pairs: Vec<(String, String)> = vec![
            ("showLine".into(), self.show_line.to_string()),
            ("showHint".into(), self.show_hint.to_string()),
            ("wind".into(), self.wind.to_string()),
            ("anim".into(), self.anim.to_string()),
            ("takeOver".into(), self.take_over.to_string()),
            ("clipboardWatch".into(), self.clipboard_watch.to_string()),
            ("maxPegs".into(), self.max_pegs.to_string()),
            ("firstRunDone".into(), self.first_run_done.to_string()),
        ];
        for (k, v) in &self.extra {
            pairs.push((json_escape(k), v.clone()));
        }
        let mut out = String::from("{\n");
        for (n, (k, v)) in pairs.iter().enumerate() {
            out.push_str("  \"");
            out.push_str(k);
            out.push_str("\": ");
            out.push_str(v);
            if n + 1 < pairs.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push('}');
        out
    }

    /// 等价 save(patch)：合并 -> 写回；写失败静默吞掉，返回值仍是合并后的配置
    pub fn save(dir: &Path, patch: &Settings) -> std::io::Result<()> {
        fs::create_dir_all(dir)?;
        fs::write(dir.join("config.json"), patch.to_json())
    }
}

pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_js() {
        let s = Settings::default();
        assert!(s.show_line && s.take_over && s.clipboard_watch);
        assert!(s.show_hint);
        assert_eq!(s.wind, 2);
        assert!(s.anim);
        assert_eq!(s.max_pegs, 10);
        assert!(!s.first_run_done);
    }

    #[test]
    fn wind_and_anim_are_clamped_and_round_trip() {
        // 缺键 -> 默认：风开在标准档、动画开
        let d = Settings::parse(r#"{"maxPegs":10}"#);
        assert_eq!((d.wind, d.anim), (2, true));
        // 越界值夹紧，避免手改配置把风速写成 99 或 -3
        assert_eq!(Settings::parse(r#"{"wind":9}"#).wind, 2);
        assert_eq!(Settings::parse(r#"{"wind":-3}"#).wind, 0);
        // 关微风 + 关动画都要能写回再读回
        let s = Settings::parse(r#"{"wind":0,"anim":false}"#);
        assert_eq!((s.wind, s.anim), (0, false));
        let out = s.to_json();
        assert!(out.contains("\"wind\": 0") && out.contains("\"anim\": false"));
        let back = Settings::parse(&out);
        assert_eq!((back.wind, back.anim), (0, false));
    }

    #[test]
    fn hint_flag_defaults_on_and_round_trips() {
        // 老配置里没有 showHint：按默认 true，不能变成立即隐藏
        assert!(Settings::parse(r#"{"showLine":true}"#).show_hint);
        // 显式关闭要生效
        let s = Settings::parse(r#"{"showHint":false}"#);
        assert!(!s.show_hint);
        // 写回后仍能读回来（托盘切换依赖这条）
        let out = s.to_json();
        assert!(out.contains("\"showHint\": false"));
        assert!(!Settings::parse(&out).show_hint);
    }

    #[test]
    fn broken_json_falls_back_to_defaults() {
        assert_eq!(Settings::parse("{ not json"), Settings::default());
        assert_eq!(Settings::parse(""), Settings::default());
    }

    #[test]
    fn known_keys_are_merged_over_defaults() {
        let s = Settings::parse(r#"{"showLine":false,"maxPegs":4,"firstRunDone":true}"#);
        assert!(!s.show_line);
        assert_eq!(s.max_pegs, 4);
        assert!(s.first_run_done);
        assert!(s.take_over); // 未出现的键沿用默认值
    }

    #[test]
    fn unknown_keys_survive_round_trip() {
        // JS 的 {...DEFAULTS, ...parsed} 会保留未知键，save() 又原样写回
        let s = Settings::parse(r#"{"maxPegs":7,"futureFlag":true,"nested":{"a":[1,2]}}"#);
        let out = s.to_json();
        assert!(out.contains("\"futureFlag\": true"));
        assert!(out.contains("\"nested\": {\"a\":[1,2]}"));
        assert_eq!(Settings::parse(&out).max_pegs, 7);
    }

    #[test]
    fn formatting_matches_json_stringify_indent_2() {
        assert_eq!(
            Settings::default().to_json(),
            "{\n  \"showLine\": true,\n  \"showHint\": true,\n  \"wind\": 2,\n  \"anim\": true,\n  \"takeOver\": true,\n  \"clipboardWatch\": true,\n  \"maxPegs\": 10,\n  \"firstRunDone\": false\n}"
        );
    }
}
