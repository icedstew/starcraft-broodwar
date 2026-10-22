//! Scripted input (`scbw glue <out.png>` with SCBW_GLUE_INPUT, written by
//! re/record_glue.py from a recording of the real game): one queue of steps for the glue
//! and the game. Glue steps are timed in ms (glue.rs); game steps are "@ frame dt" followed
//! by one action, which applies once the game is at that frame and `dt` ms of it passed:
//! "mv x y" (mouse), "md" / "mu" / "rd" / "ru" (left / right button down / up), "kd key" /
//! "ku key" (minifb key names), "ch code" (a typed character), "s file" (the screen),
//! "cam x y n" (the recorded game's screen position before its shot n), "set speed mouse
//! key" (its game speed and scroll speeds, 0x6cdfd4 / d8 / dc).

use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;
use web_time::Instant;

thread_local! {
    // SCBW_GLUE_INPUT="step;step;...", or SCBW_GLUE_SCRIPT=<file>, a step per line.
    static STEPS: RefCell<VecDeque<String>> = RefCell::new(match std::env::var_os("SCBW_GLUE_SCRIPT") {
        Some(f) => std::fs::read_to_string(f).unwrap_or_default().lines().filter(|s| !s.is_empty()).map(str::to_string).collect(),
        None => std::env::var("SCBW_GLUE_INPUT").map(|v| v.split(';').filter(|s| !s.is_empty()).map(str::to_string).collect()).unwrap_or_default(),
    });
}

pub fn pop() -> Option<String> {
    STEPS.with(|s| s.borrow_mut().pop_front())
}

pub fn push_front(step: String) {
    STEPS.with(|s| s.borrow_mut().push_front(step))
}

fn front() -> Option<String> {
    STEPS.with(|s| s.borrow().front().cloned())
}

pub fn pending() -> bool {
    STEPS.with(|s| !s.borrow().is_empty())
}

/// A game step left for the glue: the recorded game went on longer than ours.
pub fn is_game_step(s: &str) -> bool {
    s.starts_with('@')
}

/// Drops the game steps (gate and action) at the front; how many.
pub fn drop_game_steps() -> usize {
    STEPS.with(|s| {
        let mut s = s.borrow_mut();
        let mut n = 0;
        while s.front().is_some_and(|f| is_game_step(f)) {
            s.pop_front();
            s.pop_front();
            n += 1;
        }
        n
    })
}

/// The keys held, from the window's presses and releases (not its key states): a key
/// pressed or let go while another window has the focus would otherwise stay held, so
/// they are all let go while the window is not active.
#[derive(Default)]
pub struct Held(Vec<Key>);

impl Held {
    pub fn sync(&mut self, w: &mut Window) {
        if !w.is_active() {
            self.0.clear();
            return;
        }
        for k in w.get_keys_pressed(KeyRepeat::No) {
            if !self.0.contains(&k) {
                self.0.push(k);
            }
        }
        let up = w.get_keys_released();
        self.0.retain(|k| !up.contains(k));
    }

    pub fn down(&self, k: Key) -> bool {
        self.0.contains(&k)
    }
}

/// The game loop's input: the window, or the script when there is none.
pub struct Io<W> {
    pub win: Option<W>,
    held: Held,
    s: Scripted,
}

#[derive(Default)]
struct Scripted {
    mouse: (i32, i32),
    left: bool,
    right: bool,
    down: Vec<Key>,
    pressed: Vec<Key>,
    /// Virtual time: 16 ms per pass, as the window's 60 per second.
    ms: u64,
    frame: u32,
    frame_at: u64,
    /// When the last step applied: a stall is counted from the later of the two.
    step_at: u64,
    shot: Option<String>,
    /// A "cam" step's position, held (after the scroll steps of each pass) until its shot.
    camera: Option<(i32, i32, u32)>,
    camera_logged: bool,
    settings: Option<(i16, i16, i16)>,
    done: bool,
}

impl<W: std::ops::DerefMut<Target = Window>> Io<W> {
    pub fn new(win: Option<W>) -> Self {
        Io { win, held: Held::default(), s: Scripted { mouse: (320, 240), ..Default::default() } }
    }

    pub fn now(&self, base: Instant) -> Instant {
        match self.win {
            Some(_) => Instant::now(),
            None => base + Duration::from_millis(self.s.ms),
        }
    }

    /// One pass of the script at game frame `frame`; false once it ran out (or the glue's
    /// steps follow: the recorded game ended).
    pub fn step(&mut self, frame: u32, typed: &RefCell<Vec<u32>>) -> bool {
        if let Some(w) = self.win.as_mut() {
            self.held.sync(w);
            return true;
        }
        let s = &mut self.s;
        s.pressed.clear();
        s.ms += 16;
        if frame != s.frame {
            (s.frame, s.frame_at) = (frame, s.ms);
        }
        let Some(gate) = front() else {
            s.done = true;
            return false;
        };
        let Some((f, dt)) = gate_of(&gate) else {
            s.done = true;
            return false;
        };
        if frame < f && s.ms > s.frame_at.max(s.step_at) + 30_000 {
            // Ours stopped (paused, or a dialog the recorded game didn't have): no use going on.
            eprintln!("script: the game stays at frame {frame}, the next step is at {f}; it ends");
            s.done = true;
            return false;
        }
        if frame < f || s.ms < s.frame_at + dt {
            return true;
        }
        pop();
        s.step_at = s.ms;
        let act = pop().unwrap_or_default();
        let a: Vec<&str> = act.split_whitespace().collect();
        let num = |i: usize| a.get(i).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
        match a.first().copied() {
            Some("mv") => s.mouse = (num(1), num(2)),
            Some("md") => s.left = true,
            Some("mu") => s.left = false,
            Some("rd") => s.right = true,
            Some("ru") => s.right = false,
            Some("kd") => {
                if let Some(k) = a.get(1).and_then(|n| key_named(n)) {
                    s.pressed.push(k);
                    s.down.push(k);
                }
            }
            Some("ku") => {
                if let Some(k) = a.get(1).and_then(|n| key_named(n)) {
                    s.down.retain(|&d| d != k);
                }
            }
            Some("ch") => typed.borrow_mut().push(num(1) as u32),
            Some("s") => s.shot = a.get(1).map(|p| p.to_string()),
            Some("set") => s.settings = Some((num(1) as i16, num(2) as i16, num(3) as i16)),
            Some("cam") => (s.camera, s.camera_logged) = (Some((num(1), num(2), num(3) as u32)), false),
            _ => eprintln!("script: unknown game step {act:?}"),
        }
        true
    }

    /// A scripted "cam" step: the screen goes to the recorded game's (x, y); a different
    /// one is logged for the report as "script: cam <shot> <ours> <recorded>".
    pub fn camera(&mut self, v: &mut (i32, i32)) {
        if let Some((x, y, n)) = self.s.camera {
            if *v != (x, y) && !self.s.camera_logged {
                eprintln!("script: cam {n} {} {} {x} {y}", v.0, v.1);
            }
            self.s.camera_logged = true;
            *v = (x, y);
        }
    }

    /// A scripted "set" step: game speed, mouse and key scroll speeds.
    pub fn take_settings(&mut self) -> Option<(i16, i16, i16)> {
        self.s.settings.take()
    }

    /// The game frames a tick may step: none past the frame of the next scripted event.
    pub fn cap_steps(&self, steps: u32, frame: u32) -> u32 {
        if self.win.is_some() {
            return steps;
        }
        match front().as_deref().and_then(gate_of) {
            Some((f, _)) => steps.min(f.saturating_sub(frame)),
            None => steps,
        }
    }

    pub fn is_open(&self) -> bool {
        self.win.as_ref().map_or(!self.s.done, |w| w.is_open())
    }

    pub fn is_active(&mut self) -> bool {
        self.win.as_mut().is_none_or(|w| w.is_active())
    }

    pub fn get_mouse_pos(&self, mode: MouseMode) -> Option<(f32, f32)> {
        match &self.win {
            Some(w) => w.get_mouse_pos(mode),
            None => Some((self.s.mouse.0 as f32, self.s.mouse.1 as f32)),
        }
    }

    pub fn get_mouse_down(&self, b: MouseButton) -> bool {
        match &self.win {
            Some(w) => w.get_mouse_down(b),
            None => match b {
                MouseButton::Left => self.s.left,
                MouseButton::Right => self.s.right,
                _ => false,
            },
        }
    }

    pub fn is_key_down(&self, k: Key) -> bool {
        match &self.win {
            Some(_) => self.held.down(k),
            None => self.s.down.contains(&k),
        }
    }

    pub fn get_keys_pressed(&self, r: KeyRepeat) -> Vec<Key> {
        match &self.win {
            Some(w) => w.get_keys_pressed(r),
            None => self.s.pressed.clone(),
        }
    }

    /// Shows the frame, or writes it for a scripted "s" step.
    pub fn present(&mut self, rgb: &[u32], w: usize, h: usize) -> std::io::Result<()> {
        match self.win.as_mut() {
            Some(win) => crate::view::present(win, rgb, w, h),
            None => match self.s.shot.take() {
                Some(p) => {
                    self.s.camera = None;
                    crate::write_png_rgb(&p, w, h, rgb)
                }
                None => Ok(()),
            },
        }
    }
}

fn gate_of(s: &str) -> Option<(u32, u64)> {
    let mut it = s.strip_prefix('@')?.split_whitespace();
    Some((it.next()?.parse().ok()?, it.next().and_then(|d| d.parse().ok()).unwrap_or(0)))
}

/// A minifb key by its name (`Key`'s variant).
pub fn key_named(n: &str) -> Option<Key> {
    macro_rules! keys {
        ($($k:ident)*) => {
            match n { $(stringify!($k) => Some(Key::$k),)* _ => None }
        };
    }
    keys!(A B C D E F G H I J K L M N O P Q R S T U V W X Y Z
        Key0 Key1 Key2 Key3 Key4 Key5 Key6 Key7 Key8 Key9
        F1 F2 F3 F4 F5 F6 F7 F8 F9 F10 F11 F12
        Escape Enter Space Tab Backspace Insert Delete Home End PageUp PageDown
        Left Right Up Down LeftShift RightShift LeftCtrl RightCtrl LeftAlt RightAlt
        Minus Equal Comma Period Slash Semicolon Apostrophe LeftBracket RightBracket Backslash Backquote
        NumPad0 NumPad1 NumPad2 NumPad3 NumPad4 NumPad5 NumPad6 NumPad7 NumPad8 NumPad9
        NumPadEnter NumPadPlus NumPadMinus NumPadAsterisk NumPadSlash NumPadDot Pause)
}
