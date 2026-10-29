// The game's stage in a page (index.html, the campaign map): import this before
// pkg/scbw.js. It sizes the stage, passes the mouse to minifb's canvas (pointer lock, Esc)
// and keeps the sound running after a user gesture.

// scbw.js reads AudioContext when it is imported (hence this module first). cpal resumes
// its AudioContext only once, when the stream starts; made before any user gesture the
// browser keeps it suspended. Resume every context on each gesture.
const audioContexts = [];
window.AudioContext = class extends AudioContext {
  constructor(...a) { super(...a); audioContexts.push(this); }
};
for (const type of ["pointerdown", "keydown"]) addEventListener(type, () => {
  for (const c of audioContexts) if (c.state === "suspended") c.resume();
}, true);

// minifb's window: 640x480 at Scale::X2. It reads offsetX/offsetY in this space.
const WIN_W = 1280, WIN_H = 960;

// `box` holds minifb's canvas (the window's container, id "scbw"); `stage` around it is
// sized to the largest whole scale of 640x480 that fits; `fs` (optional) a fullscreen button.
export function setupStage(box, stage, fs) {
  box.addEventListener("contextmenu", e => e.preventDefault());

  // The largest whole scale of 640x480 in device pixels that fits the space (at least 1x).
  function fit() {
    const dpr = devicePixelRatio || 1;
    const full = document.fullscreenElement === stage;
    const w = innerWidth, h = full ? innerHeight : stage.parentElement.clientHeight;
    const n = Math.max(1, Math.floor(Math.min(w * dpr / 640, h * dpr / 480)));
    stage.style.width = n * 640 / dpr + "px";
    stage.style.height = n * 480 / dpr + "px";
  }
  fit();
  addEventListener("resize", fit);
  document.addEventListener("fullscreenchange", fit);
  if (fs) fs.onclick = () => stage.requestFullscreen().then(() => box.querySelector("canvas")?.focus());

  // Every mouse event goes to minifb as an untrusted copy whose offsetX/offsetY are in
  // its 1280x960 space, whatever size the canvas is drawn at.
  // Pointer lock: a click captures the mouse (Esc frees it), so the cursor stops at the
  // canvas edges for edge scrolling and Firefox's shift+right-click menu stays away. The
  // game draws its own cursor; locked moves become a virtual position.
  let vx = 0, vy = 0;
  for (const type of ["mousedown", "mousemove", "mouseup"]) box.addEventListener(type, e => {
    const canvas = box.querySelector("canvas");
    if (!e.isTrusted || !canvas) return;
    e.stopPropagation();
    const r = canvas.getBoundingClientRect();
    if (document.pointerLockElement === canvas) {
      vx = Math.min(Math.max(vx + e.movementX, 0), r.width - 1);
      vy = Math.min(Math.max(vy + e.movementY, 0), r.height - 1);
    } else {
      vx = e.clientX - r.left; vy = e.clientY - r.top;
      if (type === "mousedown") canvas.requestPointerLock();
    }
    const ev = new MouseEvent(type, {
      clientX: r.left + vx, clientY: r.top + vy, button: e.button, buttons: e.buttons,
      shiftKey: e.shiftKey, ctrlKey: e.ctrlKey, altKey: e.altKey, metaKey: e.metaKey,
    });
    // Firefox answers every event's client/offset coordinates with the point the lock
    // started at while the pointer is locked, so set offsetX/offsetY on the event itself.
    Object.defineProperties(ev, {
      offsetX: { value: Math.floor(vx * WIN_W / r.width) },
      offsetY: { value: Math.floor(vy * WIN_H / r.height) },
    });
    canvas.dispatchEvent(ev);
  }, true);

  // Esc also frees the lock, and the browser may keep that keypress from the page. Pass it
  // to the game ourselves unless it got through; the next click locks again. A lock lost to
  // a tab switch (no focus) is not an Esc.
  let lastEsc = 0;
  box.addEventListener("keydown", e => { if (e.code === "Escape") lastEsc = performance.now(); }, true);
  document.addEventListener("pointerlockchange", () => {
    const canvas = box.querySelector("canvas");
    if (!canvas || document.pointerLockElement || !document.hasFocus()) return;
    setTimeout(() => {
      if (performance.now() - lastEsc < 200) return;
      const key = type => canvas.dispatchEvent(new KeyboardEvent(type, { code: "Escape", key: "Escape" }));
      key("keydown");
      // Held past a 24 fps frame so the game's per-frame key poll sees it.
      setTimeout(() => key("keyup"), 100);
    }, 50);
  });
  return fit;
}
