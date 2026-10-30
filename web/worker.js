// The game's Web Worker (started by game.js). The whole game runs here: the wasm build,
// its frame loop (worker timers keep running in a background tab, where the page's
// requestAnimationFrame stops), the drawing into the page's canvas (an OffscreenCanvas) and
// the network links. The page forwards input and plays the sound this posts.
import init, * as scbw from "./pkg/scbw.js";

// Console lines go to the page (its console is the one tests and users read).
for (const level of ["log", "info", "warn", "error", "debug"]) {
  const own = console[level].bind(console);
  console[level] = (...args) => {
    try { postMessage({ t: "log", level, args: args.map(a => typeof a === "string" ? a : String(a?.stack ?? a)) }); }
    catch { own(...args); }
  };
}

// The mixer's samples (src/audio.rs `audio_take`, 44100 Hz stereo) as wall time passes.
function startAudio() {
  let last = performance.now();
  setInterval(() => {
    const now = performance.now();
    const frames = Math.min(Math.round((now - last) * 44.1), 8192);
    last = now;
    if (frames <= 0) return;
    const s = scbw.audio_take(frames);
    postMessage({ t: "audio", samples: s }, [s.buffer]);
  }, 25);
}

onmessage = async e => {
  const m = e.data;
  switch (m.t) {
    case "key": scbw.minifb_key(m.down, m.code, m.key, m.ctrl, m.alt, m.meta); break;
    case "mouse": scbw.minifb_mouse(m.ev, m.x, m.y, m.button); break;
    case "focus": scbw.minifb_focus(m.on); break;
    case "call":
      try {
        let value;
        if (m.fn === "init") {
          await init();
          scbw.minifb_set_canvas(m.args[0]);
          startAudio();
        } else {
          value = await scbw[m.fn](...m.args);
        }
        postMessage({ t: "ret", id: m.id, value });
      } catch (err) {
        postMessage({ t: "ret", id: m.id, err: String(err?.message ?? err) });
      }
      break;
  }
};
