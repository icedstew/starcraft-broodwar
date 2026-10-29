// The player's own StarCraft 1.16.1 files for the browser build (nothing of the game is
// served by us): an install folder picked in the page, or the player's Google Drive folder
// (their API key and folder id). What was loaded is kept in IndexedDB so a reload does not
// ask again. The key and folder id are neither kept nor logged.

// storage.rs ARCHIVES order; the CD archives (BroodWar.mpq, Starcraft.mpq: music,
// ~600 MB each) are left out to keep memory down.
export const ARCHIVES = ["patch_rt.mpq", "broodat.mpq", "stardat.mpq"];

const DB = "scbw-files", STORE = "install", KEY = "files";

// One IndexedDB request in a transaction; its result once the transaction completed.
function idb(mode, request) {
  return new Promise((ok, fail) => {
    const open = indexedDB.open(DB, 1);
    open.onupgradeneeded = () => open.result.createObjectStore(STORE);
    open.onerror = () => fail(open.error);
    open.onsuccess = () => {
      const t = open.result.transaction(STORE, mode);
      const r = request(t.objectStore(STORE));
      t.oncomplete = () => { open.result.close(); ok(r.result); };
      t.onerror = () => fail(t.error);
    };
  });
}

// { storm, archives } kept from an earlier load, or null (also when storage is blocked).
export async function cached() {
  try {
    const f = await idb("readonly", s => s.get(KEY));
    return f && f.storm && f.archives ? f : null;
  } catch (e) {
    return null;
  }
}

export async function keep(files) {
  try {
    await idb("readwrite", s => s.put(files, KEY));
  } catch (e) {
    console.warn("the game files could not be kept: " + (e && e.message));
  }
}

export async function forget() {
  try {
    await idb("readwrite", s => s.delete(KEY));
  } catch (e) {}
}

const bytes = async f => new Uint8Array(await f.arrayBuffer());

// The install folder an <input type=file webkitdirectory> picked: its top-level files.
export async function fromFolder(list) {
  const top = new Map();
  for (const f of list) {
    if (f.webkitRelativePath.split("/").length === 2) top.set(f.name.toLowerCase(), f);
  }
  if (!top.has("storm.dll")) throw new Error("pick the StarCraft install folder (it has storm.dll)");
  const archives = [];
  for (const name of ARCHIVES) {
    if (!top.has(name)) throw new Error(`the folder has no ${name}`);
    archives.push(await bytes(top.get(name)));
  }
  return { storm: await bytes(top.get("storm.dll")), archives };
}

// A Google Drive folder shared "Anyone with the link", read with the player's API key.
export async function fromDrive(key, folder) {
  const api = "https://www.googleapis.com/drive/v3/files";
  const r = await fetch(`${api}?q='${encodeURIComponent(folder)}'+in+parents&fields=files(id,name)&pageSize=1000&key=${encodeURIComponent(key)}`);
  if (!r.ok) throw new Error(`drive folder: ${r.status}`);
  const ids = new Map((await r.json()).files.map(f => [f.name.toLowerCase(), f.id]));
  const get = async name => {
    const id = ids.get(name);
    if (!id) throw new Error(`the drive folder has no ${name}`);
    const f = await fetch(`${api}/${id}?alt=media&key=${encodeURIComponent(key)}`);
    if (!f.ok) throw new Error(`drive ${name}: ${f.status}`);
    return new Uint8Array(await f.arrayBuffer());
  };
  return { storm: await get("storm.dll"), archives: await Promise.all(ARCHIVES.map(get)) };
}

// Files under a URL (tests only: a host serving its own auto/ copy on opt-in).
export async function fromUrl(base) {
  const get = async name => {
    const r = await fetch(base + name);
    if (!r.ok) throw new Error(`${base}${name}: ${r.status}`);
    return new Uint8Array(await r.arrayBuffer());
  };
  return { storm: await get("storm.dll"), archives: await Promise.all(ARCHIVES.map(get)) };
}
