//! Client-side unit selection: which units a drag box picks (0x46fa40), as the local
//! player sees them. The result becomes the client selection (0x6284b8) and is sent
//! to the other players as select commands.

use super::list::NIL;
use super::{Data, State};

/// 0x46ed80: types a box never picks (critters, map revealers, beacons...).
pub(super) fn never_picked(t: u16) -> bool {
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
    /// 0x402a70: status 0x800 set and 0x10 clear (inline here).
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
            // 0x46fa00 on the merged count.
            return if sel.len() == 1 { self.alt_group_shortcut(d, sel[0]) } else { Some(sel) };
        }
        // 0x46fa00 (count 1) and the marker tail.
        let u = picked[0];
        let n = picked.len();
        let sel = if n == 1 { self.alt_group_shortcut(d, u) } else { Some(picked) };
        if n == 1 {
            self.rally_marker_if_own(d, u);
        }
        sel
    }

    /// 0x46fa00's shortcut (count 1 and 0x596a2a): the unit's best recent group, else the
    /// unit itself.
    fn alt_group_shortcut(&mut self, d: &Data, u: u32) -> Option<Vec<u32>> {
        if self.key_alt {
            let h = self.unit_handle(u);
            if let Some(g) = self.recent_group_of(h) {
                let sel = self.select_recent_group(d, g);
                return if sel.is_empty() { None } else { Some(sel) };
            }
        }
        Some(vec![u])
    }

    /// 0x46fa40 / 0x46fb40 tail: a selected rally-capable unit of the local player
    /// refreshes its order target marker (0x468670).
    fn rally_marker_if_own(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].player == self.local_player && super::command::has_rally(self.units[u as usize].unit_type) {
            self.rally_marker(d, u);
        }
    }

    /// 0x468670: show where unit `u`'s rally point is: a fog sprite there flashes its
    /// selection circle, else the right-click marker moves there.
    pub fn rally_marker(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        let (rx, ry) = un.rally_pos;
        if (rx != 0 || ry != 0) && (rx != un.fl.pos.0 || ry != un.fl.pos.1) {
            match self.fog_thingy_at(d, rx as i32, ry as i32) {
                Some(t) => {
                    let s = self.fog_thingies[t as usize].sprite;
                    self.sprites[s as usize].selection_timer = 0x1f;
                }
                None => self.show_marker(d, rx, ry),
            }
        }
    }

    /// 0x487a10: the fogged copy whose placement box (centered on its sprite) holds
    /// map point (x, y), skipping the unit types of 0x46ed80.
    pub(crate) fn fog_thingy_at(&self, d: &Data, x: i32, y: i32) -> Option<u32> {
        let mut t = self.active_fog_thingies.head;
        while t != NIL {
            let th = &self.fog_thingies[t as usize];
            let ty = th.hp as u16;
            if !never_picked(ty) {
                let sp = &self.sprites[th.sprite as usize];
                let b = d.dat.units.placement_box.get(ty as usize).copied().unwrap_or(0);
                let (w, h) = ((b & 0xffff) as i16 as i32, (b >> 16) as i16 as i32);
                if ((w / 2 + sp.x as i32 - x) as u32) < sp.width as u32 && ((h / 2 + sp.y as i32 - y) as u32) < sp.height as u32 {
                    return Some(t);
                }
            }
            t = th.link.next;
        }
        None
    }

    /// 0x496d30 (search part): the best of the recent groups 10..=17 holding unit handle
    /// `h`, the one used last (ties: the first found, highest group first), as g - 10.
    /// The rank read at -1 is the previous player's group 7 (flat 0x63fe40 table).
    pub fn recent_group_of(&self, h: u32) -> Option<u8> {
        let p = self.local_player as usize;
        let rank = |g: i32| {
            let i = p as i32 * 8 + g;
            if i < 0 { 0 } else { self.recent_used[i as usize / 8][i as usize % 8] }
        };
        let mut best = -1i32;
        for g in (0..8).rev() {
            let row = self.hotkeys.get(p * 18 + 10 + g as usize)?;
            if row.iter().any(|&e| e != 0 && e == h) && rank(best) < rank(g) {
                best = g;
            }
        }
        (best >= 0).then_some(best as u8)
    }

    /// 0x496b40 (selection part): the live units of recent group `g` (0..8) that the local
    /// player may select; a lone rally-capable unit refreshes its marker. Native then
    /// sets the selection and sends it (0x49ae40, 0x4c07b0).
    pub fn select_recent_group(&mut self, d: &Data, g: u8) -> Vec<u32> {
        let row = self.hotkeys[self.local_player as usize * 18 + 10 + g as usize];
        let n = row.iter().position(|&h| h == 0).unwrap_or(12);
        let mut sel = Vec::new();
        let mut last = None;
        for &h in &row[..n] {
            last = self.unit_from_handle(h).filter(|&u| {
                let un = &self.units[u as usize];
                self.sprites[un.sprite as usize].flags & super::sprite::sflag::HIDDEN == 0
                    && un.player == self.local_player
                    && (self.multi_selectable(d, u) || n <= 1)
            });
            if let Some(u) = last {
                sel.push(u);
            }
        }
        if let (1, Some(u)) = (sel.len(), last) {
            self.rally_marker_if_own(d, u);
        }
        sel
    }
}

/// 0x4d4db0: whether pixel (fx, fy) of a GRP frame is opaque (RLE line walk).
fn grp_pixel(grp: &[u8], frame: u16, fx: u32, fy: u32) -> bool {
    let h = 6 + frame as usize * 8;
    let Some(fh) = grp.get(h..h + 8) else { return false };
    let (w, ht) = (fh[2] as u32, fh[3] as u32);
    if fx >= w || fy >= ht {
        return false;
    }
    let lines = u32::from_le_bytes([fh[4], fh[5], fh[6], fh[7]]) as usize;
    let rd = |o: usize| grp.get(o).copied().unwrap_or(0);
    let mut p = lines + u16::from_le_bytes([rd(lines + fy as usize * 2), rd(lines + fy as usize * 2 + 1)]) as usize;
    let mut x = fx as i32;
    loop {
        let b = rd(p);
        p += 1;
        if b & 0x80 != 0 {
            x -= (b & 0x7f) as i32;
        } else if b & 0x40 == 0 {
            x -= b as i32;
            p += b as usize;
        } else {
            x -= (b & 0xbf) as i32;
            p += 1;
        }
        if x <= 0 {
            return b & 0x80 == 0;
        }
    }
}

impl State {
    /// 0x4d57b0 (position part): an image's top-left in map coordinates; y is the kept
    /// `pos_y` while FREEZE_Y is set.
    fn image_map_pos(&self, d: &Data, i: u32) -> (i16, i16) {
        let (x, y) = self.image_map_pos_now(d, i);
        let im = &self.images[i as usize];
        (x, if im.flags & super::sprite::iflag::FREEZE_Y != 0 { im.pos_y } else { y })
    }

    /// 0x4d57b0's y (+0x26), unless FREEZE_Y keeps the old one.
    pub(crate) fn image_update_pos_y(&mut self, d: &Data, i: u32) {
        if self.images[i as usize].flags & super::sprite::iflag::FREEZE_Y == 0 {
            self.images[i as usize].pos_y = self.image_map_pos_now(d, i).1;
        }
    }

    /// The callers of 0x4d57b0 (sprite moves 0x4983a0, frame / offset changes, image
    /// init) keep +0x26 current: refreshed for every image after each step.
    /// ponytail: per step, not at each call; differs only if an image moves and is frozen
    /// in the same step.
    pub fn images_update_pos_y(&mut self, d: &Data) {
        for i in 0..self.images.len() {
            if self.images[i].in_use && self.images[i].sprite != NIL {
                self.image_update_pos_y(d, i as u32);
            }
        }
    }

    fn image_map_pos_now(&self, d: &Data, i: u32) -> (i16, i16) {
        let im = &self.images[i as usize];
        let g = d.grp_file(im.grp);
        let (_, gw, gh) = (d.grp)(im.grp);
        let h = 6 + im.frame as usize * 8;
        let f = |k: usize| g.get(h + k).copied().unwrap_or(0) as u16;
        let x = if im.flags & super::sprite::iflag::FLIPPED == 0 {
            f(0).wrapping_sub(gw >> 1)
        } else {
            (gw >> 1).wrapping_sub(f(2)).wrapping_sub(f(0))
        };
        let sp = &self.sprites[im.sprite as usize];
        let x = sp.x.wrapping_add(im.x as i16).wrapping_add(x as i16);
        let y = f(1).wrapping_sub(gh >> 1).wrapping_add(im.y as i16 as u16).wrapping_add(sp.y as u16) as i16;
        (x, y)
    }

    /// 0x4977c0: bounds (left, top, right, bottom, inclusive) of a sprite's clickable images.
    fn sprite_click_bounds(&self, d: &Data, s: u32) -> Option<[i32; 4]> {
        let mut r: Option<[i32; 4]> = None;
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let im = &self.images[i as usize];
            if im.flags & super::sprite::iflag::CLICKABLE != 0 {
                let (x, y) = self.image_map_pos(d, i);
                let h = 6 + im.frame as usize * 8;
                let g = d.grp_file(im.grp);
                let (w, ht) = (g.get(h + 2).copied().unwrap_or(0) as i32, g.get(h + 3).copied().unwrap_or(0) as i32);
                let b = [x as i32, y as i32, w - 1 + x as i32, ht - 1 + y as i32];
                r = Some(match r {
                    None => b,
                    Some(a) => [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])],
                });
            }
            i = im.link.next;
        }
        r
    }

    /// 0x4029b0 / 0x497e10 body: image pixel under map point (px, py).
    fn image_hit(&self, d: &Data, i: u32, px: i32, py: i32) -> bool {
        let im = &self.images[i as usize];
        let (x, y) = self.image_map_pos(d, i);
        let g = d.grp_file(im.grp);
        let mut fx = px - x as i32;
        if im.flags & super::sprite::iflag::FLIPPED != 0 {
            let gw = (d.grp)(im.grp).1 as i32;
            let frame_x = g.get(6 + im.frame as usize * 8).copied().unwrap_or(0) as i32;
            fx = gw - frame_x * 2 - fx;
        }
        grp_pixel(g, im.frame, fx as u32, (py - y as i32) as u32)
    }

    /// 0x402a00: point (px, py) is on unit `u`'s graphics (bounds test = 0x498220).
    fn unit_hit(&self, d: &Data, u: u32, px: i32, py: i32) -> bool {
        let s = self.units[u as usize].sprite;
        let Some(b) = self.sprite_click_bounds(d, s) else { return false };
        if px < b[0] || py < b[1] || px >= b[2] || py >= b[3] {
            return false;
        }
        if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x100 != 0 {
            // 0x497e10: any clickable image.
            let mut i = self.sprites[s as usize].images.head;
            while i != NIL {
                if self.images[i as usize].flags & super::sprite::iflag::CLICKABLE != 0 && self.image_hit(d, i, px, py) {
                    return true;
                }
                i = self.images[i as usize].link.next;
            }
            return false;
        }
        self.image_hit(d, self.sprites[s as usize].main_image, px, py)
    }

    /// 0x431030: units whose clickable bounds contain map point (x, y).
    fn clickable_units_at(&mut self, d: &Data, x: i16, y: i16) -> Vec<u32> {
        let (mw, mh) = (self.search.max_w as i16, self.search.max_h as i16);
        let b = [x.wrapping_sub(0x20), y.wrapping_sub(0x20), mw.wrapping_add(x).wrapping_add(0x20), mh.wrapping_add(y).wrapping_add(0x20)];
        let start = self.units_in_finder_box_now(b);
        let found = self.search.results(start);
        self.search.pop();
        let (x, y) = (x as i32, y as i32);
        found
            .into_iter()
            .filter(|&u| {
                self.sprite_click_bounds(d, self.units[u as usize].sprite)
                    .is_some_and(|b| b[0] <= x && b[1] <= y && x < b[2] && y < b[3])
            })
            .collect()
    }

    /// 0x46f3a0: the unit drawn topmost under map point (x, y) whose graphics are
    /// hit, else the smallest one whose bounds contain it.
    pub fn unit_at_point(&mut self, d: &Data, x: i16, y: i16) -> u32 {
        let mask = self.local_vision_mask;
        let (px, py) = (x as i32, y as i32);
        let mut best = NIL;
        let mut area = i32::MAX;
        for mut u in self.clickable_units_at(d, x, y) {
            if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x10 != 0 {
                u = self.units[u as usize].subunit;
            }
            let un = &self.units[u as usize];
            // ponytail: replay vision (0x6d0f14 / 0x6d0f18 / 0x6d0f1c) not handled.
            if self.sprites[un.sprite as usize].visibility & mask == 0
                || un.status & 0x300 != 0 && un.visibility & mask as u32 == 0
                || never_picked(un.unit_type)
            {
                continue;
            }
            let place_area = |st: &State, u: u32| {
                let pb = d.dat.units.placement_box[st.units[u as usize].unit_type as usize];
                (pb as u16 as i16 as i32) * ((pb >> 16) as u16 as i16 as i32)
            };
            let hit = |st: &State, u: u32| {
                st.unit_hit(d, u, px, py) || {
                    let s = st.units[u as usize].subunit;
                    s != NIL && st.unit_hit(d, s, px, py)
                }
            };
            if best == NIL {
                best = u;
                area = place_area(self, u);
                continue;
            }
            let smaller = if self.draw_key(best) < self.draw_key(u) {
                if hit(self, u) {
                    let sp = &self.sprites[un.sprite as usize];
                    best = u;
                    area = sp.height as i32 * sp.width as i32;
                    continue;
                }
                true
            } else {
                !hit(self, best)
            };
            if smaller {
                let a = place_area(self, u);
                if a < area {
                    best = u;
                    area = a;
                }
            }
        }
        best
    }
}

impl State {
    /// 0x46fb40: click selection at map point `p`. Shift toggles the unit in `cur`;
    /// Ctrl or a double-click on a selected unit picks the units like it in the
    /// screen rect `screen` (x0, y0, x1, y1). Returns the new selection, or None.
    /// ponytail: "selected" is `cur.contains`, not the sprite's selection flag 0x8.
    #[allow(clippy::too_many_arguments)]
    pub fn click_select(&mut self, d: &Data, p: (i16, i16), shift: bool, ctrl: bool, double: bool, screen: [i16; 4], cur: &[u32]) -> Option<Vec<u32>> {
        let u = self.unit_at_point(d, p.0, p.1);
        if u == NIL {
            return None;
        }
        let dbl = double && cur.contains(&u);
        if !(ctrl || dbl || shift && !cur.is_empty()) {
            let sel = self.alt_group_shortcut(d, u);
            self.rally_marker_if_own(d, u);
            return sel;
        }
        if ctrl || dbl {
            let start = self.units_in_finder_box_now(screen);
            let found = self.search.results(start);
            self.search.pop();
            let picked = self.select_filter(d, &found, u);
            if picked.is_empty() {
                return None;
            }
            if !shift {
                return Some(picked);
            }
            let mut sel = cur.to_vec();
            self.select_merge(d, &mut sel, &picked, u);
            return Some(sel);
        }
        let mut sel = cur.to_vec();
        if let Some(i) = sel.iter().position(|&s| s == u) {
            sel.remove(i);
            return Some(sel);
        }
        let own = |st: &State, u: u32| st.multi_selectable(d, u) && !st.replay && st.units[u as usize].player == st.local_player;
        if sel.len() < 12 && own(self, sel[0]) && own(self, u) {
            sel.push(u);
            return Some(sel);
        }
        None
    }

    /// 0x42ff80 over this state's finder.
    pub(super) fn units_in_finder_box_now(&mut self, b: [i16; 4]) -> usize {
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        self.search = search;
        start
    }
}

/// The units of a NIL-terminated selection row.
pub fn selection_units(row: &[u32; 12]) -> &[u32] {
    let n = row.iter().position(|&u| u == NIL).unwrap_or(12);
    &row[..n]
}

impl State {
    /// 0x49a740: clear player `p`'s selection.
    fn selection_clear(&mut self, p: usize) {
        if p < 8 {
            self.selections[p] = [NIL; 12];
        }
    }

    /// 0x49af80: put `u` in slot `i` of player `p`'s selection; slots after the first
    /// take only `player`'s multi-selectable units.
    fn selection_put(&mut self, d: &Data, p: usize, i: usize, u: u32, player: u8) -> bool {
        if p > 7 || i > 11 || self.sprites[self.units[u as usize].sprite as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            return false;
        }
        if i > 0 && (!self.multi_selectable(d, u) || self.units[u as usize].player != player) {
            return false;
        }
        self.selections[p][i] = u;
        true
    }

    /// 0x49a170: take `u` out of player `p`'s selection; returns the remaining count.
    fn selection_remove(&mut self, p: usize, u: u32) -> usize {
        if p >= 8 {
            return 0;
        }
        let row = &mut self.selections[p];
        let mut n = row.iter().position(|&s| s == NIL).unwrap_or(12);
        let at = row[..n].iter().rposition(|&s| s == u);
        if u != NIL && n != 0 {
            if let Some(i) = at {
                n -= 1;
                row.copy_within(i + 1..=n, i);
                row[n] = NIL;
            }
        }
        n
    }

    /// 0x49a7f0 (selection part): a dying or hidden unit leaves every selection.
    pub fn selections_remove_unit(&mut self, u: u32) {
        for p in 0..8 {
            self.selection_remove(p, u);
        }
    }

    /// 0x496560: the recent-selection group (0..8) used longest ago (0xff if none).
    fn least_recent_group(&self, p: usize) -> u8 {
        let mut best = (0xffffu16, 0xffu8);
        for i in (0..8).rev() {
            if self.recent_used[p][i] < best.0 {
                best = (self.recent_used[p][i], i as u8);
            }
        }
        best.1
    }

    /// Remember a selection of more than one unit in the least recent of groups 10..=17.
    fn remember_selection(&mut self, d: &Data, p: usize, player: u8) {
        let g = self.least_recent_group(p);
        self.hotkey_save(d, p, g.wrapping_add(10) as usize, true, player);
        if let Some(s) = self.recent_used[p].get_mut(g as usize) {
            *s = self.frame as u16;
        }
    }

    /// 0x4965d0: save player `p`'s selection as hotkey group `g`, replacing it when
    /// `fresh`, else adding to it.
    fn hotkey_save(&mut self, d: &Data, p: usize, g: usize, fresh: bool, player: u8) {
        let slot = g + p * 18;
        if slot >= self.hotkeys.len() {
            return;
        }
        let mut n = 0;
        if fresh {
            self.hotkeys[slot] = [0; 12];
        } else {
            if let Some(u) = self.unit_from_handle(self.hotkeys[slot][0]) {
                if !self.multi_selectable(d, u) {
                    return;
                }
            }
            n = self.hotkeys[slot].iter().position(|&h| h == 0).unwrap_or(12);
        }
        let sel = self.selections.get(p).copied().unwrap_or([NIL; 12]);
        for &u in selection_units(&sel) {
            if self.units[u as usize].player != player {
                return;
            }
            let h = self.unit_handle(u);
            if !fresh && (self.hotkeys[slot][..n].contains(&h) || n != 0 && !self.multi_selectable(d, u)) {
                continue;
            }
            // Native writes at flat index n + slot*12: a full group (n == 12) spills into
            // the next group's first entry.
            if let Some(e) = self.hotkeys.get_mut(slot + n / 12) {
                e[n % 12] = h;
            }
            n += 1;
            if n > 11 {
                return;
            }
        }
    }

    /// 0x496940: select hotkey group `g`, dropping units that are gone.
    fn hotkey_select(&mut self, d: &Data, p: usize, g: usize, player: u8) {
        let slot = g + p * 18;
        if slot >= self.hotkeys.len() {
            return;
        }
        let mut n = self.hotkeys[slot].iter().position(|&h| h == 0).unwrap_or(12);
        if n == 0 {
            return;
        }
        self.selection_clear(p);
        let mut i = 0;
        while i < n {
            let h = self.hotkeys[slot][i];
            let keep = self.unit_from_handle(h).filter(|&u| {
                let un = &self.units[u as usize];
                un.player == player
                    && self.sprites[un.sprite as usize].flags & super::sprite::sflag::HIDDEN == 0
                    && (self.multi_selectable(d, u) || n <= 1)
            });
            let Some(u) = keep else {
                n -= 1;
                self.hotkeys[slot][i] = self.hotkeys[slot][n];
                self.hotkeys[slot][n] = 0;
                continue;
            };
            if p < 8 && i < 12 && (i < 1 || self.multi_selectable(d, u)) {
                self.selections[p][i] = u;
            }
            i += 1;
        }
        if g > 9 {
            if let Some(s) = self.recent_used[p].get_mut(g - 10) {
                *s = self.frame as u16;
            }
        } else {
            self.remember_selection(d, p, player);
        }
    }

    /// 0x496b40 (the part before it selects): the units of hotkey group `g` of `player` that
    /// 0x496940 would keep, without changing the group table.
    pub fn hotkey_group_units(&self, d: &Data, player: u8, g: u8) -> Vec<u32> {
        let slot = g as usize + player as usize * 18;
        let Some(group) = self.hotkeys.get(slot) else { return Vec::new() };
        let n = group.iter().position(|&h| h == 0).unwrap_or(12);
        let mut out = Vec::new();
        for &h in &group[..n] {
            let Some(u) = self.unit_from_handle(h).filter(|&u| {
                let un = &self.units[u as usize];
                un.player == player
                    && self.sprites[un.sprite as usize].flags & super::sprite::sflag::HIDDEN == 0
                    && (self.multi_selectable(d, u) || n <= 1)
            }) else {
                continue;
            };
            out.push(u);
        }
        out
    }

    /// Command 0x09 (0x4c2750): select the units of `handles`.
    pub fn command_select(&mut self, d: &Data, player: u8, handles: &[u16]) {
        let p = player as usize;
        if handles.len() >= 13 {
            return;
        }
        self.selection_clear(p);
        let mut n = 0;
        for &h in handles {
            let Some(u) = self.unit_from_handle(h as u32) else { continue };
            if p < 8 && selection_units(&self.selections[p]).contains(&u) {
                continue;
            }
            if self.units[u as usize].unit_type != 0xe && self.selection_put(d, p, n, u, player) {
                n += 1;
            }
        }
        if n > 1 {
            self.remember_selection(d, p, player);
        }
    }

    /// Command 0x0a (0x4c2560): add the units of `handles` to the selection.
    pub fn command_select_add(&mut self, d: &Data, player: u8, handles: &[u16]) {
        let p = player as usize;
        let mut n = if p < 8 { selection_units(&self.selections[p]).len() } else { 0 };
        if handles.len() + n >= 13 {
            return;
        }
        for &h in handles {
            let Some(u) = self.unit_from_handle(h as u32) else { continue };
            if p < 8 && selection_units(&self.selections[p]).contains(&u) {
                continue;
            }
            let hidden = self.sprites[self.units[u as usize].sprite as usize].flags & super::sprite::sflag::HIDDEN != 0;
            if p < 8 && n < 12 && !hidden && (n < 1 || self.multi_selectable(d, u) && self.units[u as usize].player == player) {
                self.selections[p][n] = u;
                n += 1;
            }
        }
        if n > 1 {
            self.remember_selection(d, p, player);
        }
    }

    /// Command 0x0b (0x4bfb40): take the units of `handles` out of the selection.
    pub fn command_select_remove(&mut self, d: &Data, player: u8, handles: &[u16]) {
        let p = player as usize;
        if handles.len() >= 13 {
            return;
        }
        let mut n = 0;
        for &h in handles {
            let Some(u) = self.unit_from_handle(h as u32) else { continue };
            n = if p < 8 { self.selection_remove(p, u) } else { 0 };
        }
        if n > 1 {
            self.remember_selection(d, p, player);
        }
    }

    /// Command 0x13 (0x4c2870): hotkey group `g`: 0 = set, 1 = select, 2 = add.
    pub fn command_hotkey(&mut self, d: &Data, player: u8, action: u8, g: u8) {
        if g >= 0x13 {
            return;
        }
        let p = player as usize;
        match action {
            0 => self.hotkey_save(d, p, g as usize, true, player),
            1 => self.hotkey_select(d, p, g as usize, player),
            2 => self.hotkey_save(d, p, g as usize, false, player),
            _ => {}
        }
    }
}

impl State {
    /// 0x4c0860: the select commands that tell the other players the local selection
    /// changed from `sent` (0x59724c, updated) to `new`: one 0x09 with the whole
    /// selection (in reverse order) when that is shorter, else 0x0b removals then 0x0a
    /// additions (only of units the local player sees).
    pub fn selection_packets(&self, sent: &mut Vec<u32>, new: &[u32]) -> Vec<Vec<u8>> {
        let mask = if self.replay { 0 } else { self.local_vision_mask };
        let h = |u: u32| (self.unit_handle(u) as u16).to_le_bytes();
        let added: Vec<u32> = new
            .iter()
            .copied()
            .filter(|u| !sent.contains(u) && self.sprites[self.units[*u as usize].sprite as usize].visibility & mask != 0)
            .collect();
        let removed: Vec<u32> = sent.iter().copied().filter(|u| !new.contains(u)).collect();
        *sent = new.to_vec();
        let pkt = |op: u8, us: &mut dyn Iterator<Item = u32>| {
            let hs: Vec<u8> = us.flat_map(h).collect();
            let mut p = vec![op, (hs.len() / 2) as u8];
            p.extend(hs);
            p
        };
        if new.len() <= added.len() + removed.len() {
            return vec![pkt(0x09, &mut new.iter().rev().copied())];
        }
        let mut out = Vec::new();
        if !removed.is_empty() {
            out.push(pkt(0x0b, &mut removed.into_iter()));
        }
        if !added.is_empty() {
            out.push(pkt(0x0a, &mut added.into_iter()));
        }
        out
    }
}

impl State {
    /// 0x49b0e0 (AL = player; `mode` = 0x6d5bbe): the color slot a player's units are drawn
    /// in. With mode 2 (ally / enemy colors) it is 2 for the local player, 7 for an ally and
    /// 0 for an enemy, mapped to the slot (0x57f21c index) that holds that color; otherwise
    /// the player itself.
    pub fn team_color_slot(&self, mode: u8, player: u8) -> u8 {
        if mode != 2 {
            return player;
        }
        let v = if self.local_player == player { 2 } else { ((self.alliances[self.local_player as usize][player as usize] == 0) as u8).wrapping_sub(1) & 7 };
        match self.color_slot.iter().position(|&c| c == v as u32) {
            Some(i) => i as u8,
            None => v,
        }
    }

    /// 0x49b870: rebuild the selection circle colors (0x581d6a) from the alliances of the
    /// local player (all 0 but neutral player 11 -> 1 while watching a replay), then redo
    /// the local selection's sprites so their circles take the new color.
    pub fn selection_circle_colors(&mut self, d: &Data) {
        let local = self.local_player as usize;
        for p in 0..12 {
            self.circle_color[p] = if !self.replay {
                if p == local { 0 } else { (self.alliances[local][p] == 0) as u8 + 1 }
            } else if p == 11 {
                1
            } else {
                0
            };
        }
        for i in 0..self.selected.len().min(12) {
            let u = self.selected[i];
            if self.replay || self.units[u as usize].player != self.local_player {
                // 0x4e6290, 0x4e6180: the 0x9c (flag 0xdc & 1) calls need !replay and an
                // own unit, never both here.
                self.sprite_deselect(d, u);
                self.sprite_select(d, u, i as u8);
                if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x10 != 0 {
                    self.selected[i] = self.units[u as usize].subunit;
                }
            }
        }
        if self.multiplayer && self.template[0x10] != 0 {
            // Other members of the local player's force (0x57eeea): each selected unit not the
            // local player's (or any, in a replay) takes one more ally-selection mark.
            let me = self.local_player as usize;
            for p in 0..8 {
                if p == me || self.player_force[me] != self.player_force[p] {
                    continue;
                }
                for i in 0..12 {
                    let u = self.selections[p][i];
                    if u == NIL {
                        break;
                    }
                    if !self.replay && self.units[u as usize].player == self.local_player {
                        continue;
                    }
                    let s = self.units[u as usize].sprite;
                    let b = self.sprites[s as usize].flags;
                    if (b >> 1) & 3 != 0 {
                        let b = (((b >> 1).wrapping_sub(1)).wrapping_mul(2) ^ b) & 6 ^ b;
                        self.sprites[s as usize].flags = b;
                        if b & 6 == 0 {
                            self.ally_selection_image_remove(s);
                        }
                    }
                    let c = self.circle_color[self.units[u as usize].player as usize];
                    self.ally_selection_add(d, s, c);
                }
            }
        }
    }
}

impl State {
    /// 0x45ea30 (portrait 0x52 with a unit of `owner`): the advisor portrait in the owner's
    /// color slot, 0x52 + slot, or 0x62 + the slot's override (0x62 + owner for 8 and up).
    pub fn advisor_portrait(&self, owner: u8) -> u16 {
        let mut b = owner as u32;
        if b < 8 {
            b = self.color_slot[b as usize];
        }
        let b = b as u8;
        if b >= 8 {
            return 0x62 + b as u16;
        }
        match self.color_override[b as usize] {
            0 => 0x52 + b as u16,
            o => 0x62 + o as u16,
        }
    }

    /// 0x4e6290: the unit's sprite leaves the selected look (flag 8; the circle, flag 1).
    /// The circle and health bar images are drawn by the front end from `selected`.
    fn sprite_deselect(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite as usize;
        let f = &mut self.sprites[s].flags;
        if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x2000_0000 == 0 {
            // 0x497620
            if *f & 8 != 0 {
                *f &= !9;
            }
            if self.own_finished_pylon(u) {
                self.psi_hide_fields(d);
            }
        } else {
            // 0x4975d0
            *f &= !9;
        }
    }

    /// 0x4975d0: the unit's sprite drops its selection circle (flag 1).
    pub(super) fn sprite_deselect_circle(&mut self, u: u32) {
        let s = self.units[u as usize].sprite as usize;
        self.sprites[s].flags &= !1;
    }

    /// 0x4e6180 (EAX = unit, stack = slot): the unit's sprite takes the selected look
    /// (flag 8) and the slot it holds in the selection.
    pub(super) fn sprite_select(&mut self, d: &Data, u: u32, slot: u8) {
        let s = self.units[u as usize].sprite as usize;
        let sp = &mut self.sprites[s];
        if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x2000_0000 != 0 || sp.flags & 8 == 0 {
            sp.selection_index = slot;
            sp.flags |= 8;
        }
        if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x2000_0000 == 0 && self.own_finished_pylon(u) {
            self.psi_show_fields(d);
        }
    }

    /// The test of 0x4e6180 / 0x4e6290 before 0x493640 / 0x494100: not a replay, the local
    /// player's, finished (status bit 0) Pylon (0x9c).
    fn own_finished_pylon(&self, u: u32) -> bool {
        let un = &self.units[u as usize];
        !self.replay && un.player == self.local_player && un.status & 1 != 0 && un.unit_type == 0x9c
    }

    /// 0x497590: free a teammate's ally-selection image (0x23b..=0x244) of the sprite.
    pub fn ally_selection_image_remove(&mut self, s: u32) -> u8 {
        self.ui_image_remove(s, 0x23b, 0x244)
    }

    /// 0x485710: the most teammates that can select a unit at once + 1 (template teams byte
    /// 0x596875: 2 -> 4, 3 -> 3, 4 -> 2), 0 otherwise.
    fn ally_selection_limit(&self) -> u8 {
        match self.template[0x10] {
            2 => 4,
            3 => 3,
            4 => 2,
            _ => {
                crate::todo_port!("0x485710", "teams byte other than 2..4: one-time init (0x4c3d20, 0x4bb300, 0x4dcc50)");
                0
            }
        }
    }

    /// 0x499a10 (ESI = sprite, stack = color; 0x4e65c0 gives the owner's 0x581d6a color):
    /// one more teammate selects the sprite (count in flag bits 1..2); below 3 it gets the
    /// ally-selection image 0x23b (0x4d7070), undone if none is free.
    pub fn ally_selection_add(&mut self, d: &Data, s: u32, color: u8) {
        let lim = self.ally_selection_limit();
        let b = self.sprites[s as usize].flags;
        if (b >> 1) & 3 != lim.wrapping_sub(1) {
            let b = ((b & 0xfe).wrapping_add(2) ^ b) & 6 ^ b;
            self.sprites[s as usize].flags = b;
            if b & 6 < 3 && self.ui_circle_image(d, s, color, 0x23b).is_none() {
                self.sprites[s as usize].flags &= 0xf9;
            }
        }
    }

    /// 0x4e65c0 (from the tail of 0x49af80): after the unit entered another team member's selection (0x6284e8,
    /// 0x49a110 true) its sprite takes the ally-selection image (0x4e65c0). Hook: call it
    /// where a network game puts a unit in a player's selection (scbw has none, 0x57f0b4 = 0).
    pub fn ally_selection_unit(&mut self, d: &Data, u: u32) {
        let c = self.circle_color[self.units[u as usize].player as usize];
        let s = self.units[u as usize].sprite;
        self.ally_selection_add(d, s, c);
    }

    /// 0x49ae40 (EAX = array, stack = count): make the first `count` units of `units`
    /// the selection (0x6284b8). A unit with a subunit (type flag 0x10) is selected by
    /// that subunit, which replaces it in `units`.
    pub fn selection_set(&mut self, d: &Data, units: &mut [u32], count: usize) {
        for u in std::mem::take(&mut self.selected) {
            let t = self.units[u as usize].unit_type;
            let u = if d.dat.units.flags[t as usize] & 0x10 != 0 { self.units[u as usize].subunit } else { u };
            self.sprite_deselect(d, u);
        }
        for i in 0..count.min(12) {
            let u = units[i];
            self.selected.push(u);
            let t = self.units[u as usize].unit_type;
            if d.dat.units.flags[t as usize] & 0x10 == 0 {
                self.sprite_select(d, u, i as u8);
            } else {
                let sub = self.units[u as usize].subunit;
                self.sprite_select(d, sub, i as u8);
                self.selected[i] = sub;
            }
            units[i] = self.selected[i];
        }
    }

    /// 0x4eed10 (tail, after 0x4cfef0 / 0x4c3bb0 and 0x4d92a0): 0x596b7c = the local
    /// player's row (0x512688) of 0x6284e8, then all rows are cleared.
    pub fn take_start_selection(&mut self) {
        self.start_selection = self.selections[self.local_player as usize & 7];
        self.selections = [[NIL; 12]; 8];
    }

    /// 0x4d0820 (game start, from 0x4d9950 after the first steps): reselect what the saved
    /// game's local player had selected. 0x4eed10 copied that row of 0x6284e8 to 0x596b7c
    /// and zeroed all rows (nothing for a new game); the first 12 entries that have a
    /// sprite, are alive and are seen by the local vision mask become the selection (0x49ae40,
    /// sent by 0x4c0860 = the caller's `sim_selection`). A lone unit with Alt held (0x596a2a)
    /// takes its best recent group instead when it is in one (0x496d30 -> 0x496b40).
    /// The row is `start_selection`, taken by `take_start_selection` before the steps.
    pub fn game_start_selection(&mut self, d: &Data) {
        let row = self.start_selection;
        let mut list: Vec<u32> = selection_units(&row)
            .iter()
            .copied()
            .filter(|&u| {
                let un = &self.units[u as usize];
                un.sprite != NIL
                    && (un.order != 0 || un.order_state != 1)
                    && self.sprites[un.sprite as usize].visibility & self.local_vision_mask != 0
            })
            .collect();
        if !list.is_empty() {
            let mut done = false;
            if list.len() == 1 && self.key_alt {
                let h = self.unit_handle(list[0]);
                if let Some(g) = self.recent_group_of(h) {
                    let sel = self.select_recent_group(d, g);
                    if !sel.is_empty() {
                        let n = sel.len();
                        self.selection_set(d, &mut sel.clone(), n);
                    }
                    done = true;
                }
            }
            if !done {
                let n = list.len();
                self.selection_set(d, &mut list, n);
            }
        }
        // 0x59723c (when selected), 0x68c1b0, 0x68ac74, 0x68c1f8 = 1; 0x68c1e8 / 0x68c1ec = 0:
        // UI refresh flags.
    }

    /// 0x49f7a0 (EAX = unit): a selected unit (sprite flag 8) leaves the selection, by
    /// the slot its sprite holds; an emptied selection ends placement and targeting.
    pub fn selection_remove_local(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite as usize;
        if self.sprites[s].flags & 8 == 0 {
            return;
        }
        let slot = self.sprites[s].selection_index as usize;
        let mut list = self.selected.clone();
        if slot < list.len() {
            list.remove(slot);
        }
        let n = list.len();
        self.selection_set(d, &mut list, n);
        // 0x59723c, 0x68c1b0, 0x68ac74, 0x68c1f8: redraw flags; 0x68c1e8 / 0x68c1ec = 0.
        if n == 0 {
            self.selection_emptied = true;
        }
    }

    /// 0x4c3b40 (EDX = unit): deselect `u`, telling the other players (0x4c0860: the
    /// select commands go out when the front end next sends a command, see
    /// `selection_packets`).
    pub fn selection_deselect(&mut self, d: &Data, u: u32) {
        let mut list: Vec<u32> = self.selected.iter().copied().filter(|&x| x != u).collect();
        let n = list.len();
        self.selection_set(d, &mut list, n);
    }

    /// 0x45d040: `u` joins the selection in its first empty slot (when there is one);
    /// the select commands as in 0x4c3b40.
    pub fn selection_add_local(&mut self, d: &Data, u: u32) {
        let mut list = self.selected.clone();
        let i = list.len();
        if i < 12 {
            list.push(u);
            self.selection_set(d, &mut list, i + 1);
        }
    }

    /// 0x49a2c0: true when no selected unit is a plain, working one: every one is
    /// flagged 0x400, locked down, stasised or under maelstrom (also for an empty
    /// selection).
    pub fn selection_all_disabled(&self) -> bool {
        self.selected.iter().all(|&u| {
            let un = &self.units[u as usize];
            un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0
        })
    }
}
