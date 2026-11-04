#!/usr/bin/env bash
# 图墙 / PicLine —— 分发物构建
#
# 产物（名字与形态对齐原版 electron-builder 的 artifactName）：
#   dist/PicLine-0.1.1-win-x64-setup.exe      NSIS 安装版
#   dist/PicLine-0.1.1-win-x64-portable.exe  真·单文件（图标/版本信息已内嵌，无需 assets/）
#   dist/PicLine-0.1.1-win-x64-portable.zip  便携 zip（可选分发渠道）
#   dist/SHA256SUMS.txt
#
# 用法：
#   bash scripts/build-dist.sh                  正式构建
#   bash scripts/build-dist.sh --test-installer  额外产出 -setup-test.exe 供隔离安装验收
#
# 依赖（本机均已具备，无需联网）：
#   cargo(msvc) / Windows SDK rc.exe（由 build.rs 调）
#   --test-installer 会额外把隔离验收用安装器放到 target/dist-verify/，
#   **不会污染 dist/**（那个宏把快捷方式重定向，绝不能当正式产物分发）。
#
# 依赖（本机均已具备，无需联网）：
#   cargo(msvc) / Windows SDK rc.exe（由 build.rs 调）
#   NSIS makensis.exe（优先 TQ_MAKENSIS，其次 electron-builder 缓存）
#   7z / NanaZip（可选，只影响 zip）

set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CARGO="${CARGO:-$HOME/.cargo/bin/cargo.exe}"
if [[ ! -x "$CARGO" ]]; then CARGO="$(command -v cargo || true)"; fi
[[ -n "$CARGO" ]] || { echo "ERROR: 找不到 cargo" >&2; exit 1; }

VERSION="$(grep -m1 '^version[[:space:]]*=' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
[[ -n "$VERSION" ]] || { echo "ERROR: 读不到 Cargo.toml 版本" >&2; exit 1; }
PREFIX="PicLine-${VERSION}-win-x64"
DIST="$ROOT/dist"
mkdir -p "$DIST" "$ROOT/installer/.build"

echo "==> [1/6] cargo test + release build (v$VERSION)"
"$CARGO" test --quiet
"$CARGO" build --release --quiet

SRC="$ROOT/target/release/picline-core.exe"
[[ -f "$SRC" ]] || { echo "ERROR: $SRC 不存在" >&2; exit 1; }

echo "==> [2/6] 校验 exe 内嵌资源（图标 + 中文版本信息）"
python - "$(cygpath -w "$SRC")" <<'PY'
import ctypes, ctypes.wintypes as w, sys
p = sys.argv[1]
sh = ctypes.windll.shell32
n = sh.ExtractIconExW(p, -1, None, None, 0)
assert n >= 1, "exe 缺少内嵌图标资源（rc.exe 没链接成功？）"
v = ctypes.windll.version
sz = v.GetFileVersionInfoSizeW(p, None)
assert sz > 0, "exe 缺少 VERSIONINFO 资源"
buf = ctypes.create_string_buffer(sz + 16)
assert v.GetFileVersionInfoW(p, 0, sz, buf)
def val(k):
    out, ln = ctypes.c_void_p(), w.UINT()
    ok = v.VerQueryValueW(buf, "\\StringFileInfo\\080404b0\\" + k,
                          ctypes.byref(out), ctypes.byref(ln))
    return ctypes.wstring_at(out.value, ln.value).rstrip("\0") if ok and out.value else ""
assert val("ProductName") == "图墙", "版本资源中文乱码或缺失: " + val("ProductName")
print(f"    icons={n} ProductName={val('ProductName')} FileVersion={val('FileVersion')}")
PY

echo "==> [3/6] portable 单文件 + zip"
cp -f "$SRC" "$DIST/${PREFIX}-portable.exe"
if command -v 7z >/dev/null 2>&1; then
  ( cd "$DIST" && 7z a -bso0 -bsp0 -y "${PREFIX}-portable.zip" "${PREFIX}-portable.exe" >/dev/null ) || true
fi

echo "==> [4/6] 定位 makensis"
MAKENSIS="${TQ_MAKENSIS:-}"
if [[ -z "$MAKENSIS" ]]; then
  MAKENSIS="$(python - <<'PY'
import glob, os
base = os.environ.get("LOCALAPPDATA", "")
pats = [
    os.path.join(base, "electron-builder", "Cache", "nsis-*", "nsis-*", "Bin", "makensis.exe"),
    os.path.join(base, "electron-builder", "Cache", "nsis-*", "makensis.exe"),
]
hits = sorted(h for p in pats for h in glob.glob(p))
print(hits[-1] if hits else "")
PY
  )"
fi
if [[ -z "$MAKENSIS" ]] && command -v makensis >/dev/null 2>&1; then
  MAKENSIS="$(command -v makensis)"
fi
[[ -n "$MAKENSIS" ]] || { echo "ERROR: 找不到 makensis.exe（可设 TQ_MAKENSIS=<路径>）" >&2; exit 1; }
MAKENSIS_U="$(cygpath -u "$MAKENSIS")"
[[ -x "$MAKENSIS_U" ]] || { echo "ERROR: makensis 不可执行: $MAKENSIS" >&2; exit 1; }
echo "    $MAKENSIS"

# NSIS 3 在中文 Windows 上靠 BOM 识别 UTF-8 脚本；源文件保持无 BOM 便于编辑，
# 这里生成带 BOM 的临时副本。/D 参数以 / 开头，必须关掉 MSYS 的路径转换。
NSI="$ROOT/installer/picline.nsi"
NSI_BUILD="$ROOT/installer/.build/picline.build.nsi"
python - "$NSI" "$NSI_BUILD" <<'PY'
import sys
src = open(sys.argv[1], "rb").read()
open(sys.argv[2], "wb").write(src if src.startswith(b"\xef\xbb\xbf") else b"\xef\xbb\xbf" + src)
PY

WIN_SRC="$(cygpath -w "$SRC")"
WIN_ICON="$(cygpath -w "$ROOT/assets/icon.ico")"
WIN_NSI="$(cygpath -w "$NSI_BUILD")"

compile_installer() {
  local outfile="$1"; shift
  local win_out; win_out="$(cygpath -w "$outfile")"
  echo "    -> $(basename "$outfile")"
  MSYS2_ARG_CONV_EXCL='*' MSYS_NO_PATHCONV=1 "$MAKENSIS_U" /V2 /NOCD \
    "/DAPP_VERSION=$VERSION" \
    "/DSRCEXE=$WIN_SRC" \
    "/DOUTFILE=$win_out" \
    "/DAPP_ICON=$WIN_ICON" \
    "$@" \
    "$WIN_NSI"
}

echo "==> [5/6] 编译 NSIS 安装版"
compile_installer "$DIST/${PREFIX}-setup.exe"
if [[ "${1:-}" == "--test-installer" ]]; then
  echo "    (额外产出隔离验收用安装器到 target/dist-verify/，不放进 dist)"
  VERIFY_DIR="$ROOT/target/dist-verify"
  mkdir -p "$VERIFY_DIR"
  compile_installer "$VERIFY_DIR/${PREFIX}-setup-test.exe" "/DTQ_TEST_INSTALL=1"
fi

echo "==> [6/6] 校验和 + 产物清单"
( cd "$DIST" && sha256sum "${PREFIX}"-*.exe "${PREFIX}"-*.zip 2>/dev/null > SHA256SUMS.txt || true )
ls -lh "$DIST"
echo "DONE: $DIST"
