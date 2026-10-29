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
    /// 0x6d5bb0: the track playing (index into the 0x512bb8 table).
    music_track: u32,
    /// When 0x4a6250 last ran (`now_ms`); 0x4d1c90 runs it once a second.
    music_checked: u128,
    /// The sound options' volumes, 0..=100 (0x6cdfe0 music, 0x6cdfe4 digital).
    volumes: (i16, i16),
    /// 0x4d8fd0: the music volume to restore when the credits end (it was 0).
    credits_volume: Option<i16>,
    /// 0x4a6030: the music fade-in: the attenuation (0x6d5bb8, hundredths of a dB) and when the
    /// 20 ms timer last ran (`now_ms`).
    fade_in: Option<(i32, u128)>,
    /// 0x6d638c: the music was started (set by 0x4bc320).
    music_started: bool,
    /// 0x64086c: unit speech is muted (set by 0x48ebc0, cleared by 0x48fb40).
    speech_muted: bool,
    /// 0x6d5bbd: the music is ducked by 0x2ee (set by 0x48ebc0, cleared when 0x4a6140 starts a track).
    music_duck: bool,
    /// 0x5008f0: DirectSound attenuations (hundredths of a dB) by volume 0..=99, read from
    /// the install's StarCraft.exe (not stored in the repo).
    table: [i32; 100],
    /// 0x5998f0: the 8 playing-sound slots.
    slots: RefCell<[Option<Slot>; 8]>,
    /// 0x6d127c +4 per sound id: when it last started (`now_ms`).
    last: RefCell<HashMap<u16, u128>>,
    /// 0x51a208: the wav files playing (0x4bc490); 0x4015f0 stops them.
    files: RefCell<Vec<rodio::Player>>,
}

/// Wall clock in ms (std has no clock on wasm32).
fn now_ms() -> u128 {
    #[cfg(not(target_arch = "wasm32"))]
    return web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |t| t.as_millis());
    #[cfg(target_arch = "wasm32")]
    return js_sys::Date::now() as u128;
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

/// 0x512bb8 entries 1..=9 (path, next track = byte +6; all type 2): zerg 1-3, terran
/// 4-6, protoss 7-9, each cycling through its race's three.
fn game_track(i: u32) -> Option<(&'static str, u32)> {
    Some(match i {
        1 => ("music/zerg1.wav", 2),
        2 => ("music/zerg2.wav", 3),
        3 => ("music/zerg3.wav", 1),
        4 => ("music/terran1.wav", 5),
        5 => ("music/terran2.wav", 6),
        6 => ("music/terran3.wav", 4),
        7 => ("music/protoss1.wav", 8),
        8 => ("music/protoss2.wav", 9),
        9 => ("music/protoss3.wav", 7),
        // Radio Free Zerg (cheat 0x4a62b0), then zerg1.
        0x18 => ("music/RadioFreeZerg.wav", 1),
        _ => return None,
    })
}

/// 0x51a3b4: the music variant of the last game (-1 until the process's first game).
static MUSIC_VARIANT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// 0x4ee2d0 (outside campaigns): advances the music variant 0x51a3b4 0, 1, 2, 0, ...; the
/// first game of the process starts it from `rand() % 3` (`rand` is the CRT rand
/// 0x40c7bd, drawn only then). The game's track is `game_start_track(race)`.
pub fn next_music_variant(rand: impl FnOnce() -> u32) {
    use std::sync::atomic::Ordering::Relaxed;
    let mut v = MUSIC_VARIANT.load(Relaxed);
    if v == -1 {
        v = (rand() % 3) as i32;
    }
    v += 1;
    if v > 2 {
        v = 0;
    }
    MUSIC_VARIANT.store(v, Relaxed);
}

/// 0x4ee2d0: the in-game track (0x57f1de) for `race` (0x57f1e2): 1 / 4 / 7 plus the variant.
pub fn game_start_track(race: usize) -> u32 {
    [1, 4, 7][race] + MUSIC_VARIANT.load(std::sync::atomic::Ordering::Relaxed).max(0) as u32
}

impl Audio {
    /// None when no audio device is available; the game runs silently.
    pub fn open(files: &Storage) -> Option<Audio> {
        // Ours: no sound device in a headless server process (view::forbid_windows), nor with
        // SCBW_MUTE set (tests): sound is simply off, the game is unchanged.
        crate::view::window_allowed().ok()?;
        if std::env::var_os("SCBW_MUTE").is_some() {
            return None;
        }
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().ok()?;
        sink.log_on_drop(false);
        let names = dat::parse_tbl(&files.read("arr/sfxdata.tbl").ok()?);
        Some(Audio { sink, names, cache: RefCell::new(HashMap::new()), music: None, music_track: 0, music_checked: 0, volumes: (25, 50), credits_volume: None, fade_in: None, music_started: true, speech_muted: false, music_duck: false,
            table: volume_table(), slots: RefCell::new(Default::default()), last: RefCell::new(HashMap::new()), files: RefCell::new(Vec::new()) })
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

    /// 0x4bbf50: the playing sounds (not the ones the loader still holds, flag 8) stop and
    /// their slots free.
    pub fn stop_sounds(&self) {
        self.stop_files();
        for slot in self.slots.borrow_mut().iter_mut() {
            if slot.as_ref().is_some_and(|s| s.flags & 8 == 0) {
                if let Some(s) = slot.take() {
                    s.player.stop();
                }
            }
        }
    }

    /// 0x4015f0: every wav file playing (0x4bc490) stops and leaves the list.
    pub fn stop_files(&self) {
        for p in self.files.borrow_mut().drain(..) {
            p.stop();
        }
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
        let now = now_ms();
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

    /// The linear gain of the digital volume's table attenuation.
    fn digital_gain(&self) -> f32 {
        let atten = self.table[(self.volumes.1 as i32 * 99 / 100).clamp(0, 99) as usize];
        if atten <= -10000 { 0.0 } else { 10f32.powf(atten as f32 / 2000.0) }
    }

    /// A player at the digital volume for a cinematic's sound (Storm's SVid plays it through
    /// DirectSound); the caller appends the decoded chunks and drops it to stop.
    pub fn video_player(&self) -> rodio::Player {
        let player = rodio::Player::connect_new(self.sink.mixer());
        player.set_volume(self.digital_gain());
        player
    }

    /// 0x4bc490: plays a whole wav file (a trigger's, from the map archive) at the digital
    /// volume's table attenuation, `table[digital * 99 / 100]`; no slot or priority. It is kept
    /// in the list 0x51a208 (`stop_files`); the ones that ended are dropped here.
    pub fn play_file(&self, data: Vec<u8>) {
        let Ok(src) = rodio::Decoder::new(Cursor::new(Arc::<[u8]>::from(data))) else { return };
        let g = self.digital_gain();
        let player = rodio::Player::connect_new(self.sink.mixer());
        player.append(src.amplify(g));
        let mut files = self.files.borrow_mut();
        files.retain(|p| !p.empty());
        files.push(player);
    }

    /// 0x4bc320: applies the volumes. Music 0 (muted): attenuation -10000 and the stream is
    /// stopped, the started flag 0x6d638c cleared; else the stream gets the volume's table level
    /// (`table[music - 1]` = the `music * 99 / 100` index), and the first time after a mute the
    /// track starts with its fade-in (0x4a6310). The digital volume (0x6d5e3c) is read from
    /// `volumes` where used (`digital_gain`).
    // The muted stream is kept at volume 0 instead of closed: 0x4a6310 (restarting the track) is not ported.
    pub fn set_volumes(&mut self, music: i16, digital: i16) {
        self.volumes = (music, digital);
        if music == 0 {
            if let Some(p) = self.music.as_ref() {
                p.set_volume(0.0);
            }
            self.music_started = false;
        } else {
            if self.fade_in.is_none() {
                if let Some(p) = self.music.as_ref() {
                    p.set_volume(music as f32 / 50.0);
                }
                // 0x4a5ef0's level (ducked by 0x48ebc0) as a ratio to the volume's own.
                self.set_fade_volume(self.music_level());
            }
            if !self.music_started {
                self.music_started = true;
                self.start_fade_in();
            }
        }
    }

    /// 0x4d8fd0 / 0x4d90c0 (`on`): with the music volume 0 and the digital volume on, the music
    /// volume is 0x32 while the credits run; false gives it back.
    pub fn credits_volume(&mut self, on: bool) {
        if on {
            if self.volumes.0 == 0 && self.volumes.1 != 0 {
                self.music = None;
                self.credits_volume = Some(0);
                self.volumes.0 = 0x32;
            }
        } else if let Some(v) = self.credits_volume.take() {
            self.volumes.0 = v;
        }
    }

    /// 0x4a6310 with the track's fade-in flag (0x512bbd) and the music volume on: the attenuation
    /// starts at 0xfffff2bc (-3396), and 0x4a6030 (every 20 ms, `tick_fade`) raises it by 400
    /// until the volume's own level.
    pub fn start_fade_in(&mut self) {
        if self.volumes.0 != 0 && self.music.is_some() {
            self.fade_in = Some((-3396, now_ms()));
            self.set_fade_volume(-3396);
        }
    }

    /// 0x4a5ef0: the music's attenuation: the volume's table level, minus 0x2ee (not below
    /// -10000) once 0x48ebc0 ducked it.
    fn music_level(&self) -> i32 {
        let own = self.table[(self.volumes.0 as i32 * 99 / 100).clamp(0, 99) as usize];
        if self.music_duck { (own - 0x2ee).max(-10000) } else { own.max(-10000) }
    }

    /// 0x48ebc0 (the trigger action 0x4c6990, mute unit speech): sets the speech flag 0x64086c
    /// and, the first time (latch 0x6d5bbd), ducks the playing music by 0x2ee.
    pub fn mute_unit_speech(&mut self) {
        self.speech_muted = true;
        if !self.music_duck {
            self.music_duck = true;
            self.set_fade_volume(self.music_level());
        }
    }

    /// 0x4c6940 (the trigger action unmute unit speech): clears 0x64086c; when the music was
    /// ducked (0x6d5bbd) the latch clears and a playing track (0x6d5bac) gets its volume
    /// 0x6d5bb4 back (0x410268).
    pub fn unmute_unit_speech(&mut self) {
        self.speech_muted = false;
        if self.music_duck {
            self.music_duck = false;
            self.set_fade_volume(self.music_level());
        }
    }

    /// 0x64086c: unit speech is muted (0x48ed50 / 0x48ec10 scale the volume by it).
    pub fn speech_muted(&self) -> bool {
        self.speech_muted
    }

    /// 0x48fb40's resets (before the 0x4bccf0 glue sound init): the flag 0x64086c and the
    /// unit sound state 0x640870 .. 0x64087c (not modelled) go to 0; the id tables
    /// 0x640550 / 0x63ff78 / 0x6402b8 (once, 0x6d5bdc) are filled from the sfx data on demand.
    pub fn reset_sound_tables(&mut self) {
        self.speech_muted = false;
    }

    /// The music volume at attenuation `level`: the volume's own level is 1.0 of it.
    fn set_fade_volume(&self, level: i32) {
        let own = self.table[(self.volumes.0 as i32 * 99 / 100).clamp(0, 99) as usize].max(-10000);
        if let Some(p) = self.music.as_ref() {
            p.set_volume(self.volumes.0 as f32 / 50.0 * 10f32.powf((level - own).min(0) as f32 / 2000.0));
        }
    }

    /// 0x4a6030: the fade-in timer, run for the time that passed.
    pub fn tick_fade(&mut self) {
        let Some((mut level, mut last)) = self.fade_in else { return };
        let now = now_ms();
        let own = self.music_level();
        while now >= last + 20 {
            last += 20;
            level += 400;
            if own <= level {
                self.fade_in = None;
                self.set_fade_volume(own);
                return;
            }
        }
        self.fade_in = Some((level, last));
        self.set_fade_volume(level);
    }

    pub fn play_music(&mut self, files: &Storage, path: &str) {
        let Ok(data) = files.read(path) else { return };
        let Ok(src) = rodio::Decoder::new_looped(Cursor::new(Arc::<[u8]>::from(data))) else { return };
        let player = rodio::Player::connect_new(self.sink.mixer());
        player.set_volume(self.volumes.0 as f32 / 50.0);
        player.append(src);
        self.music = Some(player);
    }

    /// 0x4a6140 for the in-game tracks 1..=9 of 0x512bb8 (type 2: played once; the fade-in
    /// flag 0x512bbd is 0 for all of them): closes the playing track when `i` differs.
    pub fn start_track(&mut self, files: &Storage, i: u32) {
        if self.music.is_some() && i == self.music_track {
            return;
        }
        self.music_track = i;
        self.music_duck = false; // 0x4a6140 clears 0x6d5bbd when it starts the stream (0x4a5e70 false)
        self.music = None; // 0x4a6140: Ordinal_257 / 253 close the old stream
        let Some((path, _)) = game_track(i) else { return };
        let Ok(data) = files.read(path) else { return };
        let Ok(src) = rodio::Decoder::new(Cursor::new(Arc::<[u8]>::from(data))) else { return };
        let player = rodio::Player::connect_new(self.sink.mixer());
        player.set_volume(self.volumes.0 as f32 / 50.0);
        player.append(src);
        self.music = Some(player);
    }

    /// 0x6d5bb0: the track playing.
    pub fn music_track(&self) -> u32 {
        self.music_track
    }

    /// 0x4a6250 (run once a second by 0x4d1c90 via 0x4d1d05): when the playing type 2
    /// track has ended (Ordinal_258: position >= length), 0x4a6140 starts the table's next
    /// track (byte +6 of the entry).
    pub fn music_check(&mut self, files: &Storage) {
        let now = now_ms();
        if now < self.music_checked + 1000 {
            return;
        }
        self.music_checked = now;
        let Some((_, next)) = game_track(self.music_track) else { return };
        if self.music.as_ref().is_some_and(|p| p.empty()) {
            self.start_track(files, next);
        }
    }

    /// 0x4bc100 (`stop_sounds`, then this) / 0x4a5f50: fade the music out (attenuation falls 5, 10, 15, ... per 20 ms step
    /// from the current level until below -4000), then close it (0x4a5e40).
    pub fn fade_out(&mut self) {
        let Some(p) = self.music.take() else { return };
        let mut level = self.music_level();
        let mut step = 5;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(20));
            level -= step;
            if level < -4000 {
                break;
            }
            step += 5;
            p.set_volume(10f32.powf(level as f32 / 2000.0));
        }
        p.stop();
    }
}

/// The length of sound `id` (sfxdata.dat's file, sfxdata.tbl's name), without a sound
/// output: a game played without one (scripted, `scbw glue <out.png>`) times the talking
/// portraits the same.
pub fn duration_ms(files: &Storage, dat: &Dat, id: u16) -> Option<u32> {
    let file = *dat.sfxdata.file.get(id as usize)? as usize;
    let names = dat::parse_tbl(&files.read("arr/sfxdata.tbl").ok()?);
    let name = names.get(file.checked_sub(1)?)?;
    let wav = files.read(&format!("sound/{}", name.replace('\\', "/"))).ok()?;
    let src = rodio::Decoder::new(Cursor::new(wav)).ok()?;
    Some(src.total_duration()?.as_millis() as u32)
}
