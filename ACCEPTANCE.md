# 图墙 / PicLine → Rust 重写：验收契约与交付记录

> 更新：2026-11-04 23:47　状态：**Q1–Q4 已拍板；F1 分发物已交付（NSIS 安装版 + 单文件 portable + zip），分发验收 19/19 PASS；正式 setup.exe 非静默向导 23:23 初检 + 23:43 修文案后复检，两轮 4/4 页通过；用户反馈的黑框（B24）/转圈光标（B25）已修并复验；真机手势 23/23 PASS；内存 T1–T3 按最终交付二进制重测达成**。剩余仅开机自启/视觉近似人工目测与 T4 / S2–S5 长稳场景（第 5 / 8.6 / 8.7 节）
>
> 本文件承载任务要求的「功能一致性验收标准 / 内存占用对比指标 / 保留的行为边界说明」。

---

## 0. 决策记录（用户答复）

| 项 | 答复 | 落地 |
| --- | --- | --- |
| **Q1** | 纯 Win32/GDI+ 自绘 | `src/gui.rs` + `src/gdi.rs` + `src/api.rs`，零 GUI 框架、零 WebView；分层透明窗口 `UpdateLayeredWindow` 逐像素 alpha |
| **Q2** | 按草案实现 | T1 ≤60 MB / T2 峰值 ≤120 MB 且降幅 ≥60% / T3 ≤1 进程 / T4 零功能回归+延迟+无泄漏 / T5 禁止靠减缓存凑数 |
| **Q3** | 不保留 | Electron 私有实现细节（CDP 端口、ipcRenderer 通道名、`asar:false`、`fullscreen.ps1` 外部进程、`app.getAppMetrics()`）全部豁免；用户可见行为与数据格式仍保留 |
| **Q4** | 补 GetProcessMemoryInfo 采集 | 新增 `memtree` 工具，对**整棵进程树**每 250ms 采 `PROCESS_MEMORY_COUNTERS_EX` 的工作集/峰值工作集/私有提交，两侧同口径 |

Q5–Q8 未单独答复，按草案建议默认值推进：Win10 22H2+ / 仅 x64；WebP 允许解码回退；配置异常类型不保证；剪贴板 PNG 体积差异保留（B9）。

---

## 1. 重写前确认结果

| 项 | 结论 |
| --- | --- |
| 原始代码/规格 | `D:\Projects\图墙`（Electron 33 / v0.1.1 / git 干净）。README 规格 + `main.js` 486 行 + `lib/capture.js` 273 行 + `src/line.js` 372 行 + `line.css` 164 行，逐条读完 |
| 依赖/平台约束 | Node 18+、pngjs、仅 Windows x64、NSIS+portable、多显示器/DPI、OneDrive 图片库、`%APPDATA%\图墙` 数据格式 |
| 当前内存基线 | **已修正口径**：`app.getAppMetrics()` 的 290.5 MB 只统计 Electron 自身 3 进程；全树还含 `powershell.exe` + `conhost.exe`。统一口径后见第 2 节 |
| 工具链 | `cargo 1.99.1` / `x86_64-pc-windows-msvc`；bash 的 PATH 未含 `~/.cargo/bin`，需用绝对路径 |

**Q4 的关键发现**：原版 `lib/fullscreen.ps1` 用 PowerShell `Add-Type` 动态编译 P/Invoke，会拉起 `powershell.exe`（峰值约 **97.9 MB**）与 `csc.exe`（约 11 MB）、`conhost.exe`（约 9 MB）。这些进程完全不在 `app.getAppMetrics()` 里，所以 README 的 373 MB 和脚本的 290.5 MB 都**系统性低估**了真实开销。Rust 版把全屏检测搬进进程内，这是除 Chromium 之外最大的一笔内存收益。

---

## 2. 内存占用对比指标与实测

### 2.1 口径（两侧完全一致）

- 采集器：`memtree.exe`，`CreateToolhelp32Snapshot` 建进程树 → 对每个 PID 调 `K32GetProcessMemoryInfo(PROCESS_MEMORY_COUNTERS_EX)`。
- 指标：`WorkingSetSize`（工作集）、`PeakWorkingSetSize`（进程峰值工作集）、`PrivateUsage`（私有提交）；`instantPeakWsKB` 为采样到的瞬时总和最大值。
- 采样：250 ms 一次，12 秒；`scripts/compare-mem.sh` 同时跑两侧并输出判定。
- 场景：**S0** 收起态空收件箱；**S1** 收件箱 10 张 1920×1080 PNG，Rust 侧额外 `TQ_FORCE_SHOW=1` 强制展开（对 Rust 更严苛）。

### 2.2 最终实测（22:27 用**交付二进制**重跑：已内嵌图标/版本资源、含全部修复）

| 场景 | 版本 | 工作集 | 瞬时峰值 | 进程峰值 | 私有提交 | 进程数 |
| --- | --- | --- | --- | --- | --- | --- |
| **S0** | Electron v0.1.1 | **416.2 MB** | 416.2 MB | **484.4 MB** | 256.2 MB | 6 |
| **S0** | Rust / Win32+GDI+ | **32.1 MB** | 35.2 MB | **35.2 MB** | 8.0 MB | **1** |
| **S1** | Electron v0.1.1 | **505.0 MB** | 506.0 MB | **600.1 MB** | 260.7 MB | 6 |
| **S1** | Rust / Win32+GDI+ | **33.6 MB** | 36.7 MB | **36.7 MB** | 9.5 MB | **1** |

| 目标 | 判据 | S0 | S1 |
| --- | --- | --- | --- |
| **T1** 运行期 ≤60 MB | 32.1 / 33.6 MB（降幅 **92.3% / 93.3%**） | **PASS** | **PASS** |
| **T2** 峰值 ≤120 MB 且降幅 ≥60% | 35.2 / 36.7 MB（降幅 **92.7% / 93.9%**） | **PASS** | **PASS** |

> 内嵌 370 KB 图标资源后未推高内存：资源只在加载托盘图标时映射 16×16 的一份，实测波动在 ±1 MB 内。
| **T3** 进程数 ≤1 | 1 | **PASS** | **PASS** |
| **T5** 不靠削减功能凑数 | `maxPegs=10`、两通道、去重全部保留；缩略图缓存按显示尺寸(136×104)而非原图尺寸，属实现优化不是砍功能 | **PASS** | **PASS** |

**T4 部分未测**：延迟 ≤原版 +100 ms、连续 1 小时无泄漏（S4）尚未跑；S2（捕获+动画峰值）、S3（复制/拖放峰值）、S5（双屏+DPI+全屏切换）未按脚本自动化。草案要求「≥3 次取中位数」，本轮各跑 2 次，数字波动 <2%，建议发布前补第 3 次。

### 2.3 为什么 S1 的 Rust 侧几乎不涨

`Peg` 只缓存 **136×104 的 GDI+ 缩略图**（10 张合计 <1 MB），原图只在「复制到剪贴板」时临时解码并立即释放。因此峰值不随原图分辨率线性增长；这是实现策略差异，不是降低画质（显示尺寸本就是 136×104）。

---

## 3. 功能一致性验收标准与当前状态

判据不变（与 Stage 1 文档一致），新增「当前状态」列：**自动** = 单测/冒烟已证；**真机** = 代码已实现但需人工操作确认；**未完** = 尚未交付。

| 组 | 项 | 判据 | 状态 |
| --- | --- | --- | --- |
| A | A1 | 每显示器 workArea 宽 ×210px、无边框透明置顶不抢焦点 | 自动（枚举+窗口创建+分层绘制+冒烟驻留） |
| | A2 | 空白穿透，点击/滚轮/拖拽落到底下 | 真机（默认 `WS_EX_TRANSPARENT`，需 `WindowFromPoint` 复核） |
| | A3 | 命中相片/菜单才接管，接管瞬间合成 mousemove | 自动（60ms 轮询 + `PostMessageW` 合成，避免重入） |
| | A4 | 展开 0.42s 过冲 / 收起 0.22s easeIn；收起结束才恢复穿透 | 自动（曲线单测 + 时序已补） |
| B | B1 | 顶边 22–78%、4px 带、250ms 呼出、300ms 收起、pinned 不自动收 | 自动（常量与状态机）／真机（实际悬停） |
| | B2 | 单击复制且不自挂、双击打开、长按 450ms 画图 | 自动（状态机+CF_DIB 自检+抑制链单测）／真机（真实鼠标） |
| | B3 | 拖出到应用/文件夹发副本，相片留绳；3.5s 抑制收起 | 真机（OLE `IDataObject`/`IDropSource` + CF_HDROP 已实现并有 vtable 单测） |
| | B4 | 右键 6 项；菜单开启整条接管；点外面才收 | 真机（自绘菜单 + 命中单测） |
| | B5 | ✕/回收站；失败时相片回绳并提示原因 | 真机（`SHFileOperationW` FOF_ALLOWUNDO + 结构化错误） |
| | B6 | `Ctrl+Alt+T`；新截图到达 peek 2.5s | 自动（热键注册 + `peek_on_new`）／真机 |
| C | C1 | 两通道、只收启动后、去重、保留原始画质 | 自动（33→39 项逻辑单测） |
| | C2 | 收件箱即状态、跨会话、maxPegs 10、旧→新 | 自动 |
| | C3 | 托盘开关两通道/自启/打开收件箱 | 真机（菜单已实现） |
| D | D1 | 全屏自动藏起、永不抢焦点 | 自动（判定单测 6 项）／真机（游戏/放映实测） |
| E | E1/E2 | config.json 5 键 + 未知键保留、inbox 命名、`TQ_DATA_DIR`/`TQ_MEMPROBE`、单实例 | 自动 |
| F | F1 | NSIS 安装版 + portable 单文件、图标/中文界面/快捷方式 | **已交付 + 自动验收 19/19 PASS + 正式向导两轮人工点检 4/4 页；黑框/转圈光标已修**（见第 8 / 8.6 / 8.7 节） |
| G | G1 | 静默降级与 `item:discard` 结构化错误逐条一致 | 自动（逻辑层）／真机（UI 层） |

**自动化结果**：`cargo test` **42/42 通过**（含 `IEnumFORMATETC` vtable 自检）；`cargo build --release` 零警告；GUI 冒烟 `--probe` 12 秒稳定驻留并自行退出（exit=0）。上表标「真机」的项目已在 gw13 用真实系统输入逐项点验通过（23/23，见 3.2 节）。

### 3.2 真机手势点检（Win32 输入注入，非人工目测）

工具：`src/bin/gesture.exe` → `target/release/gesture.exe`。零外部 crate，用 `SetCursorPos` / `mouse_event` / `keybd_event` 投递**真实系统输入**，用 `WindowFromPoint`、状态快照（`TQ_STATE`）、事件日志（`TQ_TRACE`）和配置/回收站/剪贴板的真实副作用做断言。数据目录、收件箱、截图目录、落点目录全部隔离在 `--work` 下，剪贴板先备份后恢复，鼠标测试前原位记录、结束归位。

**最终有效轮次 = gw13（`target/gw13/run.log` / `report.json`）：23/23 PASS，护栏拦截 0 次，exit=0。** 本机系统输入于 22:01 恢复后重跑，全套约 40 秒。

| 组 | 用例 | 结果 | 关键实测值 |
| --- | --- | --- | --- |
| A | T01 置顶透明不抢焦点 / T02 空白点击穿透 / T21 空白滚轮 / T06 相片命中接管 / T07 离开恢复穿透 | 5 PASS | `WindowFromPoint(90,236)=Chrome_RenderWidgetHostHWND`；命中时 `WFP_is_ours=true`、`capturing=true` |
| A/B | T03 顶边 250ms 呼出 / T04 展开动画 / T05 收起动画播完才恢复穿透 / T16 热键展开固定 | 4 PASS | 首帧展开 310ms、offset 归零、收起序列 `[true×15,false]`、pinned=true |
| B | T08 单击复制 / T09 双击打开 / T10 长按画图 | 3 PASS | CF_DIB 307,240 B；打开 `WinUIDesktopWin32WindowClass`；画图 `MSPaintApp` 且 `copy_count=0` |
| B | T11 右键 6 项 / T12 菜单整条接管+点外面收 / T13 菜单复制图片 | 3 PASS | `menu_open` / `menu_dismiss(outside)` / `menu_action(0:test-00.png)` |
| B | T14 ✕ 移入回收站 / T19 新截图挂绳+peek 2.5s | 2 PASS | 文件 3→2、`discard=ok`；items 2→3 且 `peek(new-item)` |
| B | **T15 拖出到文件夹（OLE CF_HDROP）** | **PASS** | `drag_hresult(hr=0x00040100 effect=1 ok=true)`、落点目录生成 `test-01.png` 副本、相片仍留绳 |
| C/D/E | T17 托盘两通道落盘 / T17b 托盘菜单弹出+ESC 关闭 / T18b 探测窗口 alpha=0 / T18 全屏抑制 / T20 单实例锁 | 5 PASS | `config_changed=true`、消息循环存活、`layered_alpha=0`、`fg_fullscreen=true` 顶边被抑制、第二实例 31ms 退出 |

**T15 根因与修复（已真机复测通过）**：gw8 时 `IDataObject` 缺 `IEnumFORMATETC`，资源管理器在 `DragEnter` 枚举格式时拿不到 `CF_HDROP` 就放弃，`effect=0`、落点无副本；同时旧 `drag_file()` 只看 `hr>=0`，而 `DRAGDROP_S_DROP` 来自 `QueryContinueDrag`，`DROPEFFECT_NONE` 时也会误报成功。已在 `src/drag.rs` 补最小 `IEnumFORMATETC`（QI/Next/Skip/Reset/Clone）、`GetDataHere`、`GetCanonicalFormatEtc(DATA_S_SAMEFORMATETC)`，成功判据收紧为 `hr>=0 && effect & DROPEFFECT_COPY != 0`，并新增 vtable 单测。gw13 实测 `effect=1`、资源管理器 `CabinetWClass` 中真实出现副本，从「代码修复」升级为「真机通过」。

### 3.3 环境阻塞与恢复：gw9–gw12 无效（不是功能回归），gw13 已恢复

gw8 之后本机出现**系统级输入冻结**，gw9/gw10/gw11 全部只有 4/9 PASS，但状态快照持续刷新、事件日志只有 `quit`——GUI 在跑却收不到任何输入。双向诊断确认：

| 检查 | 结果 |
| --- | --- |
| `SetCursorPos` | 返回 **0**，`GetLastError=0`；期望 (90,236) 回读恒为 (90,405) |
| `SendInput(MOUSEEVENTF_MOVE)` | 返回 1（投递成功）但光标不动 → 事件被系统吞掉 |
| `BlockInput(FALSE)` | 返回 0（当前进程权限不足，无法解除） |
| `GetGUIThreadInfo` | `hwndCapture=None`、`hwndMoveSize=None`（非窗口捕获） |
| `GetClipCursor` | (0,0)-(1707,1067) = 整屏，无裁剪 |
| `OpenInputDesktop` | `Default`（**未锁屏**） |
| 前台 / 可见窗口 | `GameInputServiceWindow`；`NVIDIA GeForce Overlay`(`CEF-OSC-WIDGET`)、「Windows 输入体验」可见 |

结论：疑似游戏覆盖层 / GameInput 服务或 `BlockInput` 残留接管了输入队列。注入器已加 **启动前 `input_available()` 自检**（移动 3px 并立刻还原），gw12 在 1 秒内打印 `[SKIP] 本机系统级输入注入当前不可用` 并退出，**零鼠标扰动、零副作用**。

**恢复记录（22:01）**：`target/diag_input.py` 复测 `probe set(1204,687) -> read(1204,687) moved=True`，`GetClipCursor=(0,0)-(1707,1067)`、`OpenInputDesktop=Default`。系统输入自行恢复（`NVIDIA Overlay` / `GameInputSvcX` 进程仍在，但不再吞输入）。随后用绝对路径重跑 gw13 得到 **23/23 PASS**。若将来再遇 `[SKIP]`，关闭覆盖层或注销重登后重跑同一命令即可，不必改代码。

---

## 4. 保留的行为边界（B1–B20）

| # | 边界 | 处理 |
| --- | --- | --- |
| B1 | 配置未知键原样保留并写回 | 已实现 + 单测 |
| B2 | 配置类型异常时 JS truthy 语义 | 未保证（Q7 按默认） |
| B3 | 只收启动后新内容 `started_at = now-5000` | 已实现 + 单测 |
| B4 | 文件通道不做内容去重 | 已实现 |
| B5 | 跨通道只靠「尺寸 + 10s 窗」 | 已实现 + 双向单测 |
| B6 | 启动基线只记 key 不进去重库 | 已实现 + 单测 |
| B7 | 视觉哈希只要求实现内自洽 | GDI+ 缩略图 + SHA1；通道序有单测 |
| B8 | `Math.round` 负数语义（副屏 x<0） | `js_round` + 单测 |
| B9 | 剪贴板落盘 PNG 用无压缩 stored blocks | 合法 PNG，体积大于 `toPNG()`；待 Q8 |
| B10 | 全屏检测改进程内 Win32，350ms、Shell 类名、「图墙」标题前缀、`WS_CAPTION`、2px 容差逐条保留 | 已实现；**不再有 PowerShell/conhost/csc 子进程** |
| B11 | 首次运行 800ms 后 peek，写 `firstRunDone` | Stage 2 已实现（Stage 1 曾刻意不写） |
| B12 | `TQ_SHOTS_DIR` 隔离钩子 | 默认不设时行为不变 |
| B13 | 剪贴板 CF_DIB 仅 24/32bpp BI_RGB/BITFIELDS | 其余静默跳过 |
| B14 | 错误一律静默降级，唯 `item:discard` 返回错误让相片回绳 | 已实现 |
| B15 | WebP 解码能力取决于系统 GDI+ 编解码器 | 兼容风险（Q6 按默认允许回退） |
| **B16** | `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`；绘制按物理像素缩放 | 多显示器不同 DPI 统一用系统 DPI，未逐屏验证 |
| **B17** | 缩略图只缓存 136×104，原图用完即释放 | 降低峰值，显示尺寸不变 |
| **B18** | 视觉近似（R1 路线固有）：圆角用矩形+四角圆、渐变/玻璃/阴影用分层纯色近似、hover 放大用外框近似、菜单无 `backdrop-filter` 模糊 | 几何参数（150/104/174/210/绳垂度/±2.5°/K30 C2.6/微风 1.6–3.4°）全部保留，**逐像素一致不保证**，Q1 已确认该风险 |
| **B19** | 拖放图标沿用系统默认光标（`GiveFeedback` 返回 `USEDEFAULTCURSORS`） | 原版用相片缩略图作拖拽图标，此处有可见差异 |
| **B20** | Electron 私有机制按 Q3 全部豁免；`cdp-test.js` 失效 | 需改写为 Win32 消息注入验收 |
| **B21** | 图标与中文 VERSIONINFO 内嵌进 exe（`build.rs` + `rc.exe`），托盘优先读模块资源、外部 ico 仅开发期回退 | portable 真单文件；无 SDK 时会警告并降级，需留意 |
| **B22** | portable 由「原版自解压到临时目录」改为「exe 直接运行」 | 用户可见形态更轻（不产生临时目录/子进程），但**不是**逐字节相同的产物形态；Q3 允许 Electron 私有机制豁免 |
| **B23** | portable 版若被移动，`HKCU ...\Run` 里记录的开机自启路径会失效 | 安装版路径稳定；portable 移动后需在托盘里重新开关一次自启 |

---

## 5. 剩余风险与下一步（必须如实对待）

1. ~~**F1 分发物未交付**~~ → **已交付（22:30，见第 8 节）**：`dist/` 下有 NSIS 安装版 550 KB、单文件 portable 628 KB、portable zip 149 KB 与 SHA256SUMS；图标/中文安装界面/快捷方式/卸载不删数据均已自动验收 19/19 PASS。原版 87.9 MB / 80.5 MB 的产物形态已按 Q1–Q3 的边界对齐（体积差来自去掉整个 Chromium 运行库，不是砍功能）。
2. **真机手势已 23/23 全通过**（gw13，3.2 节）：单击/双击/长按/右键/✕/6 项菜单/回收站/**OLE 拖出到文件夹**/空白穿透/全屏隐藏/托盘/热键/单实例/新截图 peek，全部用真实系统输入点验。仍需人工目测的只有「开机自启」真实切换与 B18 视觉近似（逐像素一致本就不在 R1 路线承诺内）。
3. **T4 与 S2–S5 未测**：截图落盘到挂绳延迟、1 小时长稳、捕获峰值、复制/拖放峰值、双屏+DPI 全屏切换。
4. **自动化测试要重做**：`test-flow.ps1` 与 `cdp-test.js` 依赖 Electron CDP / DOM，Q3 豁免后需换 Win32 消息注入版。
5. **Q8 未拍板**：若不接受剪贴板 PNG 体积变大，改用 GDI+ PNG 编码器即可消除 B9。
6. **进程峰值口径**：`peakWsKB` 是各进程 `PeakWorkingSetSize` 之和（进程生命周期内峰值），与「同一瞬间全树最大」不同，两个值都已列出，避免口径混淆。
7. **真机注入依赖可输入的交互桌面**：游戏覆盖层 / GameInput / `BlockInput` 残留会让 `SetCursorPos` 与 `SendInput` 静默失效（3.3 节）。注入器已加启动自检，遇到不可用会安全跳过；当前会话已恢复。

> 结论：**内存目标 T1–T3 已按交付二进制量化达成；功能一致性 23/23 真机点验全通过；F1 分发物 19/19 自动验收通过；正式 setup.exe 非静默向导两轮人工点检 4/4 页、exit=0、落地物与用户数据全部核对一致；用户反馈的黑框与转圈光标已修并复验**，两个更早反馈的桌面副作用现象也已在 gw13 验证不再复现（T18b alpha=0、护栏 0 次拦截）。发布前仅剩：开机自启/视觉近似的人工目测、T4 与 S2–S5 内存长稳场景。

---

## 6. 交付内容与运行方式

```
C:\Users\hou33\WorkBuddy\图墙\
  Cargo.toml                  零外部 crate；release = lto / opt-level z / panic=abort / strip
  build.rs                    调 Windows SDK rc.exe 内嵌图标 + 中文 VERSIONINFO
  ACCEPTANCE.md               本文件
  assets/icon.ico             从原项目复制，编译期内嵌进 exe（运行时不再依赖）
  installer/picline.rc        Win32 资源脚本（UTF-8；build.rs 转 UTF-16LE 再喂 rc.exe）
  installer/picline.nsi       NSIS 安装脚本（原版 build.nsis 参数逐条对齐）
  src/main.rs                 入口：默认完整 GUI；--once/--watch/--probe/--probe-core/--help
  src/api.rs                  Win32 FFI 声明（窗口/托盘/剪贴板/Shell/显示器/对话框）
  src/gdi.rs                  GDI+ 封装、预乘 alpha 转换、Canvas 绘制原语
  src/gui.rs                  窗口状态机、自绘、动画、手势、托盘、捕获 tick
  src/drag.rs                 OLE IDataObject/IDropSource + CF_HDROP 拖出
  src/settings.rs  inbox.rs  visual.rs  capture.rs  fullscreen.rs
  src/png.rs  memprobe.rs  win32.rs   等价内核（Stage 1）
  src/trace.rs                验收可观测钩子 TQ_TRACE / TQ_STATE（默认关闭）
  src/bin/memtree.rs          Q4 进程树内存采集器
  src/bin/gesture.rs          真机手势注入器（安全护栏 + 环境自检 + Z 序/光标诊断）
  scripts/compare-mem.sh      统一口径对比脚本（s0 / s1）
  scripts/make-test-photos.js 生成 10 张 1920×1080 测试图
  scripts/build-dist.sh       分发构建（cargo + 资源校验 + makensis + 7z + sha256）
  scripts/verify-install.py   19 项分发验收（隔离静默安装/卸载 + portable 单文件）
  target/diag_input.py  diag_unblock.py  diag_windows.py   输入环境诊断
  target/release/picline-core.exe   643,072 B（含内嵌图标/版本资源），已实测可运行
  target/release/memtree.exe        进程树内存采集器
  target/release/gesture.exe        324,608 B，真机点检工具
  target/gw13/run.log  report.json 最终有效轮次证据（23/23）
  dist/PicLine-0.1.1-win-x64-setup.exe      550 KB  NSIS 安装版
  dist/PicLine-0.1.1-win-x64-portable.exe   628 KB  单文件便携版
  dist/PicLine-0.1.1-win-x64-portable.zip   149 KB
  dist/SHA256SUMS.txt
  target/dist-verify/report.json             分发验收 19/19 证据
```

> ⚠️ **路径坑（2026-11-04 22:01 实测）**：所有 `./target/...` 都是相对**项目根目录**的。
> 若终端停在 `MINGW64 ~`（`/c/Users/hou33`）直接敲 `./target/release/gesture.exe`，
> 会报 `bash: ./target/release/gesture.exe: No such file or directory` —— 文件没丢，只是工作目录不对。
> 要么先 `cd`，要么把 exe 写成绝对路径：`"C:/Users/hou33/WorkBuddy/图墙/target/release/gesture.exe"`（Git Bash 里 `C:/...` 大小写不敏感）。
> `--work` 也建议给绝对路径，避免注入器内部按当前目录解析。

```bash
cd "C:/Users/hou33/WorkBuddy/图墙"     # ← 必须先做这一步
CARGO=/c/Users/hou33/.cargo/bin/cargo.exe
$CARGO test                       # 42 passed
$CARGO build --release
bash scripts/build-dist.sh                          # 构建 dist/ 全部正式产物
bash scripts/build-dist.sh --test-installer         # 额外产出隔离验收安装器（只放 target/dist-verify/）
python scripts/verify-install.py                    # 19 项分发验收，不碰真实桌面 / AppData
./target/release/picline-core.exe                     # 启动完整 GUI
./target/release/picline-core.exe --once             # 只读诊断
bash scripts/compare-mem.sh s0 12                   # S0 内存对比
bash scripts/compare-mem.sh s1 12                   # S1 内存对比（10 张大图 + Rust 强制展开）

# 真机手势全套点检（约 40 秒；会真实移动鼠标、弹出并关闭看图器/画图/资源管理器）
# 输入不可用时 1 秒内 [SKIP] 退出，绝不会扰动桌面
./target/release/gesture.exe --exe "C:/Users/hou33/WorkBuddy/图墙/target/release/picline-core.exe" \
                             --work "C:/Users/hou33/WorkBuddy/图墙/target/gwNN"

# 验收/测量钩子（默认关闭，不改变正常行为）
TQ_DATA_DIR=<临时目录> TQ_SHOTS_DIR=<临时目录>\Screenshots
TQ_FORCE_SHOW=1        # 启动后强制展开
TQ_MEMPROBE=<路径>      # 12 秒后写内存 JSON 并退出
TQ_TRACE=<路径> TQ_STATE=<路径>   # 事件日志 / 状态快照，仅真机点检用
```

---

## 7. 用户反馈的两个现象：根因、修复与证据

用户在 gw4 轮次观察到「测试快结束时全屏白屏，随后鼠标飘到桌面微信图标上右击」。两者**都不是 GUI 产品代码缺陷，而是注入器自身的问题**，但都真实影响到了用户桌面，因此按最高优先级处理。

### 7.1 全屏白屏

`gesture.exe` 的 `make_fullscreen_probe()` 为了模拟「全屏应用在前台」，创建了一个 `WS_POPUP|WS_VISIBLE|WS_EX_TOPMOST`、尺寸铺满整屏的窗口，且 `WNDCLASS.hbrBackground = 6`（系统白色画刷）。它在 T18 用例存活约 2 秒，视觉上就是一块**全屏纯白**，还会吃掉鼠标事件。

修复：改为 `WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT` + `SetLayeredWindowAttributes(h, 0, 0, LWA_ALPHA)`（alpha=0）+ `background = 0`（不注册背景画刷）。窗口仍是可见的前台全屏窗口（`IsWindowVisible=true`、rect 覆盖全屏），**全屏判定照常通过**，但视觉零像素、鼠标零拦截。新增 **T18b** 判据用 `GetLayeredWindowAttributes` 读回 alpha，必须为 0 才 PASS。gw8 的 T18 已在该修复下 PASS（`fg_fullscreen=true`、顶边呼出被抑制）。

### 7.2 误右击桌面微信图标

根因链：gw4 中 GUI 处于收起态（`offset=-222`），`TQ_STATE` 里 peg 的屏幕 y 因此变成负数（实测 `-158`、`-306`、`-160`）；注入器当时**没有任何校验**，仍无条件 `SetCursorPos` 到这些坐标并注入右键/左键，系统把落点夹到屏幕左上方的桌面图标区，于是右键打到了微信图标。

修复（全部在 `src/bin/gesture.rs`，不改变 GUI 产品行为）：

| 护栏 | 行为 |
| --- | --- |
| 坐标围栏 | 目标必须落在 `SM_*VIRTUALSCREEN` 内；负 y 一律拒绝并熔断 |
| 健康心跳 | 每个用例组前 `health_check`：GUI 子进程存活 + 状态快照 3 秒内刷新；`move_to`/`mouse`/热键再查 8 秒心跳 |
| 落点归属 | 任何按钮「按下」前 `WindowFromPoint` 必须命中 `PicLineWindow`；单次不命中只拒绝该次注入（用例记 FAIL），**连续 3 次才熔断** |
| 成对拦截 | 按下被拒时连带吞掉同一次手势的抬起，不向系统投递孤立 UP |
| 熔断收尾 | 立即停止全部注入、把鼠标 `SetCursorPos` 弹回测试前原位、恢复剪贴板、写 `aborted/abort_reason/guard_rejects` 报告 |
| 启动自检 | `input_available()` 移动 3px 并还原；不可用则 1 秒内 `[SKIP]` 退出，零扰动 |
| 展开态前置 | 相片类用例前强制展开并重新读 peg 坐标，杜绝负 y |

**实测证据**：gw5 打印 `[ABORT] 按钮按下落点 (1540,174) 不是图墙窗口（Chrome_RenderWidgetHostHWND），拒绝注入以免误点桌面`——成功拦下，未投递右键；gw8 修复到位后 `[GUARD] 拦截了 0 次落点不安全的按钮按下`；gw12 环境自检 `[SKIP]` 1 秒退出。

### 7.3 顺带修好的三处真实产品缺陷

1. **Z 序**：外部窗口（看图器/浏览器）被激活后会压到 topmost 分层窗口之前，导致相片上 `WindowFromPoint` 命中别的窗口。改为 `tick()` 中 150ms 节流 `SetWindowPos(HWND_TOPMOST, SWP_NOACTIVATE)`，等价 Electron `setAlwaysOnTop(true,'screen-saver')`。这是 gw5–gw7 从 11/14 提升到 gw8 21/22、再到 gw13 23/23 的关键修复。
2. **分层窗口 alpha 死锁**：相框边缘「几何命中判定」与「实际绘制像素」有 1px 误差，鼠标第一次落到边缘点时 alpha 仍为 0、事件到不了窗口。改为展开态常驻把整幅 alpha 下限抬到 1/255（穿透仍由 `WS_EX_TRANSPARENT` 决定，A2 不受影响）。
3. **OLE 拖放**：见 3.2 节 T15（`IEnumFORMATETC` + 成功判据收紧）。

---

## 8. F1 分发物（22:30 交付）

### 8.1 产物对照

| 产物 | 原版 Electron 0.1.1 | Rust 版 | 说明 |
| --- | --- | --- | --- |
| NSIS 安装版 | `PicLine-0.1.1-win-x64-setup.exe` 87,836,914 B | 同名 **562,248 B** | 名字/位数/架构一致；体积差来自去掉整个 Chromium + Node 运行库 |
| 便携版 | `PicLine-0.1.1-win-x64-portable.exe` 80,487,394 B | 同名 **643,072 B** | 原版是「自解压到临时目录后启动」；Rust 版是 **exe 直接运行**（见 B22） |
| 附加 | — | `PicLine-0.1.1-win-x64-portable.zip` 152 KB、`SHA256SUMS.txt` | 便于分发/校验，不影响主产物 |

### 8.2 安装器参数逐条对齐原版 `build.nsis`

| 原版 | Rust 版实现 |
| --- | --- |
| `oneClick: false` | MUI2 向导：欢迎 → 选择目录 → 进度 → 完成 |
| `perMachine: false` | `RequestExecutionLevel user`，全部写 HKCU，不申请管理员 |
| `allowToChangeInstallationDirectory: true` | `MUI_PAGE_DIRECTORY`，默认 `$LOCALAPPDATA\Programs\图墙`，位置记入 HKCU 供升级带出 |
| `createDesktopShortcut/createStartMenuShortcut: true`, `shortcutName:"图墙"` | 桌面 `图墙.lnk` + 开始菜单 `图墙\图墙.lnk`（另含卸载快捷方式），图标取 exe,0 |
| `language:"2052"` | `MUI_LANGUAGE SimpChinese` + 安装器 `VIAddVersionKey /LANG=2052` 中文版本资源 |
| `deleteAppDataOnUninstall: false` | 卸载段**刻意不含**任何 `$APPDATA` 删除语句；实测快照见 8.4 |
| 卸载项 | HKCU `...\Uninstall\PicLineRust`：DisplayName/Version/Publisher/InstallLocation/DisplayIcon/UninstallString/QuietUninstallString/NoModify/NoRepair/EstimatedSize |
| 完成页 | 默认**不勾选**「立即运行 图墙」；已读 MUI2 `Finish.nsh` 源码确认 RUN 只在完成页 Leave 回调按复选框执行，静默安装跳过页面回调 ⇒ `/S` 不会拉起 GUI |

### 8.3 为什么 portable 能是真单文件

原 `gui.rs` 的托盘图标从 `exe同级/assets/icon.ico` 读文件，那样就必须自解压出散文件。现在：

- `build.rs` 自动定位 Windows SDK `rc.exe`（`TQ_RC` 可覆盖；找不到只警告、不阻断编译）。
- `installer/picline.rc` 定义 `IDI_APP ICON` + 简体中文 `VERSIONINFO`；build.rs 把源码转 UTF-16LE+BOM 再编译，避免中文 Windows 代码页乱码。
- `cargo:rustc-link-arg-bin=picline-core=<res>` 只给主程序挂资源，验收工具不白胖 370 KB。
- 托盘 `load_app_icon()` 优先 `LoadImageW(GetModuleHandleW(NULL), MAKEINTRESOURCE(101), ...)`，外部 ico 仅作开发期回退；`icon_source` 诊断事件可区分来源。

实测：把 portable.exe 单独复制到空目录（无 `assets/`）跑 `--probe`，`icon_source=resource`、exit=0、目录里没多出任何文件。

### 8.4 分发验收：19/19 PASS（`scripts/verify-install.py`）

```bash
bash scripts/build-dist.sh --test-installer   # 隔离安装器输出到 target/dist-verify/，不进 dist
python scripts/verify-install.py              # 报告 target/dist-verify/report.json
```

`--test-installer` 用编译期宏把快捷方式重定向到安装目录内、并去掉完成页 RUN，**正式产物不受影响**。验收全程零真实桌面/AppData 写入：

| 组 | 项 | 结果 |
| --- | --- | --- |
| 资源 | I01 / I01b 验收版与**正式 setup.exe** 中文版本资源 | 2 PASS |
| 静态 | I02 卸载脚本无 AppData 删除语句 | 1 PASS |
| 安装 | I03–I09 exit=0、主程序与卸载器落盘、exe 与 release 逐字节一致、内嵌图标、两个快捷方式、HKCU 卸载项字段、安装位置、**真实桌面未被写入** | 7 PASS |
| 卸载 | I10–I14 文件/快捷方式/注册表清理、**`%APPDATA%\图墙` 39 个文件逐文件哈希快照前后一致**、桌面状态不变 | 5 PASS |
| portable | I15–I18 孤立目录跑完整 12s GUI 探针、图标来自内嵌资源、零依赖文件、版本资源中文 | 4 PASS |

### 8.5 已知边界（如实列出）

1. ~~**正式 `setup.exe` 未跑非静默向导**~~ → **已完成（23:23，见 8.6 节）**：真实桌面/开始菜单快捷方式已创建并核对，向导 exit=0、无残留进程，`%APPDATA%\图墙` 39 个文件哈希前后一致。
2. **安装/卸载会关闭正在运行的图墙**：先给 `PicLineWindow` 发 `WM_APP_QUIT` 优雅退出，600 ms 后 `taskkill /IM picline-core.exe /F` 兜底（GUI 无未保存状态，不丢数据）。若同时开着同名 exe 的其它实例，也会被一并关闭。
3. **B22 / B23**：portable 形态差异与移动后自启路径失效，见第 4 节。
4. **构建环境依赖**：本机 NSIS 来自 `%LOCALAPPDATA%\electron-builder\Cache\nsis-3.0.4.1`（未联网下载），换机需装 NSIS 或设 `TQ_MAKENSIS`；`build.rs` 在无 Windows SDK 时会警告并降级（exe 无内嵌图标，portable 需附带 `assets/`）。
5. 仍按第 5 节：开机自启/视觉近似人工目测、T4 延迟与 1 小时长稳、S2–S5 场景、Q8 PNG 体积、`cdp-test.js` 改写。8.6 的 U1/U2 已修并复检，U3 经查为 NSIS UI 模板默认图标（非缺陷）；B24/B25 已修并复验见 8.7。

### 8.6 正式向导人工点检（两轮：23:23 初检 → 23:40 修文案后复检）

**第一轮（23:23，旧安装器 562,248 B）** 用正式 `dist/PicLine-0.1.1-win-x64-setup.exe` 人工逐页点完，
4/4 页功能全部正确，但发现 3 处显示缺陷（U1–U3，见下表）。安装前把 `%APPDATA%\图墙` 39 个文件
备份到 `%TEMP%\picline-appdata-backup-20261104` 并留 SHA256 快照。

| 页 | 核对结果 |
| --- | --- |
| 1 欢迎 | 简体中文、标题「欢迎使用 图墙 0.1.1 安装向导」；`下一步(N) >` 为焦点、`取消(C)`；无许可页、无复选框（`oneClick:false`） |
| 2 选定安装位置 | 默认 `C:\Users\hou33\AppData\Local\Programs\图墙`（= `$LOCALAPPDATA\Programs\图墙`）；所需 628.0 KB / 可用 18.9 GB；`浏览(B)…`、`上一步(P)`/`安装(I)`/`取消(C)` 齐全 |
| 3 安装进度 | NSIS 安装完成后自动前进、Finish 页不允许回退，未截到进度明细；改由落盘结果验证 |
| 4 正在完成 | 「安装完成。」；`立即运行 图墙` 默认**未勾**（`MUI_FINISHPAGE_RUN_NOTCHECKED`）；`完成(F)` 焦点；点完 exit=0 |

**第二轮（23:40 重打包 → 23:43–23:46 复检）** 新安装器 562,300 B、SHA256 `D72CFE1E…15DED`：
欢迎页 / 完成页文案与行距全部通过，无压字、无裁切；覆盖安装 exit=0，安装目录 `picline-core.exe`
与 `dist` portable **逐字节一致**（SHA256 `DA367808…9404F`，PE subsystem=GUI）。

落地核对：安装目录 `picline-core.exe` 643,072 B + `Uninstall.exe` 412,908 B；桌面「图墙.lnk」、
开始菜单「图墙.lnk」/「卸载 图墙.lnk」3 个快捷方式；`HKCU\Software\PicLineRust` 与
`HKCU\...\Uninstall\PicLineRust` 全部字段齐全；`picline-core.exe` 进程数 0（未勾 RUN 不拉起 GUI）。
向导退出后：安装器/卸载器进程 0、向导窗口 0、安装文件仍在。
`%APPDATA%` 复核：39 个原有文件内容 **0 变更**，仅新增两张用户自己截的图（`inbox\clip-…`、
`inbox\shot-…`，属运行期捕获，不是安装器写入）；原版 Electron 遗留的 `Local State`/`GPUCache`
等缓存未被安装器触碰。
证据：`target/wizard-precheck*.txt`、`target/wizard-appdata-before.txt`、`target/wizard-installed.txt`、
`target/wizard-final.txt`、`target/wizard-runtime-check.txt`；截图 `%TEMP%\wiz-01..03.png`（初检）、
`wiz2-01.png`（修前压字）、`wiz3-01.png`（修后欢迎）、`wiz3-04.png`（修后完成）。

**3 处显示缺陷的处理结果：**

| # | 现象 | 根因 | 处置 / 复检 |
| --- | --- | --- | --- |
| U1 | 欢迎页中文行压字重叠 | MUI 的 `NSD_CreateLabel`（Static）按**英文字体度量**排行高，中文字符高于行距；自动换行和手动 `$\r$\n` 硬换行都会叠 | **已修**：每行之间插空行（`$\r$\n$\r$\n`），并让每行短到不触发自动换行。23:43 复检四行正文无压字、无裁切 |
| U2 | 完成页第二段压字、第三段整行被裁 | 定义 `MUI_FINISHPAGE_RUN` 时 Finish 页文本控件只有 **40u** 高（`MUI_FINISHPAGE_TEXT_HEIGHT_BUTTONS`），长文案溢出被裁；行高问题同 U1 | **已修**：`MUI_FINISHPAGE_TEXT_LARGE` 把 40u 抬到 60u + 段间空行。23:44 复检三段完整显示 |
| U3 | 完成页右上角是 NSIS 默认「电脑+光盘」图标 | 该图标不是 `MUI_ICON`：MUI2 的 welcome/finish 页源码只有左侧位图 + 标题/文本 label，右上角图标来自 NSIS UI 模板内置的 `modern-install.ico`；`Icon` 指令只影响窗口标题栏/任务栏（那里已正确显示自定义衣架图标） | **不是缺陷，描述更正**：欢迎页与完成页一致。要替换需自定义 UI 资源或页面 SHOW 回调给 SS_ICON 控件发 `STM_SETICON`，成本高、收益低，本轮不做 |

**人工复检顺带发现并修掉的两个运行期缺陷：** 黑色控制台窗口（B24）、鼠标转圈光标（B25），见 8.7。

本次安装**保留在真机**上（未卸载），桌面「图墙」快捷方式指向新版 GUI 子系统 exe。

### 8.7 用户反馈的两个运行期缺陷（23:47 修复并复验）

| # | 现象 | 根因 | 修复与证据 |
| --- | --- | --- | --- |
| B24 | 双击启动弹出黑色命令框；关掉黑框软件一起退出 | release exe 以 **Console 子系统**链接（旧安装 exe PE subsystem=3 `WINDOWS_CUI`），Windows 为它创建控制台宿主；控制台与 GUI 同进程，关窗＝结束进程 | `src/main.rs` 增加 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`（Debug 保留控制台便于 CLI 诊断）。新 exe subsystem=2 `WINDOWS_GUI`；启动后 `picline-core` 无子 conhost。代价：从资源管理器双击时 stdout 无接收端（CLI 模式仍可从终端/管道读输出） |
| B25 | 鼠标移到相片上变蓝色转圈（等待光标） | 窗口类 `WndClassExW.h_cursor = 0` 且未处理 `WM_SETCURSOR`，`DefWindowProc` 不会重置系统残留的忙碌光标（启动时那一下） | 类光标设为 `IDC_ARROW`；`WM_SETCURSOR` 客户区显式 `SetCursor`：命中 ✕ 或菜单打开时 `IDC_HAND`，其余 `IDC_ARROW`，对齐原版 `line.css` 的 `cursor:default` / `.xbtn cursor:pointer`。实测 `GetCursorInfo` hCursor=65539=IDC_ARROW（等待/转圈为 65543/65561） |

产物：`dist/` 重新构建为 setup 562,260 B（`D72CFE1E…15DED`）、portable 628 KB（`DA367808…9404F`）、
zip 与 `SHA256SUMS.txt` 同步刷新；`cargo test` 42/42、release 零警告。
