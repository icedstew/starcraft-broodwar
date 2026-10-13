//! Client-side unit selection: which units a drag box picks (0x46fa40), as the local
//! player sees them. The result becomes the client selection (0x6284b8) and is sent
//! to the other players as select commands.

use super::list::NIL;
use super::{Data, State};

/// 0x46ed80: types a box never picks (critters, map revealers, beacons...).
fn never_picked(t: u16) -> bool {
    matches!(t, 0xe | 0x55 | 0x69 | 0xca | 0xcd..=0xd0)
}

impl State {
    /// 0x47b770: can be one of several selected units (not a building, disabled, ...).
    pub fn multi_selectable(&self, d: &Data, u: u32) -> bool {
        let un = &self.units[u as usize];
        let t = un.unit_type;
        let f = d.dat.units.flags[t as usize];
        f & 1 == 0
            && f & 0x800 == 0
            && un.status & 0x400 == 0
            && un.lockdown_timer == 0
            && un.stasis_timer == 0
            && un.maelstrom_timer == 0
            && !(0xcb..=0xd5).contains(&t)
            && !matches!(t, 0xd | 0x24 | 0x59 | 0x5a | 0x5d | 0x5e | 0x5f | 0x60 | 0xca | 0x69)
    }

    /// Sort key 0x46f040 compares: units drawn later rank higher.
    fn draw_key(&self, u: u32) -> u32 {
        let s = self.units[u as usize].sprite;
        let sp = &self.sprites[s as usize];
        let y = if sp.elevation <= 4 { sp.y as i32 as u32 } else { 0 };
        ((((sp.elevation as u32) << 13 | y) << 5 | (sp.flags & 0x10) as u32) << 9) | (s & 0xffff)
    }

    /// 0x46f040: a full list takes `u` in place of the first unit drawn below it.
    fn replace_lower(&self, list: &mut [u32], u: u32, anchor: u32) {
        let k = self.draw_key(u);
        for slot in list.iter_mut() {
            if *slot != anchor && (self.draw_key(*slot) < k || u == anchor) {
                *slot = u;
                return;
            }
        }
    }

    /// 0x46f0f0: the units a selection picks from `found`: the local player's
    /// multi-selectable units (12 at most), else one other unit. `anchor` (a
    /// double-clicked unit) limits it to units like the anchor.
    pub fn select_filter(&self, d: &Data, found: &[u32], anchor: u32) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::with_capacity(12);
        if anchor != NIL {
            out.push(anchor);
        }
        let mask = self.local_vision_mask as u32;
        let mut single = NIL;
        for &f in found {
            let mut u = f;
            if u == anchor {
                continue;
            }
            if self.units[u as usize].subunit != NIL {
                // A turret stands for its base.
                if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x10 != 0 {
                    u = self.units[u as usize].subunit;
                }
                if out.contains(&u) {
                    continue;
                }
            }
            let un = &self.units[u as usize];
            let vis = self.sprites[un.sprite as usize].visibility as u32;
            if vis & mask == 0 || un.status & 0x300 != 0 && un.visibility & mask == 0 || never_picked(un.unit_type) {
                continue;
            }
            let mut cand = u;
            let mut ok = self.multi_selectable(d, u) && !self.replay && un.player == self.local_player;
            if ok && anchor != NIL {
                cand = single;
                let an = &self.units[anchor as usize];
                let x = an.status ^ un.status;
                ok = an.player == un.player
                    && an.unit_type == un.unit_type
                    && x & 0x10 == 0
                    && (un.status & 0x800 != 0 && un.status & 0x10 == 0 || x & 0x100 == 0)
                    && x & 0x4000_0000 == 0;
            }
            if ok {
                cand = single;
                if out.len() < 12 {
                    out.push(u);
                } else {
                    self.replace_lower(&mut out, u, anchor);
                }
            }
            single = cand;
        }
        if out.is_empty() && single != NIL {
            out.push(single);
        }
        out
    }

    /// 0x46f290: shift adds `new` to `cur` when both are the local player's
    /// multi-selectable units; returns the merged count (`cur` holds 12 slots).
    fn select_merge(&self, d: &Data, cur: &mut Vec<u32>, new: &[u32], anchor: u32) {
        let n = cur.len();
        if n >= 12 {
            return;
        }
        if n != 0 {
            let own = |u: u32| self.multi_selectable(d, u) && self.units[u as usize].player == self.local_player;
            if !(own(new[0]) && !self.replay && own(cur[0])) {
                return;
            }
        }
        let mut added = 0;
        for &u in new {
            if cur[..n].contains(&u) {
                continue;
            }
            if cur.len() < 12 {
                cur.push(u);
                added += 1;
            } else {
                let (_, tail) = cur.split_at_mut(n);
                self.replace_lower(&mut tail[..added], u, anchor);
            }
        }
    }

    /// 0x46fa40: drag-box selection over map rect `r` (x0, y0, x1, y1); `shift` adds
    /// to `cur`. Returns the new selection, or None when the box picks nothing.
    pub fn box_select(&mut self, d: &Data, r: [i16; 4], shift: bool, cur: &[u32]) -> Option<Vec<u32>> {
        let start = self.units_in_box(d, r);
        let found: Vec<u32> = self.search.buf[start..self.search.top].iter().map(|&e| e - 1).collect();
        let picked = self.select_filter(d, &found, NIL);
        self.search.pop();
        if picked.is_empty() {
            return None;
        }
        if shift && !cur.is_empty() {
            let mut sel = cur.to_vec();
            self.select_merge(d, &mut sel, &picked, NIL);
            return Some(sel);
        }
        // ponytail: 0x596a2a (alt?) + 0x496d30 one-unit shortcut not handled.
        Some(picked)
    }
}
