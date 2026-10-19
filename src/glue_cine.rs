//! The cinematics (smk\*.smk, 0x4d4470): the intro, and the mission movies after a campaign
//! mission (0x4f7ae0). Storm's SVidPlayBegin / SVidPlayContinueSingle / SVidPlayEnd (Ordinal
//! 454 / 457 / 458) are replaced by smk.rs and `Audio::video_player`.
//!
//! A child module of `glue`: it extends `Glue` and uses its private state.

use super::Glue;
use crate::audio::Audio;
use crate::smk::Smk;
use crate::storage::Storage;
use crate::todo_port;

/// 0x512690: the movie file of each cinematic number (0x1c is not a string pointer).
pub(super) const MOVIES: [&str; 0x1c] = [
    "smk/blizzard.smk", "smk/starintr.smk", "smk/PreTerran_1.smk", "smk/PreTerran_2.smk",
    "smk/PreTerran_3.smk", "smk/PreTerran_4.smk", "smk/Terran1.smk", "smk/Terran2.smk",
    "smk/Terran3.smk", "smk/Terran4.smk", "smk/PreZerg_2.smk", "smk/PreZerg_3.smk",
    "smk/PreZerg_4.smk", "smk/Zerg1.smk", "smk/Zerg2.smk", "smk/Zerg3.smk", "smk/Zerg4.smk",
    "smk/PreProtoss_1.smk", "smk/PreProtoss_2.smk", "smk/PreProtoss_3.smk", "smk/Protoss1.smk",
    "smk/Protoss2.smk", "smk/Protoss3.smk", "smk/Protoss4.smk", "smk/starXintr.smk",
    "smk/xProtoss.smk", "smk/xTerran.smk", "smk/xZerg.smk",
];

/// 0x500be4 (2 bytes each: scenario, cinematic): the movie shown before the scenario's own.
const BEFORE: [(u8, u8); 10] = [(6, 2), (7, 3), (8, 4), (9, 5), (14, 10), (15, 11), (16, 12), (20, 17), (21, 18), (22, 19)];

impl Glue {
    /// 0x4f7ae0 (ESI = scenario 0x5122b8): the cinematics to play after a mission, in order:
    /// the table's entry for the scenario (scanned from the last), then the scenario's own
    /// number (0x596904 is 10 here, not 2).
    pub(super) fn cine_after_game(&self) -> Vec<u8> {
        let mut list = Vec::new();
        if let Some(&(_, movie)) = BEFORE.iter().rev().find(|e| e.0 as u32 == self.scenario) {
            list.push(movie);
        }
        list.push(self.scenario as u8);
        list
    }
}

/// 0x4d4470: plays cinematic `idx` (EAX); true when it was skipped (0x5967f0: Escape, Enter or
/// Space, 0x4d4410). `win`: None writes the frame `SCBW_CINE_FRAME` (default 0) to `shot` and
/// stops instead of playing. A movie that can't be opened is a fatal error (0x4212c0) except
/// the two intros (1, 0x18).
pub fn play(files: &Storage, idx: u32, win: Option<&mut minifb::Window>, audio: Option<&Audio>, shot: Option<&str>) -> std::io::Result<bool> {
    use minifb::{Key, KeyRepeat};
    // 0x4be0b0 (hides the cursor) is `Glue::flush_notice`, run by the caller; 0x4d2ff0 clears
    // the input handlers and the window's handlers 0x5968dc / 0x5968b4 / 0x5968c0 are set to
    // 0x4d4410: the modal loop here reads the keys itself.
    todo_port!("0x4d2ff0", "input handler table reset (0x5968a0, ClipCursor)");
    let Some(name) = MOVIES.get(idx as usize) else { return Ok(false) };
    // Ordinal_454 flags 0x10280808 (+ 0x800000 from 0x19): from the file, the video's own palette.
    let video = files.read(name).ok().and_then(Smk::parse);
    let Some(mut v) = video else {
        if idx == 1 || idx == 0x18 {
            return Ok(false);
        }
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("{}\n{}", name.replace('/', "\\"), "can't open the movie")));
    };
    let (sw, sh) = (640usize, 480usize);
    // Storm's 0x200 (on by default) doubles the width when the doubled video still fits.
    let dbl = if v.w * 2 <= sw { 2 } else { 1 };
    let (vw, vh) = (v.w * dbl, v.h.min(sh));
    let (x0, y0) = ((sw - vw.min(sw)) / 2, (sh - vh) / 2);
    let mut rgb = vec![0u32; sw * sh];
    let draw = |v: &Smk, rgb: &mut [u32]| {
        for y in 0..vh {
            for x in 0..vw {
                let [r, g, b] = v.palette[v.pixels[y * v.w + x / dbl] as usize];
                rgb[(y0 + y) * sw + x0 + x] = (r as u32) << 16 | (g as u32) << 8 | b as u32;
            }
        }
    };
    let Some(win) = win else {
        let n = std::env::var("SCBW_CINE_FRAME").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
        for _ in 0..=n {
            v.next_frame();
        }
        draw(&v, &mut rgb);
        if let Some(out) = shot {
            crate::write_png_rgb(out, sw, sh, &rgb)?;
        }
        return Ok(true);
    };
    v.want_audio = audio.is_some() && v.audio_rate & 0x40000000 != 0;
    let player = audio.filter(|_| v.want_audio).map(|a| a.video_player());
    let rate = std::num::NonZero::new(v.audio_rate & 0xffffff);
    let ch = std::num::NonZero::new(if v.audio_rate & 0x10000000 != 0 { 2 } else { 1 });
    let start = std::time::Instant::now();
    let mut shown = 0usize;
    let mut skipped = false;
    // 0x4d44e0: the loop (0x4d1bf0 pumps the messages) until the keys or the end.
    while win.is_open() && !skipped {
        let due = (start.elapsed().as_millis() as u32 / v.ms.max(1)) as usize + 1;
        let mut more = true;
        while shown < due && more {
            more = v.next_frame();
            shown += more as usize;
            if let (Some(p), Some(rate), Some(ch)) = (&player, rate, ch) {
                let s: Vec<f32> = v.audio.drain(..).map(|s| s as f32 / 32768.0).collect();
                if !s.is_empty() {
                    p.append(rodio::buffer::SamplesBuffer::new(ch, rate, s));
                }
            }
        }
        if !more {
            break;
        }
        draw(&v, &mut rgb);
        win.update_with_buffer(&rgb, sw, sh).map_err(|e| std::io::Error::other(e.to_string()))?;
        skipped = [Key::Escape, Key::Enter, Key::NumPadEnter, Key::Space].iter().any(|&k| win.get_keys_pressed(KeyRepeat::Yes).contains(&k));
    }
    // The last frame's sound plays out (0x4100ee / Ordinal_458 closes the video).
    if let Some(p) = player.filter(|_| !skipped) {
        while !p.empty() && win.is_open() {
            win.update();
        }
    }
    Ok(skipped)
}
