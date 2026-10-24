//! The replacement of Storm's registry functions: Ordinal_421 SRegLoadData (0x410250),
//! 422 SRegLoadString (0x4100ac), 423 SRegLoadValue (0x410076), 424 SRegSaveData (0x410256),
//! 425 SRegSaveString (0x410226), 426 SRegSaveValue (0x41027a). The game keeps its settings
//! (key "Starcraft", HKCU then HKLM: "Gamma", "Game Speed", "Recent Maps", ...) there; here
//! the values by name are in a plain text file next to the saves (`<exe dir>/scbw_registry.txt`,
//! the exe dir being `saveload::exe_dir`), one value per line:
//!
//! ```text
//! d<TAB>name<TAB>decimal      a DWORD (SRegLoadValue)
//! s<TAB>name<TAB>text         a string (SRegLoadString)
//! h<TAB>name<TAB>hex          a string with control characters, in hex
//! b<TAB>name<TAB>hex          binary data (SRegLoadData)
//! ```
//!
//! The key name and the flags (HKCU / HKLM) of the Storm calls are not kept: all values are
//! under "Starcraft". A missing or unreadable file is an empty registry; save errors (and
//! wasm builds, which have no file system) are ignored.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Dword(u32),
    Str(Vec<u8>),
    Data(Vec<u8>),
}

#[derive(Default)]
struct Registry {
    values: BTreeMap<String, Value>,
    /// The file the values are saved to (None: kept in memory only).
    path: Option<PathBuf>,
}

static REG: Mutex<Registry> = Mutex::new(Registry { values: BTreeMap::new(), path: None });

fn reg() -> MutexGuard<'static, Registry> {
    REG.lock().unwrap_or_else(|e| e.into_inner())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

impl Registry {
    /// The file's text: one line per value.
    fn to_text(&self) -> String {
        let mut o = String::new();
        for (k, v) in &self.values {
            match v {
                Value::Dword(d) => o.push_str(&format!("d\t{k}\t{d}\n")),
                Value::Str(s) => match std::str::from_utf8(s) {
                    Ok(t) if !t.chars().any(char::is_control) => o.push_str(&format!("s\t{k}\t{t}\n")),
                    _ => o.push_str(&format!("h\t{k}\t{}\n", hex(s))),
                },
                Value::Data(b) => o.push_str(&format!("b\t{k}\t{}\n", hex(b))),
            }
        }
        o
    }

    /// Reads the values of a file's text (lines that do not parse are dropped).
    fn parse(&mut self, text: &str) {
        for line in text.lines() {
            let mut f = line.splitn(3, '\t');
            let (Some(kind), Some(name), Some(val)) = (f.next(), f.next(), f.next()) else { continue };
            let v = match kind {
                "d" => val.parse().ok().map(Value::Dword),
                "s" => Some(Value::Str(val.as_bytes().to_vec())),
                "h" => unhex(val).map(Value::Str),
                "b" => unhex(val).map(Value::Data),
                _ => None,
            };
            if let Some(v) = v {
                self.values.insert(name.to_string(), v);
            }
        }
    }

    fn save(&self) {
        if let Some(p) = &self.path {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, self.to_text());
        }
    }
}

/// Where the registry file is: `dir`/scbw_registry.txt (the exe dir of the saves). Reads the
/// file (the values read so far are dropped). Without a call the registry lives in memory.
pub fn init(dir: &Path) {
    let path = dir.join("scbw_registry.txt");
    let mut r = reg();
    r.values.clear();
    if let Ok(t) = std::fs::read(&path) {
        r.parse(&String::from_utf8_lossy(&t));
    }
    r.path = Some(path);
}

/// Ordinal_423 (0x410076) SRegLoadValue("Starcraft", `name`, 0, &value): the DWORD.
pub fn load_value(name: &str) -> Option<u32> {
    match reg().values.get(name) {
        Some(Value::Dword(d)) => Some(*d),
        _ => None,
    }
}

/// Ordinal_426 (0x41027a) SRegSaveValue.
pub fn save_value(name: &str, v: u32) {
    let mut r = reg();
    r.values.insert(name.to_string(), Value::Dword(v));
    r.save();
}

/// Ordinal_422 (0x4100ac) SRegLoadString("Starcraft", `name`, 0, buffer, `size`): the string
/// cut to the buffer (`size` - 1 bytes).
pub fn load_string(name: &str, size: usize) -> Option<Vec<u8>> {
    match reg().values.get(name) {
        Some(Value::Str(s)) => Some(s[..s.len().min(size.saturating_sub(1))].to_vec()),
        _ => None,
    }
}

/// Ordinal_425 (0x410226) SRegSaveString.
pub fn save_string(name: &str, s: &[u8]) {
    let mut r = reg();
    let s = s.split(|&c| c == 0).next().unwrap_or(&[]).to_vec();
    r.values.insert(name.to_string(), Value::Str(s));
    r.save();
}

/// Ordinal_421 (0x410250) SRegLoadData("Starcraft", `name`, ..): the bytes.
pub fn load_data(name: &str) -> Option<Vec<u8>> {
    match reg().values.get(name) {
        Some(Value::Data(b)) => Some(b.clone()),
        _ => None,
    }
}

/// Ordinal_424 (0x410256) SRegSaveData.
pub fn save_data(name: &str, b: &[u8]) {
    let mut r = reg();
    r.values.insert(name.to_string(), Value::Data(b.to_vec()));
    r.save();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trip() {
        let mut r = Registry::default();
        r.values.insert("Gamma".into(), Value::Dword(120));
        r.values.insert("Game Type".into(), Value::Str(b"Melee".to_vec()));
        r.values.insert("Odd".into(), Value::Str(vec![1, 2, 0xff]));
        r.values.insert("Recent Maps".into(), Value::Data(b"a\\b.scm\0\0".to_vec()));
        let t = r.to_text();
        let mut r2 = Registry::default();
        r2.parse(&t);
        assert_eq!(r.values, r2.values);
    }

    #[test]
    fn file_round_trip() {
        let dir = std::env::temp_dir().join(format!("scbw_reg_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        init(&dir);
        assert_eq!(load_value("Gamma"), None);
        save_value("Gamma", 90);
        save_string("Game Speed", b"Fast");
        save_data("Recent Maps", b"x.scm\0\0");
        // A restart: the values come back from the file.
        init(&dir);
        assert_eq!(load_value("Gamma"), Some(90));
        assert_eq!(load_string("Game Speed", 0x80), Some(b"Fast".to_vec()));
        assert_eq!(load_string("Game Speed", 3), Some(b"Fa".to_vec()));
        assert_eq!(load_data("Recent Maps"), Some(b"x.scm\0\0".to_vec()));
        assert_eq!(load_value("Game Speed"), None);
        let _ = std::fs::remove_dir_all(&dir);
        reg().path = None;
    }
}
