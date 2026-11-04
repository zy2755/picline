// 路径与目录:数据目录、收件箱、系统截图文件夹解析
const path = require('path');
const fs = require('fs');
const { app } = require('electron');
const { execFile } = require('child_process');

let dataDir, inboxDir;

function initDataDirs() {
  // 显式用 appData + 产品名,避免 Electron 使用 package.json 的 name
  dataDir = path.join(app.getPath('appData'), '图墙');
  inboxDir = path.join(dataDir, 'inbox');
  fs.mkdirSync(inboxDir, { recursive: true });
  return { dataDir, inboxDir };
}

function getDataDir() { return dataDir; }
function getInboxDir() { return inboxDir; }

// 用 PowerShell 解析"图片"已知文件夹(能正确处理 OneDrive 重定向)
function resolvePicturesDir() {
  return new Promise((resolve) => {
    const fallback = path.join(app.getPath('home'), 'Pictures');
    execFile('powershell.exe',
      ['-NoProfile', '-NonInteractive', '-Command',
        "[Environment]::GetFolderPath('MyPictures')"],
      { windowsHide: true, timeout: 8000 },
      (err, stdout) => {
        const dir = (stdout || '').trim();
        resolve(dir && fs.existsSync(dir) ? dir : fallback);
      });
  });
}

module.exports = { initDataDirs, getDataDir, getInboxDir, resolvePicturesDir };
