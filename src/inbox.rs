//! 收件箱 = 晾衣绳的状态：文件在 = 挂在绳上。
//!
//! 等价目标：lib/capture.js 的 Capture.listInbox()
//!   - 只认 .png/.jpg/.jpeg/.bmp/.webp（大小写不敏感）
//!   - 非文件跳过；stat 失败静默跳过
//!   - 按 mtimeMs 升序（旧 -> 新，渲染端把新的挂右端）
//!   - 返回 {id, file, mtime, size, seq}，seq 为排序后的下标
//!   - 读目录失败返回空数组（不报错）

use crate::settings::json_escape;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const IMAGE_EXT: [&str; 5] = [".png", ".jpg", ".jpeg", ".bmp", ".webp"];

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub file: PathBuf,
    pub mtime_ms: f64,
    pub size: u64,
    pub seq: usize,
}

pub fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    IMAGE_EXT.iter().any(|e| lower.ends_with(e))
}

pub fn list_inbox(dir: &Path) -> Vec<Item> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut items: Vec<Item> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_image_name(&name) {
            continue;
        }
        let md = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !md.is_file() {
            continue;
        }
        let mtime_ms = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        items.push(Item {
            id: name,
            file: entry.path(),
            mtime_ms,
            size: md.len(),
            seq: 0,
        });
    }
    // Rust 的 sort_by 是稳定排序，与 V8 Array#sort（稳定）一致
    items.sort_by(|a, b| a.mtime_ms.partial_cmp(&b.mtime_ms).unwrap_or(std::cmp::Ordering::Equal));
    for (i, it) in items.iter_mut().enumerate() {
        it.seq = i;
    }
    items
}

/// 与 ipcMain.handle('items:get') / broadcast('items') 的序列化结果一致
pub fn items_to_json(items: &[Item]) -> String {
    let mut out = String::from("[");
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"id\":\"{}\",\"file\":\"{}\",\"mtime\":{},\"size\":{},\"seq\":{}}}",
            json_escape(&it.id),
            json_escape(&it.file.to_string_lossy()),
            format_num(it.mtime_ms),
            it.size,
            it.seq
        ));
    }
    out.push(']');
    out
}

fn format_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_filter_is_case_insensitive_and_exact() {
        assert!(is_image_name("a.PNG"));
        assert!(is_image_name("shot.jpeg"));
        assert!(is_image_name("x.webp"));
        assert!(!is_image_name("a.txt"));
        assert!(!is_image_name("a.png.bak"));
        assert!(!is_image_name("png"));
    }

    #[test]
    fn missing_dir_is_an_empty_list_not_an_error() {
        assert!(list_inbox(Path::new("Z:/definitely/not/here")).is_empty());
    }

    #[test]
    fn items_are_sorted_old_to_new_with_sequential_seq() {
        let dir = std::env::temp_dir().join(format!("picline-inbox-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let a = dir.join("a.png");
        let b = dir.join("b.jpg");
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"bb").unwrap();
        fs::write(dir.join("ignore.txt"), b"x").unwrap();
        let items = list_inbox(&dir);
        assert_eq!(items.len(), 2);
        assert!(items[0].mtime_ms <= items[1].mtime_ms);
        assert_eq!(items[0].seq, 0);
        assert_eq!(items[1].seq, 1);
        let json = items_to_json(&items);
        assert!(json.starts_with("[{\"id\":"));
        assert!(json.contains("\"seq\":1}"));
        let _ = fs::remove_dir_all(&dir);
    }
}
