// node cdp_drive.mjs <port> <url> <out dir> <step>...: drive a page in a running Chromium
// (--remote-debugging-port) and print its console. Steps, in order:
//   wait:<js>      until the expression is true (60 s at most, else exit 1)
//   eval:<js>      run it (awaited)
//   sleep:<ms>
//   key:<name>     press and release a key on the game canvas (F10, Escape, Enter, a..z, 0..9)
//   click:<x>,<y>  left click at the game's 640x480 position (the canvas scaled)
//   drag:<x1>,<y1>,<x2>,<y2>  left button down at one position, up at the other (a box)
//   shot:<name>    screenshot to <out dir>/<name>.png
//   drive          fill the page's Drive fields from SCBW_TEST_DRIVE_KEY / _FOLDER and press
//                  Load from Drive (the values are never printed)
const [port, url, out, ...steps] = process.argv.slice(2);
import { writeFileSync } from "node:fs";
let ver;
for (let i = 0; i < 50 && !ver; i++) { try { ver = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json(); } catch { await new Promise(r => setTimeout(r, 200)); } }
const t = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise(r => ws.onopen = r);
let id = 0;
const waiting = new Map();
const send = (method, params = {}) => new Promise(ok => { waiting.set(++id, ok); ws.send(JSON.stringify({ id, method, params })); });
ws.onmessage = m => {
  const d = JSON.parse(m.data);
  if (d.id && waiting.has(d.id)) { waiting.get(d.id)(d); waiting.delete(d.id); return; }
  if (d.method === "Runtime.consoleAPICalled") console.log("console:", d.params.args.map(a => a.value ?? a.description).join(" "));
  else if (d.method === "Runtime.exceptionThrown") console.log("exception:", JSON.stringify(d.params.exceptionDetails).slice(0, 300));
};
await send("Runtime.enable");
await send("Page.enable");
await send("Emulation.setDeviceMetricsOverride", { width: 1300, height: 1050, deviceScaleFactor: 1, mobile: false });
await send("Page.navigate", { url });
const evaluate = async (expr, awaitPromise = true) => (await send("Runtime.evaluate", { expression: expr, awaitPromise, returnByValue: true })).result?.result?.value;
const KEYS = { F10: [121, "F10"], Escape: [27, "Escape"], Enter: [13, "Enter"] };
for (const s of steps) {
  const [op, ...rest] = s.split(":");
  const arg = rest.join(":");
  console.log("step:", s.slice(0, 80));
  if (op === "wait") {
    let ok = false;
    for (let i = 0; i < 300 && !ok; i++) { ok = await evaluate(arg, false).catch(() => false); if (!ok) await new Promise(r => setTimeout(r, 200)); }
    if (!ok) {
      console.log("FAIL: timed out waiting for", arg);
      console.log("status:", await evaluate("document.getElementById('status')?.textContent", false));
      const d = await send("Page.captureScreenshot", { format: "png" });
      writeFileSync(`${out}/fail.png`, Buffer.from(d.result.data, "base64"));
      process.exit(1);
    }
  } else if (op === "eval") console.log("value:", JSON.stringify(await evaluate(arg)));
  else if (op === "sleep") await new Promise(r => setTimeout(r, Number(arg)));
  else if (op === "key") {
    await evaluate("document.querySelector('#scbw canvas')?.focus(), 1", false);
    const [code, key] = KEYS[arg] ?? [arg.toUpperCase().charCodeAt(0), arg];
    const text = key.length === 1 ? key : undefined;
    await send("Input.dispatchKeyEvent", { type: text ? "keyDown" : "rawKeyDown", key, code: key.length === 1 ? (/[0-9]/.test(key) ? "Digit" + key : "Key" + key.toUpperCase()) : key, windowsVirtualKeyCode: code, text });
    await new Promise(r => setTimeout(r, 120));
    await send("Input.dispatchKeyEvent", { type: "keyUp", key, windowsVirtualKeyCode: code });
  } else if (op === "click") {
    const [x, y] = arg.split(",").map(Number);
    const r = await evaluate("(() => { const c = document.querySelector('#scbw canvas').getBoundingClientRect(); return [c.left, c.top, c.width, c.height]; })()", false);
    const px = r[0] + x * r[2] / 640, py = r[1] + y * r[3] / 480;
    for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) {
      await send("Input.dispatchMouseEvent", { type, x: px, y: py, button: "left", buttons: type === "mousePressed" ? 1 : 0, clickCount: 1 });
      await new Promise(r => setTimeout(r, 80));
    }
  } else if (op === "drag") {
    const [x1, y1, x2, y2] = arg.split(",").map(Number);
    const r = await evaluate("(() => { const c = document.querySelector('#scbw canvas').getBoundingClientRect(); return [c.left, c.top, c.width, c.height]; })()", false);
    const at = (x, y) => [r[0] + x * r[2] / 640, r[1] + y * r[3] / 480];
    const [ax, ay] = at(x1, y1), [bx, by] = at(x2, y2);
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: ax, y: ay });
    await send("Input.dispatchMouseEvent", { type: "mousePressed", x: ax, y: ay, button: "left", buttons: 1, clickCount: 1 });
    for (let i = 1; i <= 8; i++) {
      await new Promise(r => setTimeout(r, 60));
      await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: ax + (bx - ax) * i / 8, y: ay + (by - ay) * i / 8, button: "left", buttons: 1 });
    }
    await new Promise(r => setTimeout(r, 100));
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", x: bx, y: by, button: "left", buttons: 0, clickCount: 1 });
  } else if (op === "drive") {
    const v = JSON.stringify([process.env.SCBW_TEST_DRIVE_KEY ?? "", process.env.SCBW_TEST_DRIVE_FOLDER ?? ""]);
    await evaluate(`(() => { const [k, f] = ${v}; document.getElementById("dkey").value = k; document.getElementById("dfolder").value = f; document.getElementById("loaddrive").click(); return 1; })()`, false);
  } else if (op === "shot") {
    const d = await send("Page.captureScreenshot", { format: "png" });
    writeFileSync(`${out}/${arg}.png`, Buffer.from(d.result.data, "base64"));
  }
}
console.log("DONE");
process.exit(0);
