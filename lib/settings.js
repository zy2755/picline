// 配置读写:%APPDATA%\图墙\config.json
const fs = require('fs');
const path = require('path');
const { getDataDir } = require('./paths');

const DEFAULTS = {
  showLine: true,        // 晾衣绳启用(对应 ⌃⌥T 的总开关)
  takeOver: true,        // 接管截图文件夹:新截图移入收件箱,不堆积在 Screenshots
  clipboardWatch: true,  // 监听剪贴板图像(捕捉 Win+Shift+S 未落盘的截图)
  maxPegs: 10,           // 晾衣绳上最多同时挂几张
  firstRunDone: false,
};

let cache = null;

function load() {
  if (cache) return cache;
  const file = path.join(getDataDir(), 'config.json');
  try {
    cache = { ...DEFAULTS, ...JSON.parse(fs.readFileSync(file, 'utf8')) };
  } catch {
    cache = { ...DEFAULTS };
  }
  return cache;
}

function save(patch = {}) {
  cache = { ...load(), ...patch };
  const file = path.join(getDataDir(), 'config.json');
  try { fs.writeFileSync(file, JSON.stringify(cache, null, 2), 'utf8'); } catch {}
  return cache;
}

function get(key) { return load()[key]; }
function set(key, value) { return save({ [key]: value })[key]; }

module.exports = { load, save, get, set };
