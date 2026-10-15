//! Browser entry (web/build.sh, web/index.html): the page hands over storm.dll, the
//! archives and a map or replay; the frame loop runs from requestAnimationFrame.

use crate::{map, mpq, replay, storage, view};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

fn js(e: std::io::Error) -> JsError {
    JsError::new(&e.to_string())
}

/// `archives`: Uint8Arrays in storage::ARCHIVES order. `name` picks how `file` is read
/// (.rep replay, .chk, else a .scm/.scx archive).
#[wasm_bindgen]
pub fn start(storm: &[u8], archives: js_sys::Array, name: &str, file: &[u8]) -> Result<(), JsError> {
    console_error_panic_hook::set_once();
    let archives = archives.iter().map(|a| js_sys::Uint8Array::new(&a).to_vec()).collect();
    let files = storage::Storage::from_mem(storm, archives).map_err(js)?;
    // The frame loop outlives this call, so the assets live for the page's lifetime.
    let a: &'static mut view::Assets = Box::leak(Box::new(view::Assets::load(files).map_err(js)?));
    let name = name.to_ascii_lowercase();
    let mut frame = if name.ends_with(".rep") {
        let rep = replay::Replay::parse(file).map_err(js)?;
        view::prepare_replay(a, &rep);
        let a: &'static view::Assets = a;
        let scene = view::Scene::from_replay(a, &rep).map_err(js)?;
        Box::new(view::frame_loop(a, scene, Some(view::ReplayPlay::new(rep))).map_err(js)?) as Box<dyn FnMut() -> std::io::Result<bool>>
    } else {
        let chk = if name.ends_with(".chk") { file.to_vec() } else { mpq::Mpq::new(file.to_vec()).and_then(|m| m.read("staredit\\scenario.chk")).map_err(js)? };
        let chk = map::Chk::parse(&chk);
        view::prepare_map(a, &chk);
        let a: &'static view::Assets = a;
        Box::new(view::frame_loop(a, view::Scene::new(a, chk).map_err(js)?, None).map_err(js)?)
    };
    // requestAnimationFrame runs at the display rate; step at the native 24 fps
    // (set_target_fps) instead.
    let win = web_sys::window().unwrap();
    let perf = win.performance().unwrap();
    let mut next = perf.now();
    let tick: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let again = tick.clone();
    *tick.borrow_mut() = Some(Closure::new(move || {
        let now = perf.now();
        if now >= next {
            next = (next + 1000.0 / 24.0).max(now - 100.0);
            match frame() {
                Ok(true) => {}
                Ok(false) => return,
                Err(e) => return web_sys::console::error_1(&e.to_string().into()),
            }
        }
        let w = web_sys::window().unwrap();
        w.request_animation_frame(again.borrow().as_ref().unwrap().as_ref().unchecked_ref()).unwrap();
    }));
    win.request_animation_frame(tick.borrow().as_ref().unwrap().as_ref().unchecked_ref()).unwrap();
    Ok(())
}
