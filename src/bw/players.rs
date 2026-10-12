//! Per-player game tables (1.16.1 addresses).

#[derive(Clone, Debug)]
pub struct Players {
    /// 0x57f0f0 / 0x57f120: minerals and gas; 0x57f180 / 0x57f150: gathered totals.
    pub minerals: [i32; 12],
    pub gas: [i32; 12],
    pub minerals_gathered: [i32; 12],
    pub gas_gathered: [i32; 12],
    /// 0x5821a4 / 0x582234 / 0x5822c4: supply caps (zerg, terran, protoss).
    pub supply_max: [[i32; 12]; 3],
    /// 0x58d2b0: upgrade levels 0..46, [player][upgrade].
    pub upgrade_sc: [[u8; 46]; 12],
    /// 0x58f32c: Brood War upgrade levels 46..61.
    pub upgrade_bw: [[u8; 15]; 12],
    /// 0x58f3e0: upgrade-in-progress bits, 8 bytes per player.
    pub upgrade_in_progress: [[u8; 8]; 12],
    /// 0x582324: unit counts [unit type][player].
    pub unit_counts: Vec<[i32; 12]>,
    /// 0x582174 / 0x582204 / 0x582294: supply used by zerg / terran / protoss units.
    pub supply_used: [[i32; 12]; 3],
    /// 0x582084: factory count, 0x581f34: building count, 0x581de4: men count.
    pub factories: [i32; 12],
    pub buildings: [i32; 12],
    pub men: [i32; 12],
    /// 0x584de4: completed unit counts [unit type][player].
    pub completed_counts: Vec<[i32; 12]>,
    /// 0x582144 / 0x5821d4 / 0x582264: supply provided (zerg / terran / protoss).
    pub supply_provided: [[i32; 12]; 3],
    /// 0x5820b4 / 0x581e14 / 0x581f64: completed factories, men, buildings.
    pub factories_completed: [i32; 12],
    pub men_completed: [i32; 12],
    pub buildings_completed: [i32; 12],
    /// 0x581e44 units produced, 0x581ed4 unit score, 0x581f94 buildings constructed, 0x582024 building score.
    pub units_produced: [i32; 12],
    pub unit_score: [i32; 12],
    pub buildings_constructed: [i32; 12],
    pub building_score: [i32; 12],
}

impl Default for Players {
    fn default() -> Players {
        Players {
            minerals: [0; 12],
            gas: [0; 12],
            minerals_gathered: [0; 12],
            gas_gathered: [0; 12],
            supply_max: [[400; 12]; 3],
            upgrade_sc: [[0; 46]; 12],
            upgrade_bw: [[0; 15]; 12],
            upgrade_in_progress: [[0; 8]; 12],
            unit_counts: vec![[0; 12]; 228],
            supply_used: [[0; 12]; 3],
            factories: [0; 12],
            buildings: [0; 12],
            men: [0; 12],
            completed_counts: vec![[0; 12]; 228],
            supply_provided: [[0; 12]; 3],
            factories_completed: [0; 12],
            men_completed: [0; 12],
            buildings_completed: [0; 12],
            units_produced: [0; 12],
            unit_score: [0; 12],
            buildings_constructed: [0; 12],
            building_score: [0; 12],
        }
    }
}

impl Players {
    pub fn upgrade_level(&self, player: u8, upgrade: usize) -> u8 {
        let p = player as usize;
        if upgrade < 46 {
            self.upgrade_sc[p][upgrade]
        } else {
            self.upgrade_bw[p][upgrade - 46]
        }
    }
}
