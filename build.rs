//! 把 Win32 资源（应用图标 + 中文 VERSIONINFO）编译进所有可执行文件。
//!
//! 设计约束：
//! - 零外部 crate（与整个项目一致），只用 std + Windows SDK 自带 rc.exe。
//! - 资源是「锦上添花」：找不到 rc.exe 时只打 warning，不阻塞编译，
//!   此时托盘图标回退到源码树里的 assets/icon.ico（开发期可用）。
//! - .rc 源码用 UTF-8 保存便于阅读/修改，但 rc.exe 在中文 Windows 上按本地
//!   代码页解析，所以这里统一转成 UTF-16LE + BOM 再交给它，中文版本信息不会乱码。
//!
//! 可用环境变量：
//!   TQ_RC=<路径>          手动指定 rc.exe
//!   TQ_SKIP_RESOURCES=1   跳过资源编译（排查链接问题时用）

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=installer/picline.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-env-changed=TQ_RC");
    println!("cargo:rerun-if-env-changed=TQ_SKIP_RESOURCES");

    if env::var_os("TQ_SKIP_RESOURCES").is_some() {
        println!("cargo:warning=TQ_SKIP_RESOURCES=1，跳过图标/版本资源");
        return;
    }
    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("msvc") {
        // 非 MSVC 目标（例如 gnu）没有可用的 rc.exe / link.exe 组合
        println!("cargo:warning=目标 {target} 不是 msvc，跳过 Win32 资源");
        return;
    }

    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let icon = root.join("assets").join("icon.ico");
    let template = root.join("installer").join("picline.rc");
    if !icon.is_file() || !template.is_file() {
        println!("cargo:warning=缺少 assets/icon.ico 或 installer/picline.rc，跳过资源");
        return;
    }

    let Some(rc) = find_rc() else {
        println!("cargo:warning=未找到 Windows SDK rc.exe（可设 TQ_RC=<路径>），跳过资源；托盘将回退到外部 ico");
        return;
    };

    // rc.exe 对路径中的反斜杠敏感，统一转正斜杠并用绝对路径
    let icon_abs = icon.to_string_lossy().replace('\\', "/");
    let text = match fs::read_to_string(&template) {
        Ok(s) => s.replace("@ICON_PATH@", &icon_abs),
        Err(e) => {
            println!("cargo:warning=读取 {template:?} 失败：{e}");
            return;
        }
    };

    // UTF-8 源 -> UTF-16LE with BOM，保证中文字符串被 rc.exe 正确解析
    let mut u16bytes: Vec<u8> = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        u16bytes.extend_from_slice(&u.to_le_bytes());
    }
    let rc_file = out.join("picline.rc");
    let res_file = out.join("picline.res");
    if let Err(e) = fs::write(&rc_file, &u16bytes) {
        println!("cargo:warning=写临时 .rc 失败：{e}");
        return;
    }

    let status = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_file)
        .arg(&rc_file)
        .output();
    let output = match status {
        Ok(o) => o,
        Err(e) => {
            println!("cargo:warning=运行 rc.exe 失败：{e}");
            return;
        }
    };
    if !output.status.success() || !res_file.is_file() {
        println!(
            "cargo:warning=rc.exe 编译资源失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return;
    }

    // link.exe 可以直接吃 .res。只给主程序挂资源：memtree / gesture 是命令行
    // 工具，不需要图标，挂上会白胖 370 KB，也会拉高验收工具自身的内存基线。
    println!("cargo:rustc-link-arg-bin=picline-core={}", res_file.display());
}

/// 依次尝试：TQ_RC -> PATH(where.exe) -> Windows Kits\10\bin\<ver>\x64\rc.exe
fn find_rc() -> Option<PathBuf> {
    if let Ok(p) = env::var("TQ_RC") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(o) = Command::new("where.exe").arg("rc.exe").output() {
        if o.status.success() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let p = PathBuf::from(line.trim());
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    let kits = env::var("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\Program Files (x86)"))
        .join("Windows Kits")
        .join("10")
        .join("bin");
    let mut versions: Vec<PathBuf> = match fs::read_dir(&kits) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect(),
        Err(_) => return None,
    };
    versions.sort(); // 版本号形如 10.0.26100.0，字典序即可，取最大
    for v in versions.iter().rev() {
        // rc.exe 是主机工具，但生成的 .res 需与目标架构一致，优先 x64
        for arch in ["x64", "arm64", "x86"] {
            let p: PathBuf = v.join(arch).join("rc.exe");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}
