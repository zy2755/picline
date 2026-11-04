; ============================================================================
; 图墙 / PicLine —— NSIS 安装脚本（对齐原版 electron-builder 的 nsis 配置）
;
; 原版 build.nsis 参数 → 本脚本的对应实现：
;   oneClick: false                 → MUI 向导页（欢迎/目录/进度/完成）
;   perMachine: false               → RequestExecutionLevel user，全部写 HKCU
;   allowToChangeInstallationDirectory: true → MUI_PAGE_DIRECTORY
;   createDesktopShortcut / createStartMenuShortcut: true → 两个「图墙」快捷方式
;   shortcutName: "图墙"            → ${APP_NAME}.lnk
;   language: "2052"                → MUI_LANGUAGE SimpChinese + 中文版本资源
;   deleteAppDataOnUninstall: false → **卸载绝不删 %APPDATA%\图墙**（见卸载段）
;
; 编译方式见 scripts/build-dist.sh。源码保持 UTF-8（无 BOM），构建脚本会复制成
; 带 BOM 的临时文件再交给 makensis —— NSIS 3 在中文 Windows 上靠 BOM 识别 UTF-8。
;
; 可由 makensis /D 传入：
;   SRCEXE=<绝对路径>  要打包的 picline-core.exe（必须已内嵌图标/版本资源）
;   OUTFILE=<绝对路径> 生成的 setup.exe
;   APP_ICON=<绝对路径> assets/icon.ico，用作安装器与卸载器图标
;   APP_VERSION=0.1.1
;   TQ_TEST_INSTALL=1  仅验收用：快捷方式重定向到 $INSTDIR\_testshortcuts，
;                      且不启用「完成页运行图墙」，静默安装/卸载零桌面副作用
; ============================================================================

Unicode true

!ifndef APP_VERSION
  !define APP_VERSION "0.1.1"
!endif
!define APP_NAME "图墙"
!define APP_EXE "picline-core.exe"
!define REG_ROOT "Software\PicLineRust"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\PicLineRust"
!define WM_APP_QUIT 0x8002          ; gui.rs: const WM_APP_QUIT = WM_APP + 2

!ifdef APP_ICON
  !define MUI_ICON "${APP_ICON}"
  !define MUI_UNICON "${APP_ICON}"
!endif

!include "MUI2.nsh"
!include "LogicLib.nsh"

Name "${APP_NAME} ${APP_VERSION}"
!ifndef OUTFILE
  !define OUTFILE "PicLine-${APP_VERSION}-win-x64-setup.exe"
!endif
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "${REG_ROOT}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
CRCCheck on
ShowInstDetails show
ShowUninstDetails show
AllowSkipFiles off

; 安装器自身的版本信息（资源管理器「属性 → 详细信息」可见）
VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey /LANG=2052 "ProductName" "图墙"
VIAddVersionKey /LANG=2052 "FileDescription" "图墙 安装程序 (Rust 版)"
VIAddVersionKey /LANG=2052 "FileVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=2052 "CompanyName" "zy2755"
VIAddVersionKey /LANG=2052 "LegalCopyright" "MIT Licensed · inspired by Tendedero"

!define MUI_ABORTWARNING
; MUI2 页面文本是 NSD_CreateLabel（Static）固定高度：欢迎页 130u；完成页因定义了 RUN
; 复选框走 BUTTONS 分支只有 40u。该控件按英文字体度量排行高，中文字符比行距高，
; 无论自动换行还是手动 $\r$\n 硬换行，相邻行都会上下压字；超高的行还会被整行裁掉。
; ⇒ ①每行之间插一个空行（$\r$\n$\r$\n），让字高 < 行距×2；
;   ②每行短到不会触发自动换行；③完成页用 TEXT_LARGE 把 40u 提到 60u。
!define MUI_WELCOMEPAGE_TEXT "本向导将把${APP_NAME}安装到你的电脑。$\r$\n$\r$\n纯 Win32/GDI+ 自绘的 Rust 版本，$\r$\n$\r$\n无需 .NET / WebView2 / Electron 运行库。$\r$\n$\r$\n数据存于 %APPDATA%\${APP_NAME}，卸载保留。"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES

; 完成页默认不勾选「运行」，避免静默安装意外拉起窗口
!ifndef TQ_TEST_INSTALL
  !define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
  !define MUI_FINISHPAGE_RUN_TEXT "立即运行 ${APP_NAME}"
  !define MUI_FINISHPAGE_RUN_NOTCHECKED
!endif
!define MUI_FINISHPAGE_NOREBOOTSUPPORT
; 定义了 RUN 复选框时 Finish.nsh 走 BUTTONS 分支，文本控件只有 40u 高，
; 长文案超出的行会被整行裁掉；TEXT_LARGE 把它抬到 60u（官方开关，可调）。
!define MUI_FINISHPAGE_TEXT_LARGE
!define MUI_FINISHPAGE_TEXT "安装完成。$\r$\n$\r$\n顶边呼出晾衣绳；Win+Shift+S 自动挂上。$\r$\n$\r$\n数据存于 %APPDATA%\${APP_NAME}，卸载保留。"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

; 快捷方式位置：验收构建重定向到安装目录内，正式构建写真实桌面/开始菜单
!ifdef TQ_TEST_INSTALL
  !define SHORTCUT_DESKTOP "$INSTDIR\_testshortcuts\Desktop"
  !define SHORTCUT_START "$INSTDIR\_testshortcuts\StartMenu\${APP_NAME}"
!else
  !define SHORTCUT_DESKTOP "$DESKTOP"
  !define SHORTCUT_START "$SMPROGRAMS\${APP_NAME}"
!endif

; ---------------------------------------------------------------------------
; 关闭正在运行的实例：先给控制器窗口发 WM_APP_QUIT 让它自己收尾，
; 600ms 后用 taskkill 兜底（GUI 无未保存状态，不会丢数据）。
; nsExec 不弹黑窗，适合静默安装。
; ---------------------------------------------------------------------------
Function CloseRunningInstance
  FindWindow $0 "PicLineWindow" ""
  ${If} $0 <> 0
    SendMessage $0 ${WM_APP_QUIT} 0 0
    Sleep 600
  ${EndIf}
  nsExec::ExecToStack '"$SYSDIR\taskkill.exe" /IM ${APP_EXE} /F'
  Pop $0
  Pop $1
FunctionEnd

Function un.CloseRunningInstance
  FindWindow $0 "PicLineWindow" ""
  ${If} $0 <> 0
    SendMessage $0 ${WM_APP_QUIT} 0 0
    Sleep 600
  ${EndIf}
  nsExec::ExecToStack '"$SYSDIR\taskkill.exe" /IM ${APP_EXE} /F'
  Pop $0
  Pop $1
FunctionEnd

; ---------------------------------------------------------------------------
Section "${APP_NAME} (必需)" SecMain
  SectionIn RO
  Call CloseRunningInstance

  SetOutPath "$INSTDIR"
  ; 单文件：图标与版本信息已内嵌在 exe 资源里，不需要复制 assets/
  File "/oname=${APP_EXE}" "${SRCEXE}"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "${SHORTCUT_DESKTOP}"
  CreateDirectory "${SHORTCUT_START}"
  CreateShortCut "${SHORTCUT_DESKTOP}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
  CreateShortCut "${SHORTCUT_START}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
  CreateShortCut "${SHORTCUT_START}\卸载 ${APP_NAME}.lnk" "$INSTDIR\Uninstall.exe" "" "$INSTDIR\Uninstall.exe" 0

  ; 记住安装位置，便于下次升级时带出目录
  WriteRegStr HKCU "${REG_ROOT}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${REG_ROOT}" "Version" "${APP_VERSION}"

  ; 「程序和功能」卸载项（HKCU = 当前用户，不申请管理员）
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP_NAME} (Rust 版)"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "zy2755"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegStr HKCU "${UNINST_KEY}" "URLInfoAbout" "https://github.com/zy2755/git-practice"
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" 700

  DetailPrint "用户数据目录 $APPDATA\${APP_NAME} 不会被安装或卸载改动。"
SectionEnd

; ---------------------------------------------------------------------------
Section "Uninstall"
  Call un.CloseRunningInstance

  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "${SHORTCUT_DESKTOP}\${APP_NAME}.lnk"
  Delete "${SHORTCUT_START}\${APP_NAME}.lnk"
  Delete "${SHORTCUT_START}\卸载 ${APP_NAME}.lnk"
  RMDir "${SHORTCUT_START}"
!ifdef TQ_TEST_INSTALL
  RMDir "${SHORTCUT_DESKTOP}"
  RMDir "$INSTDIR\_testshortcuts\StartMenu"
  RMDir "$INSTDIR\_testshortcuts"
!endif
  RMDir "$INSTDIR"

  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "${REG_ROOT}"

  ; ⚠️ 刻意不写 RMDir /r "$APPDATA\${APP_NAME}"：
  ;    对齐原版 deleteAppDataOnUninstall: false，截图、收件箱、config.json 全部保留。
  DetailPrint "已保留你的截图与设置：$APPDATA\${APP_NAME}"
SectionEnd
