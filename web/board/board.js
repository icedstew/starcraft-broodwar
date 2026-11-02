// Campaign board editor (ours; agent/plans/campaign-board.md). Edits a world.txt (areas,
// borders; other lines kept as text) and its board JSON (image, polygons, squares).
"use strict";

const COLOURS = ["#f40404", "#0c48cc", "#2cb494", "#88409c", "#f88c14", "#703014", "#cce0d0", "#fcfc38", "#088008", "#fcfc7c", "#ecc4b0", "#4068d4"];
const DEFAULT_HEAD = "faction Z 0 Zerg\nfaction T 1 Terran\nfaction P 2 Protoss\npool maps\neliminated pick\n";
const R = 20; // area circle radius (image px) when it has no polygon, as the live map

// ---- world.txt (same grammar as src/campaign/world.rs) ----

/// { head: text of the other lines, areas: [{x, y, owner, name}], borders: [{a, b, map}], board }
function parseWorld(text) {
  const w = { head: "", areas: [], borders: [], board: null };
  for (const raw of text.split("\n")) {
    const line = raw.split("#")[0].trim();
    const f = line.split(/\s+/);
    if (f[0] === "area") w.areas.push({ x: +f[1] || 0, y: +f[2] || 0, owner: f[3] === "-" || f[3] === undefined ? -1 : +f[3], name: f.slice(4).join(" ") });
    else if (f[0] === "border") w.borders.push({ a: +f[1], b: +f[2], map: f.slice(3).join(" ") });
    else if (f[0] === "board") w.board = line.slice(5).trim() || null;
    else if (line) w.head += line + "\n";
  }
  return w;
}

function writeWorld(w) {
  let s = w.head.trim() + "\n";
  for (const a of w.areas) s += `area ${Math.round(a.x)} ${Math.round(a.y)} ${a.owner < 0 ? "-" : a.owner} ${a.name}\n`;
  for (const b of w.borders) s += `border ${b.a} ${b.b} ${b.map}`.trimEnd() + "\n";
  if (w.board) s += `board ${w.board}\n`;
  return s;
}

const factionsOf = head => head.split("\n").map(l => l.split("#")[0].trim().split(/\s+/)).filter(f => f[0] === "faction").map(f => ({ race: f[1], colour: +f[2], name: f.slice(3).join(" ") }));

// ---- flood fill -> polygon ----

/// Scanline flood fill from (sx, sy) over pixels whose RGB is within `tol` (max channel) of
/// the seed's. Sets 1 in `mask` (W*H).
function floodFill(img, sx, sy, tol, mask) {
  const { width: W, height: H, data: d } = img;
  const i0 = (sy * W + sx) * 4, [r, g, b] = [d[i0], d[i0 + 1], d[i0 + 2]];
  const seen = new Uint8Array(W * H);
  const ok = p => !seen[p] && Math.max(Math.abs(d[p * 4] - r), Math.abs(d[p * 4 + 1] - g), Math.abs(d[p * 4 + 2] - b)) <= tol;
  const stack = [sy * W + sx];
  while (stack.length) {
    const p = stack.pop();
    if (!ok(p)) continue;
    const y = (p / W) | 0;
    let l = p, rr = p;
    while (l % W > 0 && ok(l - 1)) l--;
    while (rr % W < W - 1 && ok(rr + 1)) rr++;
    for (let q = l; q <= rr; q++) {
      seen[q] = 1;
      mask[q] = 1;
      if (y > 0 && ok(q - W)) stack.push(q - W);
      if (y < H - 1 && ok(q + W)) stack.push(q + W);
    }
  }
}

/// `n` passes of 8-neighbour growth (closes the gap an outline line leaves between areas).
function dilate(mask, W, H, n) {
  for (let k = 0; k < n; k++) {
    const m = mask.slice();
    for (let y = 0; y < H; y++)
      for (let x = 0; x < W; x++) {
        if (m[y * W + x]) continue;
        for (let dy = -1; dy <= 1 && !mask[y * W + x]; dy++)
          for (let dx = -1; dx <= 1; dx++) {
            const X = x + dx, Y = y + dy;
            if (X >= 0 && Y >= 0 && X < W && Y < H && m[Y * W + X]) { mask[y * W + x] = 1; break; }
          }
      }
  }
}

/// Sets every pixel not reachable from the image edge through unset pixels (holes: text, specks).
function fillHoles(mask, W, H) {
  const out = new Uint8Array(W * H), stack = [];
  const push = p => { if (!mask[p] && !out[p]) { out[p] = 1; stack.push(p); } };
  for (let x = 0; x < W; x++) { push(x); push((H - 1) * W + x); }
  for (let y = 0; y < H; y++) { push(y * W); push(y * W + W - 1); }
  while (stack.length) {
    const p = stack.pop(), x = p % W;
    if (x > 0) push(p - 1);
    if (x < W - 1) push(p + 1);
    if (p >= W) push(p - W);
    if (p < (H - 1) * W) push(p + W);
  }
  for (let p = 0; p < W * H; p++) if (!out[p]) mask[p] = 1;
}

// Clockwise in screen coordinates (y down), from west.
const DIRS = [[-1, 0], [-1, -1], [0, -1], [1, -1], [1, 0], [1, 1], [0, 1], [-1, 1]];
const dirIndex = (dx, dy) => DIRS.findIndex(d => d[0] === dx && d[1] === dy);

/// Moore-neighbour trace of the outer boundary of the (hole-free) component holding (sx, sy).
function trace(mask, W, H, sx, sy) {
  const set = (x, y) => x >= 0 && y >= 0 && x < W && y < H && mask[y * W + x] === 1;
  if (!set(sx, sy)) return [];
  while (set(sx, sy - 1)) sy--;
  const pts = [[sx, sy]];
  let [cx, cy, d] = [sx, sy, 2];
  for (let guard = 4 * W * H; guard > 0; guard--) {
    let k = 1;
    for (; k <= 8; k++) if (set(cx + DIRS[(d + k) % 8][0], cy + DIRS[(d + k) % 8][1])) break;
    if (k > 8) break; // a single pixel
    const n = (d + k) % 8, p = (n + 7) % 8;
    const [nx, ny] = [cx + DIRS[n][0], cy + DIRS[n][1]];
    // Back at the start about to repeat the first step: closed.
    if (cx === sx && cy === sy && pts.length > 1 && nx === pts[1][0] && ny === pts[1][1]) break;
    d = dirIndex(DIRS[p][0] - DIRS[n][0], DIRS[p][1] - DIRS[n][1]);
    [cx, cy] = [nx, ny];
    pts.push([cx, cy]);
  }
  return pts.map(([x, y]) => [x + 0.5, y + 0.5]);
}

/// Douglas-Peucker on a closed ring: split at the first point and the one farthest from it.
function simplify(pts, eps) {
  if (pts.length < 4) return pts;
  // Start at the leftmost (then topmost) point: always a corner, so DP keeps only corners.
  let m = 0;
  pts.forEach((p, i) => { if (p[0] < pts[m][0] || (p[0] === pts[m][0] && p[1] < pts[m][1])) m = i; });
  pts = pts.slice(m).concat(pts.slice(0, m));
  const dist = (p, a, b) => {
    const [dx, dy] = [b[0] - a[0], b[1] - a[1]], L = Math.hypot(dx, dy);
    return L === 0 ? Math.hypot(p[0] - a[0], p[1] - a[1]) : Math.abs(dy * p[0] - dx * p[1] + b[0] * a[1] - b[1] * a[0]) / L;
  };
  const dp = (s, e, out) => { // s..e inclusive, pushes s..e-1
    let best = -1, bi = -1;
    for (let i = s + 1; i < e; i++) { const v = dist(pts[i % pts.length], pts[s], pts[e % pts.length]); if (v > best) [best, bi] = [v, i]; }
    if (best > eps) { dp(s, bi, out); dp(bi, e, out); } else out.push(pts[s]);
  };
  let far = 0;
  pts.forEach((p, i) => { if (Math.hypot(p[0] - pts[0][0], p[1] - pts[0][1]) > Math.hypot(pts[far][0] - pts[0][0], pts[far][1] - pts[0][1])) far = i; });
  const out = [];
  dp(0, far, out);
  dp(far, pts.length, out);
  return out;
}

/// The polygon of the region at (x, y): fill, grow, fill holes, trace, simplify. The outline
/// traced is the one around `at` (the area's point) when the region holds it, else (x, y).
function regionPolygon(img, x, y, o, mask, at) {
  const { width: W, height: H } = img;
  mask = mask || new Uint8Array(W * H);
  floodFill(img, x, y, o.tol, mask);
  const m = mask.slice();
  dilate(m, W, H, o.grow);
  fillHoles(m, W, H);
  const [tx, ty] = at && at[0] >= 0 && at[1] >= 0 && at[0] < W && at[1] < H && m[at[1] * W + at[0]] ? at : [x, y];
  return { mask, poly: simplify(trace(m, W, H, tx, ty), o.eps).map(p => p.map(v => Math.round(v * 10) / 10)) };
}

function polyArea(p) {
  let s = 0;
  for (let i = 0; i < p.length; i++) { const [a, b] = [p[i], p[(i + 1) % p.length]]; s += a[0] * b[1] - b[0] * a[1]; }
  return s / 2;
}

function centroid(p) {
  const A = polyArea(p);
  if (!A) return p[0];
  let cx = 0, cy = 0;
  for (let i = 0; i < p.length; i++) { const [a, b] = [p[i], p[(i + 1) % p.length]], c = a[0] * b[1] - b[0] * a[1]; cx += (a[0] + b[0]) * c; cy += (a[1] + b[1]) * c; }
  return [cx / (6 * A), cy / (6 * A)];
}

function inside(p, [x, y]) {
  let r = false;
  for (let i = 0, j = p.length - 1; i < p.length; j = i++)
    if ((p[i][1] > y) !== (p[j][1] > y) && x < (p[j][0] - p[i][0]) * (y - p[i][1]) / (p[j][1] - p[i][1]) + p[i][0]) r = !r;
  return r;
}

function segDist([px, py], [ax, ay], [bx, by]) {
  const [dx, dy] = [bx - ax, by - ay], L = dx * dx + dy * dy;
  const t = L ? Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / L)) : 0;
  return Math.hypot(px - ax - t * dx, py - ay - t * dy);
}

// ---- self-test (?test) ----

function selfTest() {
  const res = [];
  const ok = (c, m) => res.push((c ? "ok   " : "FAIL ") + m);
  const cv = Object.assign(document.createElement("canvas"), { width: 100, height: 80 });
  const g = cv.getContext("2d");
  g.fillStyle = "#fff"; g.fillRect(0, 0, 100, 80);
  g.fillStyle = "#000"; g.fillRect(10, 10, 30, 20);             // rectangle 30x20
  g.fillRect(50, 10, 40, 10); g.fillRect(50, 20, 10, 40);        // L
  g.fillStyle = "#f00"; g.fillRect(20, 15, 3, 3);                // a speck inside: a hole
  const img = g.getImageData(0, 0, 100, 80), o = { tol: 10, grow: 0, eps: 1 };
  const rect = regionPolygon(img, 12, 12, o).poly;
  ok(rect.length === 4 && Math.abs(Math.abs(polyArea(rect)) - 29 * 19) < 1, `rectangle: ${rect.length} points, area ${polyArea(rect)}`);
  const L = regionPolygon(img, 52, 12, o).poly;
  ok(L.length === 6 && Math.abs(Math.abs(polyArea(L)) / (39 * 9 + 9 * 40) - 1) < 0.03, `L: ${L.length} points, area ${polyArea(L)}`);
  const grown = regionPolygon(img, 12, 12, { ...o, grow: 2 }).poly;
  ok(Math.abs(polyArea(grown)) >= 33 * 23, `grow 2: area ${polyArea(grown)}`);
  const w = "faction Z 0 Zerg\nfaction T 1 Terran\npool maps\neliminated pick\narea 0 0 0 A\narea 10 0 1 B b\nborder 0 1\nborder 1 0 x.scx\nboard w.board.json\n";
  ok(writeWorld(parseWorld(w)) === w, "world.txt round trip");
  document.body.innerHTML = `<pre style="padding:16px">${res.join("\n")}</pre>`;
  document.title = res.some(r => r.startsWith("FAIL")) ? "FAIL" : "PASS";
}

if (new URLSearchParams(location.search).has("test")) { addEventListener("load", selfTest); throw "self-test"; }

// ---- editor state ----

const $ = id => document.getElementById(id);
const cv = $("cv"), ctx = cv.getContext("2d");
let world = parseWorld(DEFAULT_HEAD);
let board = { image: "", size: [1600, 1000], fill_alpha: 0.45, outline_alpha: 0.9, areas: [], borders: [] };
let worldName = "world.txt";
let image = null, pixels = null;         // HTMLImageElement, ImageData
let mode = "areas", preview = false;
let selA = -1, selB = -1, pending = -1;  // selected area, selected border, first area of a connect
let fillMask = null, fillArea = -1;      // last fill's mask, for Shift-add
let view = { s: 1, x: 0, y: 0 };
const undoStack = [];

const snapshot = () => JSON.stringify({ world, board });
function pushUndo() { undoStack.push(snapshot()); if (undoStack.length > 100) undoStack.shift(); }
function undo() { const s = undoStack.pop(); if (!s) return; ({ world, board } = JSON.parse(s)); fillMask = null; changed(); }

/// Board entries for every area / border, so indices always line up.
function syncBoard() {
  while (board.areas.length < world.areas.length) board.areas.push({});
  board.areas.length = world.areas.length;
  while (board.borders.length < world.borders.length) board.borders.push({});
  board.borders.length = world.borders.length;
}

const factions = () => factionsOf(world.head);
const ownerColour = o => { const f = factions()[o]; return o < 0 || !f ? "#444" : COLOURS[f.colour % 12] || "#999"; };
const pt = i => [world.areas[i].x, world.areas[i].y];
function borderPts(i) {
  const b = world.borders[i];
  return [pt(b.a), ...(board.borders[i].path || []), pt(b.b)];
}
function squareOf(i) {
  if (board.borders[i].square) return board.borders[i].square;
  const p = borderPts(i);
  if (p.length % 2) return p[(p.length - 1) / 2];
  const [a, b] = [p[p.length / 2 - 1], p[p.length / 2]];
  return [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
}

// ---- drawing ----

function resize() {
  const r = cv.getBoundingClientRect(), dpr = devicePixelRatio || 1;
  cv.width = r.width * dpr; cv.height = r.height * dpr;
  draw();
}

function fitView() {
  const r = cv.getBoundingClientRect();
  let [w, h] = board.size, x0 = 0, y0 = 0;
  if (!image && world.areas.length) {
    const xs = world.areas.map(a => a.x), ys = world.areas.map(a => a.y);
    [x0, y0] = [Math.min(...xs) - 60, Math.min(...ys) - 60];
    [w, h] = [Math.max(...xs) + 60 - x0, Math.max(...ys) + 60 - y0];
  }
  view.s = Math.min(r.width / w, r.height / h) * 0.95;
  view.x = (r.width - w * view.s) / 2 - x0 * view.s;
  view.y = (r.height - h * view.s) / 2 - y0 * view.s;
  draw();
}

function draw() {
  const dpr = devicePixelRatio || 1, s = view.s;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = "#0b0f13";
  ctx.fillRect(0, 0, cv.width, cv.height);
  ctx.setTransform(s * dpr, 0, 0, s * dpr, view.x * dpr, view.y * dpr);
  if (image) ctx.drawImage(image, 0, 0);
  else { ctx.strokeStyle = "#2c3742"; ctx.lineWidth = 1 / s; ctx.strokeRect(0, 0, board.size[0], board.size[1]); }
  const fa = board.fill_alpha, oa = board.outline_alpha;
  // Polygons.
  world.areas.forEach((a, i) => {
    const p = board.areas[i].poly;
    if (!p || p.length < 3) return;
    ctx.beginPath();
    p.forEach(([x, y], k) => k ? ctx.lineTo(x, y) : ctx.moveTo(x, y));
    ctx.closePath();
    ctx.globalAlpha = fa; ctx.fillStyle = ownerColour(a.owner); ctx.fill();
    ctx.globalAlpha = oa; ctx.strokeStyle = !preview && i === selA ? "#fff" : "#000"; ctx.lineWidth = (!preview && i === selA ? 3 : 1.5) / s; ctx.stroke();
    ctx.globalAlpha = 1;
  });
  // Connections.
  world.borders.forEach((b, i) => {
    if (board.borders[i].line === false && (preview || i !== selB)) return;
    ctx.beginPath();
    borderPts(i).forEach(([x, y], k) => k ? ctx.lineTo(x, y) : ctx.moveTo(x, y));
    ctx.strokeStyle = i === selB && !preview ? "#fff" : "#5a7a8a";
    ctx.setLineDash(board.borders[i].line === false ? [6 / s, 6 / s] : []);
    ctx.lineWidth = 3 / s; ctx.stroke();
    ctx.setLineDash([]);
  });
  // Areas without polygon: circles. Names at the area point.
  world.areas.forEach((a, i) => {
    const has = board.areas[i].poly && board.areas[i].poly.length >= 3;
    if (!has) {
      ctx.beginPath(); ctx.arc(a.x, a.y, R, 0, 7);
      ctx.fillStyle = ownerColour(a.owner); ctx.fill();
      ctx.strokeStyle = i === selA && !preview ? "#fff" : i === pending ? "#39d0c0" : "#000"; ctx.lineWidth = 2 / s; ctx.stroke();
    } else if (!preview) {
      ctx.beginPath(); ctx.arc(a.x, a.y, 4 / s, 0, 7);
      ctx.fillStyle = i === pending ? "#39d0c0" : "#fff"; ctx.fill();
    }
    if (i === pending && has) { ctx.beginPath(); ctx.arc(a.x, a.y, 10 / s, 0, 7); ctx.strokeStyle = "#39d0c0"; ctx.lineWidth = 3 / s; ctx.stroke(); }
    ctx.font = `${13 / s}px system-ui, sans-serif`; ctx.textAlign = "center";
    ctx.lineWidth = 3 / s; ctx.strokeStyle = "#000"; ctx.fillStyle = "#e8eef2";
    const ty = a.y + (has ? 18 : R + 16) / (has ? s : 1);
    ctx.strokeText(a.name, a.x, ty); ctx.fillText(a.name, a.x, ty);
  });
  // Battle squares (fixed screen size, as the live map's 18 px squares).
  world.borders.forEach((b, i) => {
    const [x, y] = squareOf(i), h = 9 / s;
    ctx.fillStyle = "#c8d4dc"; ctx.fillRect(x - h, y - h, 2 * h, 2 * h);
    ctx.strokeStyle = i === selB && !preview ? "#fff" : "#000"; ctx.lineWidth = (i === selB && !preview ? 3 : 2) / s; ctx.strokeRect(x - h, y - h, 2 * h, 2 * h);
  });
  if (preview) return;
  // Selected border's bend points; selected area's polygon points (Polygon mode).
  if (selB >= 0) for (const [x, y] of board.borders[selB].path || []) { ctx.fillStyle = "#39d0c0"; ctx.fillRect(x - 4 / s, y - 4 / s, 8 / s, 8 / s); }
  if (mode === "poly" && selA >= 0) {
    const p = board.areas[selA].poly || [];
    if (p.length && p.length < 3) { ctx.beginPath(); p.forEach(([x, y], k) => k ? ctx.lineTo(x, y) : ctx.moveTo(x, y)); ctx.strokeStyle = "#fff"; ctx.lineWidth = 2 / s; ctx.stroke(); }
    p.forEach(([x, y], k) => { ctx.beginPath(); ctx.arc(x, y, 5 / s, 0, 7); ctx.fillStyle = k === hoverV ? "#f0b040" : "#fff"; ctx.fill(); ctx.strokeStyle = "#000"; ctx.lineWidth = 1 / s; ctx.stroke(); });
  }
}

// ---- side panel ----

const esc = s => String(s).replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

function panel() {
  const fs = factions();
  $("areas").innerHTML = world.areas.map((a, i) => `<div data-i="${i}" class="${i === selA ? "sel" : ""}"><span style="color:${ownerColour(a.owner)}">■</span>${i} ${esc(a.name)}${image && !(board.areas[i].poly && board.areas[i].poly.length >= 3) ? '<span class="np">no polygon</span>' : ""}</div>`).join("");
  if (selA >= 0) {
    const a = world.areas[selA];
    $("areaedit").innerHTML = `<div class="row"><span>Name</span><input type="text" id="aname" value="${esc(a.name)}"></div>
      <div class="row"><span>Owner</span><select id="aowner"><option value="-1">- (dealt at start)</option>${fs.map((f, i) => `<option value="${i}" ${a.owner === i ? "selected" : ""}>${i} ${esc(f.name)}</option>`).join("")}</select></div>
      <div class="dim">Point ${Math.round(a.x)}, ${Math.round(a.y)} · ${(board.areas[selA].poly || []).length} polygon points</div>`;
    $("aname").onchange = e => { pushUndo(); a.name = e.target.value.trim() || `Area ${selA + 1}`; changed(); };
    $("aowner").onchange = e => { pushUndo(); a.owner = +e.target.value; changed(); };
  } else $("areaedit").innerHTML = "No area selected.";
  if (selB >= 0) {
    const b = world.borders[selB], bb = board.borders[selB];
    $("borderedit").innerHTML = `<div>${esc(world.areas[b.a].name)} – ${esc(world.areas[b.b].name)}</div>
      <div class="row"><span>Map</span><input type="text" id="bmap" value="${esc(b.map)}" placeholder="(a pool map per match)"></div>
      <div class="row"><label><input type="checkbox" id="bline" ${bb.line === false ? "" : "checked"}> Draw line</label></div>
      <div class="row"><button id="bsq">Reset square</button><button id="bbend">Clear bends</button><button id="bdel">Delete</button></div>
      <div class="dim">Shift-click adds a bend point; drag square and bends.</div>`;
    $("bmap").onchange = e => { pushUndo(); b.map = e.target.value.trim(); changed(); };
    $("bline").onchange = e => { pushUndo(); if (e.target.checked) delete bb.line; else bb.line = false; changed(); };
    $("bsq").onclick = () => { pushUndo(); delete bb.square; changed(); };
    $("bbend").onclick = () => { pushUndo(); delete bb.path; changed(); };
    $("bdel").onclick = () => { pushUndo(); world.borders.splice(selB, 1); board.borders.splice(selB, 1); selB = -1; changed(); };
  } else $("borderedit").innerHTML = "No border selected.";
  if (document.activeElement !== $("head")) $("head").value = world.head;
  for (const k of ["fill_alpha", "outline_alpha"]) { const el = $(k === "fill_alpha" ? "falpha" : "oalpha"); el.value = board[k]; $(el.id + "v").textContent = board[k]; }
  for (const k of ["tol", "grow", "eps"]) $(k + "v").textContent = $(k).value;
  document.querySelectorAll("[data-mode]").forEach(b => b.classList.toggle("on", b.dataset.mode === mode));
  $("preview").classList.toggle("on", preview);
  $("hint").textContent = {
    areas: "Click an area to select, drag its point to move. Wheel zooms, drag empty space pans.",
    fill: selA < 0 ? "Select an area first (list on the right)." : `Click inside ${world.areas[selA].name}'s region on the image. Shift-click adds to it.`,
    poly: selA < 0 ? "Select an area first." : "Drag points; click an edge to insert; Delete removes the point under the cursor. With < 3 points, clicks add points.",
    connect: pending >= 0 ? `Now click the other area for ${world.areas[pending].name}.` : "Click two areas to add or remove their border. Click a square to select a border.",
  }[mode];
  checks();
}

function checks() {
  const w = [], fs = factions(), n = world.areas.length;
  if (fs.length < 2 || fs.length > 8) w.push("2 to 8 factions (faction lines).");
  const owned = world.areas.filter(a => a.owner >= 0).length;
  if (owned && owned !== n) w.push("Give every area an owner or none.");
  if (world.areas.some(a => a.owner >= fs.length)) w.push("An area's owner is not a faction.");
  const pool = /^\s*pool\s+\S/m.test(world.head);
  if (!pool && world.borders.some(b => !b.map)) w.push("Borders without a map and no pool line.");
  if (image) { const k = board.areas.filter(a => !(a.poly && a.poly.length >= 3)).length; if (k) w.push(`${k} area(s) without a polygon (drawn as circles).`); }
  // Connected graph (deal needs it).
  if (n) {
    const seen = new Set([0]), q = [0];
    while (q.length) { const a = q.pop(); for (const b of world.borders) for (const [x, y] of [[b.a, b.b], [b.b, b.a]]) if (x === a && !seen.has(y)) { seen.add(y); q.push(y); } }
    if (seen.size !== n) w.push(`Not connected: ${n - seen.size} area(s) unreachable from ${world.areas[0].name}.`);
  }
  if (image && (board.size[0] !== image.naturalWidth || board.size[1] !== image.naturalHeight)) w.push("Board size differs from the image.");
  $("warn").innerHTML = w.length ? w.map(m => `<div>${esc(m)}</div>`).join("") : '<span class="dim">None.</span>';
}

function changed() { syncBoard(); if (selA >= world.areas.length) selA = -1; if (selB >= world.borders.length) selB = -1; panel(); draw(); }

// ---- editing ----

function hitArea(p) {
  for (let i = world.areas.length - 1; i >= 0; i--) {
    const a = world.areas[i];
    if (Math.hypot(a.x - p[0], a.y - p[1]) <= Math.max(R, 8 / view.s)) return i;
  }
  for (let i = world.areas.length - 1; i >= 0; i--) { const poly = board.areas[i].poly; if (poly && poly.length >= 3 && inside(poly, p)) return i; }
  return -1;
}
const hitSquare = p => world.borders.findIndex((b, i) => { const [x, y] = squareOf(i); return Math.abs(x - p[0]) <= 10 / view.s && Math.abs(y - p[1]) <= 10 / view.s; });
const near = (a, b, r = 7) => Math.hypot(a[0] - b[0], a[1] - b[1]) <= r / view.s;

function toggleBorder(a, b) {
  pushUndo();
  const i = world.borders.findIndex(x => (x.a === a && x.b === b) || (x.a === b && x.b === a));
  if (i >= 0) { world.borders.splice(i, 1); board.borders.splice(i, 1); selB = -1; }
  else { world.borders.push({ a, b, map: "" }); board.borders.push({}); selB = world.borders.length - 1; }
}

function doFill(p, add) {
  if (selA < 0 || !pixels) { status(selA < 0 ? "select an area first" : "load an image first"); return; }
  const [x, y] = p.map(Math.floor);
  if (x < 0 || y < 0 || x >= pixels.width || y >= pixels.height) return;
  const t = performance.now();
  const keep = add && fillArea === selA ? fillMask : null;
  const a = world.areas[selA];
  const { mask, poly } = regionPolygon(pixels, x, y, { tol: +$("tol").value, grow: +$("grow").value, eps: +$("eps").value }, keep, keep && [a.x, a.y].map(Math.floor));
  if (poly.length < 3) { status("region too small"); return; }
  pushUndo();
  [fillMask, fillArea] = [mask, selA];
  board.areas[selA].poly = poly;
  if (!inside(poly, [a.x, a.y])) { const c = centroid(poly); [a.x, a.y] = inside(poly, c) ? c.map(Math.round) : [x, y]; }
  status(`polygon: ${poly.length} points (${Math.round(performance.now() - t)} ms)`);
  changed();
}

function addArea() {
  pushUndo();
  const r = cv.getBoundingClientRect();
  const p = toWorld(r.width / 2, r.height / 2).map(Math.round);
  const names = new Set(world.areas.map(a => a.name));
  let k = world.areas.length + 1;
  while (names.has(`Area ${k}`)) k++;
  world.areas.push({ x: p[0], y: p[1], owner: world.areas.some(a => a.owner >= 0) ? 0 : -1, name: `Area ${k}` });
  selA = world.areas.length - 1;
  changed();
}

function delArea() {
  if (selA < 0) return;
  pushUndo();
  const d = selA;
  const keep = world.borders.map((b, i) => [b, board.borders[i]]).filter(([b]) => b.a !== d && b.b !== d);
  world.borders = keep.map(([b]) => ({ ...b, a: b.a > d ? b.a - 1 : b.a, b: b.b > d ? b.b - 1 : b.b }));
  board.borders = keep.map(([, bb]) => bb);
  world.areas.splice(d, 1); board.areas.splice(d, 1);
  selA = -1; selB = -1; pending = -1; fillMask = null;
  changed();
}

function fitAreas() {
  if (!world.areas.length) return;
  pushUndo();
  const xs = world.areas.map(a => a.x), ys = world.areas.map(a => a.y);
  const [x0, y0] = [Math.min(...xs), Math.min(...ys)], [w, h] = [Math.max(...xs) - x0 || 1, Math.max(...ys) - y0 || 1];
  const [W, H] = board.size, k = Math.min(W * 0.8 / w, H * 0.8 / h);
  for (const a of world.areas) { a.x = Math.round(W / 2 + (a.x - x0 - w / 2) * k); a.y = Math.round(H / 2 + (a.y - y0 - h / 2) * k); }
  changed();
}

/// Area points to each polygon's most interior spot (labels sit well inside); each square to
/// the middle of the two areas' shared border, or of the line for areas that don't touch;
/// then squares closer than `gap` pushed apart. Squares of hidden lines are left as placed.
function autoLayout() {
  pushUndo();
  const polys = world.areas.map((a, i) => board.areas[i].poly && board.areas[i].poly.length >= 3 ? board.areas[i].poly : null);
  const edgeDist = (p, poly) => Math.min(...poly.map((q, k) => segDist(p, q, poly[(k + 1) % poly.length])));
  // Two passes: the spot also keeps away from the other areas' points (their labels).
  for (let pass = 0; pass < 2; pass++) world.areas.forEach((a, i) => {
    const P = polys[i];
    if (!P) return;
    // Labels are wide: a neighbour beside counts closer than one above or below.
    const score = p => Math.min(edgeDist(p, P), 0.5 * Math.min(Infinity, ...world.areas.filter((o, j) => j !== i).map(o => Math.hypot((o.x - p[0]) / 3, o.y - p[1]))));
    const xs = P.map(p => p[0]), ys = P.map(p => p[1]);
    const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    let best = [a.x, a.y], bd = inside(P, best) && pass ? score(best) : -1;
    for (let gx = 0.5; gx < 30; gx++)
      for (let gy = 0.5; gy < 30; gy++) {
        const p = [x0 + (x1 - x0) * gx / 30, y0 + (y1 - y0) * gy / 30];
        if (!inside(P, p)) continue;
        const d = score(p);
        if (d > bd) [best, bd] = [p, d];
      }
    [a.x, a.y] = best.map(Math.round);
  });
  const near = 3 + +$("grow").value * 2; // fills overlap by the grow, so touching outlines are this close
  world.borders.forEach((b, i) => {
    const bb = board.borders[i];
    if (bb.line === false) return;
    delete bb.square;
    const [A, B] = [polys[b.a], polys[b.b]];
    if (!A || !B) return;
    const shared = A.filter(p => edgeDist(p, B) <= near).concat(B.filter(p => edgeDist(p, A) <= near));
    if (!shared.length) return;
    const c = [shared.reduce((s, p) => s + p[0], 0) / shared.length, shared.reduce((s, p) => s + p[1], 0) / shared.length];
    bb.square = shared.reduce((m, p) => Math.hypot(p[0] - c[0], p[1] - c[1]) < Math.hypot(m[0] - c[0], m[1] - c[1]) ? p : m).map(Math.round);
  });
  // Push apart: squares are ~18 screen px; keep them `gap` board px apart at a full-board view,
  // and off the area points (labels), which stay put (pinned like squares of hidden lines).
  const gap = board.size[0] / 40, nb = world.borders.length;
  const sq = world.borders.map((b, i) => squareOf(i).slice()).concat(world.areas.map(a => [a.x, a.y]));
  const pinned = i => i >= nb || board.borders[i].line === false;
  for (let it = 0; it < 60; it++)
    for (let i = 0; i < sq.length; i++)
      for (let j = i + 1; j < sq.length; j++) {
        const [dx, dy] = [sq[j][0] - sq[i][0], sq[j][1] - sq[i][1]], d = Math.hypot(dx, dy) || 1;
        if (d >= gap) continue;
        const m = (gap - d) / 2 / d, [ux, uy] = d === 1 && !dx && !dy ? [m, 0] : [dx * m, dy * m];
        const fi = pinned(i) ? 0 : 1, fj = pinned(j) ? 0 : 1;
        if (!fi && !fj) continue;
        sq[i][0] -= ux * fi * (fj ? 1 : 2); sq[i][1] -= uy * fi * (fj ? 1 : 2);
        sq[j][0] += ux * fj * (fi ? 1 : 2); sq[j][1] += uy * fj * (fi ? 1 : 2);
      }
  sq.slice(0, nb).forEach((p, i) => { if (!pinned(i)) board.borders[i].square = p.map(Math.round); });
  changed();
  status("auto layout: area points centred, squares on shared borders");
}

// ---- mouse ----

const toWorld = (sx, sy) => [(sx - view.x) / view.s, (sy - view.y) / view.s];
let drag = null, hoverV = -1, spaceDown = false;

cv.addEventListener("wheel", e => {
  e.preventDefault();
  const k = Math.exp(-e.deltaY * 0.0015), [wx, wy] = toWorld(e.offsetX, e.offsetY);
  view.s = Math.min(40, Math.max(0.02, view.s * k));
  view.x = e.offsetX - wx * view.s; view.y = e.offsetY - wy * view.s;
  draw();
}, { passive: false });

cv.addEventListener("mousedown", e => {
  const p = toWorld(e.offsetX, e.offsetY);
  drag = { sx: e.offsetX, sy: e.offsetY, vx: view.x, vy: view.y, moved: false, kind: "pan", shift: e.shiftKey };
  if (e.button !== 0 || spaceDown || preview) return;
  if (mode === "areas") {
    const i = hitArea(p);
    if (i >= 0 && near(pt(i), p, Math.max(R * view.s, 8))) { selA = i; drag.kind = "point"; drag.i = i; changed(); }
  } else if (mode === "poly" && selA >= 0) {
    const poly = board.areas[selA].poly || [];
    const v = poly.findIndex(q => near(q, p));
    if (v >= 0) { drag.kind = "vertex"; drag.i = v; }
    else if (poly.length >= 3) {
      const ed = poly.findIndex((q, k) => segDist(p, q, poly[(k + 1) % poly.length]) <= 6 / view.s);
      if (ed >= 0) { pushUndo(); poly.splice(ed + 1, 0, p.map(Math.round)); drag.kind = "vertex"; drag.i = ed + 1; drag.pushed = true; draw(); }
    }
  } else if (mode === "connect") {
    const bends = selB >= 0 ? board.borders[selB].path || [] : [];
    const v = bends.findIndex(q => near(q, p));
    if (v >= 0) { drag.kind = "bend"; drag.i = v; return; }
    const sq = hitSquare(p);
    if (sq >= 0) { selB = sq; pending = -1; drag.kind = "square"; drag.i = sq; changed(); }
  }
});

addEventListener("mousemove", e => {
  const r = cv.getBoundingClientRect(), [ox, oy] = [e.clientX - r.left, e.clientY - r.top];
  const p = toWorld(ox, oy);
  if (!drag) {
    if (mode === "poly" && selA >= 0) { const v = (board.areas[selA].poly || []).findIndex(q => near(q, p)); if (v !== hoverV) { hoverV = v; draw(); } }
    return;
  }
  if (!drag.moved && Math.hypot(ox - drag.sx, oy - drag.sy) < 3) return;
  if (!drag.moved && drag.kind !== "pan" && !drag.pushed) pushUndo();
  drag.moved = true;
  const q = p.map(Math.round);
  if (drag.kind === "pan") { view.x = drag.vx + ox - drag.sx; view.y = drag.vy + oy - drag.sy; }
  else if (drag.kind === "point") [world.areas[drag.i].x, world.areas[drag.i].y] = q;
  else if (drag.kind === "vertex") board.areas[selA].poly[drag.i] = q;
  else if (drag.kind === "square") board.borders[drag.i].square = q;
  else if (drag.kind === "bend") board.borders[selB].path[drag.i] = q;
  draw();
});

addEventListener("mouseup", e => {
  const d = drag;
  drag = null;
  if (!d) return;
  if (d.moved) { if (d.kind !== "pan") panel(); return; }
  if (e.target !== cv || preview || d.kind !== "pan" && d.kind !== "point") return;
  const p = toWorld(d.sx, d.sy), i = hitArea(p);
  if (mode === "areas") { selA = i; }
  else if (mode === "fill") { if (selA < 0 && i >= 0) selA = i; else doFill(p, d.shift); }
  else if (mode === "poly") {
    const poly = board.areas[selA]?.poly;
    if (selA >= 0 && (!poly || poly.length < 3)) { pushUndo(); (board.areas[selA].poly ||= []).push(p.map(Math.round)); }
    else if (i >= 0) selA = i;
  } else if (mode === "connect") {
    if (d.shift && selB >= 0) { pushUndo(); (board.borders[selB].path ||= []).push(p.map(Math.round)); }
    else if (i < 0) { pending = -1; selB = -1; }
    else if (pending < 0 || pending === i) { pending = pending === i ? -1 : i; selA = i; }
    else { toggleBorder(pending, i); pending = -1; }
  }
  changed();
});

addEventListener("keydown", e => {
  if (e.target.matches("input, textarea, select")) return;
  if (e.key === " ") { spaceDown = true; e.preventDefault(); }
  if ((e.ctrlKey || e.metaKey) && e.key === "z") { e.preventDefault(); undo(); return; }
  if ((e.key === "Delete" || e.key === "Backspace") && mode === "poly" && selA >= 0 && hoverV >= 0) {
    pushUndo(); board.areas[selA].poly.splice(hoverV, 1); hoverV = -1; changed();
  }
  if (e.key === "Escape") { pending = -1; selB = -1; changed(); }
  const m = { 1: "areas", 2: "fill", 3: "poly", 4: "connect" }[e.key];
  if (m) setMode(m);
  if (e.key === "p" || e.key === "P") { preview = !preview; changed(); }
  if (e.key === "f" || e.key === "F") fitView();
});
addEventListener("keyup", e => { if (e.key === " ") spaceDown = false; });

function setMode(m) { mode = m; pending = -1; hoverV = -1; changed(); }

// ---- files ----

function status(s) { $("status").textContent = s; }
const readText = f => f.text();

$("fworld").onchange = async e => {
  const f = e.target.files[0]; if (!f) return;
  pushUndo();
  world = parseWorld(await readText(f));
  worldName = f.name;
  board.areas = []; board.borders = [];
  selA = selB = pending = -1; fillMask = null;
  changed(); fitView();
  status(`${f.name}: ${world.areas.length} areas, ${world.borders.length} borders` + (world.board ? ` (board ${world.board}: load it with Board…)` : ""));
  e.target.value = "";
};

$("fimage").onchange = e => {
  const f = e.target.files[0]; if (!f) return;
  if (f.type && f.type !== "image/png") { status("PNG only"); return; }
  const img = new Image();
  img.onload = () => {
    image = img;
    const c = Object.assign(document.createElement("canvas"), { width: img.naturalWidth, height: img.naturalHeight });
    const g = c.getContext("2d", { willReadFrequently: true });
    g.drawImage(img, 0, 0);
    pixels = g.getImageData(0, 0, c.width, c.height);
    board.image = f.name;
    board.size = [img.naturalWidth, img.naturalHeight];
    fillMask = null;
    changed(); fitView();
    status(`${f.name}: ${board.size[0]} × ${board.size[1]}`);
  };
  img.src = URL.createObjectURL(f);
  e.target.value = "";
};

$("fboard").onchange = async e => {
  const f = e.target.files[0]; if (!f) return;
  try {
    const b = JSON.parse(await readText(f));
    pushUndo();
    const mismatch = (b.areas || []).length !== world.areas.length || (b.borders || []).length !== world.borders.length;
    board = { image: b.image || "", size: b.size || board.size, fill_alpha: b.fill_alpha ?? 0.45, outline_alpha: b.outline_alpha ?? 0.9, areas: b.areas || [], borders: b.borders || [] };
    changed(); fitView();
    status(mismatch ? `${f.name}: area / border counts differ from the world (padded / cut)` : `${f.name} loaded${image ? "" : `; now load ${board.image}`}`);
  } catch (err) { status(`${f.name}: ${err.message}`); }
  e.target.value = "";
};

/// The board JSON: entries keep only what is set (no polygon = a circle, no square = the midpoint).
function boardJson() {
  const r = v => Math.round(v);
  const out = {
    image: board.image, size: board.size, fill_alpha: +board.fill_alpha, outline_alpha: +board.outline_alpha,
    areas: board.areas.map(a => a.poly && a.poly.length >= 3 ? { poly: a.poly.map(p => p.map(r)) } : {}),
    borders: board.borders.map(b => {
      const o = {};
      if (b.square) o.square = b.square.map(r);
      if (b.path && b.path.length) o.path = b.path.map(p => p.map(r));
      if (b.line === false) o.line = false;
      return o;
    }),
  };
  return JSON.stringify(out, null, 1).replace(/\[\s+(-?[\d.]+),\s+(-?[\d.]+)\s+\]/g, "[$1, $2]");
}

async function saveFile(name, text, type) {
  if (window.showSaveFilePicker) {
    try {
      const h = await showSaveFilePicker({ suggestedName: name });
      const w = await h.createWritable(); await w.write(text); await w.close();
      return h.name;
    } catch (e) { if (e.name === "AbortError") return null; }
  }
  const a = Object.assign(document.createElement("a"), { href: URL.createObjectURL(new Blob([text], { type })), download: name });
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  return name;
}

$("save").onclick = async () => {
  const stem = worldName.replace(/\.[^.]*$/, "");
  const boardName = world.board || `${stem}.board.json`;
  const bn = await saveFile(boardName, boardJson(), "application/json");
  if (!bn) return;
  world.board = bn;
  const wn = await saveFile(worldName, writeWorld(world), "text/plain");
  if (wn) worldName = wn;
  status(`saved ${worldName} + ${bn}${board.image ? ` (keep ${board.image} next to ${bn})` : ""}`);
  panel();
};

// ---- controls ----

document.querySelectorAll("[data-mode]").forEach(b => b.onclick = () => setMode(b.dataset.mode));
$("preview").onclick = () => { preview = !preview; changed(); };
$("undo").onclick = undo;
$("fitview").onclick = fitView;
$("addarea").onclick = addArea;
$("delarea").onclick = delArea;
$("clearpoly").onclick = () => { if (selA < 0) return; pushUndo(); delete board.areas[selA].poly; fillMask = null; changed(); };
$("fitareas").onclick = fitAreas;
$("autolayout").onclick = autoLayout;
$("areas").onclick = e => { const d = e.target.closest("[data-i]"); if (d) { selA = +d.dataset.i; changed(); } };
$("head").onchange = e => { pushUndo(); world.head = e.target.value; changed(); };
$("falpha").oninput = e => { board.fill_alpha = +e.target.value; panel(); draw(); };
$("oalpha").oninput = e => { board.outline_alpha = +e.target.value; panel(); draw(); };
for (const k of ["tol", "grow", "eps"]) $(k).oninput = () => panel();
new ResizeObserver(resize).observe($("wrap"));
changed();
fitView();
