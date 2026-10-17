//! Sound effects (sfxdata.dat -> sound/*.wav) and music, via rodio.

use crate::storage::Storage;
use crate::dat::{self, Dat};
use rodio::Source;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

pub struct Audio {
    sink: rodio::MixerDeviceSink,
    names: Vec<String>,
    cache: RefCell<HashMap<u16, Option<Arc<[u8]>>>>,
    music: Option<rodio::Player>,
    /// The sound options' volumes, 0..=100 (0x6cdfe0 music, 0x6cdfe4 digital).
    volumes: (i16, i16),
    /// 0x5008f0: DirectSound attenuations (hundredths of a dB) by volume 0..=99, read from
    /// the install's StarCraft.exe (not stored in the repo).
    table: [i32; 100],
    /// 0x5998f0: the 8 playing-sound slots.
    slots: RefCell<[Option<Slot>; 8]>,
    /// 0x6d127c +4 per sound id: when it last started (ms since `epoch`).
    last: RefCell<HashMap<u16, u128>>,
    epoch: std::time::Instant,
}

struct Slot {
    player: rodio::Player,
    unit: Option<u32>,
    flags: u8,
    priority: u8,
    id: u16,
}

/// 0x5008f0 is at file offset 0x1008f0 of StarCraft.exe (int32[100]); a linear
/// amplitude ramp when the file is missing or short.
fn volume_table() -> [i32; 100] {
    let from_exe = || -> Option<[i32; 100]> {
        let exe = std::fs::read(crate::install_dir().join("StarCraft.exe")).ok()?;
        let b = exe.get(0x1008f0..0x1008f0 + 400)?;
        let mut t = [0; 100];
        for (v, c) in t.iter_mut().zip(b.chunks_exact(4)) {
            *v = i32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        }
        Some(t)
    };
    from_exe().unwrap_or_else(|| {
        let mut t = [-10000; 100];
        for (i, v) in t.iter_mut().enumerate().skip(1) {
            *v = (2000.0 * (i as f32 / 99.0).log10()) as i32;
        }
        t
    })
}

impl Audio {
    /// None when no audio device is available; the game runs silently.
    pub fn open(files: &Storage) -> Option<Audio> {
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().ok()?;
        sink.log_on_drop(false);
        let names = dat::parse_tbl(&files.read("arr/sfxdata.tbl").ok()?);
        Some(Audio { sink, names, cache: RefCell::new(HashMap::new()), music: None, volumes: (25, 50),
            table: volume_table(), slots: RefCell::new(Default::default()), last: RefCell::new(HashMap::new()),
            epoch: std::time::Instant::now() })
    }

    /// Decoded-on-demand wav cache, kept forever. Stands in for the native sound cache:
    /// 0x4bbeb0 queues an unloaded sound for the loader thread, which starts it once loaded
    /// (here it is read and started at once); 0x4bb8e0 frees one cached sound and 0x4bbc00
    /// frees round-robin down to the 0x5129b8 memory budget; 0x4bbe50 waits for loads and
    /// 0x4bbf50 clears the flag-8 'pending' marks and frees finished buffers (alloc_slot
    /// does the latter). Memory management only, inaudible.
    fn wav(&self, files: &Storage, dat: &Dat, id: u16) -> Option<Arc<[u8]>> {
        self.cache
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| {
                let file = *dat.sfxdata.file.get(id as usize)? as usize;
                let name = self.names.get(file.checked_sub(1)?)?;
                files.read(&format!("sound/{}", name.replace('\\', "/"))).ok().map(Arc::from)
            })
            .clone()
    }

    /// `pan_volume`: 0.0..=1.0 attenuation for distance from the screen (1.0 on screen).
    /// As 0x48ed50: the volume is the larger of the distance volume (99 on screen) and
    /// the sound's sfxdata.dat minimum volume; under 11 it isn't played.
    pub fn play(&self, files: &Storage, dat: &Dat, id: u16, pan_volume: f32) {
        let min = dat.sfxdata.volume.get(id as usize).copied().unwrap_or(0) as f32;
        let vol = (pan_volume * 99.0).max(min);
        if vol < 11.0 {
            return;
        }
        self.play_pan(files, dat, id, vol as i32, 0, None);
    }

    /// 0x4bc270: plays sound `id` at volume `vol` (0..=99) with the DirectSound pan `pan`
    /// (hundredths of a dB of attenuation on the side away from the sound; negative = sound
    /// on the left) for `unit`: nothing when the digital volume is 0, else the volume
    /// table's attenuation of digital * vol / 100 goes to 0x4bbfd0.
    pub fn play_pan(&self, files: &Storage, dat: &Dat, id: u16, vol: i32, pan: i32, unit: Option<u32>) {
        if self.volumes.1 == 0 {
            return;
        }
        let atten = self.table[(self.volumes.1 as i32 * vol / 100).clamp(0, 99) as usize];
        self.play_checked(files, dat, id, atten, pan, unit);
    }

    /// 0x4bbfd0: sounds flagged 0x10 (one at a time) or 0x2 (one per unit) replace or cancel
    /// the still-running instance, then the sound starts.
    fn play_checked(&self, files: &Storage, dat: &Dat, id: u16, atten: i32, pan: i32, unit: Option<u32>) {
        let flags = dat.sfxdata.flags.get(id as usize).copied().unwrap_or(0);
        if (unit.is_some() && flags & 2 != 0) || flags & 0x10 != 0 {
            for slot in self.slots.borrow_mut().iter_mut() {
                let Some(s) = slot else { continue };
                let hit = if flags & 0x10 == 0 { s.unit == unit && s.flags & 2 != 0 } else { s.id == id };
                if hit {
                    if !s.player.empty() {
                        return;
                    }
                    *slot = None;
                }
            }
        }
        self.start(files, dat, id, atten, pan, unit, flags | 4);
    }

    /// 0x4bb740: a free slot (finished ones are freed first), else the lowest-priority slot
    /// strictly below `priority` that is neither loader-pending (8) nor locked (0x20), stopped.
    fn alloc_slot(&self, priority: u8) -> Option<usize> {
        let mut slots = self.slots.borrow_mut();
        let mut free = None;
        for (i, slot) in slots.iter_mut().enumerate() {
            match slot {
                Some(s) if s.flags & 8 == 0 && s.player.empty() => *slot = None,
                _ => {}
            }
            if slot.is_none() && free.is_none() {
                free = Some(i);
            }
        }
        if free.is_some() {
            return free;
        }
        let (mut best, mut low) = (None, priority);
        for (i, slot) in slots.iter().enumerate() {
            if let Some(s) = slot.as_ref().filter(|s| s.flags & 0x28 == 0 && s.priority < low) {
                best = Some(i);
                low = s.priority;
            }
        }
        let i = best?;
        if let Some(s) = slots[i].take() {
            s.player.stop();
        }
        Some(i)
    }

    /// 0x4bbaf0: starts the sound in a slot unless the same id began under 80 ms ago.
    fn start(&self, files: &Storage, dat: &Dat, id: u16, atten: i32, pan: i32, unit: Option<u32>, flags: u8) {
        let now = self.epoch.elapsed().as_millis();
        if self.last.borrow().get(&id).is_some_and(|&t| now < t + 80) {
            return;
        }
        let Some(data) = self.wav(files, dat, id) else { return };
        let Ok(src) = rodio::Decoder::new(Cursor::new(data)) else { return };
        let priority = dat.sfxdata.priority.get(id as usize).copied().unwrap_or(0);
        let Some(i) = self.alloc_slot(priority) else { return };
        let g = if atten <= -10000 { 0.0 } else { 10f32.powf(atten as f32 / 2000.0) };
        let far = 10f32.powf(-(pan.abs() as f32) / 2000.0);
        let (ch, rate) = (src.channels(), src.sample_rate());
        let player = rodio::Player::connect_new(self.sink.mixer());
        if pan == 0 || ch.get() > 2 {
            player.append(src.amplify(g));
        } else {
            let (l, r) = if pan < 0 { (1.0, far) } else { (far, 1.0) };
            // A mono sound becomes stereo so the far side can be quieter.
            let v: Vec<f32> = if ch.get() == 1 {
                src.flat_map(|x| [x * g * l, x * g * r]).collect()
            } else {
                src.enumerate().map(|(i, x)| x * g * if i % 2 == 0 { l } else { r }).collect()
            };
            let two = std::num::NonZero::new(2).unwrap();
            player.append(rodio::buffer::SamplesBuffer::new(two, rate, v));
        }
        self.last.borrow_mut().insert(id, now);
        self.slots.borrow_mut()[i] = Some(Slot { player, unit, flags, priority, id });
    }

    /// The sound's length in ms (the 0x6d1278 table of 0x45ef50), if known.
    pub fn duration_ms(&self, files: &Storage, dat: &Dat, id: u16) -> Option<u32> {
        let src = rodio::Decoder::new(Cursor::new(self.wav(files, dat, id)?)).ok()?;
        Some(src.total_duration()?.as_millis() as u32)
    }

    /// 0x4bc490: plays a whole wav file (a trigger's, from the map archive) at the digital
    /// volume's table attenuation, `table[digital * 99 / 100]`; no slot or priority.
    pub fn play_file(&self, data: Vec<u8>) {
        let Ok(src) = rodio::Decoder::new(Cursor::new(Arc::<[u8]>::from(data))) else { return };
        let atten = self.table[(self.volumes.1 as i32 * 99 / 100).clamp(0, 99) as usize];
        let g = if atten <= -10000 { 0.0 } else { 10f32.powf(atten as f32 / 2000.0) };
        let player = rodio::Player::connect_new(self.sink.mixer());
        player.append(src.amplify(g));
        player.detach();
    }

    pub fn set_volumes(&mut self, music: i16, digital: i16) {
        if let Some(p) = self.music.as_ref().filter(|_| self.volumes.0 != music) {
            p.set_volume(music as f32 / 50.0);
        }
        self.volumes = (music, digital);
    }

    pub fn play_music(&mut self, files: &Storage, path: &str) {
        let Ok(data) = files.read(path) else { return };
        let Ok(src) = rodio::Decoder::new_looped(Cursor::new(Arc::<[u8]>::from(data))) else { return };
        let player = rodio::Player::connect_new(self.sink.mixer());
        player.set_volume(self.volumes.0 as f32 / 50.0);
        player.append(src);
        self.music = Some(player);
    }
}
