#!/bin/bash
# 统一口径内存对比：Electron v0.1.1 基线 vs Rust / Win32+GDI+ 版
# 两侧都用 memtree 的 K32GetProcessMemoryInfo(PROCESS_MEMORY_COUNTERS_EX)，
# 250ms 采样，覆盖进程树（含 Electron 拉起的 PowerShell/conhost 子进程）。
#
# 用法: bash scripts/compare-mem.sh [s0|s1] [秒数]
#   s0 = 收起态空收件箱（对应原 mem-probe.sh 的场景）
#   s1 = 收件箱放 10 张 1920x1080 测试图；Rust 侧额外强制展开（更严苛）
set -u
ROOT="C:/Users/hou33/WorkBuddy/图墙"
SRC="D:/Projects/图墙"
MEMTREE="$ROOT/target/release/memtree.exe"
SCENE="${1:-s0}"
DUR="${2:-12}"

if [ ! -f "$MEMTREE" ]; then echo "缺少 $MEMTREE，请先 cargo build --release"; exit 1; fi

OUT="$(mktemp -d)"
OUTW="$(cygpath -w "$OUT")"
E_DATA="$OUTW\\e-data"
R_DATA="$OUTW\\r-data"
mkdir -p "$OUT/e-data/inbox" "$OUT/r-data/inbox" "$OUT/e-data/Screenshots" "$OUT/r-data/Screenshots"

if [ "$SCENE" = "s1" ]; then
  node "$ROOT/scripts/make-test-photos.js" "$OUT/e-data/inbox" 10 >/dev/null
  cp "$OUT/e-data/inbox"/*.png "$OUT/r-data/inbox/"
  echo "S1: 两侧各放 10 张 1920x1080 PNG"
fi

echo "[$SCENE] 采样 Electron ${DUR}s ..."
(
  cd "$SRC" || exit 1
  env -u ELECTRON_RUN_AS_NODE -u NODE_OPTIONS \
    TQ_DATA_DIR="$E_DATA" TQ_MEMPROBE="$E_DATA\\probe.json" \
    "$MEMTREE" --duration "$DUR" --interval 250 --out "$OUT/e.json" -- \
    ./node_modules/electron/dist/electron.exe . --user-data-dir="$E_DATA" >/dev/null 2>&1
)
sleep 1  # 等单实例锁彻底释放，避免下一场景被静默挡掉

echo "[$SCENE] 采样 Rust GUI ${DUR}s ..."
(
  cd "$ROOT" || exit 1
  if [ "$SCENE" = "s1" ]; then export TQ_FORCE_SHOW=1; fi
  env TQ_DATA_DIR="$R_DATA" TQ_SHOTS_DIR="$R_DATA\\Screenshots" \
    "$MEMTREE" --duration "$DUR" --interval 250 --out "$OUT/r.json" -- \
    "$ROOT/target/release/picline-core.exe" --probe "$R_DATA\\probe.json" >/dev/null 2>&1
)

if [ ! -f "$OUT/e.json" ] || [ ! -f "$OUT/r.json" ]; then
  echo "采样失败，原始数据目录: $OUT"
  exit 1
fi

node -e '
const fs = require("fs");
const e = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const r = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const mb = (kb) => (kb / 1024).toFixed(1);
const row = (name, m) => `${name.padEnd(10)} ws=${mb(m.wsKB).padStart(7)}MB  instantPeak=${mb(m.instantPeakWsKB).padStart(7)}MB  processPeak=${mb(m.peakWsKB).padStart(7)}MB  private=${mb(m.privateKB).padStart(7)}MB  procs=${m.maxProcessCount}`;
console.log("");
console.log(row("Electron", e));
console.log(row("Rust", r));
console.log("");
const dWs = (1 - r.wsKB / e.wsKB) * 100;
const dPeak = (1 - r.peakWsKB / e.peakWsKB) * 100;
console.log(`运行期工作集降幅 ${dWs.toFixed(1)}%   峰值工作集降幅 ${dPeak.toFixed(1)}%`);
const t1 = r.wsKB <= 60 * 1024;
const t2 = r.peakWsKB <= 120 * 1024 && dPeak >= 60;
const t3 = r.maxProcessCount <= 1;
console.log(`T1 运行期<=60MB: ${t1 ? "PASS" : "FAIL"}   T2 峰值<=120MB且降幅>=60%: ${t2 ? "PASS" : "FAIL"}   T3 进程数<=1: ${t3 ? "PASS" : "FAIL"}`);
console.log(`samples=${e.samples}/${r.samples}  raw=${process.argv[3]}`);
' "$OUT/e.json" "$OUT/r.json" "$OUT"
