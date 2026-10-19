//! The map file and the rest of saveload.cpp.

use super::{In, Out};
use crate::bw::list::NIL;
use crate::bw::{Data, State};

/// The bytes of a dat column (`w` bytes per entry, the first `n` entries).
fn col<T: Copy + Into<u64>>(v: &[T], n: usize, w: usize) -> Vec<u8> {
    (0..n).flat_map(|i| v.get(i).map_or(0, |&x| x.into()).to_le_bytes().into_iter().take(w)).collect()
}

impl State {
    /// 0x4cee00: the selections 0x6284e8 (8 x 12) as written to the file: a unit as its
    /// index + 1 (below 0x6a5, else 0) with its unique id (+0xa5) above bit 11.
    fn selections_encode(&self) -> Vec<u32> {
        self.selections
            .iter()
            .flatten()
            .map(|&u| {
                if u == NIL {
                    return 0;
                }
                let n = u + 1;
                if n < 0x6a5 { (self.units[u as usize].unique_id as u32) << 11 | n } else { 0 }
            })
            .collect()
    }

    /// 0x4ceda0: the selections 0x6284e8 from their file form: a handle that names no unit
    /// with a sprite, a dead one (order 0, state 1) or another unique id becomes empty.
    fn selections_decode(&mut self, v: &[u32]) {
        for (k, &h) in v.iter().enumerate().take(96) {
            let i = (h & 0x7ff) as usize;
            self.selections[k / 12][k % 12] = if h == 0 || i == 0 || i > self.units.len() {
                NIL
            } else {
                let un = &self.units[i - 1];
                if un.sprite == NIL || (un.order == 0 && un.order_state == 1) || un.unique_id as u32 != (h as i32 >> 11) as u32 {
                    NIL
                } else {
                    i as u32 - 1
                }
            };
        }
    }

    /// 0x4d02d0: the CHK (0x5994d8 bytes at 0x5993d4), 0x4cee00, the selections 0x6284e8 (0x180), 0x4ceda0
    pub(super) fn save_chk(&mut self, _d: &Data, o: &mut Out) -> bool {
        o.u32(self.chk.len() as u32);
        let mut ok = o.packed(&self.chk);
        let enc = self.selections_encode(); // 0x4cee00
        let b: Vec<u8> = enc.iter().flat_map(|v| v.to_le_bytes()).collect();
        ok &= o.packed(&b);
        self.selections_decode(&enc); // 0x4ceda0
        ok
    }

    /// 0x4cfef0: the CHK, 0x6284e8, 0x4ceda0
    pub(super) fn load_chk(&mut self, _d: &Data, i: &mut In) -> bool {
        let Some(n) = i.u32().filter(|&n| n != 0) else { return false };
        let Some(chk) = i.packed(n as usize) else { return false };
        self.chk = chk;
        let Some(b) = i.packed(0x180) else { return false };
        let v: Vec<u32> = b.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        self.selections_decode(&v); // 0x4ceda0
        true
    }
}

impl State {
    /// 0x4935f0 / 0x4989a0: before the images are written, sprite and image pointers
    /// become indices.
    ///
    /// 0x4935f0: every finished Pylon (0x9c) drops its field sprite (0x497b40) and leaves the
    /// provider list (0x493100). 0x4989a0: every sprite loses its selection and
    /// teammate-selection looks (0x497620 / 0x4975d0: flags 8 and 1; the ally images 0x23b..=0x244
    /// and flags 6). The pointer-to-index conversion of the native structures (0x4989a0's other
    /// effect is none) is done by the writers.
    pub(super) fn save_prepare(&mut self, _d: &Data) {
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &self.units[u as usize];
            if un.unit_type == 0x9c && un.status & 1 != 0 {
                let s = un.ptr_at(0xd0);
                if s != NIL {
                    self.free_sprite_images(s); // 0x497b40
                }
                self.units[u as usize].set_ptr_at(0xd0, NIL);
                self.remove_psi_provider(u); // 0x493100
            }
            u = self.units[u as usize].link.next;
        }
        for s in 0..self.sprites.len() as u32 {
            let f = &mut self.sprites[s as usize].flags;
            if *f & 8 != 0 {
                *f &= !9; // 0x497620 (the health bar and circle images are drawn by the front end)
            } else if *f & 1 != 0 {
                *f &= !1; // 0x4975d0
            }
            if *f & 6 != 0 {
                *f &= 0xf9;
                self.ally_selection_image_remove(s); // 0x4d4fa0
            }
        }
    }

    /// 0x499a60 / 0x494030: the pointers back after the units are written.
    ///
    /// 0x499a60: the local selection (0x6284b8) is selected again (0x4e6180; a unit with a
    /// subunit is replaced by it), and in a team game each other member of the local player's
    /// force marks the units of its selection with an ally-selection image (0x499a10).
    /// 0x494030: the Pylons' fields again (`psi_restore`).
    pub(super) fn save_restore(&mut self, d: &Data) {
        for i in 0..self.selected.len().min(12) {
            let u = self.selected[i];
            self.sprite_select(d, u, i as u8); // 0x4e6180
            if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x10 != 0 {
                self.selected[i] = self.units[u as usize].subunit;
            }
        }
        if self.multiplayer && self.template[0x10] != 0 {
            let me = self.local_player as usize;
            let force = self.player_force[me];
            for p in (0..8).rev() {
                if p == me || self.player_force[p] != force {
                    continue;
                }
                for j in 0..12 {
                    let u = self.selections[p][j];
                    if u == NIL {
                        break;
                    }
                    let c = self.circle_color[self.units[u as usize].player as usize];
                    let s = self.units[u as usize].sprite;
                    self.ally_selection_add(d, s, c); // 0x499a10
                }
            }
        }
        self.psi_restore(d);
    }

    /// 0x494030: every finished Pylon (0x9c) joins the provider list again and makes its
    /// field sprite (0x493f70); one of a rescuable-type player (0x57eee8 = 3) also gets
    /// its interruptible orders dropped and order 0xa1 queued, then the next order starts.
    fn psi_restore(&mut self, d: &Data) {
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &self.units[u as usize];
            if un.unit_type == 0x9c && un.status & 1 != 0 {
                self.order_initialize_psi_provider(d, u); // 0x493f70
                if self.player_types[self.units[u as usize].player as usize] == 3 {
                    self.units[u as usize].order_flags |= 1;
                    if self.units[u as usize].order != 0 {
                        loop {
                            let t = self.units[u as usize].order_queue_tail;
                            if t == NIL {
                                break;
                            }
                            let o = self.orders[t as usize].order;
                            if d.dat.orders.interruptible[o as usize] != 0 || o == 0xa1 {
                                self.order_remove(d, u, t); // 0x4742d0
                            } else {
                                break;
                            }
                        }
                        self.order_queue_append(d, u, 0xa1, 0xe4, (0, 0), NIL); // 0x4745f0
                    }
                    self.prepare_next_order(d, u); // 0x475000
                }
            }
            u = self.units[u as usize].link.next;
        }
    }

    /// 0x4bf390, then 0x62848c and 0x6284a8.
    ///
    /// 0x4bf390: the dat tables a map can change (UNIx hit points, shields, armor, build time,
    /// mineral and gas cost, name; weapons damage and bonus; UPGx; TECx), from the game's dat.
    /// Then the screen position in pixels (0x62848c, 0x6284a8).
    pub(super) fn save_tail(&mut self, d: &Data, o: &mut Out) -> bool {
        let (u, w, g, t) = (&d.dat.units, &d.dat.weapons, &d.dat.upgrades, &d.dat.techdata);
        let tables = [
            col(&u.hit_points, 228, 4),
            col(&u.shields, 228, 2),
            col(&u.armor, 228, 1),
            col(&u.build_time, 228, 2),
            col(&u.mineral_cost, 228, 2),
            col(&u.gas_cost, 228, 2),
            col(&u.name_string, 228, 2),
            col(&w.damage, 130, 2),
            col(&w.damage_bonus, 130, 2),
            col(&g.mineral_base, 61, 2),
            col(&g.mineral_factor, 61, 2),
            col(&g.gas_base, 61, 2),
            col(&g.gas_factor, 61, 2),
            col(&g.time_base, 61, 2),
            col(&g.time_factor, 61, 2),
            col(&t.mineral_cost, 44, 2),
            col(&t.gas_cost, 44, 2),
            col(&t.research_time, 44, 2),
            col(&t.energy_cost, 44, 2),
        ];
        let mut ok = true;
        for b in &tables {
            ok &= o.packed(b);
        }
        // not kept: the screen position in pixels (0x62848c, 0x6284a8; 8-pixel aligned); the
        // tile position gives it to 32 pixels.
        o.u32(self.screen_tile.0 as u32 * 32);
        o.u32(self.screen_tile.1 as u32 * 32);
        ok
    }

    /// 0x4bf020, 0x62848c, 0x6284a8, then 0x4ce440, 0x4ead50, 0x4dbc00 and 0x49c440.
    ///
    /// 0x4bf020 (a version 3 file, as 0x4bf390 writes): the dat tables of `save_tail`. Data is
    /// read-only here: they are read and dropped (not kept; the map's own CHK set the same).
    /// 0x4ead50: the unit count from the free list, then the Pylons' fields (`psi_restore`).
    /// 0x49c440: the screen position, clamped to the map.
    pub(super) fn load_tail(&mut self, d: &Data, i: &mut In) -> bool {
        for n in [0x390, 0x1c8, 0xe4, 0x1c8, 0x1c8, 0x1c8, 0x1c8, 0x104, 0x104, 0x7a, 0x7a, 0x7a, 0x7a, 0x7a, 0x7a, 0x58, 0x58, 0x58, 0x58] {
            if i.packed(n).is_none() {
                return false;
            }
        }
        let (Some(x), Some(y)) = (i.u32(), i.u32()) else { return false };
        // 0x4ce440: close the file (the caller drops `In`).
        // 0x4ead50
        let mut free = 0;
        let mut u = self.free_units.head;
        while u != NIL {
            free += 1;
            u = self.units[u as usize].link.next;
        }
        self.unit_count = self.max_units - free;
        self.psi_restore(d);
        // 0x4dbc00: 0x51ca1c (a campaign map continues) = 0x57f1e3 when not multiplayer. The
        // glue's flag is `GameEnd.camp_cont`, which reads `camp_cont` (loaded with the game
        // data) when the game ends.
        // 0x49c440 (0x658ac0, a scroll in progress, is 0): the screen position in 8-pixel
        // steps and tiles, within the map (0x628488, 0x6284b0). The parallax scroll (0x49bfd0)
        // is not kept (UI).
        let (mw, mh) = (self.map_px.0 as i64 / 32, self.map_px.1 as i64 / 32);
        let (maxx, maxy) = (((mw - 20) * 32).max(0), ((mh - 12) * 32 + 8).max(0));
        let (x, y) = ((x as i32 as i64).clamp(0, maxx), (y as i32 as i64).clamp(0, maxy));
        self.screen_tile = ((x >> 5) as u16, (y >> 5) as u16);
        true
    }
}
