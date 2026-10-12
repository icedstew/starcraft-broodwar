//! Frame loop: game_step (1.16.1 0x4d94b0) and update_units (0x4ed000).
//!
//! Literal ports; each callee is either ported (bw::*) or an explicit todo_port!.

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

impl State {
    /// 0x4d94b0 game_step.
    pub fn game_step(&mut self, d: &Data) {
        self.rng.enabled = true; // 0x4dc540(1)
        self.random_event_tick(d);
        todo_port!("0x4a2b00", "ai_update");
        if self.counter_100 == 0 {
            self.counter_100 = 100;
        }
        self.counter_100 -= 1;
        self.is_100th_frame = self.counter_100 == 0;
        if self.is_100th_frame {
            todo_port!("0x49c4c0", "creep update (every 100 frames)");
            self.vision_reset();
        }
        self.update_units(d);
        self.update_bullets(d);
        todo_port!("0x496330", "update flingies (0x63ff34)");
        self.update_thingies(d);
        self.rng.enabled = false; // 0x4dc540(0)
    }

    /// 0x47e410: random event timer (draws from the RNG inside 0x47e130).
    fn random_event_tick(&mut self, _d: &Data) {
        if self.random_event_timer != 0 {
            self.random_event_timer -= 1;
            return;
        }
        todo_port!("0x47e410", "reload from table 0x513c24[0x65eb26 >> 7], then 0x47e130");
    }

    /// 0x4ed000 update_units.
    pub fn update_units(&mut self, d: &Data) {
        self.var_6957d0 = 0;
        // 0x49a2c0 + 0x48d9a0/0x48e310/0x48ca10 refresh the selection UI only.
        self.stagger_150 = self.stagger_150.wrapping_sub(1);
        if self.stagger_150 == 0 {
            let mut c = 0u8;
            for u in super::list::iter(&self.units, self.visible_units.head).collect::<Vec<_>>() {
                self.units[u as usize].order_process_timer = c;
                c = (c + 1) % 8;
            }
            self.stagger_150 = 150;
        }
        self.stagger_300 = self.stagger_300.wrapping_sub(1);
        if self.stagger_300 == 0 {
            let mut c = 0u8;
            for u in super::list::iter(&self.units, self.visible_units.head).collect::<Vec<_>>() {
                self.units[u as usize].ai_action_flag = c;
                c = (c + 1) % 30;
            }
            self.stagger_300 = 300;
        }
        todo_port!("0x4eb2f0", "detection pass");
        // Dead units (0x62842c): finish their sprites.
        let mut u = self.dead_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            self.iscript_order_unit = Some(u); // 0x6d11f4
            self.iscript_unit = Some(u); // 0x6d11fc
            self.update_dead_unit(d, u);
            if self.units[u as usize].sprite != NIL {
                if self.is_100th_frame && self.player_type(self.units[u as usize].player) == 2 {
                    let un = &self.units[u as usize];
                    let s = &self.sprites[un.sprite as usize];
                    let (mask, x, y, air) = (self.player_vision[un.player as usize], s.x as i32, s.y as i32, un.status & 4 != 0);
                    self.reveal(mask, 1, x, y, air);
                }
                self.update_unit_visibility(d, u);
            }
            u = next;
        }
        // Visible units (0x628430): movement.
        let mut u = self.visible_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            self.iscript_order_unit = Some(u); // 0x6d11f4
            self.iscript_unit = Some(u); // 0x6d11fc
            self.unit_movement(d, u);
            u = next;
        }
        // Scanner sweeps (0x6283f4).
        if self.is_100th_frame {
            let mut u = self.scanner_units.head;
            while u != NIL {
                let next = self.units[u as usize].link.next;
                self.iscript_order_unit = Some(u);
                self.iscript_unit = Some(u);
                self.unit_reveal(d, u);
                u = next;
            }
        }
        let mut u = self.visible_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            self.update_unit_visibility(d, u);
            let un = &mut self.units[u as usize];
            if un.status & 0x300 != 0 {
                un.is_cloaked = 0;
                if un.ai_action_flag == 0 {
                    todo_port!("0x443390", "cloak visibility refresh");
                    self.units[u as usize].ai_action_flag = 0x1e;
                } else {
                    un.ai_action_flag -= 1;
                }
            }
            u = next;
        }
        let mut u = self.visible_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            self.iscript_order_unit = Some(u); // 0x6d11f4
            self.iscript_unit = Some(u); // 0x6d11fc
            self.update_unit(d, u);
            u = next;
        }
        let mut u = self.hidden_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            if self.units[u as usize].status & 0x300 != 0 {
                self.units[u as usize].is_cloaked = 0;
            }
            self.iscript_order_unit = Some(u);
            self.iscript_unit = Some(u);
            self.update_hidden_unit(d, u);
            u = next;
        }
        todo_port!("0x492eb0", "cloaked units list update");
        todo_port!("0x493790", "psi power update");
        // 0x64dec8: 32 rows of 32 dwords; advance the ring and clear the new row.
        self.finder_ring = (self.finder_ring + 1) % 0x20;
        self.finder_rows[self.finder_ring as usize] = [0; 0x20];
        self.finder_64dea8 = 0;
        let mut u = self.scanner_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            self.iscript_order_unit = Some(u);
            self.iscript_unit = Some(u);
            self.update_unit(d, u);
            u = next;
        }
        self.iscript_order_unit = None;
        self.iscript_unit = None;
    }

    /// 0x4eb5e0 (dead-unit list step).
    fn update_dead_unit(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite;
        if s != NIL && self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            self.free_unit_sprite(u);
        }
        let s = self.units[u as usize].sprite;
        if s != NIL {
            self.update_unit_sprite(d, s);
            if self.sprites[s as usize].images.head == NIL {
                self.units[u as usize].sprite = NIL;
            }
        }
        if self.units[u as usize].status & 0x4000 != 0 {
            if self.dead_removal_counter != 0 {
                self.dead_removal_counter -= 1;
                return;
            }
            self.dead_removal_counter = 3;
            todo_port!("0x47df90", "delayed dead-unit removal check");
            return;
        }
        if self.units[u as usize].sprite == NIL {
            self.free_unit(d, u);
        }
    }

    /// 0x4ecf70 update_unit.
    pub fn update_unit(&mut self, d: &Data, u: u32) {
        let ut = self.units[u as usize].unit_type as usize;
        let s = self.units[u as usize].sprite;
        if d.dat.units.flags[ut] & 0x10 == 0 && self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            todo_port!("0x4ebbd0", "sprite visibility for owner");
        }
        self.update_unit_timers(d, u);
        self.execute_main_order(d, u);
        self.execute_secondary_order(d, u);
        let sub = self.units[u as usize].subunit;
        if sub != NIL && d.dat.units.flags[ut] & 0x10 == 0 {
            self.iscript_unit = Some(sub);
            self.update_unit(d, sub);
            self.iscript_unit = Some(u);
        }
        let s = self.units[u as usize].sprite;
        if s != NIL {
            self.update_unit_sprite(d, s);
            if self.sprites[s as usize].images.head == NIL {
                self.units[u as usize].sprite = NIL;
            }
        }
    }

    /// 0x4ec420: update a unit that is not on the map (hidden sprite).
    pub fn update_hidden_unit(&mut self, d: &Data, u: u32) {
        let ut = self.units[u as usize].unit_type as usize;
        let sub = self.units[u as usize].subunit;
        if sub != NIL && d.dat.units.flags[ut] & 0x10 == 0 {
            self.iscript_unit = Some(sub);
            self.update_hidden_unit(d, sub);
            self.iscript_unit = Some(u);
        }
        self.movement_step(d, u);
        self.update_unit_timers(d, u);
        self.execute_hidden_order(d, u);
        match self.units[u as usize].secondary_order {
            0x3f => todo_port!("0x466790", "TrainFighter"),
            0x6d => todo_port!("0x491790", "Cloak"),
            0x6e => self.set_secondary_order(u, 0x17),
            _ => {}
        }
        let s = self.units[u as usize].sprite;
        if s != NIL {
            self.update_unit_sprite(d, s);
            if self.sprites[s as usize].images.head == NIL {
                self.units[u as usize].sprite = NIL;
            }
        }
    }

    /// 0x4ebed0: main order of a hidden unit.
    fn execute_hidden_order(&mut self, d: &Data, u: u32) {
        let order = self.units[u as usize].order;
        match order {
            0 => todo_port!("0x479480", "Die (hidden)"),
            3 | 4 | 0x16 | 0x5c => {
                let next = if self.units[u as usize].status & 0x20 != 0 { 5 } else { 0x17 };
                self.order_now(d, u, next);
            }
            0x11 => todo_port!("0x4f3ea0", "hidden order 0x11"),
            0x17 | 0x18 | 0xa2 | 0xaf | 0xb0 => {}
            0x1d => todo_port!("0x4ea290", "hidden order 0x1d"),
            0x53 => self.order_harvest_gas(d, u),
            0x61 => todo_port!("0x4f3e10", "hidden order 0x61"),
            0x7d => todo_port!("0x464730", "hidden order 0x7d"),
            0x96 => todo_port!("0x4671b0", "hidden order 0x96"),
            0x97 => todo_port!("0x42e320", "hidden order 0x97"),
            _ => {
                let un = &mut self.units[u as usize];
                let t = un.order_process_timer;
                un.order_process_timer = t.wrapping_sub(1);
                if t != 0 {
                    return;
                }
                un.order_process_timer = 8;
                match order {
                    5 => todo_port!("0x4790a0", "hidden order 5"),
                    0x60 => todo_port!("0x4e7b70", "hidden order 0x60"),
                    0x9c if un.status & 0x20 != 0 => todo_port!("0x4790a0", "hidden order 0x9c"),
                    0xa1 => todo_port!("0x4a1ef0", "hidden order 0xa1"),
                    _ => {}
                }
            }
        }
    }

    /// 0x4ec290 update_unit_timers.
    pub fn update_unit_timers(&mut self, d: &Data, u: u32) {
        let un = &mut self.units[u as usize];
        un.main_order_timer = un.main_order_timer.saturating_sub(1);
        un.ground_cooldown = un.ground_cooldown.saturating_sub(1);
        un.air_cooldown = un.air_cooldown.saturating_sub(1);
        un.spell_cooldown = un.spell_cooldown.saturating_sub(1);
        let ut = un.unit_type as usize;
        if d.dat.units.shield_enable[ut] != 0 {
            let max = (d.dat.units.shields[ut] as i32) << 8;
            if un.shields != max {
                un.shields = (un.shields + 7).min(max);
                let s = un.sprite;
                if self.sprites[s as usize].flags & 0x8 != 0 {
                    todo_port!("0x497430", "selection hp/shield bar redraw");
                }
            }
        }
        let un = &mut self.units[u as usize];
        // Vultures (0x25) / Jim Raynor's vulture (0x36): re-enable order processing when the weapon is ready.
        if (un.unit_type == 0x25 || un.unit_type == 0x36) && un.ground_cooldown == 0 {
            un.order_process_timer = 0;
        }
        un.is_being_healed = 0;
        let s = un.sprite;
        let completed = un.status & super::unit::status::COMPLETED != 0;
        if completed || self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            let un = &mut self.units[u as usize];
            un.cycle_counter = un.cycle_counter.wrapping_add(1);
            if un.cycle_counter > 7 {
                un.cycle_counter = 0;
                todo_port!("0x492f70", "8-frame status timers (stim, ensnare, lockdown, ...)");
            }
        }
        if self.units[u as usize].status & super::unit::status::COMPLETED == 0 {
            return;
        }
        let ut = self.units[u as usize].unit_type as usize;
        // Regenerating units (units.dat flag 0x80) heal 4/256 hp per frame.
        if d.dat.units.flags[ut] & 0x80 != 0 && self.units[u as usize].hp > 0 && self.units[u as usize].hp != d.dat.units.hit_points[ut] as i32 {
            let hp = self.units[u as usize].hp + 4;
            self.set_unit_hp(d, u, hp);
        }
        todo_port!("0x4eb4b0", "energy regeneration");
        let un = &mut self.units[u as usize];
        un.recent_order_timer = un.recent_order_timer.saturating_sub(1);
        if un.remove_timer != 0 {
            un.remove_timer -= 1;
            if un.remove_timer == 0 {
                todo_port!("0x475710", "remove_timer expired: kill unit");
                return;
            }
        }
        let gf = d.dat.units.staredit_group_flags[ut];
        if gf & 5 == 0 && gf & 2 != 0 && (self.units[u as usize].status & 2 != 0 || d.dat.units.flags[ut] & 0x20 != 0) {
            todo_port!("0x4022c0/0x4797b0", "terran building burn-down when hp < 1/3");
        }
    }

    /// 0x4ec170 execute_secondary_order.
    pub fn execute_secondary_order(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.secondary_order == 0x95 {
            todo_port!("0x4f63a0", "Hallucination2 timer");
            return;
        }
        // Disabled units (doodad-disabled, lockdown, stasis, maelstrom) skip secondary orders.
        if un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0 {
            return;
        }
        match un.secondary_order {
            0x25 => todo_port!("0x4e6790", "BuildAddon"),
            0x26 => todo_port!("0x468420", "Train"),
            0x3f => todo_port!("0x466790", "TrainFighter"),
            0x44 => todo_port!("0x4932d0", "ShieldBattery"),
            0x4e => todo_port!("0x4ea780", "SpawningLarva"),
            0x66 => self.secondary_spread_creep(d, u),
            0x6d => todo_port!("0x491790", "Cloak"),
            0x6e => todo_port!("0x4633e0", "Decloak"),
            0x83 => todo_port!("0x491c20", "CloakNearbyUnits"),
            _ => {}
        }
    }

    /// Secondary order SpreadCreep (0x66), inlined in 0x4ec170.
    fn secondary_spread_creep(&mut self, d: &Data, u: u32) {
        let ut = self.units[u as usize].unit_type;
        if matches!(ut, 0x83..=0x85) {
            todo_port!("0x4ea780", "hatchery larva spawning");
        }
        let un = &mut self.units[u as usize];
        // Spread creep timer: union +0xcc.
        if un.u8_at(0xcc) == 0 {
            un.set_u8_at(0xcc, 0xf);
            todo_port!("0x414680", "spread creep step (returns whether still spreading)");
            let spreading = false;
            if !spreading {
                let hatch = matches!(ut, 0x83..=0x85);
                let next = if hatch { 0x4e } else { 0x17 };
                self.set_secondary_order(u, next);
            }
            let _ = d;
            return;
        }
        let t = un.u8_at(0xcc);
        un.set_u8_at(0xcc, t - 1);
    }

    /// Set the secondary order (clears its state like 0x1401c9840 / inline in 1.16.1).
    pub fn set_secondary_order(&mut self, u: u32, order: u8) {
        let un = &mut self.units[u as usize];
        if un.secondary_order != order {
            un.secondary_order = order;
            un.secondary_order_pos = (0, 0);
            un.current_build_unit = NIL;
            un.secondary_order_state = 0;
        }
    }

    /// 0x497920 update_unit_sprite: run each image's update hook and iscript; free empty sprites.
    pub fn update_unit_sprite(&mut self, d: &Data, s: u32) {
        let mut img = self.sprites[s as usize].images.head;
        while img != NIL {
            let next = self.images[img as usize].link.next;
            self.image_update_hook(d, img);
            super::iscript::run(self, d, img, false, None);
            img = next;
            if self.sprites[s as usize].images.head == NIL {
                self.free_sprite(s);
            }
        }
    }

    /// Image update function pointer (CImage +0x38), selected by draw function.
    fn image_update_hook(&mut self, _d: &Data, img: u32) {
        let m = self.images[img as usize].modifier;
        if m != 0 && m != 9 && m != 10 && m != 0xe {
            todo_port!("image update hooks", "cloak/hallucination/warp per-modifier update functions");
        }
    }

    /// Unlink an empty sprite from its row list and return it to the free list.
    pub fn free_sprite(&mut self, s: u32) {
        let row = self.sprite_row(s);
        let mut l = self.sprite_rows[row];
        super::list::remove(&mut self.sprites, s, &mut l);
        self.sprite_rows[row] = l;
        super::list::insert_after_head(&mut self.sprites, s, &mut self.free_sprites);
    }

    /// 0x488510: update map sprites (0x654874) and fog sprites (0x654868).
    pub fn update_thingies(&mut self, d: &Data) {
        let mut t = self.active_thingies.head;
        while t != NIL {
            let next = self.thingies[t as usize].link.next;
            self.update_thingy(d, t);
            t = next;
        }
        todo_port!("0x488350", "fog sprites (0x654868)");
    }

    /// 0x488020: one map sprite; freed once its images are gone.
    fn update_thingy(&mut self, d: &Data, t: u32) {
        todo_port!("0x487690/0x4877b0", "map sprite visibility from fog");
        let s = self.thingies[t as usize].sprite;
        if s != NIL {
            self.update_unit_sprite(d, s);
            if self.sprites[s as usize].images.head == NIL {
                self.thingies[t as usize].sprite = NIL;
            }
        }
        if self.thingies[t as usize].sprite == NIL {
            super::list::remove(&mut self.thingies, t, &mut self.active_thingies);
            super::list::insert_after_head(&mut self.thingies, t, &mut self.free_thingies);
        }
    }

    /// 0x488550 tail: the hidden placeholder sprite 0x13e created with the pools.
    pub fn init_thingies(&mut self, d: &Data) {
        let Some(t) = self.create_thingy(d, 0x13e, 0, 0, 0) else { return };
        self.marker_thingy = Some(t);
        let s = self.thingies[t as usize].sprite;
        self.sprites[s as usize].flags |= super::sprite::sflag::HIDDEN;
        self.sprite_set_visibility(s, 0);
    }
}
