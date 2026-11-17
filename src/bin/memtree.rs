//! memtree —— 进程树内存取样器（Q4：给两侧补统一的峰值口径）
//!
//! 与 picline-core 的 memprobe.rs 使用完全相同的
//! PROCESS_MEMORY_COUNTERS_EX / K32GetProcessMemoryInfo，
//! 因此 Electron 与 Rust 版的工作集、峰值工作集、私有提交可以直接相减比较。
//!
//! 用法：
//!   memtree --duration 12 --out out.json [--interval 250] --pid <pid>
//!   memtree --duration 12 --out out.json -- <command> [args...]
//!   memtree --duration 12 --out out.json --kill -- <command> [args...]

#![cfg_attr(not(windows), allow(dead_code))]

use std::time::{Duration, Instant};

#[cfg(windows)]
mod ffi {
    #[repr(C)]
    pub struct ProcessEntry32W {
        pub dw_size: u32,
        pub cnt_usage: u32,
        pub th32_process_id: u32,
        pub th32_default_heap_id: usize,
        pub th32_module_id: u32,
        pub cnt_threads: u32,
        pub th32_parent_process_id: u32,
        pub pc_pri_class_base: i32,
        pub dw_flags: u32,
        pub sz_exe_file: [u16; 260],
    }

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

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
        pub fn Process32FirstW(snap: isize, entry: *mut ProcessEntry32W) -> i32;
        pub fn Process32NextW(snap: isize, entry: *mut ProcessEntry32W) -> i32;
        pub fn CloseHandle(h: isize) -> i32;
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        pub fn K32GetProcessMemoryInfo(
            h: isize,
            counters: *mut MemoryCountersEx,
            cb: u32,
        ) -> i32;
    }
}

#[cfg(windows)]
fn process_table() -> Vec<(u32, u32, String)> {
    use ffi::*;
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(2, 0); // TH32CS_SNAPPROCESS
        if snap == -1 || snap == 0 {
            return out;
        }
        let mut e: ProcessEntry32W = std::mem::zeroed();
        e.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let end = e.sz_exe_file.iter().position(|&c| c == 0).unwrap_or(260);
            out.push((
                e.th32_process_id,
                e.th32_parent_process_id,
                String::from_utf16_lossy(&e.sz_exe_file[..end]),
            ));
            e.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(not(windows))]
fn process_table() -> Vec<(u32, u32, String)> {
    Vec::new()
}

/// 以 root 为根做闭包，得到整棵进程树
fn tree_from(root: u32, table: &[(u32, u32, String)]) -> Vec<(u32, String)> {
    let mut tree: Vec<(u32, String)> = table
        .iter()
        .find(|(pid, _, _)| *pid == root)
        .map(|(pid, _, name)| (*pid, name.clone()))
        .into_iter()
        .collect();
    loop {
        let before = tree.len();
        for (pid, ppid, name) in table {
            if tree.iter().any(|(p, _)| p == pid) {
                continue;
            }
            if tree.iter().any(|(p, _)| *p == *ppid) {
                tree.push((*pid, name.clone()));
            }
        }
        if tree.len() == before {
            break;
        }
    }
    tree
}

#[cfg(windows)]
fn read_memory(pid: u32) -> Option<(u64, u64, u64)> {
    use ffi::*;
    unsafe {
        // PROCESS_QUERY_INFORMATION | PROCESS_VM_READ
        let h = OpenProcess(0x0400 | 0x0010, 0, pid);
        if h == 0 {
            return None;
        }
        let mut c: MemoryCountersEx = std::mem::zeroed();
        c.cb = std::mem::size_of::<MemoryCountersEx>() as u32;
        let ok = K32GetProcessMemoryInfo(h, &mut c, c.cb);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        Some((
            c.working_set_size as u64,
            c.peak_working_set_size as u64,
            c.private_usage as u64,
        ))
    }
}

#[cfg(not(windows))]
fn read_memory(_pid: u32) -> Option<(u64, u64, u64)> {
    None
}

#[derive(Default)]
struct Args {
    duration: u64,
    interval: u64,
    out: String,
    pid: Option<u32>,
    kill: bool,
    command: Vec<String>,
}

fn parse_args() -> Args {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut a = Args { duration: 12, interval: 250, ..Default::default() };
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--" => {
                a.command = raw[i + 1..].to_vec();
                break;
            }
            "--duration" => {
                i += 1;
                a.duration = raw.get(i).and_then(|s| s.parse().ok()).unwrap_or(12);
            }
            "--interval" => {
                i += 1;
                a.interval = raw.get(i).and_then(|s| s.parse().ok()).unwrap_or(250);
            }
            "--out" => {
                i += 1;
                a.out = raw.get(i).cloned().unwrap_or_default();
            }
            "--pid" => {
                i += 1;
                a.pid = raw.get(i).and_then(|s| s.parse().ok());
            }
            "--kill" => a.kill = true,
            _ => {}
        }
        i += 1;
    }
    a
}

fn main() {
    let a = parse_args();
    let mut child = if !a.command.is_empty() {
        let mut c = std::process::Command::new(&a.command[0]);
        c.args(&a.command[1..]);
        match c.spawn() {
            Ok(ch) => Some(ch),
            Err(e) => {
                eprintln!("memtree: 启动失败: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    let root = a.pid.or_else(|| child.as_ref().map(|c| c.id()));
    let root = match root {
        Some(r) => r,
        None => {
            eprintln!("memtree: 需要 --pid 或 -- <command>");
            std::process::exit(1);
        }
    };

    let start = Instant::now();
    let total = Duration::from_secs(a.duration);
    let step = Duration::from_millis(a.interval);
    let mut samples = 0u64;
    let mut instant_peak_ws = 0u64;
    let mut final_ws = 0u64;
    let mut final_private = 0u64;
    let mut per_process_peak: std::collections::HashMap<u32, (String, u64)> =
        std::collections::HashMap::new();
    let mut last_tree: Vec<(u32, String)> = Vec::new();
    let mut max_process_count = 0usize;

    while start.elapsed() < total {
        let table = process_table();
        let tree = tree_from(root, &table);
        last_tree = tree.clone();
        max_process_count = max_process_count.max(tree.len());
        let mut sum_ws = 0u64;
        let mut sum_private = 0u64;
        for (pid, name) in &tree {
            if let Some((ws, peak, private)) = read_memory(*pid) {
                sum_ws += ws;
                sum_private += private;
                let slot = per_process_peak
                    .entry(*pid)
                    .or_insert_with(|| (name.clone(), 0));
                slot.1 = slot.1.max(peak);
            }
        }
        instant_peak_ws = instant_peak_ws.max(sum_ws);
        final_ws = sum_ws;
        final_private = sum_private;
        samples += 1;
        std::thread::sleep(step);
    }

    if a.kill {
        if let Some(c) = child.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    let sum_of_peaks: u64 = per_process_peak.values().map(|(_, p)| *p).sum();
    let mut procs: Vec<String> = per_process_peak
        .iter()
        .map(|(pid, (name, peak))| {
            format!(
                "{{\"pid\":{},\"name\":\"{}\",\"peakWsKB\":{}}}",
                pid,
                name.replace('\\', "\\\\").replace('"', "\\\""),
                peak / 1024
            )
        })
        .collect();
    procs.sort();
    let json = format!(
        "{{\"rootPid\":{},\"durationMs\":{},\"intervalMs\":{},\"samples\":{},\"processCount\":{},\"maxProcessCount\":{},\"wsKB\":{},\"instantPeakWsKB\":{},\"peakWsKB\":{},\"privateKB\":{},\"tree\":[{}],\"processes\":[{}]}}",
        root,
        a.duration * 1000,
        a.interval,
        samples,
        last_tree.len(),
        max_process_count,
        final_ws / 1024,
        instant_peak_ws / 1024,
        sum_of_peaks / 1024,
        final_private / 1024,
        last_tree
            .iter()
            .map(|(p, n)| format!("{{\"pid\":{},\"name\":\"{}\"}}", p, n.replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(","),
        procs.join(",")
    );
    if !a.out.is_empty() {
        let _ = std::fs::write(&a.out, &json);
    }
    println!("{}", json);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_closure_finds_grandchildren() {
        let t = vec![
            (10, 1, "root.exe".into()),
            (11, 10, "child.exe".into()),
            (12, 11, "grand.exe".into()),
            (99, 1, "other.exe".into()),
        ];
        let tree = tree_from(10, &t);
        assert_eq!(tree.len(), 3);
        assert!(tree.iter().any(|(p, _)| *p == 12));
        assert!(!tree.iter().any(|(p, _)| *p == 99));
    }
}
