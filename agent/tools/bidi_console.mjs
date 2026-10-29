// node bidi_console.mjs <port> <url>: open url in a running Firefox (--remote-debugging-port,
// WebDriver BiDi) with certificate errors accepted, and print its console log and, every 5 s,
// the page's #status text (cdp_console.mjs for Firefox).
const [port, url] = process.argv.slice(2);
let ws;
for (let i = 0; i < 50 && !ws; i++) {
  try { ws = await new Promise((ok, no) => { const w = new WebSocket(`ws://127.0.0.1:${port}/session`); w.onopen = () => ok(w); w.onerror = no; }); }
  catch { await new Promise(r => setTimeout(r, 300)); }
}
let id = 0;
const waiting = new Map();
const call = (method, params) => new Promise(ok => { waiting.set(++id, ok); ws.send(JSON.stringify({ id, method, params })); });
ws.onmessage = m => {
  const d = JSON.parse(m.data);
  if (d.id && waiting.has(d.id)) { waiting.get(d.id)(d); waiting.delete(d.id); return; }
  if (d.method === "log.entryAdded") console.log("console:", d.params.text ?? JSON.stringify(d.params.args));
};
const s = await call("session.new", { capabilities: { alwaysMatch: { acceptInsecureCerts: true } } });
if (s.error) { console.log("session:", s.error, s.message); process.exit(1); }
await call("session.subscribe", { events: ["log.entryAdded"] });
const tree = await call("browsingContext.getTree", {});
const ctx = tree.result.contexts[0].context;
await call("browsingContext.navigate", { context: ctx, url, wait: "none" });
setInterval(async () => {
  const r = await call("script.evaluate", { expression: "document.getElementById('status')?.textContent + ' | ' + (document.querySelector('canvas') ? 'canvas' : 'nocanvas') + ' | secure ' + window.isSecureContext", target: { context: ctx }, awaitPromise: false });
  console.log("status:", r.result?.result?.value);
}, 5000);
