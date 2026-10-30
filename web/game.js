// The game in a page (index.html, the campaign map): `loadGame(box)` puts a canvas in `box`,
// hands it to the game's Web Worker (worker.js) as an OffscreenCanvas and returns the
// worker's calls (start, net_join, campaign_load, campaign_play, rbtest) as promises. The
// page side forwards the keyboard (the canvas's) and the mouse (stage.js `setupStage`) and
// plays the sound the worker mixes.

const RATE = 44100;

// Every ArrayBuffer under `v` (Uint8Arrays, arrays of them): moved to the worker, not copied.
function buffers(v, out = new Set()) {
  if (ArrayBuffer.isView(v)) out.add(v.buffer);
  else if (Array.isArray(v)) v.forEach(x => buffers(x, out));
  return out;
}

// The worker's samples, played back to back a little ahead of the clock. The browser
// keeps a context made before any user gesture suspended: resume it on each gesture.
function audioOut() {
  const ctx = new AudioContext({ sampleRate: RATE });
  for (const type of ["pointerdown", "keydown"]) addEventListener(type, () => {
    if (ctx.state === "suspended") ctx.resume();
  }, true);
  let next = 0;
  return samples => {
    if (ctx.state !== "running") return;
    const n = samples.length / 2;
    const now = ctx.currentTime;
    // Fell behind (or first chunk): start 60 ms ahead; far ahead (a stall): drop the chunk.
    if (next < now) next = now + 0.06;
    if (next > now + 0.4) return;
    const buf = ctx.createBuffer(2, n, RATE);
    const l = buf.getChannelData(0), r = buf.getChannelData(1);
    for (let i = 0; i < n; i++) { l[i] = samples[2 * i]; r[i] = samples[2 * i + 1]; }
    const src = ctx.createBufferSource();
    src.buffer = buf;
    src.connect(ctx.destination);
    src.start(next);
    next += n / RATE;
  };
}

export async function loadGame(box) {
  const canvas = document.createElement("canvas");
  canvas.tabIndex = 0;
  canvas.width = 640;
  canvas.height = 480;
  box.replaceChildren(canvas);
  const worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
  const play = audioOut();
  let seq = 0;
  const pending = new Map();
  worker.onmessage = e => {
    const m = e.data;
    if (m.t === "ret") {
      const p = pending.get(m.id);
      pending.delete(m.id);
      if (m.err !== undefined) p.reject(new Error(m.err)); else p.resolve(m.value);
    } else if (m.t === "log") console[m.level](...m.args);
    else if (m.t === "audio") play(m.samples);
  };
  worker.onerror = e => console.error("game worker: " + (e.message ?? e));
  const call = (fn, ...args) => new Promise((resolve, reject) => {
    const id = ++seq;
    pending.set(id, { resolve, reject });
    worker.postMessage({ t: "call", id, fn, args }, [...buffers(args)]);
  });
  const off = canvas.transferControlToOffscreen();
  await new Promise((resolve, reject) => {
    const id = ++seq;
    pending.set(id, { resolve, reject });
    worker.postMessage({ t: "call", id, fn: "init", args: [off] }, [off]);
  });

  const key = (down, e) => worker.postMessage({ t: "key", down, code: e.code, key: e.key, ctrl: e.ctrlKey, alt: e.altKey, meta: e.metaKey });
  canvas.addEventListener("keydown", e => { e.preventDefault(); key(true, e); });
  canvas.addEventListener("keyup", e => { e.preventDefault(); key(false, e); });
  canvas.addEventListener("focus", () => worker.postMessage({ t: "focus", on: true }));
  canvas.addEventListener("blur", () => worker.postMessage({ t: "focus", on: false }));

  return {
    canvas,
    // `type` mousedown / mousemove / mouseup; (x, y) in the window's 1280x960 space.
    mouse(type, x, y, button) {
      worker.postMessage({ t: "mouse", ev: type === "mousedown" ? 1 : type === "mouseup" ? 2 : 0, x, y, button });
    },
    key,
    start: (...a) => call("start", ...a),
    rbtest: (...a) => call("rbtest", ...a),
    net_join: (...a) => call("net_join", ...a),
    campaign_load: (...a) => call("campaign_load", ...a),
    campaign_play: (...a) => call("campaign_play", ...a),
  };
}
