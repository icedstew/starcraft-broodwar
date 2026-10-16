//! Cloaking and detection: the Cloak / Decloak commands and secondary orders, the list
//! of units asking to stay cloaked (0x63ff5c, linked through +0xf0 / +0xf4), which
//! players detect a cloaked unit (+0xe4), the cloaking image fades and energy
//! regeneration (cloaked units pay for their cloak instead of regenerating).

use super::list::NIL;
use super::sprite::iflag;
use super::{Data, State};

/// Tech that cloaks unit type `t`: Personal Cloaking (ghosts, Sarah, Duran), Cloaking
/// Field (wraiths, Tom Kazansky), else none (0x2c).
fn cloak_tech(t: u16) -> u16 {
    match t {
        1 | 0x10 | 100 | 99 | 0x68 | 0x33 => 10,
        8 | 0x15 => 9,
        _ => 0x2c,
    }
}

impl State {
    /// 0x4c0720: Cloak command.
    pub fn command_cloak(&mut self, d: &Data, player: u8, sel: &[u32]) {
        for u in self.live_selection(sel) {
            let t = cloak_tech(self.units[u as usize].unit_type);
            if self.can_use_tech(d, u, t, player) == Ok(1) {
                self.start_cloak_order(d, u);
            }
        }
    }

    /// 0x4c0660: Decloak command.
    pub fn command_decloak(&mut self, d: &Data, player: u8, sel: &[u32]) {
        for u in self.live_selection(sel) {
            let t = cloak_tech(self.units[u as usize].unit_type);
            if self.can_use_tech(d, u, t, player) == Ok(1) {
                self.set_secondary_order(u, 0x6e);
            }
        }
    }

    /// 0x491b30: pay the cloak's energy and switch to the Cloak secondary order.
    fn start_cloak_order(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].status & 0x100 != 0 {
            return;
        }
        let cost = d.dat.techdata.energy_cost[cloak_tech(self.units[u as usize].unit_type) as usize];
        let cheat = self.game_options & 0x800 != 0;
        if cheat || cost.wrapping_mul(0x100) <= self.units[u as usize].energy {
            if !cheat {
                let e = &mut self.units[u as usize].energy;
                *e = e.wrapping_sub(cost.wrapping_mul(0x100));
            }
            self.set_secondary_order(u, 0x6d);
        }
    }

    /// 0x491790: Cloak (secondary): ask to stay cloaked this frame; the first ask joins
    /// the cloak list.
    pub fn secondary_cloak(&mut self, u: u32) {
        let un = &mut self.units[u as usize];
        let c = un.is_cloaked;
        un.is_cloaked = c.wrapping_add(1);
        if c == 0 && un.status & 0x100 == 0 && un.next_burrowed == NIL {
            let head = self.cloak_list_head;
            let un = &mut self.units[u as usize];
            un.next_burrowed = head;
            un.prev_burrowed = NIL;
            if head != NIL {
                self.units[head as usize].prev_burrowed = u;
            }
            // 0x68c1b0 etc.: UI refresh.
            self.cloak_list_head = u;
        }
    }

    /// 0x4633e0: Decloak (secondary): back to Nothing.
    pub fn secondary_decloak(&mut self, u: u32) {
        self.set_secondary_order(u, 0x17);
    }

    /// 0x4916e0: leave the cloak list if nothing asked to stay cloaked this frame.
    fn cloak_list_leave(&mut self, u: u32) -> bool {
        if self.units[u as usize].is_cloaked != 0 {
            return false;
        }
        if self.cloak_list_head == u {
            self.cloak_list_head = self.units[u as usize].next_burrowed;
        }
        let (prev, next) = (self.units[u as usize].prev_burrowed, self.units[u as usize].next_burrowed);
        if prev != NIL {
            self.units[prev as usize].next_burrowed = next;
        }
        if next != NIL {
            self.units[next as usize].prev_burrowed = prev;
        }
        let un = &mut self.units[u as usize];
        un.status &= !0x800;
        un.prev_burrowed = NIL;
        un.next_burrowed = NIL;
        true
    }

    /// 0x492eb0: units that stopped asking decloak; the others cloak if not yet.
    pub fn update_cloak_list(&mut self, d: &Data) {
        let mut u = self.cloak_list_head;
        while u != NIL {
            let next = self.units[u as usize].next_burrowed;
            if self.cloak_list_leave(u) {
                self.end_cloak(d, u);
            } else {
                let un = &mut self.units[u as usize];
                if un.status & 0x10 == 0 && un.secondary_order == 0x6d && un.is_cloaked == 1 && un.status & 0x800 != 0 {
                    un.status &= !0x800;
                }
                if self.units[u as usize].status & 0x100 == 0 {
                    self.begin_cloak(d, u);
                }
            }
            u = next;
        }
        // 0x68c1b0 etc.: UI refresh.
    }

    /// 0x49b5b0: start cloaking: images fade out, nobody detects the unit yet.
    pub(super) fn begin_cloak(&mut self, d: &Data, u: u32) {
        // 0x48ed50: the cloak sound (UI).
        if self.units[u as usize].status & 0x10 != 0 {
            self.burrowed_cloak(d, u);
            self.refresh_detection(d, u);
            self.units[u as usize].ai_action_flag = 0x1e;
            return;
        }
        let s = self.units[u as usize].sprite;
        self.sprite_start_cloaking(d, s);
        let un = &mut self.units[u as usize];
        un.status = (un.status & !0x200) | 0x100;
        un.visibility = 0x8000_0000;
        let sub = un.subunit;
        if sub != NIL {
            let ss = self.units[sub as usize].sprite;
            self.sprite_start_cloaking(d, ss);
            let su = &mut self.units[sub as usize];
            su.status = (su.status & !0x200) | 0x100;
            su.visibility = 0x8000_0000;
        }
        self.refresh_detection(d, u);
        self.units[u as usize].ai_action_flag = 0x1e;
    }

    /// 0x49b440: stop cloaking: images fade back in.
    pub(super) fn end_cloak(&mut self, d: &Data, u: u32) {
        // 0x48ed50: the decloak sound (UI).
        if self.units[u as usize].status & 0x10 != 0 {
            let s = self.units[u as usize].sprite;
            self.sprite_uncloak_images(s);
            self.sprite_unhide_cloak_images(d, s);
            self.units[u as usize].status &= !0x300;
            return;
        }
        let s = self.units[u as usize].sprite;
        self.sprite_start_decloaking(d, s);
        let un = &mut self.units[u as usize];
        un.status = (un.status & !0x100) | 0x200;
        let sub = un.subunit;
        if sub != NIL {
            let ss = self.units[sub as usize].sprite;
            self.sprite_start_decloaking(d, ss);
            let su = &mut self.units[sub as usize];
            su.status = (su.status & !0x100) | 0x200;
        }
    }

    /// 0x49b2b0: a burrowed unit cloaks at once (draw function 3, no fade): it joins
    /// the cloak list and keeps the Cloak secondary order.
    pub(super) fn burrowed_cloak(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].status & 0x100 != 0 {
            return;
        }
        self.secondary_cloak(u);
        let s = self.units[u as usize].sprite;
        self.sprite_cloak_underground(d, s);
        let un = &mut self.units[u as usize];
        un.status |= 0x300;
        if un.secondary_order != 0x6d {
            un.secondary_order = 0x6d;
            un.secondary_order_pos = (0, 0);
            un.current_build_unit = NIL;
            un.secondary_order_state = 0;
        }
        un.visibility = 0x8000_0000;
    }

    /// 0x497fd0: images not drawn while cloaked hide; the rest take the cloaked draw
    /// function of `mode` (1 cloaking, 2 decloaking, 3 cloaked) with colouring
    /// `coloring` (+0x30 / +0x31; mode 3 keeps theirs).
    pub(super) fn sprite_cloak_mode(&mut self, d: &Data, s: u32, mode: u8, coloring: u16) {
        if mode == 3 {
            self.sprite_cloak_underground(d, s);
            return;
        }
        self.sprite_hide_cloak_images(d, s);
        if mode != 1 && mode != 2 {
            return;
        }
        let m = if mode == 1 { 2 } else { 4 };
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let im = &mut self.images[i as usize];
            if im.modifier == 0 {
                im.modifier = m;
                im.flags |= iflag::REDRAW;
                im.coloring = (im.coloring & !0xffff) | coloring as u64;
            }
            i = im.link.next;
        }
    }

    /// 0x497fd0(3, 0): images not drawn while cloaked hide, the rest take draw
    /// function 3 (cloaked, no fade).
    fn sprite_cloak_underground(&mut self, d: &Data, s: u32) {
        self.sprite_hide_cloak_images(d, s);
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let im = &mut self.images[i as usize];
            if matches!(im.modifier, 0 | 2 | 4) {
                im.modifier = 3;
                im.flags |= iflag::REDRAW;
                im.coloring &= !0xffff;
            }
            i = im.link.next;
        }
    }

    /// 0x49b1e0: a spider mine that burrowed cloaks at once and keeps the Cloak
    /// secondary order.
    pub(super) fn mine_cloak(&mut self, d: &Data, u: u32) {
        self.secondary_cloak(u);
        // 0x68c1b0 etc.: UI refresh.
        self.units[u as usize].ai_action_flag = 0;
        let s = self.units[u as usize].sprite;
        self.sprite_cloak_underground(d, s);
        let un = &mut self.units[u as usize];
        un.status |= 0x300;
        un.visibility = 0x8000_0000;
        if un.secondary_order != 0x6d {
            un.secondary_order = 0x6d;
            un.secondary_order_pos = (0, 0);
            un.current_build_unit = NIL;
            un.secondary_order_state = 0;
        }
    }

    /// 0x497e80: the sprite's cloaked images (draw functions 2..=7) draw normally again.
    fn sprite_uncloak_images(&mut self, s: u32) {
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let im = &mut self.images[i as usize];
            if (2..8).contains(&im.modifier) {
                im.modifier = 0;
                im.flags |= iflag::REDRAW;
            }
            i = im.link.next;
        }
    }

    /// 0x4976c0: images not drawn while cloaked hide.
    fn sprite_hide_cloak_images(&mut self, d: &Data, s: u32) {
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let im = &mut self.images[i as usize];
            if d.dat.images.draw_if_cloaked[im.image_id as usize] == 0 && im.flags & iflag::HIDDEN == 0 {
                // 0x42d280: screen redraw (UI).
                im.flags |= iflag::HIDDEN;
            }
            i = im.link.next;
        }
    }

    /// 0x4980f0: the sprite's images start the cloaking fade (draw functions 0/4/7 ->
    /// 2); images not drawn while cloaked hide (0x4976c0).
    fn sprite_start_cloaking(&mut self, d: &Data, s: u32) {
        self.sprite_hide_cloak_images(d, s);
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let im = &mut self.images[i as usize];
            if matches!(im.modifier, 0 | 4 | 7) {
                im.modifier = 2;
                im.flags |= iflag::REDRAW;
                im.coloring = (im.coloring & !0xffff) | 3 << 8;
            }
            i = im.link.next;
        }
    }

    /// 0x498a90: the sprite's images start fading back in (2/3 -> 4, 5/6 -> 7), and
    /// hidden-while-cloaked images show again (0x498170).
    fn sprite_start_decloaking(&mut self, d: &Data, s: u32) {
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let im = &mut self.images[i as usize];
            if matches!(im.modifier, 2 | 3 | 5 | 6) {
                im.modifier = if im.modifier < 4 { 4 } else { 7 };
                im.flags |= iflag::REDRAW;
                im.coloring = (im.coloring & !0xffff) | 8 | 3 << 8;
            }
            i = im.link.next;
        }
        self.sprite_unhide_cloak_images(d, s);
    }

    /// 0x498170: images hidden while cloaked show again.
    fn sprite_unhide_cloak_images(&mut self, d: &Data, s: u32) {
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let im = &mut self.images[i as usize];
            if d.dat.images.draw_if_cloaked[im.image_id as usize] == 0 && im.flags & iflag::HIDDEN != 0 {
                im.flags = (im.flags & !iflag::HIDDEN) | iflag::REDRAW;
            }
            i = im.link.next;
        }
    }

    /// 0x443390: recompute which players detect cloaked `u`; players that lose it
    /// forget it, and its images switch between the detected and undetected looks.
    pub fn refresh_detection(&mut self, d: &Data, u: u32) {
        let m = self.detected_by(d, u);
        let old = self.units[u as usize].visibility;
        if old == m {
            return;
        }
        let old = if old == 0x8000_0000 { !m & 0xff } else { old };
        let changed = old ^ m;
        self.units[u as usize].visibility = m;
        let lost = !m & changed & 0xfff;
        for p in 0..12 {
            if lost & (1 << p) == 0 {
                continue;
            }
            // 0x49e8a0: the units of players that lost sight of it forget it.
            let mut o = self.player_units[p];
            while o != NIL {
                let next = self.units[o as usize].player_next;
                self.forget_unit_pub(d, o, u);
                o = next;
            }
        }
        let lm = self.local_vision_mask as u32;
        if changed & lm != 0 {
            let s = self.units[u as usize].sprite;
            let sub = self.units[u as usize].subunit;
            if m & lm == 0 {
                self.unit_undetected_locally(d, u);
            } else {
                self.sprite_detected(s);
                if sub != NIL {
                    let ss = self.units[sub as usize].sprite;
                    self.sprite_detected(ss);
                }
            }
        }
    }

    /// 0x443350: the local player stops detecting `u`: deselect (0x4c3b40) and drop
    /// its selection circle (0x4975d0) (UI), then the undetected look for it and its
    /// subunit.
    fn unit_undetected_locally(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite;
        self.sprite_undetected(d, s);
        let sub = self.units[u as usize].subunit;
        if sub != NIL {
            let ss = self.units[sub as usize].sprite;
            self.sprite_undetected(d, ss);
        }
    }

    /// 0x497ed0: images take the detected look (2..4 -> 5..7, 0 -> 6).
    pub(super) fn sprite_detected(&mut self, s: u32) {
        if self.sprites[s as usize].flags & 0x40 != 0 {
            self.sprite_uncloak_images(s);
            return;
        }
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let im = &mut self.images[i as usize];
            match im.modifier {
                0 => {
                    im.modifier = 6;
                    im.flags |= iflag::REDRAW;
                }
                2..=4 => {
                    im.modifier += 3;
                    im.flags |= iflag::REDRAW;
                }
                _ => {}
            }
            i = im.link.next;
        }
    }

    /// 0x498a10: images lose the detected look (5..7 -> 2..4).
    fn sprite_undetected(&mut self, d: &Data, s: u32) {
        if self.sprites[s as usize].flags & 0x40 != 0 {
            self.sprite_cloak_underground(d, s);
            return;
        }
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let im = &mut self.images[i as usize];
            if (5..8).contains(&im.modifier) {
                im.modifier -= 3;
                im.flags |= iflag::REDRAW;
            }
            i = im.link.next;
        }
    }

    /// 0x441130: players that see cloaked `u`: its owner, players sharing the owner's
    /// vision, parasite owners, and owners of detectors within range (0x4408a0).
    fn detected_by(&mut self, d: &Data, u: u32) -> u32 {
        // 0x4f3a80: carrying a flag.
        if self.units[u as usize].under_spell() || self.carries_flag(d, u) {
            return 0xff;
        }
        let (x, y) = self.unit_sprite_pos(u);
        let b = [x.wrapping_sub(0x160), y.wrapping_sub(0x160), x.wrapping_add(0x160), y.wrapping_add(0x160)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let near = search.results(start);
        search.pop();
        self.search = search;
        let mut m = 0u32;
        for c in near.into_iter().chain(super::list::iter(&self.units, self.scanner_units.head).collect::<Vec<_>>()) {
            m |= self.detector_mask(d, c, u);
        }
        let un = &self.units[u as usize];
        let pf = un.parasite_flags as u32;
        m |= 1 << (un.player & 0x1f) | self.player_vision[un.player as usize] | pf;
        for p in 0..12 {
            if pf & (1 << p) != 0 {
                m |= self.player_vision[p];
            }
        }
        m
    }

    /// 0x4408a0: the players detector `c` reveals `u` to (0 when it doesn't).
    fn detector_mask(&self, d: &Data, c: u32, u: u32) -> u32 {
        let cu = &self.units[c as usize];
        if cu.status & 0x4000_0000 != 0 || c == u {
            return 0;
        }
        // 0x403430: a working detector.
        let det = d.dat.units.flags[cu.unit_type as usize] & 0x8000 != 0
            && cu.status & 1 != 0
            && cu.status & 0x400 == 0
            && cu.lockdown_timer == 0
            && cu.stasis_timer == 0
            && cu.maelstrom_timer == 0
            && cu.is_blind == 0;
        if !det || self.sprites[self.units[u as usize].sprite as usize].visibility & (1 << (cu.player & 0x1f)) == 0 {
            return 0;
        }
        let r = if cu.status & 2 == 0 { (self.unit_sight_range(d, c, false) as u32) << 5 } else { 0xe0 };
        if !self.box_distance_within(d, c, r, u) {
            return 0;
        }
        cu.parasite_flags as u32 | self.player_vision[cu.player as usize] | 1 << (cu.player & 0x1f)
    }

    /// 0x4f3a80: a worker carrying a flag, or a transport carrying one.
    fn carries_flag(&self, d: &Data, u: u32) -> bool {
        let un = &self.units[u as usize];
        let t = un.unit_type as usize;
        if d.dat.units.flags[t] & 8 != 0 {
            let p = un.ptr_at(0xc0);
            return p != NIL && self.units[p as usize].unit_type == 0xd7;
        }
        if un.status & 0x4000_0000 != 0 || d.dat.units.space_provided[t] == 0 {
            return false;
        }
        if un.unit_type == 0x2a && self.players.upgrade_sc[un.player as usize][0x18] == 0 {
            return false;
        }
        self.passenger_carries_flag(d, u)
    }

    /// 0x491c00: energy cap (a dark archon still warping in has 50).
    fn energy_cap(&self, d: &Data, u: u32) -> u16 {
        let un = &self.units[u as usize];
        if un.unit_type == 0x3f && un.order == 0x6a && un.order_state == 0 {
            0x3200
        } else {
            self.max_energy(d, u) as u16
        }
    }

    /// 0x4eb4b0: spellcasters regain 8/256 energy a frame; cloaked ones pay the cloak
    /// instead and stop cloaking when they run out.
    pub fn regenerate_energy(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        let t = un.unit_type;
        let st = un.status;
        if d.dat.units.flags[t as usize] & 0x20_0000 == 0 || st & 0x4000_0000 != 0 || st & 1 == 0 {
            return;
        }
        if st & 0x300 == 0 || st & 0x800 != 0 {
            let cap = self.energy_cap(d, u);
            if self.units[u as usize].energy == cap {
                return;
            }
            let e = self.units[u as usize].energy.wrapping_add(8);
            self.units[u as usize].energy = e;
            if self.energy_cap(d, u) < e {
                self.units[u as usize].energy = self.energy_cap(d, u);
            }
        } else {
            let cost = cloak_energy_cost(t);
            if self.energy_short(u, cost) {
                if self.units[u as usize].secondary_order == 0x6d {
                    self.set_secondary_order(u, 0x17);
                }
                return;
            }
            self.spend_energy(u, cost);
        }
        // 0x497430: selected units redraw their energy bar (UI).
    }

    /// 0x4eb2f0: the "under disruption web" status (0x8000) is recomputed each frame.
    pub fn web_pass(&mut self, d: &Data) {
        let mut u = self.visible_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            let st = self.units[u as usize].status;
            if st & 4 == 0 || st & 0x8000 != 0 {
                self.units[u as usize].status = st & !0x8000;
                let un = &self.units[u as usize];
                let t = un.unit_type;
                let transport = st & 0x4000_0000 == 0
                    && (t != 0x2a || self.players.upgrade_sc[un.player as usize][0x18] != 0)
                    && d.dat.units.space_provided[t as usize] != 0;
                if transport {
                    for i in 0..8 {
                        if let Some(c) = self.loaded_unit(u, i) {
                            self.units[c as usize].status &= !0x8000;
                        }
                    }
                } else if un.subunit != NIL {
                    let sub = un.subunit;
                    self.units[sub as usize].status &= !0x8000;
                }
            }
            u = next;
        }
        // 0x5861c0: completed disruption webs (unit 0x69) of player 11.
        if self.players.completed_counts[0x69][11] == 0 {
            return;
        }
        let mut n = self.player_units[11];
        while n != NIL {
            if self.units[n as usize].unit_type == 0x69 {
                let b = self.unit_box(d, n);
                let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
                let start = search.units_in_finder_box(self, b);
                let found = search.results(start);
                search.pop();
                self.search = search;
                for c in found {
                    self.web_mark(d, c);
                }
            }
            n = self.units[n as usize].player_next;
        }
    }

    /// 0x4eb170: a ground unit under a web can't attack (0x8000), with its passengers
    /// or turret.
    fn web_mark(&mut self, d: &Data, u: u32) {
        let st = self.units[u as usize].status;
        if st & 4 != 0 {
            return;
        }
        self.units[u as usize].status = st | 0x8000;
        let un = &self.units[u as usize];
        let t = un.unit_type;
        let transport = st & 0x4000_0000 == 0
            && (t != 0x2a || self.players.upgrade_sc[un.player as usize][0x18] != 0)
            && d.dat.units.space_provided[t as usize] != 0;
        if transport {
            for i in 0..8 {
                let h = self.units[u as usize].loaded_units[i] as u32;
                if h == 0 {
                    continue;
                }
                let c = (h & 0x7ff) - 1;
                let cu = &self.units[c as usize];
                if cu.sprite != NIL && (cu.order != 0 || cu.order_state != 1) && cu.unique_id as u32 == h >> 11 {
                    self.units[c as usize].status |= 0x8000;
                }
            }
        } else if un.subunit != NIL {
            let sub = un.subunit;
            self.units[sub as usize].status |= 0x8000;
        }
    }
}

/// 0x491a90: energy a cloaked unit of type `t` pays each frame (0 when not a cloaker).
pub(super) fn cloak_energy_cost(t: u16) -> u16 {
    match t {
        1 | 0x10 | 100 | 99 | 0x68 | 0x33 => 10,
        8 | 0x15 => 0xd,
        _ => 0,
    }
}
