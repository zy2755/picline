#!/bin/bash
# 录制 README 顶部的演示 GIF。
#
# 流程:临时数据目录 + 代码生成的"假截图" → 起一块干净的渐变"舞台"窗口当背景
#      → 启动应用 → 真实移动光标(顶边悬停呼出 → 悬停相片 → 单击复制 → 移开收起)
#      → 逐帧截图 → ffmpeg 合成调色板优化的 GIF。
#
# 不碰你真实的收件箱、壁纸和桌面:背景是 scripts/demo-backdrop.js 画的窗口。
#
# 用法: bash scripts/make-demo-gif.sh [输出文件]
set -eu
cd "$(dirname "$0")/.." || exit 1

OUT="${1:-docs/demo.gif}"
# 统一用 POSIX 风格的临时路径,需要 Windows 路径时再 cygpath -w
TMP="$(cygpath -u "${TEMP:-/tmp}")/tq-demo"
DATA="$TMP/data"
UD="$TMP/userdata"
STAGE_UD="$TMP/stage-ud"
FRAMES="$TMP/frames"
FPS=12
HEIGHT=0            # 0 = 由录制器按 DPI 自动算(面板 210 + 余量)

rm -rf "$TMP"
mkdir -p "$DATA/inbox" "$UD" "$STAGE_UD" "$FRAMES" "$(dirname "$OUT")"

# 关掉截图接管与剪贴板监听:录制时别去碰真实的 Screenshots 文件夹
cat > "$DATA/config.json" <<'JSON'
{
  "showLine": true,
  "takeOver": false,
  "clipboardWatch": false,
  "maxPegs": 10,
  "firstRunDone": true
}
JSON

node scripts/make-demo-photos.js "$DATA/inbox" 5 >/dev/null

echo "起背景板…"
env -u ELECTRON_RUN_AS_NODE -u NODE_OPTIONS \
  ./node_modules/electron/dist/electron.exe scripts/demo-backdrop.js \
  --user-data-dir="$(cygpath -w "$STAGE_UD")" >/dev/null 2>&1 &
STAGE_PID=$!
sleep 2

echo "启动应用(隔离数据目录)…"
env -u ELECTRON_RUN_AS_NODE -u NODE_OPTIONS \
  TQ_DATA_DIR="$(cygpath -w "$DATA")" \
  ./node_modules/electron/dist/electron.exe . \
  --user-data-dir="$(cygpath -w "$UD")" >/dev/null 2>&1 &
APP_PID=$!
sleep 3

echo "录制中…"
python scripts/record-demo.py --out "$(cygpath -w "$FRAMES")" --fps "$FPS" --height "$HEIGHT"

kill "$APP_PID" "$STAGE_PID" 2>/dev/null || true

echo "合成 GIF…"
ffmpeg -y -v error -framerate "$FPS" -i "$FRAMES/f%03d.bmp" \
  -vf "scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=160:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle" \
  -loop 0 "$OUT"

ls -lh "$OUT"
python - "$(cygpath -w "$OUT")" <<'PY'
import sys, struct
p = sys.argv[1]
d = open(p, 'rb').read(10)
w, h = struct.unpack('<HH', d[6:10])
print(f'GIF 尺寸 {w}x{h}')
PY
