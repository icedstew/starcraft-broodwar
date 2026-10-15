//! Spellcasting: the shared cast order (0x492850) for the targeted spells, its target
//! and range checks, and iscript `castspell` (0x4d74c0 op 0x27).

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

impl State {
    /// 0x401210: `u` is a real (not hallucinated) spellcaster.
    pub(super) fn is_spellcaster(&self, d: &Data, u: u32) -> bool {
        let un = &self.units[u as usize];
        d.dat.units.flags[un.unit_type as usize] & 0x20_0000 != 0 && un.status & 0x4000_0000 == 0
    }

    /// 0x491e80: the stat_txt error for casting tech `tech` on `t` by `player`'s unit
    /// (0 = fine).
    pub(super) fn tech_target_error(&self, d: &Data, t: u32, tech: u8, player: u8) -> u16 {
        let tu = &self.units[t as usize];
        if tu.stasis_timer != 0 {
            return 0x384;
        }
        let building = d.dat.units.flags[tu.unit_type as usize] & 1 != 0;
        match tech {
            // Defensive Matrix, Irradiate, Restoration, Optical Flare.
            6 | 7 | 0x18 | 0x1e => {
                if building { 0x36d } else { 0 }
            }
            // Hallucination: not on interceptors.
            0x14 => {
                if tu.unit_type == 0x49 {
                    0x36c
                } else if building {
                    0x36d
                } else {
                    0
                }
            }
            // Spawn Broodlings (0x402210).
            0xc => {
                if self.damaged_6a(d, t) { 0 } else { 0x366 }
            }
            // Consume: own Zerg units, not buildings or larvae.
            0x10 => {
                if building || tu.player != player || self.race_of(d, t) != 0 || tu.unit_type == 0x23 {
                    0x381
                } else {
                    0
                }
            }
            // Mind Control.
            0x1b => {
                if player == tu.player {
                    0x52f
                } else if building {
                    0x36d
                } else if matches!(tu.unit_type, 0xd | 0x23 | 0x24 | 0x3b | 0x49 | 0x55 | 0x61) {
                    0x36c
                } else {
                    0
                }
            }
            // Feedback.
            0x1d => {
                if building {
                    0x36d
                } else if self.is_spellcaster(d, t) {
                    0
                } else {
                    0x532
                }
            }
            _ => 0x36c,
        }
    }

    /// 0x492140: the current spell order's target is no longer valid.
    fn spell_target_invalid(&self, d: &Data, u: u32) -> bool {
        let un = &self.units[u as usize];
        let order = un.order;
        let (x, y) = (un.order_target_pos.0 as i32, un.order_target_pos.1 as i32);
        let t = un.order_target_unit;
        let w = d.dat.orders.weapon[order as usize];
        if un.ai != NIL {
            let tu = (t != NIL).then(|| &self.units[t as usize]);
            match order {
                0x73 | 0x93 => {
                    if t == NIL || self.disabled(t) {
                        return true;
                    }
                }
                0x78 => {
                    if tu.map_or(true, |tu| tu.parasite_flags != 0) {
                        return true;
                    }
                }
                0x7a => {
                    if let Some(tu) = tu {
                        if !self.is_spellcaster(d, t) || (tu.energy >> 8) < 100 {
                            if d.dat.units.shield_enable[tu.unit_type as usize] == 0 {
                                return true;
                            }
                            if (tu.shields & !0xff) < 0x6400 {
                                return true;
                            }
                        }
                    }
                }
                0x8f => {
                    if tu.map_or(true, |tu| tu.irradiate_timer != 0) {
                        return true;
                    }
                }
                0x90 => {
                    if tu.map_or(true, |tu| tu.plague_timer != 0) {
                        return true;
                    }
                }
                0x92 => {
                    if tu.is_some_and(|tu| tu.ensnare_timer != 0) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        if d.dat.orders.use_weapon_targeting[order as usize] != 0 {
            return !self.weapon_can_target(d, w, t, u);
        }
        let tech = d.dat.orders.tech[order as usize];
        if tech > 0x2b {
            return false;
        }
        match tech {
            2 | 4 | 0xe | 0xf | 0x11 | 0x13 | 0x15 | 0x16 | 0x19 | 0x1f => false,
            // Spider mines (0x472f80): the target tile must be ground or creep.
            3 => {
                let i = (y / 32) * self.terrain.width as i32 + x / 32;
                self.terrain.tile_flags[i as usize] & 0x41_0000 == 0
            }
            _ => {
                t == NIL
                    || self.units[t as usize].status & 0x400_0000 != 0
                    || self.tech_target_error(d, t, tech, un.player) != 0
            }
        }
    }

    /// 0x491db0: the spell target is visible and within the spell's range (plus the
    /// stopping distance while moving).
    fn spell_in_range(&self, d: &Data, u: u32) -> bool {
        if !self.order_target_visible(u) {
            return false;
        }
        let un = &self.units[u as usize];
        let w = d.dat.orders.weapon[un.order as usize];
        let range = self.weapon_range(d, u, w);
        let t = un.order_target_unit;
        if t != NIL {
            return self.in_attack_range(d, u, range, t);
        }
        let (x, y) = (un.order_target_pos.0 as i32, un.order_target_pos.1 as i32);
        let range = range.wrapping_add((super::flingy::halt_distance(&un.fl, d) >> 8) as u32);
        let dist = super::path::approx_distance(un.fl.exact.0, x << 8, un.fl.exact.1, y << 8);
        dist >> 8 <= range
    }

    /// 0x492620: approach the spell target; true when in range (then stop).
    fn spell_approach(&mut self, d: &Data, u: u32, first: bool) -> bool {
        let t = self.units[u as usize].order_target_unit;
        if t != NIL {
            self.units[u as usize].order_target_pos = self.unit_sprite_pos(t);
        }
        let in_range = self.spell_in_range(d, u);
        if !in_range {
            if t == NIL {
                if self.units[u as usize].recent_order_timer == 0 {
                    let p = self.units[u as usize].order_target_pos;
                    self.set_move_target_xy(d, u, p);
                }
            } else if !first {
                self.follow_move(d, u, t);
            } else {
                self.move_to_unit(d, u, t);
            }
        } else if self.units[u as usize].fl.flags & 4 == 0 {
            self.unit_halt(d, u);
        }
        let un = &mut self.units[u as usize];
        if un.order_target_pos != un.fl.next_target_waypoint {
            un.fl.next_target_waypoint = un.order_target_pos;
        }
        in_range
    }

    /// 0x492850: FireYamatoGun, CastLockdown, CastDarkSwarm, CastParasite,
    /// CastSpawnBroodlings, CastEMPShockwave, CastPsionicStorm, CastIrradiate, CastPlague,
    /// CastConsume, CastEnsnare, CastStasisField, CastRestoration, CastDisruptionWeb,
    /// CastOpticalFlare, CastMaelstrom.
    pub(super) fn order_cast_spell(&mut self, d: &Data, u: u32) {
        let order = self.units[u as usize].order;
        let w = d.dat.orders.weapon[order as usize];
        if self.units[u as usize].order_state == 2 {
            if self.units[u as usize].order_signal & 2 != 0 {
                self.units[u as usize].order_signal &= !2;
                self.order_done_or_next(d, u);
                return;
            }
            let t = self.units[u as usize].order_target_unit;
            if t == NIL {
                return;
            }
            let p = self.unit_sprite_pos(t);
            let un = &mut self.units[u as usize];
            un.order_target_pos = p;
            if un.fl.next_target_waypoint != p {
                un.fl.next_target_waypoint = p;
            }
            return;
        }
        if self.spell_target_invalid(d, u) {
            // 0x48ef30: the error message and sound for the local player (UI).
            self.unit_halt(d, u);
            self.order_done_or_next(d, u);
            return;
        }
        let tech = d.dat.orders.tech[order as usize];
        let cost = if tech < 0x2c { (d.dat.techdata.energy_cost[tech as usize] as u8 as u16) << 8 } else { 0 };
        let cheat = self.game_options & 0x800 != 0;
        if !cheat && self.units[u as usize].energy < cost {
            // 0x48ccb0: "Not enough energy" for the local player (UI).
            self.order_done_or_next(d, u);
            return;
        }
        let first = self.units[u as usize].order_state == 0;
        let in_range = self.spell_approach(d, u, first);
        if first {
            self.units[u as usize].order_state = 1;
        }
        if !in_range {
            if self.unit_at_move_target(u) == 2 {
                self.order_done_or_next(d, u);
            }
            return;
        }
        if self.units[u as usize].spell_cooldown != 0 || !self.weapon_ready(d, u, w) {
            return;
        }
        if self.units[u as usize].fl.flags & 2 != 0 || self.unit_at_move_target(u) == 0 {
            return;
        }
        let t = self.units[u as usize].order_target_unit;
        if self.units[u as usize].ai != NIL && t != NIL {
            // AI casters skip targets already under the effect.
            let tu = &self.units[t as usize];
            let mut busy = tu.irradiate_timer as u32;
            let skip = match w {
                0x20 | 0x6c => {
                    busy |= self.disabled(t) as u32;
                    false
                }
                0x22 => false,
                0x38 => {
                    busy |= tu.parasite_flags as u32;
                    false
                }
                0x3a => {
                    busy |= tu.ensnare_timer as u32;
                    false
                }
                0x3c => {
                    busy |= tu.plague_timer as u32;
                    false
                }
                0x6b => {
                    busy |= tu.is_blind as u32;
                    false
                }
                _ => true,
            };
            if !skip && busy != 0 {
                self.order_done_or_next(d, u);
                return;
            }
        }
        if !cheat {
            self.units[u as usize].energy -= cost;
        }
        let cd = self.weapon_cooldown(d, u, w);
        let r = self.rng.rand(0x31) as u8;
        let un = &mut self.units[u as usize];
        un.spell_cooldown = cd.wrapping_add((r & 3).wrapping_sub(1));
        un.order_signal &= !2;
        un.fl.flags |= 8;
        let s = un.sprite;
        let anim = d.dat.orders.animation[order as usize];
        self.sprite_images_run_anim(d, s, anim);
        self.units[u as usize].order_state = 2;
        if self.units[u as usize].ai != NIL {
            todo_port!("0x492850", "AI: player AI cast flag (0x690115)");
        }
    }

    /// 0x4d74c0 op 0x27 `castspell`: fire the order's weapon when its target is valid
    /// (0x479d40: twice for two-bullet weapons).
    pub(super) fn iscript_cast_spell(&mut self, d: &Data, u: u32) {
        let w = d.dat.orders.weapon[self.units[u as usize].order as usize];
        if w >= 0x82 || self.spell_target_invalid(d, u) {
            return;
        }
        self.fire_projectile(d, u, w);
        if d.dat.weapons.damage_factor[w as usize] == 2 {
            self.fire_projectile(d, u, w);
        }
    }
}

impl State {
    /// Overlay size of `t` for spell overlays: 0 small, 1 medium, 2 large (units.dat
    /// flags 0x2000000 / 0x4000000).
    fn overlay_size(&self, d: &Data, t: u32) -> u16 {
        let f = d.dat.units.flags[self.units[t as usize].unit_type as usize];
        if f & 0x200_0000 != 0 { 1 } else { ((f >> 25) & 2) as u16 }
    }

    /// 0x498ea0: image `id` on top of sprite `s` (drawn above all its images).
    fn add_image_top(&mut self, d: &Data, s: u32, id: u16) {
        let head = self.sprites[s as usize].images.head;
        self.create_image(d, s, head, id, 0, 0, 0);
    }

    /// Spell overlay `base + size` on top of `t` (its turret when it has one).
    fn add_spell_overlay(&mut self, d: &Data, t: u32, base: u16) {
        let size = self.overlay_size(d, t);
        let sub = self.units[t as usize].subunit;
        let host = if sub != NIL { sub } else { t };
        let s = self.units[host as usize].sprite;
        self.add_image_top(d, s, base + size);
    }

    /// 0x454e00: irradiate `t` (by `src` of `player`).
    pub(super) fn apply_irradiate(&mut self, d: &Data, t: u32, src: u32, player: u8) {
        let tu = &self.units[t as usize];
        if tu.irradiate_timer == 0 && tu.status & 0x10 == 0 {
            self.add_spell_overlay(d, t, 0x17c);
        }
        let tu = &mut self.units[t as usize];
        tu.irradiate_timer = d.dat.weapons.cooldown[0x22];
        tu.irradiated_by = src;
        tu.irradiate_player = player;
    }

    /// 0x455530: irradiate damage from `src` (irradiated) to `c`: organic non-buildings
    /// (not larvae, eggs or lurker eggs) within 0x20, or `src` itself when burrowed.
    fn irradiate_hit(&mut self, d: &Data, c: u32, src: u32) {
        let ct = self.units[c as usize].unit_type;
        let f = d.dat.units.flags[ct as usize];
        if f & 0x1_0000 == 0 || f & 1 != 0 || matches!(ct, 0x23 | 0x24 | 0x61) {
            return;
        }
        if self.units[c as usize].status & 0x10 != 0 && src != c {
            return;
        }
        if self.units[src as usize].status & 0x40 == 0 && !self.box_distance_within(d, src, 0x20, c) {
            return;
        }
        let dmg = ((d.dat.weapons.damage[0x22] as i32) << 8) / d.dat.weapons.cooldown[0x22] as i32;
        let (by, player) = (self.units[src as usize].irradiated_by, self.units[src as usize].irradiate_player);
        self.damage_unit(d, c, dmg as u32, 0x22, 1, 0, by, player);
    }

    /// 0x4555c0: one frame of irradiate damage around `u`.
    pub(super) fn irradiate_tick(&mut self, d: &Data, u: u32) {
        let st = self.units[u as usize].status;
        if st & 0x10 != 0 {
            self.irradiate_hit(d, u, u);
            return;
        }
        if st & 0x40 == 0 {
            let (x, y) = self.unit_sprite_pos(u);
            let b = [x - 0xa0, y - 0xa0, x + 0xa0, y + 0xa0];
            let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
            let start = search.units_in_finder_box(self, b);
            let found = search.results(start);
            search.pop();
            self.search = search;
            for c in found {
                self.irradiate_hit(d, c, u);
            }
            return;
        }
        // In a transport: the other passengers (0x4e6d00).
        let tr = self.units[u as usize].connected_unit;
        if tr == NIL {
            return;
        }
        let n = d.dat.units.space_provided[self.units[tr as usize].unit_type as usize] as usize;
        for i in 0..n.min(8) {
            let h = self.units[tr as usize].loaded_units[i] as u32;
            if h == 0 {
                continue;
            }
            let c = (h & 0x7ff) - 1;
            let cu = &self.units[c as usize];
            if cu.sprite != NIL && (cu.order != 0 || cu.order_state != 1) && cu.unique_id as u32 == h >> 11 {
                self.irradiate_hit(d, c, u);
            }
        }
    }

    /// 0x454f90: defensive matrix on `t`.
    fn apply_defense_matrix(&mut self, d: &Data, t: u32) {
        let tu = &self.units[t as usize];
        if tu.defense_matrix_timer == 0 && tu.status & 0x10 == 0 {
            self.add_spell_overlay(d, t, 0x173);
            let size = self.overlay_size(d, t);
            let s = self.units[t as usize].sprite;
            self.add_image_below_main(d, s, 0x176 + size);
        }
        let tu = &mut self.units[t as usize];
        tu.defense_matrix_damage = 64000;
        tu.defense_matrix_timer = 0xa8;
    }

    /// 0x454ed0: the matrix on `t` absorbs `dmg`; it breaks when that is all it had left.
    pub(super) fn defense_matrix_hit(&mut self, d: &Data, t: u32, dmg: i32) {
        let hp = self.units[t as usize].defense_matrix_damage;
        if dmg < hp as i32 {
            self.units[t as usize].defense_matrix_damage = hp - dmg as u16;
        } else {
            let tu = &mut self.units[t as usize];
            tu.defense_matrix_damage = 0;
            tu.defense_matrix_timer = 0;
            self.remove_overlay_in(t, 0x173, 0x175);
            let s = self.units[t as usize].sprite;
            let mut i = self.sprites[s as usize].images.head;
            while i != NIL {
                if (0x176..=0x178).contains(&self.images[i as usize].image_id) {
                    self.destroy_image(i);
                    break;
                }
                i = self.images[i as usize].link.next;
            }
        }
        let tu = &self.units[t as usize];
        if tu.defense_matrix_timer != 0 && tu.status & 0x10 == 0 {
            self.add_spell_overlay(d, t, 0x179);
        }
    }

    /// 0x4f4550: plague `t`.
    fn apply_plague(&mut self, d: &Data, t: u32) {
        let tu = &self.units[t as usize];
        if tu.plague_timer == 0 && tu.status & 0x10 == 0 {
            self.add_spell_overlay(d, t, 0x184);
        }
        self.units[t as usize].plague_timer = 0x4b;
    }

    /// 0x4f4b30: plague everything but the caster within 0x40 of (x, y) (callback 0x4f4af0).
    fn plague_area(&mut self, d: &Data, x: i16, y: i16, src: u32) {
        let b = [x - 0x40, y - 0x40, x + 0x40, y + 0x40];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        for c in found {
            if c == src || self.units[c as usize].status & 0x400_0010 != 0 {
                continue;
            }
            self.apply_plague(d, c);
            if src != NIL {
                self.attacked_by(d, c, src, true);
            }
        }
    }

    /// 0x4f48a0: a dark swarm (neutral unit 0xca) at (x, y) for 900 frames.
    fn create_dark_swarm(&mut self, d: &Data, x: i16, y: i16, player: u8) {
        let mut p = (x, y);
        self.clamp_unit_pos(d, 0xca, &mut p);
        let Some(s) = self.create_unit(d, 0xca, p.0, p.1, 0xb) else {
            self.show_last_error(player);
            return;
        };
        self.units[s as usize].status |= 0x20_0000;
        let sp = self.units[s as usize].sprite;
        self.sprites[sp as usize].elevation = 0xb;
        self.finish_unit_pre(d, s);
        self.finish_unit_post(d, s);
        let un = &mut self.units[s as usize];
        if un.remove_timer == 0 || un.remove_timer > 900 {
            un.remove_timer = 900;
        }
    }

    /// 0x4f47b0: `src` consumes `t` for 50 energy (none from hallucinations).
    fn consume(&mut self, d: &Data, t: u32, src: u32) {
        if self.units[t as usize].status & 0x400_0000 != 0 {
            return;
        }
        let player = self.units[src as usize].player;
        if self.tech_target_error(d, t, 0x10, player) != 0 {
            return;
        }
        self.count_death(d, t, player);
        self.kill_unit(d, t);
        if self.units[t as usize].status & 0x4000_0000 == 0 {
            let e = (self.units[src as usize].energy as u32 + 0x3200).min(self.max_energy(d, src));
            self.units[src as usize].energy = e as u16;
        }
    }

    /// The spell parts of 0x48b770 (iscript `domissiledmg`) for bullet `b`.
    pub(super) fn bullet_spell_effect(&mut self, d: &Data, b: u32, et: u8) {
        let bu = &self.bullets.pool[b as usize];
        let (t, src, player, pos) = (bu.target, bu.source, bu.player, bu.target_pos);
        let alive = |st: &State, t: u32| t != NIL && st.units[t as usize].sprite != NIL && st.units[t as usize].order != 0;
        match et {
            // Irradiate.
            9 => {
                if alive(self, t) {
                    // 0x48ed50: the irradiate sound.
                    self.apply_irradiate(d, t, src, player);
                }
            }
            // Plague.
            0xb => self.plague_area(d, pos.0, pos.1, src),
            // Dark Swarm.
            0xd => self.create_dark_swarm(d, pos.0, pos.1, player),
            // Consume.
            0xe => {
                if src != NIL && alive(self, t) {
                    self.consume(d, t, src);
                }
            }
            // Lockdown.
            4 => {
                if alive(self, t) {
                    self.apply_disable(d, t, 0, 0x83);
                }
            }
            // EMP Shockwave.
            8 => self.emp(d, b),
            // Restoration.
            0x10 => {
                if alive(self, t) {
                    // 0x48ed50: the restoration sound.
                    self.restoration(d, t);
                }
            }
            // Optical Flare.
            0x15 => {
                if alive(self, t) {
                    self.optical_flare(d, t, player);
                }
            }
            // Parasite.
            6 => {
                if alive(self, t) {
                    self.parasite(t, src, player);
                }
            }
            // Spawn Broodlings.
            7 => {
                if src != NIL && alive(self, t) {
                    if self.player_type(self.units[t as usize].player) == 1 {
                        todo_port!("0x43f320", "AI reacts to broodlings");
                    }
                    self.spawn_broodlings(d, src, t);
                }
            }
            // Ensnare.
            0xa => {
                let (x, y) = self.bullet_sprite_pos(b);
                self.ensnare_area(d, x, y, src);
            }
            // Stasis Field.
            0xc => {
                if pos != (0, 0) {
                    self.stasis_area(d, pos.0, pos.1, src);
                }
            }
            // Disruption Web.
            0x11 => self.create_disruption_web(d, pos.0, pos.1, player),
            // Corrosive Acid: the hit, then spores around it (not from hallucinations).
            0x12 => {
                let flags = self.bullets.pool[b as usize].flags;
                if t != NIL && flags & 1 == 0 {
                    self.weapon_hit(d, b, t, 1);
                    if self.bullets.pool[b as usize].flags & 2 == 0 {
                        let (x, y) = self.bullet_sprite_pos(b);
                        self.acid_spore_area(d, x, y, player);
                    }
                }
            }
            // Maelstrom.
            0x16 => {
                if pos != (0, 0) {
                    self.maelstrom_area(d, pos.0, pos.1, src);
                }
            }
            _ => todo_port!("0x48b770", "spell explosion type"),
        }
    }

    /// 0x492020 for a unit target (type 0xe4): tech `tech` may be cast by `u` on `t`;
    /// Err holds the message.
    fn tech_target_ok(&self, d: &Data, u: u32, tech: u8, t: u32, p: (i16, i16)) -> Result<(), u16> {
        match tech {
            2 | 4 | 0xe | 0xf | 0x11 | 0x13 | 0x15 | 0x16 | 0x19 | 0x1f => Ok(()),
            3 => {
                let i = (p.1 as i32 / 32) * self.terrain.width as i32 + p.0 as i32 / 32;
                if self.terrain.tile_flags[i as usize] & 0x41_0000 != 0 { Ok(()) } else { Err(0x386) }
            }
            _ => {
                if t == NIL {
                    return Err(0x36e);
                }
                if self.units[t as usize].status & 0x400_0000 != 0 {
                    return Err(0x36c);
                }
                match self.tech_target_error(d, t, tech, self.units[u as usize].player) {
                    0 => Ok(()),
                    e => Err(e),
                }
            }
        }
    }

    /// Give up the current order: the next queued one, else idle (0x475000 / 0x475310).
    fn spell_give_up(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_queue_head != NIL {
            self.units[u as usize].order_flags |= 1;
            self.prepare_next_order(d, u);
        } else if self.units[u as usize].ai != NIL {
            self.order_now(d, u, 0x9c);
        } else {
            let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
            self.order_now(d, u, o);
        }
    }

    /// 0x492450: approach the target to within `range` and face it; true when ready.
    fn approach_and_face(&mut self, d: &Data, u: u32, range: u32) -> bool {
        let t = self.units[u as usize].order_target_unit;
        if t == NIL {
            self.unit_halt(d, u);
            self.spell_give_up(d, u);
            return false;
        }
        let p = self.unit_sprite_pos(t);
        let un = &mut self.units[u as usize];
        un.order_target_pos = p;
        if un.fl.next_target_waypoint != p {
            un.fl.next_target_waypoint = p;
        }
        if self.order_target_visible(u) && self.in_attack_range(d, u, range, t) {
            self.unit_halt(d, u);
            let f = &self.units[u as usize].fl;
            if f.next_target_waypoint == f.pos {
                return true;
            }
            let s = self.unit_sprite_pos(u);
            let w = f.next_target_waypoint;
            let dir = crate::game::flingy::direction_to(s.0 as i32, s.1 as i32, w.0 as i32, w.1 as i32);
            let mut diff = f.heading as i32 - dir as i32;
            if diff < 0 {
                diff += 0x100;
            }
            if diff > 0x80 {
                diff = 0x100 - diff;
            }
            return diff <= 5;
        }
        let un = &self.units[u as usize];
        if un.fl.move_target == self.unit_sprite_pos(u) && un.status & 0x8_0000 != 0 {
            self.unit_halt(d, u);
            self.order_done_or_next(d, u);
            return false;
        }
        let t = self.units[u as usize].order_target_unit;
        if t == NIL {
            if self.units[u as usize].recent_order_timer == 0 {
                let p = self.units[u as usize].order_target_pos;
                self.set_move_target_xy(d, u, p);
            }
        } else {
            self.follow_move(d, u, t);
        }
        false
    }

    /// 0x4926d0: a direct cast of `tech` on the order target: valid, affordable, and in
    /// `range` facing it. `cost` gets the energy cost.
    fn direct_cast_ready(&mut self, d: &Data, u: u32, tech: u8, range: u32, cost: &mut u16) -> bool {
        let t = self.units[u as usize].order_target_unit;
        if t == NIL || self.tech_target_ok(d, u, tech, t, self.unit_sprite_pos(t)).is_err() {
            // 0x48ef30: the error for the local player (UI).
            self.spell_give_up(d, u);
            return false;
        }
        *cost = (d.dat.techdata.energy_cost[tech as usize] as u8 as u16) << 8;
        if self.game_options & 0x800 == 0 && self.units[u as usize].energy < *cost {
            // 0x48ccb0: "Not enough energy" for the local player (UI).
            self.order_done_or_next(d, u);
            return false;
        }
        self.approach_and_face(d, u, range)
    }

    /// 0x4550a0: CastDefensiveMatrix.
    pub(super) fn order_cast_defensive_matrix(&mut self, d: &Data, u: u32) {
        let range = (self.unit_sight_range(d, u, true) as u32) << 5;
        let mut cost = 0;
        if !self.direct_cast_ready(d, u, 6, range, &mut cost) {
            return;
        }
        if self.game_options & 0x800 == 0 {
            self.units[u as usize].energy -= cost;
        }
        let t = self.units[u as usize].order_target_unit;
        self.apply_defense_matrix(d, t);
        // 0x48ed50: the matrix sound.
        let s = self.units[u as usize].sprite;
        let main = self.sprites[s as usize].main_image;
        self.create_image(d, s, main, 0x21b, 0, 0, 0);
        self.spell_give_up(d, u);
    }
}

impl State {
    /// Image `id` on top of `t`'s turret (or `t`).
    fn add_overlay_image(&mut self, d: &Data, t: u32, id: u16) {
        let sub = self.units[t as usize].subunit;
        let host = if sub != NIL { sub } else { t };
        let s = self.units[host as usize].sprite;
        self.add_image_top(d, s, id);
    }

    /// 0x4e5c90: `u` (or its turret) has an image with id `id`.
    fn has_overlay(&self, u: u32, id: u16) -> bool {
        let has = |x: u32| {
            let mut i = self.sprites[self.units[x as usize].sprite as usize].images.head;
            while i != NIL {
                if self.images[i as usize].image_id == id {
                    return true;
                }
                i = self.images[i as usize].link.next;
            }
            false
        };
        let sub = self.units[u as usize].subunit;
        has(u) || (sub != NIL && has(sub))
    }

    /// 0x4f41f0: the acid spore overlay for `u`'s spore count and size.
    fn acid_overlay_id(&self, d: &Data, u: u32) -> u16 {
        let n = ((self.units[u as usize].acid_spore_count >> 1) as u16).min(3);
        let f = d.dat.units.flags[self.units[u as usize].unit_type as usize];
        if f & 0x200_0000 != 0 { n + 0x3de } else { n + 0x3da + ((f >> 25) & 2) as u16 * 4 }
    }

    /// The overlay half of 0x4f42c0 after the spore timers ran (`before` = the count
    /// before them).
    pub(super) fn acid_overlay_update(&mut self, d: &Data, u: u32, before: u8) {
        if self.units[u as usize].acid_spore_count == 0 {
            if before != 0 {
                self.remove_overlay_in(u, 0x3da, 0x3e5);
            }
            return;
        }
        let id = self.acid_overlay_id(d, u);
        if !self.has_overlay(u, id) {
            self.remove_overlay_in(u, 0x3da, 0x3e5);
            self.add_overlay_image(d, u, id);
        }
    }

    /// 0x4655f0: a disabled carrier calls its launched interceptors back (0x45).
    pub(super) fn recall_interceptors(&mut self, d: &Data, u: u32) {
        let mut f = self.units[u as usize].ptr_at(0xc4);
        while f != NIL {
            self.units[f as usize].order_flags |= 1;
            if self.units[f as usize].order != 0 {
                loop {
                    let tail = self.units[f as usize].order_queue_tail;
                    if tail == NIL {
                        break;
                    }
                    let o = self.orders[tail as usize].order;
                    if d.dat.orders.interruptible[o as usize] == 0 && o != 0x45 {
                        break;
                    }
                    self.order_remove(d, f, tail);
                }
                self.order_queue_append(d, f, 0x45, 0xe4, (0, 0), NIL);
            }
            self.prepare_next_order(d, f);
            f = self.units[f as usize].ptr_at(0xc4);
        }
    }

    /// 0x492cc0: lockdown / stasis / maelstrom take hold of `u`.
    fn disable_unit(&mut self, d: &Data, u: u32) {
        let sub = self.units[u as usize].subunit;
        if sub != NIL {
            self.units[sub as usize].status |= 0x400;
        }
        self.unit_halt(d, u);
        let t = self.units[u as usize].unit_type;
        let c = self.units[u as usize].connected_unit;
        if matches!(t, 1 | 0x10 | 0x64 | 0x63 | 0x68) && c != NIL && self.units[c as usize].unit_type == 0xe {
            // A ghost loses its nuke.
            self.units[u as usize].connected_unit = NIL;
            self.units[c as usize].connected_unit = NIL;
        }
        if matches!(t, 0x48 | 0x52) {
            self.recall_interceptors(d, u);
        }
        let o = self.units[u as usize].order;
        if o == 0x22 || o == 0x46 {
            self.units[u as usize].order_state = 0;
        }
        if d.dat.orders.keep_target_while_disabled[o as usize] == 0 {
            self.units[u as usize].order_target_unit = NIL;
        }
        self.order_start(d, u);
        // 0x4c3b40: selection refresh (UI).
        self.units[u as usize].button_set = 0xe4;
    }

    /// 0x455380 / 0x4553f0 / 0x4f67b0: lock down, maelstrom or stasis `t` for at least
    /// `time` frames.
    fn apply_disable(&mut self, d: &Data, t: u32, kind: u8, time: u8) {
        let (cur, base) = match kind {
            0 => (self.units[t as usize].lockdown_timer, 0x169),
            1 => (self.units[t as usize].maelstrom_timer, 0x3d6),
            _ => (self.units[t as usize].stasis_timer, 0x16d),
        };
        if cur == 0 {
            self.add_spell_overlay(d, t, base);
            if kind == 2 {
                self.units[t as usize].status |= 0x400_0000;
            }
        }
        let v = cur.max(time);
        let tu = &mut self.units[t as usize];
        match kind {
            0 => tu.lockdown_timer = v,
            1 => tu.maelstrom_timer = v,
            _ => tu.stasis_timer = v,
        }
        self.disable_unit(d, t);
    }

    /// 0x4f45e0: ensnare `t`.
    fn apply_ensnare(&mut self, d: &Data, t: u32) {
        let tu = &self.units[t as usize];
        if tu.ensnare_timer == 0 && tu.status & 0x10 == 0 {
            self.add_spell_overlay(d, t, 0x180);
        }
        self.units[t as usize].ensnare_timer = 0x4b;
        self.update_speed(d, t);
    }

    /// 0x455010: put the defensive matrix overlay back after unburrowing (or drop the
    /// matrix of a unit that can no longer have one).
    fn reapply_defense_matrix(&mut self, d: &Data, t: u32) {
        let timer = self.units[t as usize].defense_matrix_timer;
        if timer == 0 {
            return;
        }
        let player = self.units[t as usize].player;
        if self.units[t as usize].status & 0x400_0000 == 0 && self.tech_target_error(d, t, 6, player) == 0 {
            let hp = self.units[t as usize].defense_matrix_damage;
            let tu = &mut self.units[t as usize];
            tu.defense_matrix_timer = 0;
            tu.defense_matrix_damage = 0;
            self.apply_defense_matrix(d, t);
            let tu = &mut self.units[t as usize];
            tu.defense_matrix_damage = hp;
            tu.defense_matrix_timer = timer;
        } else {
            let tu = &mut self.units[t as usize];
            tu.defense_matrix_timer = 0;
            tu.defense_matrix_damage = 0;
        }
    }

    /// 0x454e80: put the irradiate overlay back (or drop the irradiate of a unit the
    /// weapon can no longer hit).
    fn reapply_irradiate(&mut self, d: &Data, t: u32) {
        let timer = self.units[t as usize].irradiate_timer;
        if timer == 0 {
            return;
        }
        let ok = self.weapon_can_target(d, 0x22, t, t);
        self.units[t as usize].irradiate_timer = 0;
        if !ok {
            self.units[t as usize].irradiated_by = NIL;
            return;
        }
        let (by, player) = (self.units[t as usize].irradiated_by, self.units[t as usize].irradiate_player);
        self.apply_irradiate(d, t, by, player);
        self.units[t as usize].irradiate_timer = timer;
    }

    /// 0x492da0: put back the overlays of the spells affecting `u`.
    pub(super) fn reapply_status_overlays(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].defense_matrix_timer != 0 {
            self.reapply_defense_matrix(d, u);
        }
        let v = self.units[u as usize].lockdown_timer;
        if v != 0 {
            self.units[u as usize].lockdown_timer = 0;
            self.apply_disable(d, u, 0, v);
        }
        let v = self.units[u as usize].maelstrom_timer;
        if v != 0 {
            self.units[u as usize].maelstrom_timer = 0;
            self.apply_disable(d, u, 1, v);
        }
        if self.units[u as usize].irradiate_timer != 0 {
            self.reapply_irradiate(d, u);
        }
        let v = self.units[u as usize].ensnare_timer;
        if v != 0 {
            self.units[u as usize].ensnare_timer = 0;
            self.apply_ensnare(d, u);
            self.units[u as usize].ensnare_timer = v;
        }
        let v = self.units[u as usize].stasis_timer;
        if v != 0 {
            self.units[u as usize].stasis_timer = 0;
            self.apply_disable(d, u, 2, v);
        }
        let v = self.units[u as usize].plague_timer;
        if v != 0 {
            self.units[u as usize].plague_timer = 0;
            self.apply_plague(d, u);
            self.units[u as usize].plague_timer = v;
        }
        if self.units[u as usize].acid_spore_count != 0 {
            let id = self.acid_overlay_id(d, u);
            self.add_overlay_image(d, u, id);
        }
    }

    /// 0x4f4160: acid spores wear off at once.
    fn clear_acid_spores(&mut self, u: u32) {
        let un = &mut self.units[u as usize];
        if un.acid_spore_count == 0 {
            return;
        }
        un.acid_spore_count = 0;
        un.acid_spore_time = [0; 9];
        self.remove_overlay_in(u, 0x3da, 0x3e5);
    }

    /// 0x455230: restoration cures `t` (a hallucination dies).
    fn restoration(&mut self, d: &Data, t: u32) {
        if self.units[t as usize].status & 0x4000_0000 != 0 {
            self.kill_unit(d, t);
            return;
        }
        self.add_spell_overlay(d, t, 0x3c7);
        let tu = &mut self.units[t as usize];
        tu.parasite_flags = 0;
        tu.is_blind = 0;
        if self.units[t as usize].ensnare_timer != 0 {
            self.units[t as usize].ensnare_timer = 0;
            self.remove_overlay_in(t, 0x180, 0x182);
            self.units[t as usize].ensnare_timer = 0;
            self.update_speed(d, t);
        }
        if self.units[t as usize].plague_timer != 0 {
            self.units[t as usize].plague_timer = 0;
            self.remove_overlay_in(t, 0x184, 0x186);
            self.units[t as usize].plague_timer = 0;
        }
        if self.units[t as usize].irradiate_timer != 0 {
            self.units[t as usize].irradiate_timer = 0;
            self.remove_overlay_in(t, 0x17c, 0x17e);
            let tu = &mut self.units[t as usize];
            tu.irradiate_timer = 0;
            tu.irradiated_by = NIL;
            tu.irradiate_player = 8;
        }
        if self.units[t as usize].lockdown_timer != 0 {
            // 0x454d90
            self.units[t as usize].lockdown_timer = 0;
            self.spell_overlay_ends(d, t, 0x169, 0x16b);
        }
        if self.units[t as usize].maelstrom_timer != 0 {
            // 0x454d20
            self.units[t as usize].maelstrom_timer = 0;
            self.spell_overlay_ends(d, t, 0x3d6, 0x3d8);
        }
        if self.units[t as usize].acid_spore_count != 0 {
            self.clear_acid_spores(t);
        }
        // 0x68c1b0 ...: status panel refresh (UI).
    }

    /// 0x455170: optical flare blinds `t` for `player` (a hallucination dies).
    fn optical_flare(&mut self, d: &Data, t: u32, player: u8) {
        if self.units[t as usize].status & 0x4000_0000 != 0 {
            self.kill_unit(d, t);
            return;
        }
        self.units[t as usize].is_blind |= 1 << (player & 0x1f);
        // 0x48ed50: the flare sound.
        self.add_spell_overlay(d, t, 0x3d0);
    }

    /// 0x492ba0: EMP drains shields and energy around the bullet (not the caster or its
    /// turret); hallucinations die, units in stasis are spared.
    fn emp(&mut self, d: &Data, b: u32) {
        let bu = &self.bullets.pool[b as usize];
        let (src, w) = (bu.source, bu.weapon);
        let _ = w;
        let sp = &self.sprites[bu.sprite as usize];
        // 0x6568ca: the EMP weapon's (0x21) inner splash radius.
        let r = d.dat.weapons.inner_splash[0x21] as i16;
        let mut bx = [sp.x - r, sp.y - r, sp.x + r, sp.y + r];
        // Clamped to the map (0x628450 / 0x6284b4); a box over the top edge keeps its
        // bottom unclamped, as in 1.16.1.
        if bx[0] < 0 {
            bx[0] = 0;
        }
        if bx[2] as i32 > self.map_px.0 as i32 {
            bx[2] = self.map_px.0 as i16;
        }
        if bx[1] < 0 {
            bx[1] = 0;
        } else if bx[3] as i32 > self.map_px.1 as i32 {
            bx[3] = self.map_px.1 as i16;
        }
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, bx);
        let found = search.results(start);
        search.pop();
        self.search = search;
        let sub = if src != NIL { self.units[src as usize].subunit } else { NIL };
        for c in found {
            if c == src || (src != NIL && c == sub) {
                continue;
            }
            if self.units[c as usize].status & 0x4000_0000 != 0 {
                self.kill_unit(d, c);
            } else if self.units[c as usize].stasis_timer == 0 {
                let cu = &mut self.units[c as usize];
                cu.energy = 0;
                cu.shields = 0;
                // 0x497430: selection bar redraw (UI).
            }
        }
    }
}

impl State {
    /// 0x488210 + 0x4878f0: an effect sprite at (x, y) at `elevation`.
    fn spell_sprite(&mut self, d: &Data, sprite_id: u16, x: i16, y: i16, elevation: u8) {
        if let Some(th) = self.create_thingy(d, sprite_id, x, y, 0) {
            let ts = self.thingies[th as usize].sprite;
            self.sprites[ts as usize].elevation = elevation;
            self.sprite_fog_visibility(ts);
        }
    }

    /// Units whose finder boxes overlap (x - r, y - r, x + r, y + r) (0x4e8280).
    fn units_around(&mut self, x: i16, y: i16, r: i16) -> Vec<u32> {
        let b = [x - r, y - r, x + r, y + r];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        found
    }

    /// 0x4f4830: ensnare around (x, y): every non-building but the caster (callback
    /// 0x4f46c0).
    fn ensnare_area(&mut self, d: &Data, x: i16, y: i16, src: u32) {
        self.spell_sprite(d, 0x16c, x, y, 0x13);
        for c in self.units_around(x, y, 0x40) {
            let cu = &self.units[c as usize];
            if c == src || d.dat.units.flags[cu.unit_type as usize] & 1 != 0 || cu.status & 0x400_0010 != 0 {
                continue;
            }
            self.apply_ensnare(d, c);
        }
    }

    /// 0x4f6a80: stasis field around (x, y) (callback 0x4f68d0).
    fn stasis_area(&mut self, d: &Data, x: i16, y: i16, src: u32) {
        self.spell_sprite(d, 0x169, x, y, 0x11);
        for c in self.units_around(x, y, 0x30) {
            let cu = &self.units[c as usize];
            if c == src || cu.status & 0x400_0010 != 0 || d.dat.units.flags[cu.unit_type as usize] & 1 != 0 {
                continue;
            }
            self.apply_disable(d, c, 2, 0x83);
        }
    }

    /// 0x4f6850: maelstrom around (x, y) (callback 0x4f6760): organic units freeze,
    /// hallucinations die.
    fn maelstrom_area(&mut self, d: &Data, x: i16, y: i16, src: u32) {
        self.spell_sprite(d, 0x202, x, y, 0x11);
        // 0x48ed50: the maelstrom sound.
        for c in self.units_around(x, y, 0x30) {
            let cu = &self.units[c as usize];
            if cu.status & 0x4000_0000 != 0 {
                self.kill_unit(d, c);
                continue;
            }
            let f = d.dat.units.flags[cu.unit_type as usize];
            if c == src || cu.status & 0x400_0010 != 0 || f & 1 != 0 || f & 0x1_0000 == 0 {
                continue;
            }
            self.apply_disable(d, c, 1, 0x16);
        }
    }

    /// 0x4f4480: one more acid spore on `t` (at most 9); the oldest timer is renewed
    /// when all nine are in use.
    fn add_acid_spore(&mut self, d: &Data, t: u32) {
        if self.units[t as usize].acid_spore_count < 9 {
            self.units[t as usize].acid_spore_count += 1;
        }
        let id = self.acid_overlay_id(d, t);
        if !self.has_overlay(t, id) {
            self.remove_overlay_in(t, 0x3da, 0x3e5);
            let id = self.acid_overlay_id(d, t);
            self.add_overlay_image(d, t, id);
        }
        let times = &mut self.units[t as usize].acid_spore_time;
        let mut oldest = 0;
        let mut min = 99999u32;
        for i in 0..9 {
            if times[i] == 0 {
                times[i] = 0x96;
                return;
            }
            if (times[i] as u32) < min {
                min = times[i] as u32;
                oldest = i;
            }
        }
        times[oldest] = 0x96;
    }

    /// 0x4f4770: acid spores on the enemy air units around (x, y) (callback 0x4f4670).
    fn acid_spore_area(&mut self, d: &Data, x: i16, y: i16, player: u8) {
        for c in self.units_around(x, y, 0x40) {
            let cu = &self.units[c as usize];
            if cu.player == player || d.dat.units.flags[cu.unit_type as usize] & 1 != 0 {
                continue;
            }
            if cu.status & 4 == 0 || cu.status & 0x400_0000 != 0 {
                continue;
            }
            if cu.status & 0x300 != 0 && cu.visibility & (1 << (player & 0x1f)) == 0 {
                continue;
            }
            self.add_acid_spore(d, c);
        }
    }

    /// 0x4f6af0: a disruption web (neutral unit 0x69) at (x, y) for 0x168 frames.
    fn create_disruption_web(&mut self, d: &Data, x: i16, y: i16, player: u8) {
        let mut p = (x, y);
        self.clamp_unit_pos(d, 0x69, &mut p);
        let Some(s) = self.create_unit(d, 0x69, p.0, p.1, 0xb) else {
            self.show_last_error(player);
            return;
        };
        self.units[s as usize].status |= 0x20_0000;
        let sp = self.units[s as usize].sprite;
        self.sprites[sp as usize].elevation = 0xb;
        self.finish_unit_pre(d, s);
        self.finish_unit_post(d, s);
        let un = &mut self.units[s as usize];
        if un.remove_timer == 0 || un.remove_timer > 0x168 {
            un.remove_timer = 0x168;
        }
    }
}

impl State {
    /// 0x4f4940: spawn broodlings: two broodlings (0x28) of `src`'s owner where `t`
    /// stood (unless `t` is a hallucination), then `t` dies and `src` gets the kill.
    fn spawn_broodlings(&mut self, d: &Data, src: u32, t: u32) {
        if self.units[t as usize].status & 0x4000_0000 == 0 {
            let (tx, ty) = self.unit_sprite_pos(t);
            let (mut x, mut y) = (tx - 2, ty - 2);
            let player = self.units[src as usize].player;
            for _ in 0..2 {
                let mut spot = self.terrain_fits(d, 0x28, x, y).then_some((x, y));
                if spot.is_none() {
                    // 0x49d3e0 with the caster's dimensions, as 1.16.1 does.
                    let b = [x - 0x20, y - 0x20, x + 0x20, y + 0x20];
                    spot = self.find_unit_placement(d, src, (x, y), Some(b), false);
                    if let Some(q) = spot {
                        (x, y) = q;
                    }
                }
                if spot.is_some() {
                    let mut p = (x, y);
                    self.clamp_unit_pos(d, 0x28, &mut p);
                    match self.create_unit(d, 0x28, p.0, p.1, player) {
                        None => self.show_last_error(player),
                        Some(b) => {
                            self.finish_unit_pre(d, b);
                            self.finish_unit_post(d, b);
                            let bu = &mut self.units[b as usize];
                            let timer = if bu.status & 0x4000_0000 != 0 {
                                0x546
                            } else if bu.unit_type == 0x28 {
                                0x708
                            } else {
                                0
                            };
                            if bu.remove_timer == 0 || timer < bu.remove_timer {
                                bu.remove_timer = timer;
                            }
                            if self.units[src as usize].ai != NIL {
                                todo_port!("0x4a2830", "AI: broodlings join the caster's AI");
                            }
                        }
                    }
                }
                x += 4;
                y += 4;
            }
        }
        let sp = self.units[src as usize].player;
        self.count_death(d, t, sp);
        self.kill_unit(d, t);
        let su = &mut self.units[src as usize];
        if su.kill_count != 0xff {
            su.kill_count += 1;
        }
        if su.unit_type == 0x49 {
            // 0x4759c0: the carrier gets the kill too.
            let mut k = su.ptr_at(0xc0);
            while k != NIL {
                let ku = &mut self.units[k as usize];
                if ku.kill_count != 0xff {
                    ku.kill_count += 1;
                }
                if ku.unit_type != 0x49 {
                    break;
                }
                k = ku.ptr_at(0xc0);
            }
        }
    }

    /// 0x48b770 types 6 / 7: parasite `t` for `player`; spawn broodlings from `t`.
    fn parasite(&mut self, t: u32, src: u32, player: u8) {
        // 0x48ed50: the parasite sound; 0x4c36c0: status panel refresh (UI).
        self.units[t as usize].parasite_flags |= 1 << (player & 0x1f);
        if src != NIL && self.player_type(self.units[t as usize].player) == 1 {
            todo_port!("0x43f320", "AI reacts to a parasite");
        }
    }
}

impl State {
    /// 0x4f63a0: Hallucination2 (secondary): any spell effect pops a hallucination.
    pub(super) fn secondary_hallucination(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        let hit = un.defense_matrix_damage != 0
            || un.stim_timer != 0
            || un.ensnare_timer != 0
            || un.lockdown_timer != 0
            || un.irradiate_timer != 0
            || un.stasis_timer != 0
            || un.parasite_flags != 0
            || un.storm_timer != 0
            || un.plague_timer != 0
            || un.is_blind != 0
            || un.maelstrom_timer != 0;
        if hit {
            self.kill_unit(d, u);
        }
    }
}

impl State {
    /// 0x4f6350: effect sprite `id` on `t`, one layer above it.
    fn sprite_on_unit(&mut self, d: &Data, t: u32, id: u16) {
        let (x, y) = self.unit_sprite_pos(t);
        if let Some(th) = self.create_thingy(d, id, x, y, 0) {
            let ts = self.thingies[th as usize].sprite;
            let s = self.units[t as usize].sprite;
            self.sprites[ts as usize].elevation = self.sprites[s as usize].elevation.wrapping_add(1);
            self.sprite_fog_visibility(ts);
        }
    }

    /// 0x4f6d40: CastFeedback: the target's energy becomes damage to it (hallucinated
    /// casters' targets just die).
    pub(super) fn order_cast_feedback(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        let ok = t != NIL
            && d.dat.units.flags[self.units[t as usize].unit_type as usize] & 0x20_0000 != 0
            && self.units[t as usize].status & 0x4000_0000 == 0;
        if !ok {
            self.spell_give_up(d, u);
            return;
        }
        let range = (self.unit_sight_range(d, u, true) as u32) << 5;
        let mut cost = 0;
        if !self.direct_cast_ready(d, u, 0x1d, range, &mut cost) {
            return;
        }
        let cheat = self.game_options & 0x800 != 0;
        let size = self.overlay_size(d, t);
        if self.units[t as usize].status & 0x4000_0000 != 0 {
            self.sprite_on_unit(d, t, 0x1f4 + size);
            if !cheat {
                self.units[u as usize].energy -= cost;
            }
            self.kill_unit(d, t);
        } else if self.units[t as usize].energy != 0 {
            let e = self.units[t as usize].energy as u32;
            let player = self.units[u as usize].player;
            self.damage_unit(d, t, e, 0x6a, 1, 1, u, player);
            self.units[t as usize].energy = 0;
            if !cheat {
                self.units[u as usize].energy -= cost;
            }
            // 0x48ed50: the feedback sound.
            let tu = &self.units[t as usize];
            if tu.sprite != NIL && tu.order != 0 {
                self.add_spell_overlay(d, t, 0x3d3);
            } else {
                self.sprite_on_unit(d, t, 0x1f4 + size);
            }
        }
        self.order_done_or_next(d, u);
    }
}

impl State {
    /// 0x4f6180: a freshly created copy becomes a hallucination (and its turret): full
    /// hp, out of the counts, the computer idle order, hallucination buttons and draw
    /// function, the Hallucination2 secondary order.
    fn hallucinate_new(&mut self, d: &Data, u: u32) {
        let mut u = u;
        loop {
            self.modify_unit_counters(d, u, -1);
            let t = self.units[u as usize].unit_type as usize;
            if !self.disabled(u) || d.dat.units.flags[t] & 1 != 0 {
                self.units[u as usize].button_set = 0xf4;
            }
            let un = &mut self.units[u as usize];
            un.status |= 0x4000_0001;
            un.hp = d.dat.units.hit_points[t] as i32;
            self.modify_unit_counters(d, u, 1);
            self.modify_completed_counters(d, u, 1, false);
            self.units[u as usize].order_flags |= 1;
            let o = d.dat.units.ai_computer_idle[t];
            self.issue_order(d, u, o, true, (0, 0), NIL, 0xe4);
            self.prepare_next_order(d, u);
            if self.replay || self.units[u as usize].player == self.local_player {
                // 0x497dc0: the hallucination draw function.
                let mut i = self.sprites[self.units[u as usize].sprite as usize].images.head;
                while i != NIL {
                    let img = &mut self.images[i as usize];
                    if img.modifier == 0 || img.modifier == 0x10 {
                        img.modifier = 0x10;
                        img.flags |= 1;
                        img.coloring = 0;
                    }
                    i = img.link.next;
                }
            }
            if d.dat.units.flags[t] & 0x10 != 0 {
                return;
            }
            // 0x431800: AI strengths (AI only).
            self.set_secondary(u, 0x95);
            u = self.units[u as usize].subunit;
            if u == NIL {
                return;
            }
        }
    }

    /// 0x4f6b90: a hallucination of `t` for `player` (reavers bring hallucinated scarabs).
    fn create_hallucination(&mut self, d: &Data, t: u32, player: u8) -> u32 {
        let (x, y) = self.unit_sprite_pos(t);
        let ty = self.units[t as usize].unit_type;
        let Some(h) = self.create_unit(d, ty, x, y, player) else {
            self.show_last_error(player);
            return NIL;
        };
        self.hallucinate_new(d, h);
        let hu = &mut self.units[h as usize];
        let timer = if hu.status & 0x4000_0000 != 0 { 0x546 } else if hu.unit_type == 0x28 { 0x708 } else { 0 };
        if hu.remove_timer == 0 || timer < hu.remove_timer {
            hu.remove_timer = timer;
        }
        if matches!(self.units[h as usize].unit_type, 0x53 | 0x51) {
            // 0x4f65b0: copies of the first docked scarab, one per scarab the reaver has.
            let template = self.units[t as usize].ptr_at(0xc0);
            let n = self.units[t as usize].u8_at(0xc8) as u32 + self.units[t as usize].u8_at(0xc9) as u32;
            for _ in 0..n {
                let s = self.create_hallucination(d, template, player);
                if s == NIL {
                    continue;
                }
                self.units[s as usize].set_ptr_at(0xc0, h);
                self.hide_unit(d, s);
                let head = self.units[h as usize].ptr_at(0xc0);
                self.units[s as usize].set_ptr_at(0xc4, head);
                self.units[s as usize].set_ptr_at(0xc8, NIL);
                if head != NIL {
                    self.units[head as usize].set_ptr_at(0xc8, s);
                }
                self.units[h as usize].set_ptr_at(0xc0, s);
                let c = self.units[h as usize].u8_at(0xc8);
                self.units[h as usize].set_u8_at(0xc8, c.wrapping_add(1));
                self.units[s as usize].set_u8_at(0xcc, 0);
            }
        }
        if self.player_type(player) == 1 {
            todo_port!("0x4a2830", "AI: a hallucination joins the computer's forces");
        }
        h
    }

    /// 0x4f66d0: put a new hallucination on the map next to where it was made (some
    /// types face a random way); false when there is no room.
    fn place_hallucination(&mut self, d: &Data, h: u32) -> bool {
        let p = self.unit_sprite_pos(h);
        let Some(q) = self.find_unit_placement(d, h, p, None, false) else {
            return false;
        };
        self.place_unit_at(d, h, q.0, q.1);
        if self.units[h as usize].status & 0x1_0000 != 0 {
            let dir = self.rng.rand_between(0x1e, 0, 0x20) as u8;
            let s = self.units[h as usize].sprite;
            let mut i = self.sprites[s as usize].images.head;
            while i != NIL {
                let next = self.images[i as usize].link.next;
                self.image_set_direction32(d, i, dir);
                i = next;
            }
            self.set_heading_now(d, h, dir.wrapping_mul(8));
        }
        self.show_unit(d, h);
        true
    }

    /// 0x4f6c40: CastHallucination: two hallucinations of the target.
    pub(super) fn order_cast_hallucination(&mut self, d: &Data, u: u32) {
        let range = (self.unit_sight_range(d, u, true) as u32) << 5;
        let mut cost = 0;
        if !self.direct_cast_ready(d, u, 0x14, range, &mut cost) {
            return;
        }
        let player = self.units[u as usize].player;
        for _ in 0..2 {
            let t = self.units[u as usize].order_target_unit;
            let h = self.create_hallucination(d, t, player);
            if h == NIL {
                break;
            }
            if !self.place_hallucination(d, h) {
                self.remove_subunit(d, h);
                break;
            }
        }
        if self.game_options & 0x800 == 0 {
            self.units[u as usize].energy -= cost;
        }
        // 0x48ed50: the hallucination sound.
        let t = self.units[u as usize].order_target_unit;
        let sub = self.units[t as usize].subunit;
        let host = if sub != NIL { sub } else { t };
        let s = self.units[host as usize].sprite;
        self.add_image_top(d, s, 0x221);
        self.spell_give_up(d, u);
    }
}

impl State {
    /// 0x4c8040: give `u` to `player` with what belongs to it: the worker inside a
    /// refinery, passengers, fighters, an addon, an addon being built, a nydus exit.
    pub(super) fn give_unit_with_parts(&mut self, d: &Data, u: u32, player: u8) -> bool {
        if player >= 12 || u == NIL {
            return false;
        }
        let un = &self.units[u as usize];
        let t = un.unit_type;
        if un.status & 0x4000_0000 != 0
            || matches!(t, 0xb0 | 0xb1 | 0xb2 | 0xbc | 0x49 | 0x55)
            || d.dat.units.flags[t as usize] & 2 != 0
            || un.player == player
        {
            return true;
        }
        // 0x4688b0 / 0x468a60: a completed refinery gives its gas worker too.
        if un.status & 1 != 0 && matches!(t, 0x6e | 0x9d | 0x95) {
            let mut h = self.player_units[un.player as usize];
            while h != NIL {
                let hu = &self.units[h as usize];
                if hu.order == 0x53
                    && d.dat.units.flags[hu.unit_type as usize] & 8 != 0
                    && self.sprites[hu.sprite as usize].flags & 0x20 != 0
                    && hu.order_target_unit == u
                {
                    break;
                }
                h = hu.player_next;
            }
            if h != NIL {
                self.give_unit(d, h, player);
            }
        }
        self.give_unit(d, u, player);
        if self.can_carry(d, u) {
            let n = d.dat.units.space_provided[self.units[u as usize].unit_type as usize] as usize;
            for i in 0..n.min(8) {
                let hd = self.units[u as usize].loaded_units[i] as u32;
                if hd == 0 {
                    continue;
                }
                let c = (hd & 0x7ff) - 1;
                let cu = &self.units[c as usize];
                if cu.sprite != NIL && (cu.order != 0 || cu.order_state != 1) && cu.unique_id as u32 == hd >> 11 {
                    self.give_unit(d, c, player);
                }
            }
        }
        if matches!(self.units[u as usize].unit_type, 0x48 | 0x52 | 0x53 | 0x51) {
            for (list, count) in [(0xc0, 0xc8), (0xc4, 0xc9)] {
                let mut f = self.units[u as usize].ptr_at(list);
                for _ in 0..self.units[u as usize].u8_at(count) {
                    let next = self.units[f as usize].ptr_at(0xc4);
                    self.give_unit(d, f, player);
                    f = next;
                }
            }
        }
        if self.units[u as usize].status & 2 != 0 {
            let addon = self.units[u as usize].ptr_at(0xc0);
            if addon != NIL {
                self.give_unit(d, addon, player);
            }
            // 0x4e66b0: an addon under construction goes too (0x466a30 picks it).
            let un = &self.units[u as usize];
            let b = un.current_build_unit;
            if un.secondary_order == 0x25 && b != NIL && self.units[b as usize].status & 1 == 0 {
                let tgt = un.order_target_unit;
                let building_target = un.order == 0x1f && tgt != NIL && self.units[tgt as usize].status & 1 == 0;
                let x = if matches!(un.order, 0x1e | 0x22 | 0x21) || building_target { tgt } else { b };
                self.give_unit(d, x, player);
            }
            let un = &self.units[u as usize];
            if un.unit_type == 0x86 {
                let exit = un.ptr_at(0xd0);
                if exit != NIL {
                    self.give_unit(d, exit, player);
                }
            }
        }
        true
    }

    /// 0x4f6950: CastMindControl: the target changes sides (a hallucination dies); the
    /// dark archon loses its shields.
    pub(super) fn order_cast_mind_control(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if t == NIL {
            self.spell_give_up(d, u);
            return;
        }
        if self.units[u as usize].player == self.units[t as usize].player {
            self.order_done_or_next(d, u);
            return;
        }
        let range = (self.unit_sight_range(d, u, true) as u32) << 5;
        let mut cost = 0;
        if !self.direct_cast_ready(d, u, 0x1b, range, &mut cost) {
            return;
        }
        if self.units[t as usize].status & 0x4000_0000 == 0 {
            self.add_spell_overlay(d, t, 0x3cd);
            // 0x4f6910
            let player = self.units[u as usize].player;
            self.give_unit_with_parts(d, t, player);
            if self.units[t as usize].unit_type == 0x3f {
                self.units[t as usize].energy = 0;
            }
            self.order_done_or_next(d, t);
        } else {
            self.kill_unit(d, t);
        }
        self.units[u as usize].shields = 0;
        if self.game_options & 0x800 == 0 {
            self.units[u as usize].energy -= cost;
        }
        // 0x48ed50: the mind control sound.
        self.order_done_or_next(d, u);
    }
}

impl State {
    /// 0x493ca0(0): `u` leaves the finder, tiles, repulse grid and paths before being
    /// moved (it stays visible).
    fn lift_off_map(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite;
        if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            return;
        }
        self.clear_references_visible(d, u);
        self.untarget_bullets_pub(u);
        self.finder_remove(u);
        if self.units[u as usize].status & 2 != 0 {
            let p = self.unit_sprite_pos(u);
            self.mark_building_tiles(d, u, p, false);
        }
        if self.units[u as usize].status & 4 != 0 {
            self.repulse_remove(d, u);
        }
        // 0x497530: redraw (UI).
        self.path_delete(u);
        self.units[u as usize].movement_state = 0;
        let low = (self.sprites[s as usize].elevation < 0xc) as u8;
        let pf = &mut self.units[u as usize].pathing_flags;
        *pf = (*pf & !1) | low;
    }

    /// 0x494160: a moved unit (and its turret) goes back into the finder, tiles,
    /// repulse grid and vision.
    fn put_back_on_map(&mut self, d: &Data, u: u32) {
        let sub = self.units[u as usize].subunit;
        let turret = sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0;
        if turret {
            let f = self.units[u as usize].fl.clone();
            let sf = &mut self.units[sub as usize].fl;
            sf.exact = f.exact;
            sf.pos = f.pos;
            let ss = self.units[sub as usize].sprite;
            self.move_sprite(ss, f.pos.0, f.pos.1);
        }
        let s = self.units[u as usize].sprite;
        if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            todo_port!("0x494160", "re-showing a hidden unit (visible list)");
        }
        self.unit_reveal(d, u);
        self.update_unit_visibility(d, u);
        self.finder_insert(d, u);
        if self.units[u as usize].status & 2 != 0 {
            let p = self.unit_sprite_pos(u);
            self.mark_building_tiles(d, u, p, true);
        }
        self.update_unit_overlaps(d, u);
        if self.units[u as usize].status & 4 != 0 {
            self.repulse_init(d, u);
        }
        let units = if turret { vec![u, sub] } else { vec![u] };
        for x in units {
            self.path_free(x);
            self.units[x as usize].movement_state = 0;
            let xs = self.units[x as usize].sprite;
            let low = (self.sprites[xs as usize].elevation < 0xc) as u8;
            let pf = &mut self.units[x as usize].pathing_flags;
            *pf = (*pf & !1) | low;
        }
    }

    /// 0x4942d0: recall `c` next to the arbiter `a` (it stays where it was when there
    /// is no room).
    fn recall_unit(&mut self, d: &Data, c: u32, a: u32) -> bool {
        let orig = self.unit_sprite_pos(c);
        let ap = self.unit_sprite_pos(a);
        // 0x4ebae0 (the same code as 0x4eb9f0).
        self.place_unit_at(d, c, ap.0, ap.1);
        let Some(q) = self.find_unit_placement(d, c, ap, None, false) else {
            self.place_unit_at(d, c, orig.0, orig.1);
            return false;
        };
        self.lift_off_map(d, c);
        self.units[c as usize].order_target_unit = NIL;
        self.place_unit_at(d, c, q.0, q.1);
        self.put_back_on_map(d, c);
        let t = self.units[c as usize].unit_type;
        if t != 0x3b {
            let o = d.dat.units.ai_return_to_idle[t as usize];
            self.order_now(d, c, o);
        }
        let cs = self.units[c as usize].sprite;
        let e = self.sprites[cs as usize].elevation.wrapping_add(1);
        self.spell_sprite(d, 0x17b, q.0, q.1, e);
        let k = self.units[c as usize].connected_unit;
        if matches!(t, 1 | 0x10 | 100 | 99 | 0x68) && k != NIL && self.units[k as usize].unit_type == 0xe {
            self.units[c as usize].connected_unit = NIL;
            self.units[k as usize].connected_unit = NIL;
        }
        true
    }

    /// 0x494470: CastRecall: pay, show the recall field, and after 0x16 frames bring the
    /// caster's units from around the target spot.
    pub(super) fn order_cast_recall(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_state == 1 {
            if self.units[u as usize].main_order_timer != 0 {
                return;
            }
            let (x, y) = self.units[u as usize].order_target_pos;
            let player = self.units[u as usize].player;
            let mut any = false;
            for c in self.units_around(x, y, 0x40) {
                let cu = &self.units[c as usize];
                let ct = cu.unit_type;
                if self.sprites[cu.sprite as usize].flags & super::sprite::sflag::HIDDEN != 0
                    || cu.player != player
                    || d.dat.units.flags[ct as usize] & 1 != 0
                    || cu.status & 0x400_0010 != 0
                    || matches!(ct, 0x23 | 0x24 | 0x61)
                    || cu.status & 0x4000_0000 != 0
                    || c == u
                {
                    continue;
                }
                any |= self.recall_unit(d, c, u);
            }
            if any {
                // 0x48ed50: one of the two recall sounds (0x228 + rand & 1).
                self.rng.rand(0x12);
            }
            self.order_done_or_next(d, u);
            return;
        }
        let cost = (d.dat.techdata.energy_cost[0x15] as u8 as u16) << 8;
        let cheat = self.game_options & 0x800 != 0;
        if !cheat && self.units[u as usize].energy < cost {
            // 0x48ccb0: "Not enough energy" (UI).
            self.order_done_or_next(d, u);
            return;
        }
        if !cheat {
            self.units[u as usize].energy -= cost;
        }
        let t = self.units[u as usize].order_target_unit;
        if t != NIL {
            self.units[u as usize].order_target_pos = self.unit_sprite_pos(t);
        }
        let p = self.units[u as usize].order_target_pos;
        let s = self.units[u as usize].sprite;
        let e = self.sprites[s as usize].elevation.wrapping_add(1);
        self.spell_sprite(d, 0x17b, p.0, p.1, e);
        self.rng.rand(0x11);
        // 0x48ec10: the recall sound at the spot (UI).
        self.units[u as usize].main_order_timer = 0x16;
        self.units[u as usize].order_state = 1;
    }
}
