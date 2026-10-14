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
}

impl Audio {
    /// None when no audio device is available; the game runs silently.
    pub fn open(files: &Storage) -> Option<Audio> {
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().ok()?;
        sink.log_on_drop(false);
        let names = dat::parse_tbl(&files.read("arr/sfxdata.tbl").ok()?);
        Some(Audio { sink, names, cache: RefCell::new(HashMap::new()), music: None })
    }

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
        let Some(data) = self.wav(files, dat, id) else { return };
        if let Ok(src) = rodio::Decoder::new(Cursor::new(data)) {
            self.sink.mixer().add(src.amplify(vol / 100.0));
        }
    }

    pub fn play_music(&mut self, files: &Storage, path: &str) {
        let Ok(data) = files.read(path) else { return };
        let Ok(src) = rodio::Decoder::new_looped(Cursor::new(Arc::<[u8]>::from(data))) else { return };
        let player = rodio::Player::connect_new(self.sink.mixer());
        player.set_volume(0.5);
        player.append(src);
        self.music = Some(player);
    }
}
