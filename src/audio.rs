//! Sound effects (sfxdata.dat -> sound/*.wav) and music, via rodio.

use crate::casc::Casc;
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
    pub fn open(casc: &Casc) -> Option<Audio> {
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().ok()?;
        sink.log_on_drop(false);
        let names = dat::parse_tbl(&casc.read("arr/sfxdata.tbl").ok()?);
        Some(Audio { sink, names, cache: RefCell::new(HashMap::new()), music: None })
    }

    fn wav(&self, casc: &Casc, dat: &Dat, id: u16) -> Option<Arc<[u8]>> {
        self.cache
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| {
                let file = *dat.sfxdata.file.get(id as usize)? as usize;
                let name = self.names.get(file.checked_sub(1)?)?;
                casc.read(&format!("sound/{}", name.replace('\\', "/"))).ok().map(Arc::from)
            })
            .clone()
    }

    /// `pan_volume`: 0.0..=1.0 attenuation for distance from the screen.
    pub fn play(&self, casc: &Casc, dat: &Dat, id: u16, pan_volume: f32) {
        let Some(data) = self.wav(casc, dat, id) else { return };
        let vol = dat.sfxdata.volume.get(id as usize).copied().unwrap_or(100).max(1) as f32 / 100.0;
        if let Ok(src) = rodio::Decoder::new(Cursor::new(data)) {
            self.sink.mixer().add(src.amplify(vol * pan_volume));
        }
    }

    pub fn play_music(&mut self, casc: &Casc, path: &str) {
        let Ok(data) = casc.read(path) else { return };
        let Ok(src) = rodio::Decoder::new_looped(Cursor::new(Arc::<[u8]>::from(data))) else { return };
        let player = rodio::Player::connect_new(self.sink.mixer());
        player.set_volume(0.5);
        player.append(src);
        self.music = Some(player);
    }
}
