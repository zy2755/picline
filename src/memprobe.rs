//! 内存基线/对比取样。
//!
//! 兼容目标：scripts/mem-probe.sh 用 TQ_MEMPROBE 读取的 JSON 数组
//!   [{type, pid, wsKB, ...}]，消费脚本累加 wsKB 得到"工作集合计(MB)"。
//! 原版 app.getAppMetrics() 会列出所有 Electron 子进程；Rust 版是单进程，
//! 所以数组只有一项（type 仍写 "Browser" 以便旧脚本直接复用），
//! 并额外提供 peakWsKB / privateKB —— 原版探针没有峰值字段，这是本次要补的口径。

use crate::win32;

#[derive(Clone, Debug, Default)]
pub struct MemSample {
    pub pid: u32,
    pub working_set_kb: u64,
    pub peak_working_set_kb: u64,
    pub private_kb: u64,
}

pub fn sample() -> MemSample {
    let (ws, peak, private) = win32::process_memory();
    MemSample {
        pid: std::process::id(),
        working_set_kb: ws / 1024,
        peak_working_set_kb: peak / 1024,
        private_kb: private / 1024,
    }
}

impl MemSample {
    /// 兼容 TQ_MEMPROBE 的输出（字段 wsKB 必须保留，mem-probe.sh 依赖它）
    pub fn to_probe_json(&self) -> String {
        format!(
            "[{{\"type\":\"Browser\",\"pid\":{},\"wsKB\":{},\"peakWsKB\":{},\"privateKB\":{}}}]",
            self.pid, self.working_set_kb, self.peak_working_set_kb, self.private_kb
        )
    }

    pub fn one_line(&self) -> String {
        format!(
            "ws={}KB peak={}KB private={}KB",
            self.working_set_kb, self.peak_working_set_kb, self.private_kb
        )
    }
}

/// 运行期峰值追踪：常驻只有 3 个 u64，不缓存历史样本。
#[derive(Debug, Default)]
pub struct MemTracker {
    pub peak_ws_kb: u64,
    pub peak_private_kb: u64,
    pub peak_working_set_kb: u64,
    pub samples: u64,
}

impl MemTracker {
    pub fn tick(&mut self) -> MemSample {
        let s = sample();
        self.samples += 1;
        self.peak_ws_kb = self.peak_ws_kb.max(s.working_set_kb);
        self.peak_private_kb = self.peak_private_kb.max(s.private_kb);
        self.peak_working_set_kb = self.peak_working_set_kb.max(s.peak_working_set_kb);
        s
    }

    pub fn report(&self) -> String {
        format!(
            "samples={} peakWS={}KB peakPrivate={}KB",
            self.samples, self.peak_ws_kb, self.peak_private_kb
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_json_is_consumable_by_existing_shell_script() {
        let s = MemSample { pid: 42, working_set_kb: 2048, peak_working_set_kb: 4096, private_kb: 1024 };
        let j = s.to_probe_json();
        assert!(j.contains("\"wsKB\":2048"));
        assert!(j.contains("\"peakWsKB\":4096"));
        assert!(j.starts_with("[{") && j.ends_with("}]"));
    }

    #[test]
    fn tracker_keeps_maxima_only() {
        let mut t = MemTracker::default();
        t.peak_ws_kb = 100;
        t.peak_private_kb = 50;
        t.tick();
        assert!(t.peak_ws_kb >= 100);
        assert_eq!(t.samples, 1);
    }
}
