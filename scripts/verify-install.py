#!/usr/bin/env python3
"""图墙 NSIS 安装器的隔离验收（只读真实用户目录，绝不写桌面 / AppData）。

用法：
    python scripts/verify-install.py dist/PicLine-0.1.1-win-x64-setup-test.exe

验收项：
  1. 安装器 exe 的版本资源（中文 ProductName 不乱码）
  2. /S 静默安装到临时目录：主程序、卸载器、注册表项、两个快捷方式
  3. 卸载 /S：注册表清理、文件与快捷方式清理
  4. **卸载不删 %APPDATA%\\图墙**（安装前后只读快照比对 + 脚本静态检查）
  5. 真实桌面没有多出「图墙.lnk」（测试构建把快捷方式重定向到了安装目录内）
  6. portable 单文件在孤立目录可运行、托盘图标来自内嵌资源

输出 JSON 报告到 target/dist-verify/report.json；任一断言失败返回非 0。
"""

import ctypes
import ctypes.wintypes as w
import glob
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import time
import winreg

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DIST = os.path.join(ROOT, "dist")
REPORT = os.path.join(ROOT, "target", "dist-verify", "report.json")
UNINST_KEY = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\PicLineRust"
APP_KEY = r"Software\PicLineRust"
VERSION = "0.1.1"


def results_holder():
    return []


R = results_holder()


def check(name, ok, detail=""):
    R.append({"check": name, "pass": bool(ok), "detail": str(detail)})
    print(("  [PASS] " if ok else "  [FAIL] ") + name + (" :: " + str(detail) if detail else ""))
    return bool(ok)


def ver_value(path, key):
    """读 PE VERSIONINFO，自动按 VarFileInfo\\Translation 找语言/代码页。"""
    v = ctypes.windll.version
    sz = v.GetFileVersionInfoSizeW(path, None)
    if not sz:
        return None
    buf = ctypes.create_string_buffer(sz + 16)
    if not v.GetFileVersionInfoW(path, 0, sz, buf):
        return None
    out, ln = ctypes.c_void_p(), w.UINT()
    if not v.VerQueryValueW(buf, "\\VarFileInfo\\Translation", ctypes.byref(out), ctypes.byref(ln)):
        return None
    raw = ctypes.string_at(out.value, ln.value)
    for i in range(0, len(raw) - 3, 4):
        lang, cp = int.from_bytes(raw[i:i + 2], "little"), int.from_bytes(raw[i + 2:i + 4], "little")
        q = "\\StringFileInfo\\%04x%04x\\%s" % (lang, cp, key)
        o, l = ctypes.c_void_p(), w.UINT()
        if v.VerQueryValueW(buf, q, ctypes.byref(o), ctypes.byref(l)) and o.value:
            return ctypes.wstring_at(o.value, l.value).rstrip("\0")
    return None


def reg_values(key):
    try:
        h = winreg.OpenKey(winreg.HKEY_CURRENT_USER, key)
    except FileNotFoundError:
        return None
    d = {}
    i = 0
    while True:
        try:
            n, val, _ = winreg.EnumValue(h, i)
        except OSError:
            break
        d[n] = val
        i += 1
    winreg.CloseKey(h)
    return d


def appdata_snapshot():
    """只读快照：真实 %APPDATA%\\图墙 的文件列表 + 大小 + mtime + 内容哈希。"""
    p = os.path.join(os.environ.get("APPDATA", ""), "图墙")
    if not os.path.isdir(p):
        return None
    snap = {}
    for base, _, files in os.walk(p):
        for f in files:
            fp = os.path.join(base, f)
            try:
                st = os.stat(fp)
                h = hashlib.sha256(open(fp, "rb").read()).hexdigest() if st.st_size < 8 << 20 else "skip-large"
                snap[os.path.relpath(fp, p)] = [st.st_size, st.st_mtime_ns, h]
            except OSError as e:
                snap[os.path.relpath(fp, p)] = ["error", str(e)]
    return snap


def desktop_lnk_state():
    """常见桌面位置里「图墙.lnk」是否存在（只读）。"""
    cands = [os.path.join(os.environ.get("USERPROFILE", ""), "Desktop"),
             os.path.join(os.environ.get("USERPROFILE", ""), "OneDrive", "Desktop"),
             os.path.join(os.environ.get("PUBLIC", ""), "Desktop")]
    return {c: os.path.exists(os.path.join(c, "图墙.lnk")) for c in cands if c}


def wait_until(pred, timeout=15):
    end = time.time() + timeout
    while time.time() < end:
        if pred():
            return True
        time.sleep(0.25)
    return pred()


def main():
    setup = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        ROOT, "target", "dist-verify", "PicLine-%s-win-x64-setup-test.exe" % VERSION))
    if not os.path.isfile(setup):
        print("ERROR: 找不到隔离验收安装器，先跑 bash scripts/build-dist.sh --test-installer", file=sys.stderr)
        return 2
    os.makedirs(os.path.dirname(REPORT), exist_ok=True)
    print("安装器:", setup)

    # --- 1. 安装器自身版本资源 ---
    print("\n[1] 安装器版本资源")
    pn = ver_value(setup, "ProductName")
    fv = ver_value(setup, "FileVersion")
    check("I01 验收安装器中文 ProductName 不乱码", pn == "图墙", "ProductName=%r FileVersion=%r" % (pn, fv))
    official = os.path.join(DIST, "PicLine-%s-win-x64-setup.exe" % VERSION)
    opn = ver_value(official, "ProductName") if os.path.isfile(official) else None
    check("I01b 正式 setup.exe 版本资源中文正常（只读检查，不静默安装以免写真实桌面）",
          opn == "图墙", "ProductName=%r size=%s" % (opn, os.path.getsize(official) if os.path.isfile(official) else 0))

    # --- 2. 安装前基线（真实桌面 / AppData 只读） ---
    desktop_before = desktop_lnk_state()
    data_before = appdata_snapshot()
    print("\n[2] 安装前基线（只读）")
    print("    AppData/图墙:", "不存在" if data_before is None else "%d 个文件" % len(data_before))
    print("    桌面 图墙.lnk:", desktop_before)
    nsi = open(os.path.join(ROOT, "installer", "picline.nsi"), encoding="utf-8").read()
    # 只看有效代码行：注释里刻意写了一句「不写 RMDir /r ...」作为说明
    nsi_code = "\n".join(l for l in nsi.splitlines() if not l.lstrip().startswith(";"))
    check("I02 脚本静态检查：卸载段无删除 AppData 的语句",
          'RMDir /r "$APPDATA' not in nsi_code and 'Delete "$APPDATA' not in nsi_code)

    # --- 3. 静默安装 ---
    tmp = tempfile.mkdtemp(prefix="tq-inst-")
    inst = os.path.join(tmp, "installed")
    print("\n[3] 静默安装 /S ->", inst)
    r = subprocess.run([setup, "/S", "/D=" + inst], capture_output=True, timeout=120)
    print("    exit=", r.returncode)
    exe = os.path.join(inst, "picline-core.exe")
    uninst = os.path.join(inst, "Uninstall.exe")
    sc_desktop = os.path.join(inst, "_testshortcuts", "Desktop", "图墙.lnk")
    sc_start = os.path.join(inst, "_testshortcuts", "StartMenu", "图墙", "图墙.lnk")
    check("I03 安装器 exit=0 且主程序/卸载器落盘",
          r.returncode == 0 and os.path.isfile(exe) and os.path.isfile(uninst),
          "exe=%s uninst=%s" % (os.path.isfile(exe), os.path.isfile(uninst)))
    installed_exe = None
    if os.path.isfile(exe):
        installed_exe = hashlib.sha256(open(exe, "rb").read()).hexdigest()
        src = os.path.join(ROOT, "target", "release", "picline-core.exe")
        same = installed_exe == hashlib.sha256(open(src, "rb").read()).hexdigest()
        check("I04 安装出的 exe 与 release 二进制逐字节一致", same, installed_exe[:16])
        check("I05 安装版 exe 仍含内嵌图标",
              ctypes.windll.shell32.ExtractIconExW(exe, -1, None, None, 0) >= 1)
    check("I06 桌面 + 开始菜单快捷方式已创建（测试构建重定向目录）",
          os.path.isfile(sc_desktop) and os.path.isfile(sc_start),
          "desktop=%s start=%s" % (os.path.isfile(sc_desktop), os.path.isfile(sc_start)))
    ur = reg_values(UNINST_KEY) or {}
    ar = reg_values(APP_KEY) or {}
    check("I07 HKCU 卸载项字段完整",
          ur.get("DisplayName") == "图墙 (Rust 版)" and ur.get("DisplayVersion") == VERSION
          and "Uninstall.exe" in str(ur.get("UninstallString", "")) and ur.get("NoModify") == 1,
          json.dumps(ur, ensure_ascii=False)[:220])
    check("I08 安装位置记入 HKCU", os.path.normcase(ar.get("InstallLocation", "")) == os.path.normcase(inst),
          ar.get("InstallLocation"))
    check("I09 真实桌面未被写入快捷方式", desktop_lnk_state() == desktop_before,
          "%s -> %s" % (desktop_before, desktop_lnk_state()))

    # --- 4. 静默卸载 ---
    print("\n[4] 静默卸载 /S")
    r2 = subprocess.run([uninst, "/S"], capture_output=True, timeout=120) if os.path.isfile(uninst) else None
    print("    exit=", r2.returncode if r2 else "no uninstaller")
    gone = wait_until(lambda: not os.path.isfile(exe) and not os.path.isfile(uninst))
    check("I10 主程序与卸载器已删除", gone,
          "exe=%s uninst=%s" % (os.path.isfile(exe), os.path.isfile(uninst)))
    check("I11 快捷方式已删除", not os.path.isfile(sc_desktop) and not os.path.isfile(sc_start))
    check("I12 注册表项已清理", reg_values(UNINST_KEY) is None and reg_values(APP_KEY) is None)
    data_after = appdata_snapshot()
    check("I13 **卸载未改动 %APPDATA%\\图墙**", data_after == data_before,
          "before=%s after=%s" % ("不存在" if data_before is None else len(data_before),
                                  "不存在" if data_after is None else len(data_after)))
    check("I14 卸载后桌面状态未变", desktop_lnk_state() == desktop_before)

    # --- 5. portable 单文件在孤立目录 ---
    print("\n[5] portable 单文件")
    portable = os.path.join(DIST, "PicLine-%s-win-x64-portable.exe" % VERSION)
    if os.path.isfile(portable):
        iso = os.path.join(tmp, "isolated")
        os.makedirs(iso, exist_ok=True)
        pexe = os.path.join(iso, "portable.exe")
        import shutil
        shutil.copy2(portable, pexe)
        env = dict(os.environ)
        env.update({"TQ_DATA_DIR": os.path.join(tmp, "data"),
                    "TQ_SHOTS_DIR": os.path.join(tmp, "shots"),
                    "TQ_TRACE": os.path.join(tmp, "trace.jsonl")})
        # --probe 会真正起 GUI 并驻留 12 秒，能顺带验证托盘图标来自内嵌资源
        try:
            p = subprocess.run([pexe, "--probe", os.path.join(tmp, "probe.json")],
                               capture_output=True, timeout=40, env=env)
            trace = open(env["TQ_TRACE"], encoding="utf-8", errors="replace").read() \
                if os.path.isfile(env["TQ_TRACE"]) else ""
            src_ok = '"ev":"icon_source","detail":"resource"' in trace
            check("I15 portable 单文件在孤立目录可跑完整 GUI（12s 探针）", p.returncode == 0,
                  "exit=%d" % p.returncode)
            check("I16 托盘图标来自 exe 内嵌资源（无外部 ico）", src_ok,
                  "icon_source events=%d" % trace.count("icon_source"))
        except subprocess.TimeoutExpired:
            check("I15 portable 单文件在孤立目录可跑完整 GUI（12s 探针）", False, "timeout")
        check("I17 孤立目录没有产生任何依赖文件",
              sorted(os.listdir(iso)) == ["portable.exe"], os.listdir(iso))
        check("I18 portable exe 含内嵌图标与中文版本资源",
              ctypes.windll.shell32.ExtractIconExW(pexe, -1, None, None, 0) >= 1
              and ver_value(pexe, "ProductName") == "图墙")
    else:
        check("I15 portable 产物存在", False, portable)

    passed = sum(1 for x in R if x["pass"])
    report = {"setup": setup, "install_dir": inst, "pass": passed, "total": len(R),
              "results": R, "appdata_snapshot_unchanged": data_after == data_before}
    open(REPORT, "w", encoding="utf-8").write(json.dumps(report, ensure_ascii=False, indent=2))
    print("\n==== 分发物验收：%d/%d PASS ====" % (passed, len(R)))
    print("report:", REPORT)
    if os.environ.get("TQ_KEEP_INSTALL_TMP") != "1":
        import shutil
        shutil.rmtree(tmp, ignore_errors=True)  # 只删本次验证自己创建的临时目录
    else:
        print("临时目录保留:", tmp)
    return 0 if passed == len(R) else 1


if __name__ == "__main__":
    sys.exit(main())
