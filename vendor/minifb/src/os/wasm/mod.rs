// NOTE(stefano): add missing implementations of methods where possible
#![cfg(target_arch = "wasm32")]

mod keycodes;

use crate::{
    check_buffer_size, key_handler::KeyHandler, CursorStyle, Icon, InputCallback, Key, KeyRepeat,
    MenuHandle, MenuItem, MenuItemHandle, MouseButton, MouseMode, Result, Scale, UnixMenu,
    UnixMenuItem, WindowOptions,
};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WebDisplayHandle, WebWindowHandle, WindowHandle,
};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    rc::Rc,
    time::Duration,
};
use wasm_bindgen::{prelude::*, Clamped, JsCast};
use web_sys::{ImageData, OffscreenCanvas, OffscreenCanvasRenderingContext2d};

// scbw (vendor/minifb): this backend runs in a Web Worker. The page hands over its canvas as
// an OffscreenCanvas (`minifb_set_canvas`) and forwards keyboard, mouse and focus events
// (`minifb_key`, `minifb_mouse`, `minifb_focus`); every window draws into that one canvas
// and the input goes to the window drawn last. No DOM is used here.
thread_local! {
    static CANVAS: RefCell<Option<OffscreenCanvas>> = const { RefCell::new(None) };
    static CURRENT: RefCell<Option<(Rc<RefCell<KeyHandler>>, Rc<MouseState>)>> = const { RefCell::new(None) };
    static FOCUSED: Cell<bool> = const { Cell::new(true) };
}

/// The page's canvas, transferred to this worker (`transferControlToOffscreen`).
#[wasm_bindgen]
pub fn minifb_set_canvas(canvas: OffscreenCanvas) {
    CANVAS.with(|c| *c.borrow_mut() = Some(canvas));
}

/// A key went down or up on the page's canvas (`KeyboardEvent.code` / `.key`, modifiers).
/// A key down also types its character, as Windows' WM_CHAR would (none with Alt / Meta:
/// WM_SYSCHAR; Ctrl+letter is the control code).
#[wasm_bindgen]
pub fn minifb_key(down: bool, code: &str, key: &str, ctrl: bool, alt: bool, meta: bool) {
    let Some((kh, _)) = CURRENT.with(|c| c.borrow().clone()) else { return };
    let mut kh = kh.borrow_mut();
    kh.set_key_state(keycodes::code_to_key(code), down);
    if !down || alt || meta {
        return;
    }
    let mut it = key.chars();
    let c = match (it.next(), it.next()) {
        (Some(c), None) if ctrl && c.is_ascii_alphabetic() => c.to_ascii_lowercase() as u32 & 0x1f,
        (Some(c), None) if !ctrl => c as u32,
        _ => match key {
            "Enter" => 13,
            "Backspace" => 8,
            "Escape" => 27,
            "Tab" => 9,
            _ => return,
        },
    };
    if let Some(cb) = kh.key_callback.as_mut() {
        cb.add_char(c);
    }
}

/// The mouse on the page's canvas, in the window's scaled space (width * scale):
/// `event` 0 move, 1 a button down, 2 a button up (`button` as `MouseEvent.button`).
#[wasm_bindgen]
pub fn minifb_mouse(event: u32, x: i32, y: i32, button: i32) {
    let Some((_, ms)) = CURRENT.with(|c| c.borrow().clone()) else { return };
    ms.pos.set(Some((x, y)));
    if event == 0 {
        return;
    }
    let down = event == 1;
    match button {
        0 => ms.left_button.set(down),
        1 => ms.middle_button.set(down),
        2 => ms.right_button.set(down),
        3 => ms.back_button.set(down),
        4 => ms.forward_button.set(down),
        _ => (),
    }
}

/// The page's canvas has the keyboard focus (`is_active`).
#[wasm_bindgen]
pub fn minifb_focus(on: bool) {
    FOCUSED.with(|f| f.set(on));
}

#[inline(always)]
#[allow(dead_code)] // Only used on 32-bit builds currently
pub fn u32_as_u8<'a>(src: &'a [u32]) -> &'a [u8] {
    unsafe {
        std::slice::from_raw_parts(
            src.as_ptr() as *mut u8,
            src.len() * std::mem::size_of::<u32>(),
        )
    }
}

struct MouseState {
    pos: Cell<Option<(i32, i32)>>,
    //scroll: Cell<Option<(i32, i32)>>,
    left_button: Cell<bool>,
    right_button: Cell<bool>,
    middle_button: Cell<bool>,
    back_button: Cell<bool>,
    forward_button: Cell<bool>,
}

struct Context2D {
    context: OffscreenCanvasRenderingContext2d,
    img_data: ImageData,
}

// IDEA(stefano): possibly have this contain a "document" field, so not to recompute it every time
pub struct Window {
    width: u32,
    height: u32,
    bg_color: u32,
    window_scale: usize,
    canvas: OffscreenCanvas,
    // 2D context is created lazily since its creation precludes creation of webgl & webgpu contexts.
    context2d: Option<Context2D>,
    mouse_state: Rc<MouseState>,
    key_handler: Rc<RefCell<KeyHandler>>,
    menu_counter: MenuHandle,
    menus: Vec<UnixMenu>,
    raw_handle_id: u32,
}

impl Window {
    /// Create a new window
    /// * `container` - The id of a DOM element to use as a container
    pub fn new(
        container: &str,
        width: usize,
        height: usize,
        opts: WindowOptions,
    ) -> Result<Window> {
        let window_scale = match opts.scale {
            Scale::X1 => 1,
            Scale::X2 => 2,
            Scale::X4 => 4,
            Scale::X8 => 8,
            Scale::X16 => 16,
            Scale::X32 => 32,
            Scale::FitScreen => 1, //TODO: Resize the canvas and implement this
        };
        let _ = container;
        let canvas = CANVAS
            .with(|c| c.borrow().clone())
            .ok_or_else(|| crate::Error::WindowCreate("no canvas from the page (minifb_set_canvas)".into()))?;
        canvas.set_width(width as u32);
        canvas.set_height(height as u32);
        let raw_handle_id = 0;
        let key_handler = Rc::new(RefCell::new(KeyHandler::new()));
        let mouse_state = Rc::new(MouseState {
            pos: Cell::new(None),
            left_button: Cell::new(false),
            right_button: Cell::new(false),
            middle_button: Cell::new(false),
            back_button: Cell::new(false),
            forward_button: Cell::new(false),
        });
        CURRENT.with(|c| *c.borrow_mut() = Some((key_handler.clone(), mouse_state.clone())));

        Ok(Window {
            width: width as u32,
            height: height as u32,
            bg_color: 0,
            window_scale,
            canvas,
            context2d: None,
            key_handler,
            mouse_state,
            menu_counter: MenuHandle(0),
            menus: Vec::new(),
            raw_handle_id,
        })
    }

    fn create_2d_context(&self) -> OffscreenCanvasRenderingContext2d {
        let context: OffscreenCanvasRenderingContext2d = self
            .canvas
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<OffscreenCanvasRenderingContext2d>()
            .unwrap();
        context.set_image_smoothing_enabled(false);
        context
    }

    #[inline]
    pub fn set_title(&mut self, _title: &str) {}

    #[inline]
    pub fn set_rate(&mut self, _rate: Option<Duration>) {}

    #[inline]
    pub fn get_delta_time(&self) -> Option<Duration> {
        None
    }

    #[inline]
    pub fn update_rate(&mut self) {}

    #[inline]
    pub fn get_window_handle(&self) -> *mut c_void {
        std::ptr::null_mut()
    }

    #[inline]
    pub fn topmost(&self, _topmost: bool) {
        // TODO?
    }

    #[inline]
    pub fn set_background_color(&mut self, bg_color: u32) {
        self.bg_color = bg_color;
    }

    #[inline]
    pub fn set_cursor_visibility(&mut self, _visibility: bool) {
        // The page's canvas has no cursor (the game draws its own).
    }


    pub fn update_with_buffer_stride(
        &mut self,
        buffer: &[u32],
        buf_width: usize,
        buf_height: usize,
        buf_stride: usize,
    ) -> Result<()> {
        check_buffer_size(buffer, buf_width, buf_height, buf_stride)?;
        // scaling not implemented. It's faster to just update the buffer
        //unsafe { self.scale_buffer(buffer, buf_width, buf_height, buf_stride) };
        self.update_with_buffer(&buffer).unwrap();

        Ok(())
    }

    pub fn update_with_buffer(&mut self, buffer: &[u32]) -> Result<()> {
        check_buffer_size(
            buffer,
            self.width as usize,
            self.height as usize,
            self.window_scale,
        )?;
        let mut data = u32_as_u8(buffer);

        let image_data = ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(&mut data),
            self.width,
            self.height,
        )
        .unwrap();

        if let Some(context2d) = &mut self.context2d {
            context2d.img_data = image_data;
        } else {
            self.context2d = Some(Context2D {
                context: self.create_2d_context(),
                img_data: image_data,
            });
        }

        self.update();

        Ok(())
    }

    #[inline]
    pub fn update(&mut self) {
        self.key_handler.borrow_mut().update();
        // The window drawn last gets the input; another window may have drawn in between.
        CURRENT.with(|c| *c.borrow_mut() = Some((self.key_handler.clone(), self.mouse_state.clone())));
        if self.canvas.width() != self.width || self.canvas.height() != self.height {
            self.canvas.set_width(self.width);
            self.canvas.set_height(self.height);
            if let Some(c) = &self.context2d {
                c.context.set_image_smoothing_enabled(false);
            }
        }

        if let Some(context2d) = &self.context2d {
            context2d
                .context
                .put_image_data(&context2d.img_data, 0.0, 0.0)
                .unwrap();
        }
    }

    #[inline]
    pub fn set_icon(&mut self, _icon: Icon) {}

    #[inline]
    pub fn set_position(&mut self, _x: isize, _y: isize) {}

    #[inline]
    pub fn get_position(&self) -> (isize, isize) {
        (0isize, 0isize)
    }

    #[inline]
    pub fn get_size(&self) -> (usize, usize) {
        (self.width as usize, self.height as usize)
    }

    #[inline]
    pub fn get_mouse_pos(&self, mode: MouseMode) -> Option<(f32, f32)> {
        if let Some((mouse_x, mouse_y)) = self.mouse_state.pos.get() {
            mode.get_pos(
                mouse_x as f32,
                mouse_y as f32,
                self.window_scale as f32,
                self.width as f32 * self.window_scale as f32,
                self.height as f32 * self.window_scale as f32,
            )
        } else {
            None
        }
    }

    #[inline]
    pub fn get_unscaled_mouse_pos(&self, mode: MouseMode) -> Option<(f32, f32)> {
        if let Some((mouse_x, mouse_y)) = self.mouse_state.pos.get() {
            mode.get_pos(
                mouse_x as f32,
                mouse_y as f32,
                1.0 as f32,
                self.width as f32 * self.window_scale as f32,
                self.height as f32 * self.window_scale as f32,
            )
        } else {
            None
        }
    }

    #[inline]
    pub fn get_mouse_down(&self, button: MouseButton) -> bool {
        match button {
            MouseButton::Left => self.mouse_state.left_button.get(),
            MouseButton::Middle => self.mouse_state.middle_button.get(),
            MouseButton::Right => self.mouse_state.right_button.get(),
            MouseButton::Back => self.mouse_state.back_button.get(),
            MouseButton::Forward => self.mouse_state.forward_button.get(),
        }
    }

    #[inline]
    pub fn get_scroll_wheel(&self) -> Option<(f32, f32)> {
        None
    }

    #[inline]
    pub fn set_cursor_style(&mut self, _cursor: CursorStyle) {}

    #[inline]
    pub fn get_keys(&self) -> Vec<Key> {
        self.key_handler.borrow().get_keys()
    }

    #[inline]
    pub fn get_keys_pressed(&self, repeat: KeyRepeat) -> Vec<Key> {
        self.key_handler.borrow().get_keys_pressed(repeat)
    }

    #[inline]
    pub fn is_key_down(&self, key: Key) -> bool {
        self.key_handler.borrow().is_key_down(key)
    }

    #[inline]
    pub fn set_key_repeat_delay(&mut self, delay: f32) {
        self.key_handler.borrow_mut().set_key_repeat_delay(delay)
    }

    #[inline]
    pub fn set_key_repeat_rate(&mut self, rate: f32) {
        self.key_handler.borrow_mut().set_key_repeat_rate(rate)
    }

    #[inline]
    pub fn is_key_pressed(&self, key: Key, repeat: KeyRepeat) -> bool {
        self.key_handler.borrow().is_key_pressed(key, repeat)
    }

    #[inline]
    pub fn is_key_released(&self, key: Key) -> bool {
        self.key_handler.borrow().is_key_released(key)
    }

    #[inline]
    pub fn set_input_callback(&mut self, callback: Box<dyn InputCallback>) {
        self.key_handler.borrow_mut().set_input_callback(callback)
    }

    #[inline]
    pub fn is_open(&self) -> bool {
        true
    }

    #[inline]
    pub fn get_keys_released(&self) -> Vec<Key> {
        self.key_handler.borrow().get_keys_released()
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        FOCUSED.with(|f| f.get())
    }

    #[inline]
    fn next_menu_handle(&mut self) -> MenuHandle {
        let handle = self.menu_counter;
        self.menu_counter.0 += 1;
        handle
    }

    #[inline]
    pub fn add_menu(&mut self, menu: &Menu) -> MenuHandle {
        let handle = self.next_menu_handle();
        let mut menu = menu.internal.clone();
        menu.handle = handle;
        self.menus.push(menu);
        handle
    }

    #[inline]
    pub fn remove_menu(&mut self, handle: MenuHandle) {
        self.menus.retain(|ref menu| menu.handle != handle);
    }

    #[inline]
    pub fn is_menu_pressed(&mut self) -> Option<usize> {
        None
    }
}

pub struct Menu {
    pub internal: UnixMenu,
}

impl Menu {
    pub fn new(name: &str) -> Result<Menu> {
        Ok(Menu {
            internal: UnixMenu {
                handle: MenuHandle(0),
                item_counter: MenuItemHandle(0),
                name: name.to_owned(),
                items: Vec::new(),
            },
        })
    }

    #[inline]
    pub fn add_sub_menu(&mut self, _name: &str, _sub_menu: &Menu) {}

    #[inline]
    fn next_item_handle(&mut self) -> MenuItemHandle {
        let handle = self.internal.item_counter;
        self.internal.item_counter.0 += 1;
        handle
    }

    #[inline]
    pub fn add_menu_item(&mut self, item: &MenuItem) -> MenuItemHandle {
        let item_handle = self.next_item_handle();
        self.internal.items.push(UnixMenuItem {
            sub_menu: None,
            handle: self.internal.item_counter,
            id: item.id,
            label: item.label.clone(),
            enabled: item.enabled,
            key: item.key,
            modifier: item.modifier,
        });
        item_handle
    }

    #[inline]
    pub fn remove_item(&mut self, _handle: &MenuItemHandle) {}
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let handle = WebWindowHandle::new(self.raw_handle_id);
        let raw_handle = RawWindowHandle::Web(handle);
        unsafe { Ok(WindowHandle::borrow_raw(raw_handle)) }
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> std::result::Result<DisplayHandle<'_>, HandleError> {
        let handle = WebDisplayHandle::new();
        let raw_handle = RawDisplayHandle::Web(handle);
        unsafe { Ok(DisplayHandle::borrow_raw(raw_handle)) }
    }
}
