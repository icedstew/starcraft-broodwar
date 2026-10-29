// node cdp_console.mjs <port> <url> [proceed]: open url in a running Chromium
// (--remote-debugging-port) and print its console, exceptions and log entries; every 5 s the
// page's #status text (webjoin.sh). `proceed`: 3 s in, type "thisisunsafe" (Chrome's way past
// a certificate warning page, as a user clicking through).
const [port, url, proceed] = process.argv.slice(2);
let ver;
for (let i = 0; i < 50 && !ver; i++) { try { ver = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json(); } catch { await new Promise(r => setTimeout(r, 200)); } }
const t = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
const ws = new WebSocket(t.webSocketDebuggerUrl);
let id = 0;
const send = (method, params = {}) => ws.send(JSON.stringify({ id: ++id, method, params }));
ws.onopen = () => { send("Runtime.enable"); send("Log.enable"); send("Page.navigate", { url }); };
ws.onmessage = m => {
  const d = JSON.parse(m.data);
  if (d.method === "Runtime.consoleAPICalled") console.log("console:", d.params.args.map(a => a.value ?? a.description).join(" "));
  else if (d.method === "Runtime.exceptionThrown") console.log("exception:", JSON.stringify(d.params.exceptionDetails).slice(0, 400));
  else if (d.method === "Log.entryAdded") console.log("log:", d.params.entry.level, d.params.entry.text.slice(0, 300));
};
if (proceed) setTimeout(() => { for (const c of "thisisunsafe") send("Input.dispatchKeyEvent", { type: "char", text: c }); }, 3000);
setInterval(() => send("Runtime.evaluate", { expression: "document.getElementById('status')?.textContent + ' | ' + (document.querySelector('canvas') ? 'canvas' : 'nocanvas') + ' | secure ' + window.isSecureContext", returnByValue: true }), 5000);
const prev = ws.onmessage;
ws.onmessage = m => { prev(m); const d = JSON.parse(m.data); if (d.result?.result?.value !== undefined) console.log("status:", d.result.result.value); };
