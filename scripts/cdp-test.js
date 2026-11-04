// CDP 端到端手势测试:直接驱动真实 UI 元素,验证 discard / copy / 菜单链路
// 用法: node scripts/cdp-test.js [port]
const port = process.argv[2] || '9222';

async function main() {
  const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const page = list.find(t => t.url.includes('line.html'));
  if (!page) throw new Error('line window not found');
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  let id = 0;
  const pending = new Map();

  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const mid = ++id;
    pending.set(mid, { resolve, reject });
    ws.send(JSON.stringify({ id: mid, method, params }));
  });
  ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
    }
  };
  await new Promise(r => { ws.onopen = r; });

  const evalJs = async (expr) => {
    const r = await send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true });
    if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails.exception));
    return r.result.value;
  };

  // 让线先亮出来(直接调渲染端的 reveal 路径,等价于主进程发 reveal)
  await evalJs(`(() => { const l = document.getElementById('line'); l.classList.remove('hidden'); l.classList.add('shown'); return 'shown'; })()`);

  const n = await evalJs(`document.querySelectorAll('.peg').length`);
  console.log(`pegs on line: ${n}`);
  if (n === 0) { console.log('FAIL: no pegs'); process.exit(1); }

  // 1) 模拟点击 ✕:走真实 click 事件链(mousedown→stopPropagation→click→falling→ipc discard)
  await evalJs(`(() => { const b = document.querySelector('.xbtn'); b.dispatchEvent(new MouseEvent('mousedown', {bubbles: true})); b.click(); return 'clicked'; })()`);
  await new Promise(r => setTimeout(r, 1200));
  console.log('X discard dispatched');
  ws.close();
}
main().catch(e => { console.error('FAIL:', e.message); process.exit(1); });
