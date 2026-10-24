//! saveload.cpp's file level: where a saved game lives (save dir, extension, full path), its
//! text head and 0xb5-byte header, and opening a save by name. The game state after the
//! header is `bw::save`. Paths are 1.16.1's Windows-style strings (`\` separators, built under
//! the "exe dir" = the parent of `Setup::characters_dir`); only the file system calls convert
//! them to host paths. Not wired into the menus yet, so items are unused.
#![allow(dead_code)]

use crate::bw::save::{In, Out};
use crate::bw::{Data, State};
use std::path::{Path, PathBuf};

/// The registry stand-in (src/registry.rs; declared here until main.rs lists it).
#[path = "registry.rs"]
pub mod registry;

/// The exe dir with a trailing `\` (GetModuleFileNameA cut after its last `\`, 0x4ce5b0).
pub fn exe_dir(characters_dir: &Path) -> String {
    let p = characters_dir.parent().unwrap_or(Path::new(""));
    format!("{}\\", p.to_string_lossy())
}

pub fn host(p: &str) -> PathBuf {
    PathBuf::from(p.replace('\\', "/"))
}

/// Ordinal 503 (SStrCat) with the buffer size `max`: appends at most `max - 1 - len` bytes.
fn sstr_cat(dst: &mut String, src: &str, max: usize) {
    let room = (max - 1).saturating_sub(dst.len());
    dst.push_str(&src[..src.len().min(room)]);
}

/// 0x4ce5b0 (and 0x4ce4d0 with `replay` false): the directory of the saves of `profile`:
/// <exe dir>\maps\replays\ for a replay, <exe dir>\maps\ in a multiplayer game, else
/// <exe dir>\save\<profile>\. `size` is the buffer size (0x104). None when the profile name
/// has 0x19 characters or more or the path does not fit.
pub fn save_dir(exe_dir: &str, profile: &[u8], multi: bool, replay: bool, size: usize) -> Option<String> {
    if profile.len() >= 0x19 || exe_dir.len() + 0x24 >= size {
        return None;
    }
    let mut d = String::new();
    sstr_cat(&mut d, exe_dir, size);
    if replay {
        sstr_cat(&mut d, "maps\\replays\\", size);
    } else if multi {
        sstr_cat(&mut d, "maps\\", size);
    } else {
        sstr_cat(&mut d, "save\\", size);
        sstr_cat(&mut d, &String::from_utf8_lossy(profile), size);
        sstr_cat(&mut d, "\\", size);
    }
    Some(d)
}

/// 0x4cee60: the extension of a save (with the leading dot): .mlx / .snx in Brood War,
/// .mlt / .sng in the classic game (multiplayer / single player).
pub fn save_ext(expansion: bool, multi: bool) -> &'static str {
    match (expansion, multi) {
        (true, true) => ".mlx",
        (true, false) => ".snx",
        (false, true) => ".mlt",
        (false, false) => ".sng",
    }
}

/// 0x41eab0: create every directory of `path` (each prefix up to a `\`), succeeding when one
/// exists already. False when a prefix is 0x104 characters or longer or cannot be created.
pub fn create_dirs(path: &str) -> bool {
    let b = path.as_bytes();
    if b.is_empty() {
        return true;
    }
    let mut esi = 0;
    loop {
        while esi < b.len() && b[esi] != b'\\' {
            esi += 1;
        }
        let prefix = &path[..(esi + 1).min(b.len())];
        if prefix.len() >= 0x104 {
            return false;
        }
        let h = host(prefix);
        // create_dir_all: the host's exe dir may not exist yet (the game's exists)
        if std::fs::create_dir_all(&h).is_err() && !h.is_dir() {
            return false;
        }
        if esi >= b.len() {
            return true;
        }
        esi += 1;
        if esi >= b.len() {
            return true;
        }
    }
}

/// 0x4cf2a0: the full path of the save `name` (its directory created): the save dir, the
/// name and the extension, or "rep" for a replay (`replay`, the caller's EBX). None when a
/// step fails.
pub fn save_path(exe_dir: &str, profile: &[u8], multi: bool, expansion: bool, replay: bool, name: &str) -> Option<String> {
    let mut p = save_dir(exe_dir, profile, multi, replay, 0x104)?;
    if p.len() + 0x22 >= 0x104 || !create_dirs(&p) {
        return None;
    }
    sstr_cat(&mut p, name, 0x104);
    if replay {
        sstr_cat(&mut p, "rep", 0x104);
    } else {
        sstr_cat(&mut p, save_ext(expansion, multi), 0x104);
    }
    Some(p)
}

fn line(f: &mut Vec<u8>, parts: &[&[u8]]) {
    // SStrCopy / SStrCat into a 0x100-byte buffer
    let mut s: Vec<u8> = Vec::new();
    for p in parts {
        let room = 0xffusize.saturating_sub(s.len());
        s.extend_from_slice(&p[..p.len().min(room)]);
    }
    f.extend_from_slice(&s);
}

fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// 0x4ce950: the text head of a save, ending with 0x1a: separator lines, the version
/// line, "Single/Multi Player Save Format ver 105.3", the file name, the players (type 2
/// slots of 0x57eee0), the game type (`game_type`, 0x4aad40; "Custom" when None), the map
/// title (0x596845) and path (0x57fd3c). None when the game would fail (no player, or not
/// exactly one in a single player game).
pub fn save_text(name: &[u8], multi: bool, slots: &[[u8; 0x24]], game_type: Option<&[u8]>, title: &[u8], path: &[u8]) -> Option<Vec<u8>> {
    let stars: &[u8] = b"****************************************************\r\n";
    let mut f = Vec::new();
    line(&mut f, &[stars]);
    line(&mut f, &[b"** ", b"Starcraft", b" ", b"internal version unknown", b"\r\n"]);
    let fmt = if multi { "** Multi Player Save Format ver" } else { "** Single Player Save Format ver" };
    f.extend_from_slice(format!("{fmt} {}.{} \r\n", 0x69, 3).as_bytes());
    line(&mut f, &[stars]);
    line(&mut f, &[b"\r\n  Filename:  ", cstr(name), b"\r\n"]);
    let mut n = 0;
    for s in slots.iter().take(8) {
        if s[8] == 2 {
            let lead: &[u8] = if n == 0 { b"   Players:  " } else { b"             " };
            n += 1;
            line(&mut f, &[lead, cstr(&s[0xb..]), b"\r\n"]);
        }
    }
    if if multi { n < 1 } else { n != 1 } {
        return None;
    }
    line(&mut f, &[b" Game Type:  ", game_type.unwrap_or(b"Custom"), b"\r\n"]);
    line(&mut f, &[b" SCM Title:  ", cstr(title), b"\r\n"]);
    line(&mut f, &[b"  SCM Path:  ", cstr(path), b"\r\n\r\n"]);
    f.push(0x1a);
    Some(f)
}

/// The 0x8d-byte game info 0x5967f8 as the header holds it.
pub type GameInfo = [u8; 0x8d];

/// 0x4cf160: u32 0x690003, then the packed 0xb5-byte header: u16 0x69, u16 3, the seed
/// (0x51ca14), the game info at +12. Sets in `info` (0x5967f8) what the game sets: +0 = 0,
/// +0x1c (0x596814) = `p596814`, +0x24 / +0x25 (0x59681c / d) = the two 0x4c40f0 results
/// `net_a` / `net_b`, +0x33 (0x59682b) = the number of the 8 slots of type 1.
pub fn save_header(o: &mut Out, info: &mut GameInfo, p596814: u32, net_a: u8, net_b: u8, slots: &[[u8; 0x24]], seed: u32) -> bool {
    let mut h = [0u8; 0xb5];
    h[0..2].copy_from_slice(&0x69u16.to_le_bytes());
    h[2..4].copy_from_slice(&3u16.to_le_bytes());
    info[0..4].fill(0);
    info[0x1c..0x20].copy_from_slice(&p596814.to_le_bytes());
    info[0x24] = net_a;
    info[0x25] = net_b;
    info[0x33] = slots.iter().take(8).filter(|s| s[8] == 1).count() as u8;
    h[4..8].copy_from_slice(&seed.to_le_bytes());
    h[12..12 + 0x8d].copy_from_slice(info);
    o.u32(0x690003);
    o.packed(&h)
}

/// What 0x4cf0f0 / 0x4cf5f0 take from the 0xb5-byte header.
pub struct Header {
    /// +0: 0x69 in a save of this game (0x4cfa10 refuses others).
    pub id: u16,
    /// +2 (-> 0x596bb8): the save format version (3 written).
    pub format: u16,
    /// +4 (-> 0x596bb4): the seed.
    pub seed: u32,
    /// +12 (-> 0x5967f8): the game info.
    pub info: GameInfo,
}

/// 0x4cf0f0: the u32 (high half 0x69) and the packed 0xb5-byte header.
pub fn read_header(i: &mut In) -> Option<Header> {
    let v = i.u32()?;
    if v & 0xffff_0000 != 0x0069_0000 {
        return None;
    }
    let h = i.packed(0xb5)?;
    Some(Header {
        id: u16::from_le_bytes([h[0], h[1]]),
        format: u16::from_le_bytes([h[2], h[3]]),
        seed: u32::from_le_bytes(h[4..8].try_into().unwrap()),
        info: h[12..12 + 0x8d].try_into().unwrap(),
    })
}

/// 0x4cf5f0: skip the text to the 0x1a byte, read the header (0x4cf0f0), then the player
/// table and game data (`State::load_head`). The caller applies `Header::info` / `format`
/// (0x5967f8, 0x596bb8). None on a short file or a failed part (the game closes the file).
pub fn read_save_head(st: &mut State, d: &Data, i: &mut In) -> Option<Header> {
    loop {
        match i.bytes(1)?[0] {
            0x1a => break,
            _ => {}
        }
    }
    let h = read_header(i)?;
    i.format = h.format;
    st.load_head(d, i).then_some(h)
}

/// The globals 0x4cf8e0 sets when a save is opened.
#[derive(Default)]
pub struct OpenGlobals {
    pub g596904: u32,
    pub g6d11ec: u32,
    pub g51ce90: u32,
    pub g6d0f14: u32,
    pub g6d0f31: u32,
    pub g57f23c: u32,
}

/// 0x4cf8e0: open the save `path` ("rb"; 0x420540) and, in a single player game, set the
/// globals. None when it cannot be opened.
pub fn open_file(path: &str, multi: bool, g: &mut OpenGlobals) -> Option<Vec<u8>> {
    let f = std::fs::read(host(path)).ok()?;
    if !multi {
        if g.g596904 == 4 {
            g.g596904 = 1;
            return Some(f);
        }
        g.g6d11ec = 0;
        g.g51ce90 = 1;
        if g.g6d0f14 == 0 {
            g.g6d0f31 = g.g57f23c;
        }
    }
    Some(f)
}

fn join_dir(dir: &str, name: &str) -> String {
    let mut p = String::new();
    sstr_cat(&mut p, dir, 0x104);
    if !dir.ends_with('\\') {
        sstr_cat(&mut p, "\\", 0x104);
    }
    sstr_cat(&mut p, name, 0x104);
    p
}

/// 0x4cf330 (pattern "*.*", files only, recursing into subdirectories) with the callback
/// 0x4ceff0 and no filter, as 0x4cf5a0 uses them: the first file called `name` (case
/// insensitive) under `dir` whose full path fits in 0x104; Some(full path). Directories are
/// listed in sorted order; names starting with '.' count as hidden.
fn find_file_in(dir: &str, name: &str) -> Option<String> {
    let mut ents: Vec<(String, bool)> = std::fs::read_dir(host(dir))
        .ok()?
        .flatten()
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path().is_dir())))
        .filter(|(n, _)| !n.starts_with('.'))
        .collect();
    ents.sort();
    for (n, is_dir) in &ents {
        if !is_dir && n.eq_ignore_ascii_case(name) && dir.len() + n.len() + 1 < 0x104 {
            return Some(join_dir(dir, n));
        }
    }
    ents.iter().filter(|(_, d)| *d).find_map(|(n, _)| find_file_in(&join_dir(dir, n), name))
}

/// 0x4cf7b0: delete every saved game of `profile` (0x4cf330 on the save dir 0x4ce5b0 with
/// the pattern "*<extension>" of the game (.snx / .mlx / .sng / .mlt), files only, no
/// subdirectories, not hidden; the callback 0x4ce8d0 makes a read-only file writable and
/// deletes it).
pub fn delete_all(exe_dir: &str, profile: &[u8], multi: bool, expansion: bool) {
    let Some(dir) = save_dir(exe_dir, profile, multi, false, 0x104) else { return };
    let ext = save_ext(expansion, multi);
    let Ok(rd) = std::fs::read_dir(host(&dir)) else { return };
    for e in rd.flatten() {
        let Ok(n) = e.file_name().into_string() else { continue };
        if n.starts_with('.') || e.path().is_dir() || !n.to_ascii_lowercase().ends_with(ext) {
            continue;
        }
        let path = host(&join_dir(&dir, &n));
        if let Ok(m) = std::fs::metadata(&path) {
            let mut p = m.permissions();
            if p.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                p.set_readonly(false);
                let _ = std::fs::set_permissions(&path, p);
            }
        }
        let _ = std::fs::remove_file(&path);
    }
}

/// 0x4cf5a0: the full path of the file `name` somewhere under `dir`.
pub fn find_file(dir: &str, name: &str) -> Option<String> {
    find_file_in(dir, name)
}

/// 0x4cf950: open the save `name` of the profile: the file <name><extension> found
/// (0x4cf5a0) under the save dir (0x4ce4d0), opened by 0x4cf8e0. None when a step fails.
pub fn open_save(exe_dir: &str, profile: &[u8], multi: bool, expansion: bool, name: &str, g: &mut OpenGlobals) -> Option<Vec<u8>> {
    let dir = save_dir(exe_dir, profile, multi, false, 0x104)?;
    let mut n = String::new();
    sstr_cat(&mut n, name, 0x20);
    sstr_cat(&mut n, save_ext(expansion, multi), 0x20);
    let path = find_file(&dir, &n)?;
    open_file(&path, multi, g)
}

/// 0x45a8b0 (__fastcall, AL = `start`, DL = `force`): the first slot of the 8 (offset 0x57eee0,
/// stride 0x24) of type 6 in force `force`, searching round-robin from `start + 1`; 8 if none.
pub fn next_slot_in_force(slots: &[[u8; 0x24]], start: u8, force: u8) -> u8 {
    let mut i = start.wrapping_add(1) as u32;
    for _ in 0..8 {
        let s = &slots[(i & 7) as usize];
        if s[0xa] == force && s[8] == 6 {
            return (i & 7) as u8;
        }
        i += 1;
    }
    8
}

/// 0x45a900 (__fastcall, DL = `force`): the index of the first of the 8 slots of type 6 in
/// force `force`; 8 if none.
pub fn first_type6_slot_in_force(slots: &[[u8; 0x24]], force: u8) -> u8 {
    slots[..8].iter().position(|s| s[0xa] == force && s[8] == 6).unwrap_or(8) as u8
}

/// 0x45a920 (__fastcall, CL = `force`): whether any of the 8 slots of type 5 is in force `force`.
pub fn force_has_type5_slot(slots: &[[u8; 0x24]], force: u8) -> bool {
    slots[..8].iter().any(|s| s[0xa] == force && s[8] == 5)
}

/// 0x45ad20: the force 1..=4 with the most type 6 slots (the lower force on a tie); 0 if none.
pub fn force_with_most_type6(slots: &[[u8; 0x24]]) -> u8 {
    let (mut best, mut best_n) = (0u8, 0u8);
    for f in 1..=4u8 {
        let n = slots[..8].iter().filter(|s| s[0xa] == f && s[8] == 6).count() as u8;
        if n > best_n {
            best = f;
            best_n = n;
        }
    }
    best
}

/// 0x45ade0: the first type 6 slot of the force with the most of them (0x45ad20); 0xff when
/// there is no such force, 8 when none is found.
pub fn first_slot_of_largest_force(slots: &[[u8; 0x24]]) -> u8 {
    match force_with_most_type6(slots) {
        0 => 0xff,
        f => first_type6_slot_in_force(slots, f),
    }
}
