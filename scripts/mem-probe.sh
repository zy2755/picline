#!/bin/bash
# 内存取样:跑一次应用,12 秒后自己把 app.getAppMetrics() 写盘并退出。
# 用法: scripts/mem-probe.sh <标签> [额外的 chromium 参数...]
set -u
cd "$(dirname "$0")/.." || exit 1

LABEL="$1"; shift
OUT="$TEMP/tq-mem-$LABEL.json"
UD="$TEMP/tq-mem-ud-$LABEL"
rm -rf "$UD" "$OUT"

# Electron 相关环境变量必须清掉:ELECTRON_RUN_AS_NODE=1 会让主程序按 node 跑
env -u ELECTRON_RUN_AS_NODE -u NODE_OPTIONS \
  TQ_MEMPROBE="$(cygpath -w "$OUT")" TQ_DATA_DIR="$(cygpath -w "$UD")" \
  ./node_modules/electron/dist/electron.exe . --user-data-dir="$(cygpath -w "$UD")" "$@" >/dev/null 2>&1

if [ ! -f "$OUT" ]; then echo "$LABEL: 取样失败(未生成 $OUT)"; exit 1; fi
node -e "
const m = require(process.argv[1]);
const total = m.reduce((s, p) => s + p.wsKB, 0);
const by = {};
for (const p of m) by[p.type] = (by[p.type] || 0) + p.wsKB;
console.log('$LABEL'.padEnd(18), (total / 1024).toFixed(1).padStart(6) + ' MB',
  ' 进程数 ' + String(m.length).padStart(2),
  ' | ' + Object.entries(by).map(([k, v]) => k + ' ' + (v / 1024).toFixed(0) + 'M').join(', '));
" "$OUT"
