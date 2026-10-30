// The game's stage in a page (index.html, the campaign map): it sizes the stage and passes
// the mouse to the game (game.js `loadGame`, set with `setGame`; pointer lock, Esc).

// minifb's window: 640x480 at Scale::X2. It reads mouse positions in this space.
const WIN_W = 1280, WIN_H = 960;

// The game the mouse goes to (game.js `loadGame`'s object).
let game = null;
export function setGame(g) { game = g; }

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

  // Every mouse event goes to the game's worker in minifb's 1280x960 space, whatever size
  // the canvas is drawn at.
  // Pointer lock: a click captures the mouse (Esc frees it), so the cursor stops at the
  // canvas edges for edge scrolling and Firefox's shift+right-click menu stays away. The
  // game draws its own cursor; locked moves become a virtual position.
  let vx = 0, vy = 0;
  for (const type of ["mousedown", "mousemove", "mouseup"]) box.addEventListener(type, e => {
    const canvas = box.querySelector("canvas");
    if (!e.isTrusted || !canvas || !game) return;
    e.stopPropagation();
    const r = canvas.getBoundingClientRect();
    if (document.pointerLockElement === canvas) {
      vx = Math.min(Math.max(vx + e.movementX, 0), r.width - 1);
      vy = Math.min(Math.max(vy + e.movementY, 0), r.height - 1);
    } else {
      vx = e.clientX - r.left; vy = e.clientY - r.top;
      if (type === "mousedown") canvas.requestPointerLock()?.catch?.(() => {});   // rejects when the document is not focused
    }
    game.mouse(type, Math.floor(vx * WIN_W / r.width), Math.floor(vy * WIN_H / r.height), e.button);
  }, true);

  // Esc also frees the lock, and the browser may keep that keypress from the page. Pass it
  // to the game ourselves unless it got through; the next click locks again. A lock lost to
  // a tab switch (no focus) is not an Esc.
  let lastEsc = 0;
  box.addEventListener("keydown", e => { if (e.code === "Escape") lastEsc = performance.now(); }, true);
  document.addEventListener("pointerlockchange", () => {
    const canvas = box.querySelector("canvas");
    if (!canvas || !game || document.pointerLockElement || !document.hasFocus()) return;
    setTimeout(() => {
      if (performance.now() - lastEsc < 200) return;
      const esc = { code: "Escape", key: "Escape", ctrlKey: false, altKey: false, metaKey: false };
      game.key(true, esc);
      // Held past a 24 fps frame so the game's per-frame key poll sees it.
      setTimeout(() => game.key(false, esc), 100);
    }, 50);
  });
  return fit;
}
