//! The trigger leaderboard dialog (rez\statlb.bin, statlb.cpp): 0x4bed70 creates it,
//! 0x4be670 (one value per player) and 0x4be860 (minerals and gas, action 0x28) fill its
//! labels from the table 0x4538b0 sorts (`bw::leaderboard`), 0x4be430 sets the title; the
//! labels draw through 0x4be3d0, the player boxes through 0x4be370 and the two icons
//! through 0x4be280.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::ui::{TextDst, TextOpts, Ui};
use crate::view::{blit_frame, Scene, SCREEN_H, SCREEN_W};

/// 0x597288: all rows shown (1, set when 0x4bed70 creates the dialog) or only the first.
static ALL_ROWS: AtomicBool = AtomicBool::new(true);

/// 0x4bec00 (the leaderboard key): toggles 0x597288 and refills the rows (the next `draw`).
pub fn toggle_rows() {
    ALL_ROWS.fetch_xor(true, Ordering::Relaxed);
}

/// 0x4bed70 sets 0x597288 := 1 when it creates the dialog.
pub fn reset_rows() {
    ALL_ROWS.store(true, Ordering::Relaxed);
}

/// 0x4be3d0: a label's text, left and vertically centered in its rect, color 7.
fn label(ui: &Ui, id: i16, text: &[u8], out: &mut [u8]) {
    let Some(r) = ui.statlb.screen_rect(id) else { return };
    let o = TextOpts { flags: 0x21, clip: r, ..TextOpts::new() };
    ui.font.draw_bytes(text, r.0, r.1, 7, &o, TextDst { rows: &ui.text_colors, out, w: SCREEN_W, h: SCREEN_H });
}

/// 0x4be370: the player's color box.
fn color_box(ui: &Ui, id: i16, color: u8, out: &mut [u8]) {
    let Some((l, t, r, b)) = ui.statlb.screen_rect(id) else { return };
    for y in t.max(0)..=b.min(SCREEN_H - 1) {
        for x in l.max(0)..=r.min(SCREEN_W - 1) {
            out[(y * SCREEN_W + x) as usize] = color;
        }
    }
}

/// 0x4be280: an icons.grp frame centered vertically in the control.
fn icon(ui: &Ui, id: i16, frame: usize, out: &mut [u8]) {
    let Some(r) = ui.statlb.screen_rect(id) else { return };
    let Some(f) = ui.icons.frame(frame) else { return };
    blit_frame(&ui.icons, frame, r.0, r.1 + (r.3 - r.1 + 1 - f.h) / 2, f.w, f.h, None, out, SCREEN_W, SCREEN_H);
}

/// 0x4bed70 creates the dialog unless a network game with victory conditions, starting
/// units or 0x596877 is running (a replay always has it).
fn exists(scene: &Scene) -> bool {
    let st = &scene.st;
    let t = &st.template;
    !(st.multiplayer && t[8] != 3 && t[8] != 6 && (t[8] != 0 || t[0xc] != 0 || t[0x12] != 0)) || st.replay
}

/// The dialog as 0x4beb90 (every second) leaves it: 0x4be670 / 0x4be860 for the action
/// 0x58d70c, nothing for none (0x4beb40).
/// ponytail: drawn from the current table every frame; the dialog's 1000 ms timer lags it.
pub fn draw(scene: &Scene, ui: &Ui, out: &mut [u8]) {
    let st = &scene.st;
    let lb = &st.trig.leaderboard;
    let two = match lb.0 {
        0x28 => true,
        0x11..=0x15 | 0x21..=0x25 => false,
        _ => return,
    };
    if !exists(scene) {
        return;
    }
    // 0x4be670 / 0x4be860: only the first row while 0x597288 (`toggle_rows`) is 0.
    let all = ALL_ROWS.load(Ordering::Relaxed);
    let rows: Vec<(usize, u32, u32)> = st.trig.leaderboard_table.iter().enumerate().filter(|(i, r)| (*i == 0 || all) && r.0 != crate::bw::leaderboard::NO_PLAYER).map(|(i, r)| (i, r.0, r.1)).collect();
    for &(i, p, v) in &rows {
        let i = i as i16;
        color_box(ui, -10 - i, scene.minimap.player_colors[p as usize & 7], out);
        if two {
            let pl = &st.players;
            label(ui, -30 - i, (pl.minerals[p as usize & 7]).to_string().as_bytes(), out);
            label(ui, -40 - i, (pl.gas[p as usize & 7]).to_string().as_bytes(), out);
        } else {
            label(ui, -20 - i, (v as i32).to_string().as_bytes(), out);
        }
    }
    if two {
        icon(ui, -48, 0, out);
        icon(ui, -49, 1 + scene.local_race(), out);
        return;
    }
    // 0x4be430: the title starts after the first row's text, at most at the value label's
    // right edge + 4; an empty first row hides it.
    let Some(first) = rows.first().filter(|r| r.0 == 0) else { return };
    let Some(vr) = ui.statlb.screen_rect(-20) else { return };
    let (w0, tr) = (ui.font.width(&(first.2 as i32).to_string()), ui.statlb.screen_rect(-28));
    let Some(mut tr) = tr else { return };
    let x = (vr.0 + w0 + 4).min(vr.2 + 4);
    tr.0 = x;
    // The map string (0x58d714), else the stat_txt default of the action.
    let id = lb.4 as u16;
    let text: Vec<u8> = if id != 0 {
        scene.chk.string_bytes(id).unwrap_or(&[]).to_vec()
    } else {
        let n = match lb.0 {
            0x11..=0x15 => 0x330 + lb.0 as usize,
            0x21..=0x25 => 0x325 + lb.0 as usize,
            _ => 0,
        };
        ui.stat_txt.get(n.wrapping_sub(1)).map_or(Vec::new(), |s| s.split('\0').next().unwrap_or("").as_bytes().to_vec())
    };
    let o = TextOpts { flags: 0x21, clip: tr, ..TextOpts::new() };
    ui.font.draw_bytes(&text, tr.0, tr.1, 7, &o, TextDst { rows: &ui.text_colors, out, w: SCREEN_W, h: SCREEN_H });
}
