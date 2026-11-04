//! 截图捕获管线的状态机（与 lib/capture.js 逐条对齐）
//!
//! 行为边界（全部来自原实现，不得"顺手改好"）：
//!   - 只收启动之后的新内容：started_at = now - 5000，mtime 早于它的一律不动
//!   - 启动时把当前剪贴板记为"已知"，但不进去重库（之后再复制同一张应照挂）
//!   - 文件通道不做内容去重：每个新文件都是一次新截图
//!   - 跨通道判重只靠 尺寸 + 10s 时间窗，不靠视觉哈希跨通道比对
//!   - 剪贴板通道抢先挂了同一张时，文件通道用原始文件替换掉转码版（保住画质）
//!   - 所有 IO 异常都是静默跳过，不向上传播

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const VISUAL_TTL_MS: i64 = 90_000;
pub const CROSS_TTL_MS: i64 = 10_000;
pub const SUPPRESS_MS: i64 = 1_500;
pub const SETTLE_INTERVAL_MS: u64 = 250;
pub const SETTLE_MAX_TRIES: usize = 12;
pub const COPIED_KEYS_LIMIT: usize = 32;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Clone, Debug)]
pub struct ClipCapture {
    pub dims: String,
    pub ts: i64,
    pub file: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClipDecision {
    /// 应当忽略（含"剪贴板没变 / 抑制窗 / 自己复制出去的 / 与刚捕获文件同尺寸"）
    Skip,
    /// 应当写入收件箱，并返回是否需要额外记一次 capture
    Accept,
}

#[derive(Debug)]
pub struct CaptureCore {
    pub take_over: bool,
    pub clipboard_watch: bool,
    /// now - 5000：防线，绝不扫走历史截图
    pub started_at: i64,
    pub suppress_until: i64,
    pub last_clipboard_key: Option<String>,
    pub copied_keys: HashSet<String>,
    pub last_file_capture: Option<(String, i64)>,
    pub last_clip_capture: Option<ClipCapture>,
    pub seen_visual: HashMap<String, i64>,
    pub stopped: bool,
}

impl CaptureCore {
    pub fn start(now: i64) -> Self {
        Self {
            take_over: true,
            clipboard_watch: true,
            started_at: now - 5_000,
            suppress_until: 0,
            last_clipboard_key: None,
            copied_keys: HashSet::new(),
            last_file_capture: None,
            last_clip_capture: None,
            seen_visual: HashMap::new(),
            stopped: false,
        }
    }

    pub fn set_options(&mut self, take_over: Option<bool>, clipboard_watch: Option<bool>) {
        if let Some(v) = take_over {
            self.take_over = v;
        }
        if let Some(v) = clipboard_watch {
            self.clipboard_watch = v;
        }
    }

    /// 启动基线：只记 lastClipboardKey（变化检测），不进 seen/copied 去重库
    pub fn set_baseline_key(&mut self, key: Option<String>) {
        self.last_clipboard_key = key;
    }

    /// 等价 pollClipboard() 的判定链。调用方保证 key 非空、已取到 dims。
    pub fn poll_clipboard(&mut self, key: &str, dims: &str, now: i64) -> ClipDecision {
        if self.stopped || !self.clipboard_watch {
            return ClipDecision::Skip;
        }
        if self.last_clipboard_key.as_deref() == Some(key) {
            return ClipDecision::Skip; // 剪贴板没变
        }
        self.last_clipboard_key = Some(key.to_string());
        if now < self.suppress_until {
            return ClipDecision::Skip; // 自己复制出去的（时窗保险）
        }
        if self.copied_keys.contains(key) {
            return ClipDecision::Skip;
        }
        if let Some((d, ts)) = &self.last_file_capture {
            if d == dims && now - *ts < CROSS_TTL_MS {
                return ClipDecision::Skip; // 文件通道已经挂了同一张
            }
        }
        ClipDecision::Accept
    }

    /// 等价 suppressCopy()：点"单击复制"后，别把刚写出去的图再挂一次
    pub fn suppress_copy(&mut self, key: &str, now: i64) {
        self.copied_keys.insert(key.to_string());
        self.last_clipboard_key = Some(key.to_string());
        self.suppress_until = now + SUPPRESS_MS;
    }

    /// 剪贴板通道成功落盘后登记，供文件通道"换回原始文件"判定
    pub fn on_clip_capture(&mut self, dims: &str, file: PathBuf, now: i64) {
        self.last_clip_capture = Some(ClipCapture {
            dims: dims.to_string(),
            ts: now,
            file,
        });
    }

    /// 文件通道成功移入收件箱后调用。
    /// 返回 Some(path) 表示需要删掉剪贴板通道那份转码版（sameAsClip）。
    pub fn on_file_capture(&mut self, dims: &str, now: i64) -> Option<PathBuf> {
        let replacement = match &self.last_clip_capture {
            Some(c) if c.dims == dims && now - c.ts < CROSS_TTL_MS => Some(c.file.clone()),
            _ => None,
        };
        if replacement.is_some() {
            self.last_clip_capture = None;
        }
        self.last_file_capture = Some((dims.to_string(), now));
        replacement
    }

    /// 等价 pruneKeys()：10s 一次
    pub fn prune(&mut self, now: i64) {
        let cutoff = now - VISUAL_TTL_MS;
        self.seen_visual.retain(|_, ts| *ts >= cutoff);
        if self.copied_keys.len() > COPIED_KEYS_LIMIT {
            self.copied_keys.clear();
        }
    }

    /// "记录见过；已在 -> true"，保留给扩展用（原 JS 的 rememberVisual）
    pub fn remember_visual(&mut self, key: &str, now: i64) -> bool {
        let existed = self.seen_visual.contains_key(key);
        self.seen_visual.insert(key.to_string(), now);
        existed
    }

    /// 只收启动之后的新文件（元数据事件/云同步引起的旧文件不动）
    pub fn is_fresh(&self, mtime_ms: f64) -> bool {
        mtime_ms >= self.started_at as f64
    }
}

/// 等待文件写完：连续两次（间隔 250ms）大小一致且 >0 才动它。
/// 抽成纯函数便于验证；None 表示文件消失 / 不是文件 / 解不开。
pub fn settle_decision(sizes: &[Option<u64>]) -> Option<u64> {
    let mut prev: Option<u64> = None;
    for (i, &cur) in sizes.iter().take(SETTLE_MAX_TRIES).enumerate() {
        let cur = cur?;
        if cur == 0 {
            return None;
        }
        if i > 0 && prev == Some(cur) {
            return Some(cur);
        }
        prev = Some(cur);
    }
    None
}

/// 等价 uniqueName()：shot-/clip- + UTC yyyyMMddHHmmss + 3 字节随机 hex + 扩展名
pub fn unique_name(dir: &Path, stem: &str, ext: &str, now: i64, rand_hex: &str) -> PathBuf {
    dir.join(format!("{}-{}-{}{}", stem, utc_stamp(now), rand_hex, ext))
}

/// new Date().toISOString().replace(/[-:T]/g,'').slice(0,14) == UTC yyyyMMddHHmmss
pub fn utc_stamp(now_ms: i64) -> String {
    let secs = now_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}",
        y,
        m,
        d,
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

/// days since 1970-01-01 -> (y,m,d)，Howard Hinnant 的 civil_from_days
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_does_not_seed_dedup_store() {
        let mut c = CaptureCore::start(10_000);
        c.set_baseline_key(Some("k|1x1".into()));
        assert_eq!(c.copied_keys.len(), 0);
        assert!(c.seen_visual.is_empty());
        // 同一张图在基线中 -> 第一次轮询被跳过（剪贴板没变）
        assert_eq!(c.poll_clipboard("k|1x1", "1x1", 10_100), ClipDecision::Skip);
        // 但之后用户主动再复制同一张，应照挂
        c.last_clipboard_key = None;
        assert_eq!(c.poll_clipboard("k|1x1", "1x1", 10_200), ClipDecision::Accept);
    }

    #[test]
    fn history_files_are_never_taken() {
        let c = CaptureCore::start(100_000); // started_at = 95_000
        assert!(!c.is_fresh(94_999.0));
        assert!(c.is_fresh(95_000.0));
        assert!(c.is_fresh(100_001.0));
    }

    #[test]
    fn clipboard_suppression_chain() {
        let mut c = CaptureCore::start(10_000);
        c.suppress_copy("self|2x2", 20_000);
        assert_eq!(c.poll_clipboard("self|2x2", "2x2", 20_100), ClipDecision::Skip);
        assert_eq!(c.poll_clipboard("other|2x2", "2x2", 20_100), ClipDecision::Skip); // 抑制窗
        // 换一张新图 + 抑制窗过期 -> 接受；同一个 key 会因"剪贴板没变"被跳过
        assert_eq!(c.poll_clipboard("fresh|2x2", "2x2", 21_600), ClipDecision::Accept);
        assert_eq!(c.poll_clipboard("self|2x2", "2x2", 21_700), ClipDecision::Skip); // copiedKeys 长期拦
    }

    #[test]
    fn cross_channel_10s_window_in_both_directions() {
        let mut c = CaptureCore::start(0);
        c.on_clip_capture("1920x1080", PathBuf::from("clip.png"), 30_000);
        // 文件在 9s 内到达 -> 要删掉转码版，换回原始文件
        assert_eq!(
            c.on_file_capture("1920x1080", 39_000),
            Some(PathBuf::from("clip.png"))
        );
        c.on_clip_capture("800x600", PathBuf::from("clip2.png"), 40_000);
        // 文件在 11s 后才到 -> 窗口过期，不替换
        assert_eq!(c.on_file_capture("800x600", 51_000), None);

        // 反向：文件刚捕获，剪贴板同尺寸 10s 内到达 -> 跳过
        let mut c2 = CaptureCore::start(0);
        c2.on_file_capture("1280x720", 60_000);
        assert_eq!(c2.poll_clipboard("x|1280x720", "1280x720", 69_000), ClipDecision::Skip);
        assert_eq!(c2.poll_clipboard("y|1280x720", "1280x720", 70_001), ClipDecision::Accept);
    }

    #[test]
    fn option_switches() {
        let mut c = CaptureCore::start(0);
        c.set_options(Some(false), Some(false));
        assert_eq!(c.poll_clipboard("k", "1x1", 100), ClipDecision::Skip);
        c.set_options(Some(true), None);
        assert!(c.take_over);
        assert!(!c.clipboard_watch);
    }

    #[test]
    fn settle_requires_two_equal_nonzero_sizes() {
        assert_eq!(settle_decision(&[Some(10)]), None);
        assert_eq!(settle_decision(&[Some(10), Some(12), Some(12)]), Some(12));
        assert_eq!(settle_decision(&[None, Some(1)]), None);
        assert_eq!(settle_decision(&[Some(0), Some(0)]), None);
        assert_eq!(settle_decision(&[Some(5), Some(5)]), Some(5));
    }

    #[test]
    fn unique_name_matches_js_utc_stamp() {
        // 2026-11-04T11:05:07Z = 1793790307 s -> "20261104110507"
        let t = 1_793_790_307_000i64;
        assert_eq!(utc_stamp(t), "20261104110507");
        let p = unique_name(Path::new("C:/inbox"), "shot", ".png", t, "a1b2c3");
        assert!(p.to_string_lossy().ends_with("shot-20261104110507-a1b2c3.png"));
    }

    #[test]
    fn prune_ttl_and_copied_keys_cap() {
        let mut c = CaptureCore::start(0);
        c.remember_visual("old", 1_000);
        c.remember_visual("new", 100_000);
        for i in 0..33 {
            c.copied_keys.insert(format!("k{i}"));
        }
        c.prune(100_000);
        assert!(!c.seen_visual.contains_key("old"));
        assert!(c.seen_visual.contains_key("new"));
        assert!(c.copied_keys.is_empty());
    }
}
