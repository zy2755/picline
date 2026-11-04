# 图墙 · PicLine

**截图,晾起来。**

把每一张截图挂到屏幕顶端的晾衣绳上——指针在屏幕顶边一停,绳就滑下来;移开,它就收走。
Windows 版,创意与行为规格来自 Alejandro Buján 的 macOS 应用
[Tendedero](https://github.com/alejandrobujan/tendedero)(MIT)。
按其 LICENSE 要求,本项目使用自己的名字与图标(图标由 `scripts/make-icon.js` 代码绘制)。

![图标](assets/icon-256.png)

## 它能做什么

| 手势 | 效果 |
| --- | --- |
| 鼠标停靠屏幕顶边(中部约 56% 区域)0.25 秒 | 晾衣绳滑下 |
| 指针离开 / 点击空白处 | 收起 |
| **单击** 相片 | 复制到剪贴板 |
| **双击** | 用系统照片查看器打开 |
| **长按** 0.45 秒 | 用画图编辑 |
| **拖出** 到应用 | 发送一份副本,相片留在绳上 |
| **拖出** 到文件夹 | 保存一份,相片留在绳上 |
| 右键 | 复制 / 画图编辑 / 打开 / 另存为 / 取下并保存到截图文件夹 / 删除 |
| 点 **✕** 或拖到回收站 | 移入回收站 |
| `Ctrl+Alt+T` | 显示 / 收起晾衣绳 |

新截图到手的一瞬间就挂上绳,不需要等任何缩略图。

## 截图从哪来

两条通道,自动去重:

1. **截图文件夹**(默认开启):监视 `图片\Screenshots`(`Win+Shift+S`、`Win+PrintScreen`
   的落盘处)。新截图随即移入图墙的收件箱(`%APPDATA%\图墙\inbox`),
   文件夹不再堆积;在绳上"取下并保存"会放回 `图片\Screenshots`。
2. **剪贴板**(默认开启):捕捉不落盘的截图(关闭"自动保存原始屏幕截图"时的
   `Win+Shift+S`、各类工具的"复制到剪贴板")。

只认启动之后的新内容:已有的历史截图、云盘同步、复制图片等动作都不会被误收;
自己"单击复制"的图也不会被再挂一次。同一张图两条通道同时到达时只挂一次,
且保留原始文件画质。

托盘图标可随时开关这两条通道、设置开机自启、打开收件箱。

## 安全区

- 全屏应用在前台时,晾衣绳自动藏起,不会挡游戏或放映。
- 窗口不抢焦点:悬停、点击晾衣绳不会打断你正在输入的地方。

## 运行

需要 Node.js 18+(开发时使用 Node 24 与 Electron 33)。

```bash
git clone <this-repo>
cd 图墙
npm install
npm start
```

重新生成图标(可选):`npm run icon`。

### 下载安装

不想碰命令行就直接下现成的:**[Releases](https://github.com/zy2755/picline/releases)**

| 文件 | 说明 |
| --- | --- |
| `PicLine-0.1.0-win-x64-setup.exe` | 安装版,可选安装目录,建桌面/开始菜单快捷方式 |
| `PicLine-0.1.0-win-x64-portable.exe` | 免安装单文件,双击即用,适合放U盘 |

均为 Windows x64、未签名(首次运行 SmartScreen 可能提示"仍要运行")。

### 自己构建

```bash
npm install
npm run build        # = npm run ico + electron-builder --win --x64
```

产物在 `dist/`:NSIS 安装包 + 便携版 exe。

`npm run icon` 重新绘制 PNG 图标,`npm run ico` 把它们打包成多尺寸 `assets/icon.ico`
(安装包图标用它)。electron-builder 配置全在 `package.json` 的 `build` 字段里。

> 这里刻意关了 asar(`"asar": false`):全屏检测要靠独立 PowerShell 进程读
> `lib/fullscreen.ps1`,而 asar 归档外部进程读不了。别顺手打开。

## 项目结构

| 文件 | 职责 |
| --- | --- |
| `main.js` | 多显示器晾衣绳窗口、顶边悬停呼出/收起、托盘、全局快捷键、IPC |
| `lib/capture.js` | 截图捕获:文件夹监视 + 剪贴板轮询 + 视觉哈希去重 + 收件箱 |
| `lib/fullscreen.js` `lib/fullscreen.ps1` | 前台窗口轮询(Win32),判定全屏并自动隐藏 |
| `lib/paths.js` `lib/settings.js` | 收件箱/配置存储 |
| `src/line.html` `src/line.css` `src/line.js` | 晾衣绳 UI:绳、衣夹、玻璃相框、物理摆动、手势 |
| `scripts/make-icon.js` | 代码绘制应用图标(SDF + 超采样) |
| `scripts/test-flow.ps1` | 交互流程自动化测试(悬停/复制/删除/收起) |
| `scripts/cdp-test.js` | 通过 CDP 驱动真实 UI 的 discard 测试 |

### 从原版移植的参数

- 顶边悬停 250ms 呼出、新截图探头展示 2.5s、长按 0.45s
- 相框宽 150 / 照片高 104 / 间距 174 / 面板高 210 / 绳垂度 `min(30, w×0.018)`
- 每张相片随机静置倾角 ±2.5°,微风轻拂 1.6–3.4°(随机延迟 0–0.35s)
- 呼出 spring(0.42, 0.82)、收起 easeIn 0.22s(渲染端以 CSS/阻尼弹簧近似)

## 与 macOS 原版的差异

- **呼出区域**:原版是菜单栏中部三分之一;Windows 没有菜单栏,取顶边中部
  约 56%(避开最大化窗口左上/右上的控制按钮)。
- **"接管截图"**:macOS 版可改写系统截图设置(关闭浮窗、改存自家文件夹)并在退出时还原;
  Windows 的截图工具(Snipping Tool)不暴露这些设置,改为"监视 + 移入收件箱"。
  剪贴板通知横幅无法隐藏,属系统限制。
- **标注**:原版打开系统 Markup;Windows 对应为画图(mspaint)。
- 晾衣绳跨会话保留:收件箱里的图在下次启动时仍挂在绳上,直到你取下或删除。

## 隐私

无账号、无网络请求、无统计。所有数据都在本机 `%APPDATA%\图墙`。

## 许可

代码以 MIT 发布(见 LICENSE)。名称"图墙"与本项目的图标不包含在该许可内。
Tendedero 的名称与图标归其作者所有。
