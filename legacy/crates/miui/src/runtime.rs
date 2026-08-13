//! winit + softbuffer によるランタイム。
//!
//! ネイティブ (Windows / macOS / Linux) と Web (wasm) で完全に同じ経路を通る。
//! ウィンドウから受け取ったピクセルバッファへ [`miui_render::Canvas`] が
//! 直接描き込むだけなので、GPU コンテキストもプラットフォーム固有の
//! 描画 API も必要ない。

use std::collections::HashSet;
use std::num::NonZeroU32;
use std::rc::Rc;

use miui_core::event::{Event, Key, Modifiers, MouseButton};
use miui_core::geometry::{Point, Size};
use miui_core::layout::BoxConstraints;
use miui_core::widget::{CursorIcon, EventCx, Interaction, LayoutCx, PaintCx, StateStore};
use miui_render::{Canvas, Fonts};

use winit::application::ApplicationHandler;
#[cfg(not(target_arch = "wasm32"))]
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::{Window, WindowId};

use crate::app::{Application, Environment, Settings};

/// 1 行スクロールあたりの移動量 (論理ピクセル)。
const LINE_SCROLL: f32 = 40.0;

pub(crate) struct Runtime<A: Application> {
    app: A,
    settings: Settings,
    env: Environment,
    fonts: Fonts,
    store: StateStore,
    interaction: Interaction,
    pending: Vec<Event>,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    _context: Option<softbuffer::Context<Rc<Window>>>,
    cursor: CursorIcon,
    title: Option<String>,
    modifiers: Modifiers,
    pointer: Point,
}

impl<A: Application> Runtime<A> {
    pub(crate) fn new(app: A, settings: Settings) -> Self {
        let mut fonts = Fonts::new();
        if settings.load_system_fonts {
            fonts.load_system_fonts();
        }
        for spec in &settings.fonts {
            fonts.register_bytes_indexed(
                &spec.bytes,
                spec.family,
                spec.weight,
                spec.fallback,
                spec.collection_index,
            );
        }
        Self {
            app,
            settings,
            env: Environment::default(),
            fonts,
            store: StateStore::new(),
            interaction: Interaction::default(),
            pending: Vec::new(),
            window: None,
            surface: None,
            _context: None,
            cursor: CursorIcon::Default,
            title: None,
            modifiers: Modifiers::default(),
            pointer: Point::ZERO,
        }
    }

    fn push(&mut self, event: Event) {
        self.pending.push(event);
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// 1 フレーム分の「組み立て → レイアウト → イベント配送 → 描画」。
    fn redraw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        // Web ではブラウザのビューポートが唯一のサイズ源。
        #[cfg(target_arch = "wasm32")]
        web_canvas::fit_to_viewport(&window);

        let phys = window.inner_size();
        let (Some(pw), Some(ph)) = (NonZeroU32::new(phys.width), NonZeroU32::new(phys.height))
        else {
            return;
        };

        let scale = window.scale_factor() as f32;
        self.env.scale_factor = scale;
        if let Some(t) = window.theme() {
            self.env.color_mode = match t {
                winit::window::Theme::Dark => miui_core::theme::ColorMode::Dark,
                winit::window::Theme::Light => miui_core::theme::ColorMode::Light,
            };
        }

        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        if surface.resize(pw, ph).is_err() {
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };

        let mut theme = self.app.theme(&self.env);
        let logical = Size::new(phys.width as f32 / scale, phys.height as f32 / scale);
        let mut canvas = Canvas::new(
            &mut buffer,
            phys.width as usize,
            phys.height as usize,
            scale,
            &mut self.fonts,
        );

        // 1) ツリーの組み立てとレイアウト。
        let mut tree = self.app.view();
        let mut alive = HashSet::new();
        let mut focusables = Vec::new();
        {
            let mut cx = LayoutCx::new(
                &mut canvas,
                &theme,
                &mut self.store,
                &mut alive,
                &mut focusables,
                scale,
            );
            tree.layout(&mut cx, BoxConstraints::tight(logical));
        }

        // 2) 入力イベントの配送。
        let events: Vec<Event> = self.pending.drain(..).collect();
        let mut messages = Vec::new();
        let mut cursor = CursorIcon::Default;
        for event in events {
            // Tab はツリーへ流さず、ランタイムがフォーカスを移す。
            if let Event::KeyPressed {
                key: Key::Tab,
                modifiers,
            } = &event
            {
                move_focus(&mut self.interaction, &focusables, modifiers.shift);
                continue;
            }
            // ホバーは毎回作り直す。ウィンドウ外へ出たときも消す。
            if matches!(event, Event::PointerMoved(_) | Event::PointerLeft) {
                self.interaction.hovered = None;
            }
            let (handled, ev_cursor) = {
                let mut cx = EventCx::new(
                    &theme,
                    &mut self.interaction,
                    &mut canvas,
                    &mut self.store,
                    &mut messages,
                );
                tree.event(&mut cx, &event, Point::ZERO);
                (cx.is_handled(), cx.cursor())
            };
            if ev_cursor != CursorIcon::Default {
                cursor = ev_cursor;
            }
            // どのウィジェットも受け取らなかったクリックはフォーカスを外す。
            if !handled
                && matches!(
                    event,
                    Event::PointerPressed {
                        button: MouseButton::Left,
                        ..
                    }
                )
            {
                self.interaction.focused = None;
            }
        }

        // 3) メッセージを適用し、必要なら組み立て直す。
        //    テーマ自体がアプリの状態から決まる (テーマ切り替え UI など) ため、
        //    更新後に取り直してからレイアウトし直す。
        if !messages.is_empty() {
            for m in messages {
                self.app.update(m);
            }
            theme = self.app.theme(&self.env);
            tree = self.app.view();
            alive.clear();
            focusables.clear();
            let mut cx = LayoutCx::new(
                &mut canvas,
                &theme,
                &mut self.store,
                &mut alive,
                &mut focusables,
                scale,
            );
            tree.layout(&mut cx, BoxConstraints::tight(logical));
        }
        self.store.retain_alive(&alive);

        // 4) 描画。
        canvas.clear(theme.color.window_bg);
        {
            let mut cx = PaintCx {
                painter: &mut canvas,
                theme: &theme,
                interaction: &self.interaction,
                store: &self.store,
            };
            tree.paint(&mut cx, Point::ZERO);
        }
        drop(canvas);
        let _ = buffer.present();

        // 5) カーソルとタイトルの反映。
        if cursor != self.cursor {
            self.cursor = cursor;
            window.set_cursor(to_winit_cursor(cursor));
        }
        let title = self.app.title();
        if title != self.title {
            if let Some(t) = &title {
                window.set_title(t);
            }
            self.title = title;
        }
    }
}

/// Web でキャンバスをブラウザのビューポートへ追従させる。
#[cfg(target_arch = "wasm32")]
mod web_canvas {
    use std::rc::Rc;
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;
    use winit::dpi::LogicalSize;
    use winit::window::Window;

    fn viewport() -> Option<(f64, f64)> {
        let w = web_sys::window()?;
        let width = w.inner_width().ok()?.as_f64()?;
        let height = w.inner_height().ok()?.as_f64()?;
        Some((width.max(1.0), height.max(1.0)))
    }

    /// キャンバスの大きさをビューポートに合わせる (変化が無ければ何もしない)。
    pub fn fit_to_viewport(window: &Window) {
        let Some((w, h)) = viewport() else {
            return;
        };
        let want = LogicalSize::new(w, h).to_physical(window.scale_factor());
        if window.inner_size() != want {
            let _ = window.request_inner_size(want);
        }
    }

    /// ブラウザのリサイズを購読して再描画を要求する。
    pub fn watch_viewport(window: Rc<Window>) {
        let Some(browser) = web_sys::window() else {
            return;
        };
        let handler = Closure::<dyn FnMut()>::new(move || {
            fit_to_viewport(&window);
            window.request_redraw();
        });
        let _ = browser.add_event_listener_with_callback(
            "resize",
            handler.as_ref().unchecked_ref(),
        );
        // ページと同じ寿命なので解放しない。
        handler.forget();
    }
}

fn move_focus(interaction: &mut Interaction, focusables: &[miui_core::Id], backwards: bool) {
    if focusables.is_empty() {
        interaction.focused = None;
        return;
    }
    let current = interaction
        .focused
        .and_then(|id| focusables.iter().position(|x| *x == id));
    let n = focusables.len();
    let next = match current {
        Some(i) => {
            if backwards {
                (i + n - 1) % n
            } else {
                (i + 1) % n
            }
        }
        None => {
            if backwards {
                n - 1
            } else {
                0
            }
        }
    };
    interaction.focused = Some(focusables[next]);
    interaction.focus_visible = true;
}

fn to_winit_cursor(cursor: CursorIcon) -> winit::window::CursorIcon {
    match cursor {
        CursorIcon::Default => winit::window::CursorIcon::Default,
        CursorIcon::Pointer => winit::window::CursorIcon::Pointer,
        CursorIcon::Text => winit::window::CursorIcon::Text,
        CursorIcon::ResizeHorizontal => winit::window::CursorIcon::EwResize,
        CursorIcon::NotAllowed => winit::window::CursorIcon::NotAllowed,
    }
}

fn map_key(key: &WinitKey) -> Key {
    match key {
        WinitKey::Named(NamedKey::Enter) => Key::Enter,
        WinitKey::Named(NamedKey::Escape) => Key::Escape,
        WinitKey::Named(NamedKey::Backspace) => Key::Backspace,
        WinitKey::Named(NamedKey::Delete) => Key::Delete,
        WinitKey::Named(NamedKey::Tab) => Key::Tab,
        WinitKey::Named(NamedKey::Space) => Key::Space,
        WinitKey::Named(NamedKey::ArrowLeft) => Key::ArrowLeft,
        WinitKey::Named(NamedKey::ArrowRight) => Key::ArrowRight,
        WinitKey::Named(NamedKey::ArrowUp) => Key::ArrowUp,
        WinitKey::Named(NamedKey::ArrowDown) => Key::ArrowDown,
        WinitKey::Named(NamedKey::Home) => Key::Home,
        WinitKey::Named(NamedKey::End) => Key::End,
        _ => Key::Other,
    }
}

fn map_button(button: winit::event::MouseButton) -> Option<MouseButton> {
    match button {
        winit::event::MouseButton::Left => Some(MouseButton::Left),
        winit::event::MouseButton::Right => Some(MouseButton::Right),
        winit::event::MouseButton::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}

impl<A: Application> ApplicationHandler for Runtime<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title(self.settings.title.clone())
            .with_resizable(self.settings.resizable);

        // ネイティブだけがウィンドウサイズを持つ。Web ではキャンバスを
        // ビューポートに合わせるので、固定サイズも最小サイズも指定しない。
        #[cfg(not(target_arch = "wasm32"))]
        {
            attrs = attrs
                .with_inner_size(LogicalSize::new(self.settings.size.0, self.settings.size.1));
            if let Some((w, h)) = self.settings.min_size {
                attrs = attrs.with_min_inner_size(LogicalSize::new(w, h));
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowAttributesExtWebSys;
            attrs = attrs.with_append(true);
        }

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(_) => {
                event_loop.exit();
                return;
            }
        };
        // 日本語入力のために IME を有効化する。
        window.set_ime_allowed(true);

        #[cfg(target_arch = "wasm32")]
        {
            web_canvas::fit_to_viewport(&window);
            web_canvas::watch_viewport(window.clone());
        }

        let Ok(context) = softbuffer::Context::new(window.clone()) else {
            event_loop.exit();
            return;
        };
        let Ok(surface) = softbuffer::Surface::new(&context, window.clone()) else {
            event_loop.exit();
            return;
        };
        self._context = Some(context);
        self.surface = Some(surface);
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::RedrawRequested => self.redraw(),

            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            WindowEvent::ThemeChanged(_) => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let scale = self
                    .window
                    .as_ref()
                    .map(|w| w.scale_factor() as f32)
                    .unwrap_or(1.0);
                let p = Point::new(position.x as f32 / scale, position.y as f32 / scale);
                self.pointer = p;
                self.push(Event::PointerMoved(p));
            }

            WindowEvent::CursorLeft { .. } => {
                self.push(Event::PointerLeft);
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = map_button(button) else {
                    return;
                };
                let position = self.pointer;
                match state {
                    ElementState::Pressed => self.push(Event::PointerPressed { position, button }),
                    ElementState::Released => self.push(Event::PointerReleased { position, button }),
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * LINE_SCROLL, y * LINE_SCROLL),
                    MouseScrollDelta::PixelDelta(p) => {
                        let scale = self
                            .window
                            .as_ref()
                            .map(|w| w.scale_factor() as f32)
                            .unwrap_or(1.0);
                        (p.x as f32 / scale, p.y as f32 / scale)
                    }
                };
                let position = self.pointer;
                self.push(Event::Scrolled {
                    position,
                    delta_x: dx,
                    delta_y: dy,
                });
            }

            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                self.modifiers = Modifiers {
                    shift: s.shift_key(),
                    ctrl: s.control_key(),
                    alt: s.alt_key(),
                    meta: s.super_key(),
                };
                self.interaction.modifiers = self.modifiers;
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                let modifiers = self.modifiers;
                self.push(Event::KeyPressed {
                    key: map_key(&event.logical_key),
                    modifiers,
                });
                // 文字を生む打鍵はテキスト入力としても配送する。
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(text) = event.text.as_ref() {
                        if !text.is_empty() {
                            self.push(Event::Text(text.to_string()));
                        }
                    }
                }
            }

            WindowEvent::Ime(ime) => match ime {
                Ime::Commit(text) => {
                    self.push(Event::ImePreedit {
                        text: String::new(),
                        cursor: None,
                    });
                    self.push(Event::Text(text));
                }
                Ime::Preedit(text, cursor) => {
                    self.push(Event::ImePreedit { text, cursor });
                }
                Ime::Enabled | Ime::Disabled => {}
            },

            _ => {}
        }
    }
}

/// アプリケーションを起動する (この関数は戻らない)。
pub fn run<A: Application>(app: A, settings: Settings) {
    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
    }

    let event_loop = EventLoop::new().expect("イベントループを作成できませんでした");
    event_loop.set_control_flow(ControlFlow::Wait);

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut runtime = Runtime::new(app, settings);
        let _ = event_loop.run_app(&mut runtime);
    }
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(Runtime::new(app, settings));
    }
}
