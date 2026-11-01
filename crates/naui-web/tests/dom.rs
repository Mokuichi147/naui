//! Behavior checks against the browser's real DOM.
//!
//! DOM events (`click` / `input` / `change`) are **actually fired** to check that
//! they reach Rust closures and that the browser-side state changes. Sizes and
//! positions are measured with the results of `getBoundingClientRect`.
//!
//! The Web backend does not draw widgets itself; it relies directly on browser
//! behavior (clicking a label checks the box, radios with the same `name` are
//! automatically exclusive, Flexbox lays out children). That can only be verified
//! in a browser, so `wasm_bindgen_test_configure!(run_in_browser)` puts the tests in
//! a real browser.
//!
//!   cargo test --target wasm32-unknown-unknown -p naui-web
//!
//! A driver (chromedriver / geckodriver) is required. If you do not have one
//! locally, setting `NO_HEADLESS=1` makes the runner print the URL it is listening
//! on, and opening it in any browser runs the same tests.

#![cfg(target_arch = "wasm32")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    Align, ButtonRole, Color, DialogButtons, DialogResponse, EventResponse, GridCell, Key,
    KeyEvent, Length, ListItem, MenuItem, MenuShortcut, MenuSpec, NavItem, Orientation, Padding,
    Pane, Point, PointerButton, PointerPhase, PopupItem, Rect, Result, ScrollMetrics, SidebarItem,
    SidebarSection, Sizing, TableColumn, TableRow, TextColor, TextStyle, Theme, ToolbarIcon,
    ToolbarItem, WindowPlacement, WindowState, DEFAULT_SIDEBAR_WIDTH,
};
use naui_web::{run_for_test, ListRow, TableCells, Ui, WebWidgetExt, Widget};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{
    CanvasRenderingContext2d, Element, EventTarget, HtmlElement, HtmlInputElement,
    HtmlSelectElement, KeyboardEvent, KeyboardEventInit, MouseEvent, MouseEventInit, PointerEvent,
    PointerEventInit, WheelEvent, WheelEventInit,
};

wasm_bindgen_test_configure!(run_in_browser);

// ------------------------------------------------------------- 補助

/// Creates widgets and inspects them. Fails the test if building fails.
fn with_ui(build: impl FnOnce(&Ui) -> Result<()>) {
    run_for_test(build).expect("テスト用の UI を組み立てられませんでした");
}

/// A guard that lives only while placed in `<body>`.
///
/// Browsers do not lay out elements that are not in the document,
/// `getComputedStyle` returns empty values for them, and calling `click()` does not
/// fire `change` (only `checked` changes). Tests that look at positions, computed
/// styles and notifications place the element on the page first, just as a real
/// app does.
struct Mounted(Element);

impl Mounted {
    fn new(widget: &dyn Widget) -> Self {
        Self::new_element(&widget.native_element())
    }

    /// Places something that is not a [`Widget`] (a menu bar, for example).
    fn new_element(element: &Element) -> Self {
        body().append_child(element).expect("body への追加");
        Self(element.clone())
    }
}

impl Drop for Mounted {
    fn drop(&mut self) {
        self.0.remove();
    }
}

fn body() -> HtmlElement {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
        .expect("body")
}

/// Fires the same event the browser would.
fn dispatch(target: &EventTarget, kind: &str) {
    let event = web_sys::Event::new(kind).expect("イベントの生成");
    target.dispatch_event(&event).expect("イベントの配送");
}

/// Fires an Escape `keydown`. **The return value is whether the default action was
/// prevented** (`dispatchEvent` returns `false` when `preventDefault()` was
/// called).
fn press_escape(target: &EventTarget, composing: bool) -> bool {
    let init = KeyboardEventInit::new();
    init.set_key("Escape");
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_is_composing(composing);
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init)
        .expect("キーイベントの生成");
    !target.dispatch_event(&event).expect("イベントの配送")
}

/// Fires an Enter `keydown`. **The return value is whether the default action was
/// prevented.**
fn press_enter(target: &EventTarget, composing: bool) -> bool {
    let init = KeyboardEventInit::new();
    init.set_key("Enter");
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_is_composing(composing);
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init)
        .expect("キーイベントの生成");
    !target.dispatch_event(&event).expect("イベントの配送")
}

/// Fires a shortcut `keydown`. **The return value is whether the default action was
/// prevented.**
fn press_shortcut(target: &EventTarget, key: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
    let init = KeyboardEventInit::new();
    init.set_key(key);
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_ctrl_key(ctrl);
    init.set_shift_key(shift);
    init.set_alt_key(alt);
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init)
        .expect("キーイベントの生成");
    !target.dispatch_event(&event).expect("イベントの配送")
}

/// The computed style (the result as interpreted by the browser).
fn computed(element: &Element, property: &str) -> String {
    web_sys::window()
        .expect("window")
        .get_computed_style(element)
        .expect("算出スタイルの取得")
        .expect("算出スタイル")
        .get_property_value(property)
        .expect("プロパティの取得")
}

/// The first `<input>` inside the element.
fn first_input(element: &Element) -> HtmlInputElement {
    element
        .query_selector("input")
        .expect("input の検索")
        .expect("input が見つかりません")
        .unchecked_into()
}

/// Waits for the next paint. `ResizeObserver` is called after layout, so it has
/// arrived after waiting two frames.
async fn next_frames() {
    for _ in 0..2 {
        let promise = js_sys::Promise::new(&mut |resolve, _reject| {
            web_sys::window()
                .expect("window")
                .request_animation_frame(&resolve)
                .expect("requestAnimationFrame");
        });
        wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .expect("フレーム待ち");
    }
}

// ------------------------------------------------------------- Button

#[wasm_bindgen_test]
fn button_click_reaches_the_closure() {
    with_ui(|ui| {
        let count = Rc::new(Cell::new(0));
        let button = ui.button("押す")?;
        button.on_click({
            let count = count.clone();
            move || count.set(count.get() + 1)
        });
        let _mounted = Mounted::new(&button);

        button.click();
        assert_eq!(count.get(), 1, "click() が通知されていません");

        // ブラウザ側から押しても同じ。
        let element: HtmlElement = button.native_element().unchecked_into();
        element.click();
        assert_eq!(count.get(), 2, "DOM のクリックが通知されていません");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn button_on_click_replaces_the_previous_closure() {
    with_ui(|ui| {
        let first = Rc::new(Cell::new(0));
        let second = Rc::new(Cell::new(0));
        let button = ui.button("押す")?;
        button.on_click({
            let first = first.clone();
            move || first.set(first.get() + 1)
        });
        button.on_click({
            let second = second.clone();
            move || second.set(second.get() + 1)
        });
        let _mounted = Mounted::new(&button);

        button.click();
        assert_eq!(first.get(), 0, "古い購読が外れていません");
        assert_eq!(second.get(), 1);
        Ok(())
    });
}

#[wasm_bindgen_test]
fn button_disabled_does_not_notify() {
    with_ui(|ui| {
        let count = Rc::new(Cell::new(0));
        let button = ui.button("押す")?;
        button.on_click({
            let count = count.clone();
            move || count.set(count.get() + 1)
        });
        button.set_enabled(false);
        let _mounted = Mounted::new(&button);

        button.click();
        assert_eq!(count.get(), 0, "無効な <button> が押せています");
        Ok(())
    });
}

// ----------------------------------------------------------- Checkbox

#[wasm_bindgen_test]
fn checkbox_toggles_and_notifies() {
    with_ui(|ui| {
        let seen = Rc::new(Cell::new(None));
        let checkbox = ui.checkbox("同意する")?;
        checkbox.on_toggle({
            let seen = seen.clone();
            move |on| seen.set(Some(on))
        });
        let _mounted = Mounted::new(&checkbox);

        assert!(!checkbox.is_checked());
        checkbox.click();
        assert!(checkbox.is_checked());
        assert_eq!(seen.get(), Some(true));

        checkbox.click();
        assert!(!checkbox.is_checked());
        assert_eq!(seen.get(), Some(false));
        Ok(())
    });
}

#[wasm_bindgen_test]
fn checkbox_set_is_silent() {
    with_ui(|ui| {
        let seen = Rc::new(Cell::new(None));
        let checkbox = ui.checkbox("同意する")?;
        checkbox.on_toggle({
            let seen = seen.clone();
            move |on| seen.set(Some(on))
        });
        let _mounted = Mounted::new(&checkbox);

        checkbox.set_checked(true);
        assert!(checkbox.is_checked());
        assert_eq!(seen.get(), None, "set_checked が通知しています");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn checkbox_label_click_toggles_the_box() {
    with_ui(|ui| {
        let seen = Rc::new(Cell::new(None));
        let checkbox = ui.checkbox("同意する")?;
        checkbox.on_toggle({
            let seen = seen.clone();
            move |on| seen.set(Some(on))
        });
        let mounted = Mounted::new(&checkbox);

        // 文字の側を押しても入る。この結び付けはブラウザが `<label>` に対して
        // 行うもので、naui は何もしていない。
        let label: HtmlElement = mounted.0.clone().unchecked_into();
        label.click();
        assert!(checkbox.is_checked(), "ラベルのクリックが届いていません");
        assert_eq!(seen.get(), Some(true));
        Ok(())
    });
}

// ------------------------------------------------------------- Toggle

#[wasm_bindgen_test]
fn toggle_switches_and_notifies() {
    with_ui(|ui| {
        let seen = Rc::new(Cell::new(None));
        let toggle = ui.toggle("通知")?;
        toggle.on_toggle({
            let seen = seen.clone();
            move |on| seen.set(Some(on))
        });
        let _mounted = Mounted::new(&toggle);

        assert!(!toggle.is_on());
        toggle.click();
        assert!(toggle.is_on());
        assert_eq!(seen.get(), Some(true));

        // ブラウザが未対応でもスイッチとして読み上げられるようにしてある。
        let input = toggle.native_switch();
        assert_eq!(input.get_attribute("role").as_deref(), Some("switch"));
        Ok(())
    });
}

#[wasm_bindgen_test]
fn toggle_set_is_silent() {
    with_ui(|ui| {
        let seen = Rc::new(Cell::new(None));
        let toggle = ui.toggle("通知")?;
        toggle.on_toggle({
            let seen = seen.clone();
            move |on| seen.set(Some(on))
        });
        let _mounted = Mounted::new(&toggle);

        toggle.set_on(true);
        assert!(toggle.is_on());
        assert_eq!(seen.get(), None, "set_on が通知しています");
        Ok(())
    });
}

// ---------------------------------------------------------- TextInput

#[wasm_bindgen_test]
fn text_input_round_trips_japanese() {
    with_ui(|ui| {
        let input = ui.text_input("はじめの値")?;
        assert_eq!(input.text(), "はじめの値");

        input.set_text("日本語のテキスト");
        let native: HtmlInputElement = input.native_element().unchecked_into();
        assert_eq!(native.value(), "日本語のテキスト");
        assert_eq!(input.text(), "日本語のテキスト");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn text_input_notifies_while_typing() {
    with_ui(|ui| {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let input = ui.text_input("")?;
        input.on_change({
            let seen = seen.clone();
            move |text| seen.borrow_mut().push(text.to_string())
        });
        let _mounted = Mounted::new(&input);

        // 打鍵と同じ経路 (`input` イベント) を通す。
        let native: HtmlInputElement = input.native_element().unchecked_into();
        for text in ["あ", "あい", "あいう"] {
            native.set_value(text);
            dispatch(native.as_ref(), "input");
        }
        assert_eq!(*seen.borrow(), vec!["あ", "あい", "あいう"]);

        // set_text は通知しない。
        input.set_text("差し替え");
        assert_eq!(seen.borrow().len(), 3, "set_text が通知しています");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn password_input_hides_what_is_typed() {
    with_ui(|ui| {
        let input = ui.password_input()?;
        let native: HtmlInputElement = input.native_element().unchecked_into();
        assert_eq!(native.type_(), "password");

        input.set_text("ひみつ");
        assert_eq!(input.text(), "ひみつ");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn text_area_keeps_line_breaks() {
    with_ui(|ui| {
        let area = ui.text_area("1 行目\n2 行目")?;
        assert_eq!(area.text(), "1 行目\n2 行目");

        area.set_text("上\n中\n下");
        assert_eq!(area.text(), "上\n中\n下");
        Ok(())
    });
}

// ------------------------------------------------------------- Slider

#[wasm_bindgen_test]
fn slider_clamps_and_keeps_fractions() {
    with_ui(|ui| {
        let slider = ui.slider(0.0, 10.0)?;

        slider.set_value(20.0);
        assert_eq!(slider.value(), 10.0, "上限でクランプされていません");
        slider.set_value(-5.0);
        assert_eq!(slider.value(), 0.0, "下限でクランプされていません");

        // `<input type="range">` の既定の刻みは 1。連続値を扱えるよう
        // 細かくしてあることを、ブラウザの丸め結果で確かめる。
        slider.set_value(2.5);
        assert_eq!(slider.value(), 2.5);
        Ok(())
    });
}

#[wasm_bindgen_test]
fn slider_notifies_while_dragging() {
    with_ui(|ui| {
        let seen = Rc::new(Cell::new(None));
        let slider = ui.slider(0.0, 10.0)?;
        slider.on_change({
            let seen = seen.clone();
            move |value| seen.set(Some(value))
        });
        let _mounted = Mounted::new(&slider);

        let native: HtmlInputElement = slider.native_element().unchecked_into();
        native.set_value("7.5");
        dispatch(native.as_ref(), "input");
        assert_eq!(seen.get(), Some(7.5));
        assert_eq!(slider.value(), 7.5);
        Ok(())
    });
}

#[wasm_bindgen_test]
fn progress_clamps_to_the_unit_range() {
    with_ui(|ui| {
        let progress = ui.progress_bar()?;
        assert_eq!(progress.value(), 0.0);

        progress.set_value(0.25);
        assert_eq!(progress.value(), 0.25);
        progress.set_value(2.0);
        assert_eq!(progress.value(), 1.0);
        progress.set_value(-1.0);
        assert_eq!(progress.value(), 0.0);
        Ok(())
    });
}

// ----------------------------------------------------------- ComboBox

#[wasm_bindgen_test]
fn combo_box_starts_unselected() {
    with_ui(|ui| {
        let combo = ui.combo_box()?;
        combo.set_items(&["赤", "青", "緑"]);

        // `<select>` は既定で最初の `<option>` を選ぶ。naui は 4 環境で
        // 「作った直後は未選択」にそろえているので、そこを確かめる。
        assert_eq!(combo.len(), 3);
        assert_eq!(combo.selected(), None, "ブラウザの自動選択が残っています");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn combo_box_change_event_notifies() {
    with_ui(|ui| {
        let seen = Rc::new(Cell::new(None));
        let combo = ui.combo_box()?;
        combo.set_items(&["赤", "青", "緑"]);
        combo.on_select({
            let seen = seen.clone();
            move |index| seen.set(Some(index))
        });
        let _mounted = Mounted::new(&combo);

        // 利用者が選んだときと同じ経路。
        let native: HtmlSelectElement = combo.native_element().unchecked_into();
        native.set_selected_index(1);
        dispatch(native.as_ref(), "change");
        assert_eq!(seen.get(), Some(1));
        assert_eq!(combo.selected(), Some(1));

        // set_selected は通知しない。
        combo.set_selected(2);
        assert_eq!(combo.selected(), Some(2));
        assert_eq!(seen.get(), Some(1), "set_selected が通知しています");
        Ok(())
    });
}

// --------------------------------------------------------- RadioGroup

#[wasm_bindgen_test]
fn radio_group_is_exclusive_in_the_browser() {
    with_ui(|ui| {
        let radio = ui.radio_group()?;
        radio.set_items(&["小", "中", "大"]);
        let _mounted = Mounted::new(&radio);

        assert_eq!(radio.selected(), None);
        radio.set_selected(0);
        assert_eq!(radio.selected(), Some(0));

        // 排他は naui ではなくブラウザが行う (同じ `name` を共有している)。
        radio.set_selected(2);
        assert_eq!(radio.selected(), Some(2), "前の選択が外れていません");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn radio_group_notifies_the_clicked_index() {
    with_ui(|ui| {
        let seen = Rc::new(Cell::new(None));
        let radio = ui.radio_group()?;
        radio.set_items(&["小", "中", "大"]);
        radio.on_select({
            let seen = seen.clone();
            move |index| seen.set(Some(index))
        });
        let mounted = Mounted::new(&radio);

        // 2 番目のラジオをブラウザ側から押す。
        let second: HtmlElement = mounted
            .0
            .query_selector("label:nth-of-type(2) input")
            .expect("2 番目のラジオの検索")
            .expect("2 番目のラジオが見つかりません")
            .unchecked_into();
        second.click();
        assert_eq!(seen.get(), Some(1));
        assert_eq!(radio.selected(), Some(1));
        Ok(())
    });
}

// -------------------------------------------------------- ColorPicker

#[wasm_bindgen_test]
fn color_picker_round_trips() {
    with_ui(|ui| {
        let seen = Rc::new(Cell::new(None));
        let picker = ui.color_picker()?;
        picker.on_change({
            let seen = seen.clone();
            move |color| seen.set(Some(color))
        });
        let _mounted = Mounted::new(&picker);

        assert_eq!(picker.value(), Color::BLACK);
        picker.set_value(Color::rgb(0x12, 0x34, 0x56));
        assert_eq!(picker.value(), Color::rgb(0x12, 0x34, 0x56));
        assert_eq!(seen.get(), None, "set_value が通知しています");

        // 色を選び終えたときと同じ経路 (`change`)。
        let native = picker.native_input();
        native.set_value("#ff8800");
        dispatch(native.as_ref(), "change");
        assert_eq!(seen.get(), Some(Color::rgb(0xff, 0x88, 0x00)));
        Ok(())
    });
}

// ------------------------------------------------------------- Canvas

/// The drawing surface is a `<canvas>`, and the commands recorded by `on_draw` are
/// painted with the 2D context. The backing pixel count matches the CSS size ×
/// `devicePixelRatio`, and coordinates can be passed in CSS pixels as is. Pixels are
/// read with `getImageData`.
#[wasm_bindgen_test]
fn canvas_paints_recorded_commands() {
    with_ui(|ui| {
        let canvas = ui.canvas()?;
        canvas.set_sizing(
            Sizing::new()
                .width(Length::Fixed(120.0))
                .height(Length::Fixed(80.0)),
        );
        let sizes = Rc::new(RefCell::new(Vec::new()));
        canvas.on_draw({
            let sizes = sizes.clone();
            move |painter| {
                sizes.borrow_mut().push((painter.width(), painter.height()));
                painter.fill_rect(painter.bounds(), Color::WHITE);
                painter.fill_rect(Rect::new(10.0, 10.0, 40.0, 20.0), Color::rgb(0xff, 0, 0));
                painter.set_text_align(Align::End);
                painter.text("naui", Point::new(110.0, 50.0), 12.0, Color::BLACK);
            }
        });
        let _mounted = Mounted::new(&canvas);
        canvas.draw_for_test();

        assert_eq!(
            *sizes.borrow(),
            vec![(120.0, 80.0)],
            "CSS の大きさで呼ばれること"
        );
        let native = canvas.native_canvas();
        let ratio = web_sys::window().expect("window").device_pixel_ratio();
        assert_eq!(
            f64::from(native.width()),
            (120.0 * ratio).round(),
            "裏の画素数は倍率ぶん増えること"
        );

        let context: CanvasRenderingContext2d = native
            .get_context("2d")
            .expect("2D コンテキスト")
            .expect("2D コンテキスト")
            .unchecked_into();
        let read = |x: f64, y: f64| -> (u8, u8, u8, u8) {
            let data = context
                .get_image_data((x * ratio).floor(), (y * ratio).floor(), 1.0, 1.0)
                .expect("画素の読み出し")
                .data();
            (data[0], data[1], data[2], data[3])
        };
        assert_eq!(read(20.0, 15.0), (255, 0, 0, 255), "矩形の中は赤いこと");
        assert_eq!(read(80.0, 60.0), (255, 255, 255, 255), "矩形の外は白いこと");

        // 右寄せの文字は x=110 より左にだけ描かれる。
        let dark = |x: f64, y: f64| {
            let (r, g, b, _) = read(x, y);
            u32::from(r) + u32::from(g) + u32::from(b) < 600
        };
        let found = (85..110).any(|x| (50..64).any(|y| dark(f64::from(x), f64::from(y))));
        assert!(found, "文字が右寄せで描かれていること");
        assert!(
            (50..64).all(|y| !dark(114.0, f64::from(y))),
            "右端より右には描かないこと"
        );
        Ok(())
    });
}

/// Pointer presses, moves and releases arrive with positions relative to the
/// surface's top left.
#[wasm_bindgen_test]
fn canvas_reports_pointer_events() {
    with_ui(|ui| {
        let canvas = ui.canvas()?;
        canvas.set_sizing(
            Sizing::new()
                .width(Length::Fixed(100.0))
                .height(Length::Fixed(60.0)),
        );
        let seen = Rc::new(RefCell::new(Vec::new()));
        canvas.on_pointer({
            let seen = seen.clone();
            move |event| seen.borrow_mut().push(event)
        });
        let _mounted = Mounted::new(&canvas);

        let element = canvas.native_element();
        let rect = element.get_bounding_client_rect();
        // `button` は変わったボタン (動きだけなら -1)、`buttons` はいま押している組。
        let send = |kind: &str, x: f64, y: f64, button: i16, buttons: u16| {
            let init = PointerEventInit::new();
            init.set_bubbles(true);
            init.set_cancelable(true);
            init.set_client_x((rect.left() + x) as i32);
            init.set_client_y((rect.top() + y) as i32);
            init.set_pointer_id(1);
            init.set_button(button);
            init.set_buttons(buttons);
            let event = PointerEvent::new_with_event_init_dict(kind, &init)
                .expect("ポインターイベントの生成");
            element.dispatch_event(&event).expect("イベントの配送");
        };
        send("pointerdown", 30.0, 20.0, 0, 1);
        send("pointermove", 50.0, 40.0, -1, 1);
        send("pointerup", 50.0, 40.0, 0, 0);

        {
            let seen = seen.borrow();
            assert_eq!(seen.len(), 3);
            assert_eq!(seen[0].phase, PointerPhase::Down);
            assert_eq!(seen[1].phase, PointerPhase::Move);
            assert_eq!(seen[2].phase, PointerPhase::Up);
            let near = |a: Point, x: f64, y: f64| (a.x - x).abs() <= 1.0 && (a.y - y).abs() <= 1.0;
            assert!(near(seen[0].point, 30.0, 20.0), "{:?}", seen[0]);
            assert!(near(seen[2].point, 50.0, 40.0), "{:?}", seen[2]);
            let buttons: Vec<_> = seen.iter().map(|e| e.button).collect();
            assert_eq!(
                buttons,
                vec![
                    Some(PointerButton::Primary),
                    None,
                    Some(PointerButton::Primary)
                ]
            );
        }

        // 右ボタン (`button` は 2)。左を押したまま中ボタンを押すと、ブラウザは
        // `pointerdown` ではなく `pointermove` で知らせる (中は `button` が 1、
        // `buttons` のビットは 4)。
        seen.borrow_mut().clear();
        send("pointerdown", 10.0, 10.0, 2, 2);
        send("pointerup", 10.0, 10.0, 2, 0);
        send("pointerdown", 10.0, 10.0, 0, 1);
        send("pointermove", 10.0, 10.0, 1, 1 | 4);
        send("pointermove", 10.0, 10.0, 1, 1);
        send("pointerup", 10.0, 10.0, 0, 0);
        let events: Vec<_> = seen.borrow().iter().map(|e| (e.phase, e.button)).collect();
        assert_eq!(
            events,
            vec![
                (PointerPhase::Down, Some(PointerButton::Secondary)),
                (PointerPhase::Up, Some(PointerButton::Secondary)),
                (PointerPhase::Down, Some(PointerButton::Primary)),
                (PointerPhase::Down, Some(PointerButton::Middle)),
                (PointerPhase::Up, Some(PointerButton::Middle)),
                (PointerPhase::Up, Some(PointerButton::Primary)),
            ]
        );

        // 右クリックでブラウザのコンテキストメニューを出さない。
        let init = web_sys::MouseEventInit::new();
        init.set_bubbles(true);
        init.set_cancelable(true);
        init.set_button(2);
        let menu = web_sys::MouseEvent::new_with_mouse_event_init_dict("contextmenu", &init)
            .expect("contextmenu の生成");
        element.dispatch_event(&menu).expect("イベントの配送");
        assert!(menu.default_prevented(), "既定のメニューを止めること");
        Ok(())
    });
}

/// The wheel arrives in logical pixels with `deltaMode` normalized, and stops page
/// scrolling only for `Handled`. It does not reach `on_pointer`.
#[wasm_bindgen_test]
fn canvas_reports_wheel() {
    with_ui(|ui| {
        let canvas = ui.canvas()?;
        canvas.set_sizing(
            Sizing::new()
                .width(Length::Fixed(100.0))
                .height(Length::Fixed(60.0)),
        );
        let seen = Rc::new(RefCell::new(Vec::new()));
        let response = Rc::new(Cell::new(EventResponse::Continue));
        canvas.on_wheel({
            let seen = seen.clone();
            let response = response.clone();
            move |event| {
                seen.borrow_mut().push(event);
                response.get()
            }
        });
        let pointer = Rc::new(Cell::new(0));
        canvas.on_pointer({
            let pointer = pointer.clone();
            move |_| pointer.set(pointer.get() + 1)
        });
        let _mounted = Mounted::new(&canvas);

        let element = canvas.native_element();
        let rect = element.get_bounding_client_rect();
        let send = |x: f64, y: f64, dx: f64, dy: f64, mode: u32| {
            let init = WheelEventInit::new();
            init.set_bubbles(true);
            init.set_cancelable(true);
            init.set_client_x((rect.left() + x) as i32);
            init.set_client_y((rect.top() + y) as i32);
            init.set_delta_x(dx);
            init.set_delta_y(dy);
            init.set_delta_mode(mode);
            let event = WheelEvent::new_with_event_init_dict("wheel", &init).expect("wheel の生成");
            element.dispatch_event(&event).expect("イベントの配送");
            event.default_prevented()
        };

        // ピクセルはそのまま。Continue ならページのスクロールを止めない。
        assert!(!send(30.0, 20.0, 0.0, 12.5, WheelEvent::DOM_DELTA_PIXEL));
        // 行は 1 行を WHEEL_STEP の 1/3 に、ページは面の大きさに直す。
        response.set(EventResponse::Handled);
        assert!(
            send(30.0, 20.0, 0.0, -3.0, WheelEvent::DOM_DELTA_LINE),
            "Handled ならページのスクロールを止めること"
        );
        assert!(send(30.0, 20.0, 1.0, 0.0, WheelEvent::DOM_DELTA_PAGE));

        let seen = seen.borrow();
        assert_eq!(seen.len(), 3);
        assert!(seen
            .iter()
            .all(|e| e.phase == PointerPhase::Wheel && e.button.is_none()));
        let near = |a: Point, x: f64, y: f64| (a.x - x).abs() <= 1.0 && (a.y - y).abs() <= 1.0;
        assert!(near(seen[0].point, 30.0, 20.0), "{:?}", seen[0]);
        let amounts: Vec<_> = seen.iter().map(|e| (e.wheel_x, e.wheel_y)).collect();
        assert_eq!(
            amounts,
            vec![(0.0, 12.5), (0.0, -naui_core::WHEEL_STEP), (100.0, 0.0)]
        );
        assert_eq!(pointer.get(), 0, "ホイールは on_pointer へは届かないこと");
        Ok(())
    });
}

// -------------------------------------------------------- NumberInput

#[wasm_bindgen_test]
fn number_input_clamps_to_its_range() {
    with_ui(|ui| {
        let number = ui.number_input(3.0)?;
        assert_eq!(number.value(), 3.0);

        number.set_range(Some(0.0), Some(10.0));
        number.set_value(42.0);
        assert_eq!(number.value(), 10.0);
        number.set_value(-1.0);
        assert_eq!(number.value(), 0.0);
        Ok(())
    });
}

#[wasm_bindgen_test]
fn number_input_notifies_what_was_typed() {
    with_ui(|ui| {
        let seen = Rc::new(Cell::new(None));
        let number = ui.number_input(0.0)?;
        number.on_change({
            let seen = seen.clone();
            move |value| seen.set(Some(value))
        });
        let _mounted = Mounted::new(&number);

        let native = number.native_input();
        native.set_value("7");
        dispatch(native.as_ref(), "input");
        assert_eq!(seen.get(), Some(7.0));
        assert_eq!(number.value(), 7.0);
        Ok(())
    });
}

// -------------------------------------------------- Stack (レイアウト)

#[wasm_bindgen_test]
fn stack_places_children_in_a_row() {
    with_ui(|ui| {
        let stack = ui.stack(Orientation::Horizontal)?;
        let left = ui.button("左")?;
        let right = ui.button("右")?;
        stack.append(&left);
        stack.append(&right);
        assert_eq!(stack.len(), 2);
        let _mounted = Mounted::new(&stack);

        let l = left.native_element().get_bounding_client_rect();
        let r = right.native_element().get_bounding_client_rect();
        assert!(
            r.left() >= l.right(),
            "横並びになっていません (左 {} / 右 {})",
            l.right(),
            r.left()
        );
        assert!(
            (l.top() - r.top()).abs() < 1.0,
            "上端がそろっていません ({} / {})",
            l.top(),
            r.top()
        );
        Ok(())
    });
}

#[wasm_bindgen_test]
fn stack_places_children_in_a_column() {
    with_ui(|ui| {
        let stack = ui.stack(Orientation::Vertical)?;
        let top = ui.button("上")?;
        let bottom = ui.button("下")?;
        stack.append(&top);
        stack.append(&bottom);
        let _mounted = Mounted::new(&stack);

        let t = top.native_element().get_bounding_client_rect();
        let b = bottom.native_element().get_bounding_client_rect();
        assert!(
            b.top() >= t.bottom(),
            "縦並びになっていません (上 {} / 下 {})",
            t.bottom(),
            b.top()
        );
        Ok(())
    });
}

#[wasm_bindgen_test]
fn stack_spacing_separates_children() {
    with_ui(|ui| {
        let stack = ui.stack(Orientation::Horizontal)?;
        let left = ui.button("左")?;
        let right = ui.button("右")?;
        stack.append(&left);
        stack.append(&right);
        stack.set_spacing(24.0);
        let _mounted = Mounted::new(&stack);

        let gap = right.native_element().get_bounding_client_rect().left()
            - left.native_element().get_bounding_client_rect().right();
        assert!(
            (gap - 24.0).abs() < 0.5,
            "指定した間隔になっていません ({gap})"
        );
        Ok(())
    });
}

#[wasm_bindgen_test]
fn stack_padding_insets_its_children() {
    with_ui(|ui| {
        let stack = ui.stack(Orientation::Vertical)?;
        let child = ui.button("中身")?;
        stack.append(&child);
        stack.set_padding(Padding::all(16.0));
        let _mounted = Mounted::new(&stack);

        let outer = stack.native_element().get_bounding_client_rect();
        let inner = child.native_element().get_bounding_client_rect();
        assert!(
            (inner.top() - outer.top() - 16.0).abs() < 0.5,
            "上の余白が入っていません ({} / {})",
            outer.top(),
            inner.top()
        );
        Ok(())
    });
}

#[wasm_bindgen_test]
fn stack_align_moves_children_to_the_end() {
    with_ui(|ui| {
        let stack = ui.stack(Orientation::Vertical)?;
        let child = ui.button("端")?;
        stack.append(&child);
        stack.set_align(Align::End);
        let _mounted = Mounted::new(&stack);

        let element: HtmlElement = stack.native_element().unchecked_into();
        element
            .style()
            .set_property("width", "400px")
            .expect("幅の指定");

        let outer = stack.native_element().get_bounding_client_rect();
        let inner = child.native_element().get_bounding_client_rect();
        assert!(
            (outer.right() - inner.right()).abs() < 1.0,
            "右端に寄っていません ({} / {})",
            outer.right(),
            inner.right()
        );
        Ok(())
    });
}

// -------------------------------------------------------------- Label

#[wasm_bindgen_test]
fn label_wraps_only_when_asked() {
    with_ui(|ui| {
        let stack = ui.stack(Orientation::Vertical)?;
        let label =
            ui.label("折り返しの様子を確かめるための、じゅうぶんに長い日本語の文字列です。")?;
        stack.append(&label);
        let _mounted = Mounted::new(&stack);

        let element: HtmlElement = label.native_element().unchecked_into();
        element
            .style()
            .set_property("width", "80px")
            .expect("幅の指定");

        // `<span>` の既定は折り返す。naui は他の 3 環境にそろえて、
        // 既定では 1 行に収めている。
        let single = element.get_bounding_client_rect().height();
        label.set_wrap(true);
        let wrapped = element.get_bounding_client_rect().height();
        assert!(
            wrapped > single * 1.5,
            "折り返していません (1 行 {single} / 折り返し {wrapped})"
        );

        label.set_wrap(false);
        assert!(
            (element.get_bounding_client_rect().height() - single).abs() < 0.5,
            "1 行へ戻っていません"
        );
        Ok(())
    });
}

/// `set_role` only marks the button with `data-naui-role`. Browsers have no look for
/// the roles, so the drawn background stays that of an ordinary button.
#[wasm_bindgen_test]
fn button_role_only_marks_the_button() {
    with_ui(|ui| {
        let button = ui.button("保存")?;
        let _mounted = Mounted::new(&button);
        let element: HtmlElement = button.native_element().unchecked_into();
        let role = || element.get_attribute("data-naui-role");
        let background = || {
            web_sys::window()
                .and_then(|w| w.get_computed_style(&element).ok().flatten())
                .and_then(|s| s.get_property_value("background-color").ok())
                .unwrap_or_default()
        };
        assert_eq!(role(), None, "既定では属性を書かないこと");
        let normal = background();

        button.set_role(ButtonRole::Primary);
        assert_eq!(role().as_deref(), Some("primary"));
        assert_eq!(background(), normal, "見た目は変えないこと");
        button.set_role(ButtonRole::Destructive);
        assert_eq!(role().as_deref(), Some("destructive"));
        assert_eq!(background(), normal, "見た目は変えないこと");
        button.set_role(ButtonRole::Normal);
        assert_eq!(role(), None, "既定へ戻すと属性が消えること");
        Ok(())
    });
}

/// `set_style` / `set_color` change the size and color of the text actually drawn.
///
/// Only the Web has no heading levels, so naui matches the other three platforms
/// with CSS. This looks at the values after the browser has resolved them, not at
/// the specified values.
#[wasm_bindgen_test]
fn label_style_and_color_change_the_rendered_text() {
    with_ui(|ui| {
        let label = ui.label("見出し")?;
        let _mounted = Mounted::new(&label);
        let element: HtmlElement = label.native_element().unchecked_into();
        let resolved = |property: &str| {
            web_sys::window()
                .and_then(|w| w.get_computed_style(&element).ok().flatten())
                .and_then(|style| style.get_property_value(property).ok())
                .unwrap_or_default()
        };
        let font_size = || {
            resolved("font-size")
                .trim_end_matches("px")
                .parse::<f64>()
                .expect("font-size が px で返ること")
        };

        // 大きい段階ほど大きく描かれる。
        let ramp = [
            TextStyle::Caption,
            TextStyle::Body,
            TextStyle::Subtitle,
            TextStyle::Title,
            TextStyle::LargeTitle,
        ];
        let mut sizes: Vec<(TextStyle, f64)> = Vec::new();
        for style in ramp {
            label.set_style(style);
            sizes.push((style, font_size()));
        }
        for pair in sizes.windows(2) {
            assert!(
                pair[1].1 > pair[0].1,
                "段階が上がるほど大きくなること: {:?} {} / {:?} {}",
                pair[0].0,
                pair[0].1,
                pair[1].0,
                pair[1].1
            );
        }

        // `Heading` は本文と同じ大きさで、太さだけが違う。
        label.set_style(TextStyle::Body);
        let body_size = font_size();
        let body_weight = resolved("font-weight");
        label.set_style(TextStyle::Heading);
        assert!(
            (font_size() - body_size).abs() < 0.5,
            "見出しは本文と同じ大きさであること: {body_size} / {}",
            font_size()
        );
        assert_ne!(
            resolved("font-weight"),
            body_weight,
            "見出しは本文と違う太さであること"
        );

        // 本文へ戻すと naui の指定は消え、ブラウザ既定へ返る。
        label.set_style(TextStyle::Body);
        let style = element.style();
        assert_eq!(
            style.get_property_value("font-size").unwrap_or_default(),
            ""
        );
        assert_eq!(
            style.get_property_value("font-weight").unwrap_or_default(),
            ""
        );

        // どの役割も、ブラウザが解釈できる色として書かれる (指定を捨てられない)。
        for color in TextColor::ALL {
            label.set_color(color);
            assert_eq!(
                element
                    .style()
                    .get_property_value("color")
                    .unwrap_or_default()
                    .is_empty(),
                color == TextColor::Default,
                "{color:?} の指定がブラウザに受け取られること"
            );
            assert!(
                !resolved("color").is_empty(),
                "{color:?} の色が解決できていない"
            );
        }

        // `light-dark()` で決めている 3 つは、互いに違う色として解決される。
        // システム色に任せている `Secondary` と `Accent` は**ブラウザ次第**で、
        // Chromium はアクセントカラーを出さずに灰色を返す。
        let mut seen: Vec<String> = Vec::new();
        for color in [TextColor::Success, TextColor::Warning, TextColor::Danger] {
            label.set_color(color);
            let current = resolved("color");
            assert!(
                !seen.contains(&current),
                "{color:?} が別の役割と同じ色になっている: {current} \
                 (light-dark() が効いていない可能性)"
            );
            seen.push(current);
        }

        label.set_color(TextColor::Default);
        assert_eq!(
            element
                .style()
                .get_property_value("color")
                .unwrap_or_default(),
            "",
            "既定へ戻すと色の指定が消えること"
        );
        Ok(())
    });
}

#[wasm_bindgen_test]
fn label_text_round_trips() {
    with_ui(|ui| {
        let label = ui.label("はじめ")?;
        assert_eq!(label.text(), "はじめ");
        label.set_text("あと");
        assert_eq!(label.text(), "あと");
        Ok(())
    });
}

// ------------------------------------------------------ Stack / Grid / Tabs

/// Takes out the text of the children laid out in a `<div>`, in order.
fn child_texts(widget: &dyn Widget) -> Vec<String> {
    let element = widget.native_element();
    let children = element.children();
    (0..children.length())
        .filter_map(|index| children.item(index))
        .map(|child| child.text_content().unwrap_or_default())
        .collect()
}

/// A `Stack` can insert, remove and clear children later.
/// `set_sizing` after appending reaches the stack's layout, on both axes.
#[wasm_bindgen_test]
fn sizing_after_append_reaches_the_stack() {
    with_ui(|ui| {
        let column = ui.stack(Orientation::Vertical)?;
        column.set_sizing(Sizing::fixed(400.0, 100.0));
        let wide = ui.button("広がる")?;
        column.append(&wide);
        let row = ui.stack(Orientation::Horizontal)?;
        row.set_sizing(Sizing::fixed(400.0, 40.0));
        let grow = ui.button("伸びる")?;
        row.append(&grow);
        row.append(&ui.button("固定")?);
        let _column = Mounted::new(&column);
        let _row = Mounted::new(&row);

        wide.set_sizing(Sizing::fill_width());
        grow.set_sizing(Sizing::fill_width());
        let wide_width = wide.native_element().get_bounding_client_rect().width();
        let grow_width = grow.native_element().get_bounding_client_rect().width();
        assert!(
            (wide_width - 400.0).abs() < 0.5,
            "あとからの交差軸の Fill がスタックの幅になること: {wide_width}"
        );
        assert!(
            grow_width > 250.0,
            "あとからの主軸の Fill が余りを受け取ること: {grow_width}"
        );

        wide.set_sizing(Sizing::AUTO);
        let restored = wide.native_element().get_bounding_client_rect().width();
        assert!(
            restored < 200.0,
            "指定を戻せば中身の幅へ戻ること: {restored}"
        );
        Ok(())
    });
}

/// `set_sizing` after attaching reaches the placement within the grid cell.
#[wasm_bindgen_test]
fn sizing_after_attach_reaches_the_grid() {
    with_ui(|ui| {
        let grid = ui.grid()?;
        grid.set_column_track(0, naui_core::Track::FILL);
        grid.set_sizing(Sizing::fixed(400.0, 100.0));
        let button = ui.button("広がる")?;
        grid.attach(&button, GridCell::new(0, 0));
        let _mounted = Mounted::new(&grid);

        let before = button.native_element().get_bounding_client_rect().width();
        assert!(before < 200.0, "指定前は中身の幅であること: {before}");
        button.set_sizing(Sizing::fill_width());
        let filled = button.native_element().get_bounding_client_rect().width();
        assert!(
            (filled - 400.0).abs() < 0.5,
            "あとから指定してもマスいっぱいに広がること: {filled}"
        );
        button.set_sizing(Sizing::AUTO);
        let restored = button.native_element().get_bounding_client_rect().width();
        assert!(
            (restored - before).abs() < 0.5,
            "指定を戻せば中身の幅へ戻ること: {before} / {restored}"
        );
        Ok(())
    });
}

/// A `Stack` with a breakpoint lays its children out side by side once it is at least
/// that wide (its padding included) and one above the other below it, before the page
/// is painted at the new width. Going back takes a few pixels less, a field being typed
/// into keeps its focus, and the browser reports no `ResizeObserver` loop.
#[wasm_bindgen_test]
async fn stack_breakpoint_follows_its_width() {
    let errors = Rc::new(RefCell::new(Vec::<String>::new()));
    let on_error = wasm_bindgen::closure::Closure::<dyn FnMut(JsValue)>::new({
        let errors = errors.clone();
        move |event: JsValue| {
            let message = js_sys::Reflect::get(&event, &JsValue::from_str("message"))
                .ok()
                .and_then(|message| message.as_string())
                .unwrap_or_default();
            errors.borrow_mut().push(message);
        }
    });
    let window = web_sys::window().expect("window");
    window
        .add_event_listener_with_callback("error", on_error.as_ref().unchecked_ref())
        .expect("error の購読");

    let mut kept = None;
    with_ui(|ui| {
        let outer = ui.stack(Orientation::Vertical)?;
        outer.set_sizing(Sizing::fixed(700.0, 300.0));
        let form = ui.stack(Orientation::Vertical)?;
        form.set_sizing(Sizing::fill_width());
        form.set_padding(Padding::all(10.0));
        let field = ui.text_input("")?;
        let first = ui.button("前")?;
        let last = ui.button("後")?;
        form.append(&field);
        form.append(&first);
        form.append(&last);
        outer.append(&form);
        form.set_breakpoint(Some(naui_core::Breakpoint::new(
            500.0,
            Orientation::Horizontal,
        )));
        assert_eq!(
            form.current_orientation(),
            Orientation::Vertical,
            "並ぶ前は自分の向きのままであること"
        );
        let mounted = Mounted::new(&outer);
        kept = Some((mounted, outer, form, field, first, last));
        Ok(())
    });
    let (_mounted, outer, form, field, first, last) = kept.expect("組み立て");
    frames_until(|| form.current_orientation() == Orientation::Horizontal).await;
    assert_eq!(
        form.current_orientation(),
        Orientation::Horizontal,
        "しきい値より広ければ横に並ぶこと"
    );
    assert_eq!(form.orientation(), Orientation::Vertical);
    assert_eq!(computed(&form.native_element(), "flex-direction"), "row");
    let (a, b) = (
        first.native_element().get_bounding_client_rect(),
        last.native_element().get_bounding_client_rect(),
    );
    assert!(
        (a.top() - b.top()).abs() < 1.0 && b.left() >= a.right() - 0.5,
        "子が 1 行に並ぶこと: {} / {}",
        a.right(),
        b.left()
    );

    let input: HtmlElement = field.native_element().unchecked_into();
    input.focus().expect("フォーカス");
    let resize = |width: f64| outer.set_sizing(Sizing::fixed(width, 300.0));
    let check = |width: f64, expected: Orientation, message: &str| {
        assert_eq!(
            form.current_orientation(),
            expected,
            "{message} (幅 {width})"
        );
    };
    for (width, expected, message) in [
        (480.0, Orientation::Vertical, "しきい値より狭ければ縦に積む"),
        (496.0, Orientation::Vertical, "しきい値に届くまでは縦のまま"),
        (500.0, Orientation::Horizontal, "ちょうどしきい値で横に並ぶ"),
        (
            494.0,
            Orientation::Horizontal,
            "しきい値のすぐ下では戻らない",
        ),
        (490.0, Orientation::Vertical, "遊びを越えて狭まれば縦に戻る"),
    ] {
        resize(width);
        frames_until(|| form.current_orientation() == expected).await;
        // 変わらないはずの所では、監視を付け直したあとの報告まで待ってから見る。
        next_frames().await;
        check(width, expected, message);
    }
    let document = window.document().expect("document");
    assert!(
        document
            .active_element()
            .is_some_and(|active| active == *input.as_ref()),
        "向きが切り替わっても入力欄のフォーカスが外れないこと"
    );

    // 描く前に切り替える。あとから作った ResizeObserver は同じフレームの中で
    // naui の後に呼ばれるので、そこではもう横に並んでいる。
    let painted = Rc::new(RefCell::new(Vec::new()));
    let watch = wasm_bindgen::closure::Closure::<dyn FnMut(JsValue)>::new({
        let painted = painted.clone();
        let element = form.native_element();
        move |_: JsValue| {
            let width = element.get_bounding_client_rect().width();
            painted
                .borrow_mut()
                .push((width, computed(&element, "flex-direction")));
        }
    });
    let observer =
        web_sys::ResizeObserver::new(watch.as_ref().unchecked_ref()).expect("ResizeObserver");
    observer.observe(&outer.native_element());
    next_frames().await;
    painted.borrow_mut().clear();
    resize(700.0);
    next_frames().await;
    observer.disconnect();
    assert_eq!(
        painted
            .borrow()
            .first()
            .map(|(_, direction)| direction.as_str()),
        Some("row"),
        "広がった幅の最初の報告で、もう横に並んでいること: {:?}",
        painted.borrow()
    );

    form.set_breakpoint(None);
    assert_eq!(
        computed(&form.native_element(), "flex-direction"),
        "column",
        "しきい値を外すと自分の向きへすぐ戻ること"
    );
    // しきい値の向きの側にいる間に自分の向きを変えても、並びはしきい値の向きのまま。
    form.set_breakpoint(Some(naui_core::Breakpoint::new(
        400.0,
        Orientation::Vertical,
    )));
    form.set_orientation(Orientation::Horizontal);
    assert_eq!(form.orientation(), Orientation::Horizontal);
    assert_eq!(form.current_orientation(), Orientation::Vertical);
    resize(380.0);
    frames_until(|| form.current_orientation() == Orientation::Horizontal).await;
    check(
        380.0,
        Orientation::Horizontal,
        "狭まれば新しい自分の向きになる",
    );

    window
        .remove_event_listener_with_callback("error", on_error.as_ref().unchecked_ref())
        .expect("error の購読の解除");
    let loops: Vec<_> = errors
        .borrow()
        .iter()
        .filter(|message| message.contains("ResizeObserver"))
        .cloned()
        .collect();
    assert!(
        loops.is_empty(),
        "ResizeObserver のループが報告されないこと: {loops:?}"
    );
}

/// Collects the messages of the page's `error` events while it is alive.
struct PageErrors {
    messages: Rc<RefCell<Vec<String>>>,
    listener: wasm_bindgen::closure::Closure<dyn FnMut(JsValue)>,
}

impl PageErrors {
    fn listen() -> Self {
        let messages = Rc::new(RefCell::new(Vec::new()));
        let listener = wasm_bindgen::closure::Closure::<dyn FnMut(JsValue)>::new({
            let messages = messages.clone();
            move |event: JsValue| {
                let message = js_sys::Reflect::get(&event, &JsValue::from_str("message"))
                    .ok()
                    .and_then(|message| message.as_string())
                    .unwrap_or_default();
                messages.borrow_mut().push(message);
            }
        });
        web_sys::window()
            .expect("window")
            .add_event_listener_with_callback("error", listener.as_ref().unchecked_ref())
            .expect("error の購読");
        Self { messages, listener }
    }

    /// The `ResizeObserver` loop reports seen so far.
    fn resize_loops(&self) -> Vec<String> {
        self.messages
            .borrow()
            .iter()
            .filter(|message| message.contains("ResizeObserver"))
            .cloned()
            .collect()
    }
}

impl Drop for PageErrors {
    fn drop(&mut self) {
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback(
                "error",
                self.listener.as_ref().unchecked_ref(),
            );
        }
    }
}

/// A stack switching inside other stacks with breakpoints resizes them in the same
/// frame: a parent with an `Auto` height gets shorter, and a sibling in a row gets
/// another width. Neither is reported as a `ResizeObserver` loop.
#[wasm_bindgen_test]
async fn nested_stack_breakpoints_do_not_loop() {
    let errors = PageErrors::listen();
    let mut kept = None;
    with_ui(|ui| {
        let outer = ui.stack(Orientation::Vertical)?;
        outer.set_sizing(Sizing::fixed(700.0, 400.0));
        // 親は 700 では切り替わらないが、子が横に並ぶと背が縮む。
        let parent = ui.stack(Orientation::Vertical)?;
        parent.set_sizing(Sizing::fill_width());
        parent.set_breakpoint(Some(naui_core::Breakpoint::new(
            1000.0,
            Orientation::Horizontal,
        )));
        let child = ui.stack(Orientation::Vertical)?;
        child.set_sizing(Sizing::fill_width());
        for title in ["一", "二", "三"] {
            child.append(&ui.button(title)?);
        }
        child.set_breakpoint(Some(naui_core::Breakpoint::new(
            500.0,
            Orientation::Horizontal,
        )));
        parent.append(&child);
        // 横の並びで両方が余りを分け合うと、片方が向きを変えて中身の幅が
        // 変わるだけで、隣の幅も変わる。
        let row = ui.stack(Orientation::Horizontal)?;
        row.set_sizing(Sizing::fill_width());
        let left = ui.stack(Orientation::Vertical)?;
        left.set_sizing(Sizing::fill_width());
        for title in ["左の一つ目", "左の二つ目"] {
            left.append(&ui.button(title)?);
        }
        left.set_breakpoint(Some(naui_core::Breakpoint::new(
            250.0,
            Orientation::Horizontal,
        )));
        let right = ui.stack(Orientation::Vertical)?;
        right.set_sizing(Sizing::fill_width());
        right.append(&ui.button("右")?);
        right.set_breakpoint(Some(naui_core::Breakpoint::new(
            2000.0,
            Orientation::Horizontal,
        )));
        row.append(&left);
        row.append(&right);
        parent.append(&row);
        outer.append(&parent);
        let mounted = Mounted::new(&outer);
        kept = Some((mounted, outer, parent, child, left));
        Ok(())
    });
    let (_mounted, _outer, parent, child, left) = kept.expect("組み立て");
    frames_until(|| {
        child.current_orientation() == Orientation::Horizontal
            && left.current_orientation() == Orientation::Horizontal
    })
    .await;
    next_frames().await;
    assert_eq!(child.current_orientation(), Orientation::Horizontal);
    assert_eq!(left.current_orientation(), Orientation::Horizontal);
    assert_eq!(parent.current_orientation(), Orientation::Vertical);
    let loops = errors.resize_loops();
    assert!(
        loops.is_empty(),
        "ResizeObserver のループが報告されないこと: {loops:?}"
    );
}

/// `AdaptivePanes` shows the list (at its fixed width) and the detail side by side
/// while at least the breakpoint wide and one at a time below it, before the page
/// is painted at the new width. While only the detail is shown, the back button
/// returns to the list, notifies, and takes the focus out of the hidden detail.
#[wasm_bindgen_test]
async fn adaptive_panes_follow_their_width() {
    let mut kept = None;
    with_ui(|ui| {
        let outer = ui.stack(Orientation::Vertical)?;
        outer.set_sizing(Sizing::fixed(800.0, 300.0));
        let panes = ui.adaptive_panes()?;
        panes.set_sizing(Sizing::fill());
        let list = ui.button("一覧")?;
        let field = ui.text_input("詳細")?;
        panes.set_list(&list);
        panes.set_detail(&field);
        outer.append(&panes);
        assert!(!panes.is_collapsed(), "並ぶ前は 2 区画であること");
        let mounted = Mounted::new(&outer);
        kept = Some((mounted, outer, panes, list, field));
        Ok(())
    });
    let (_mounted, outer, panes, list, field) = kept.expect("組み立て");
    let backs = Rc::new(Cell::new(0));
    panes.on_back({
        let backs = backs.clone();
        move || backs.set(backs.get() + 1)
    });
    next_frames().await;
    let shown = |widget: &dyn Widget| {
        let rect = widget.native_element().get_bounding_client_rect();
        rect.width() > 0.0 && rect.height() > 0.0
    };
    let back = panes.native_back_button();
    assert!(!panes.is_collapsed());
    assert!(shown(&list) && shown(&field) && back.hidden());
    let (l, d) = (
        list.native_element().get_bounding_client_rect(),
        field.native_element().get_bounding_client_rect(),
    );
    assert!(
        (l.width() - naui_core::DEFAULT_LIST_PANE_WIDTH).abs() < 0.5 && d.left() >= l.right(),
        "一覧が決まった幅で左に、詳細がその右に並ぶこと: {} / {}",
        l.width(),
        d.left()
    );

    panes.show(Pane::Detail);
    assert!(
        shown(&list) && back.hidden(),
        "2 区画のときは戻るボタンを出さない"
    );

    let resize = |width: f64| outer.set_sizing(Sizing::fixed(width, 300.0));
    resize(500.0);
    frames_until(|| panes.is_collapsed()).await;
    assert!(
        panes.is_collapsed(),
        "しきい値より狭ければ 1 区画になること"
    );
    assert!(
        !shown(&list) && shown(&field) && !back.hidden(),
        "広いときに選んだ詳細が戻るボタン付きで出ること"
    );
    assert!(
        back.text_content()
            .is_some_and(|text| text.contains(naui_core::ui_strings().back)),
        "戻るボタンに naui の文字列が付くこと"
    );

    let input: HtmlElement = field.native_element().unchecked_into();
    input.focus().expect("フォーカス");
    back.click();
    assert_eq!(backs.get(), 1, "戻るボタンで on_back が呼ばれること");
    assert_eq!(panes.visible_pane(), Pane::List);
    assert!(shown(&list) && !shown(&field) && back.hidden());
    let document = web_sys::window()
        .and_then(|window| window.document())
        .expect("document");
    assert!(
        document
            .active_element()
            .is_some_and(|active| active == list.native_element()),
        "隠れた詳細からフォーカスが一覧へ移ること"
    );
    panes.show(Pane::Detail);
    panes.show(Pane::List);
    assert_eq!(backs.get(), 1, "show では on_back を呼ばないこと");

    for (width, collapsed, message) in [
        (636.0, true, "しきい値に届くまでは 1 区画のまま"),
        (640.0, false, "ちょうどしきい値で 2 区画になる"),
        (634.0, false, "しきい値のすぐ下では 1 区画に戻らない"),
        (630.0, true, "遊びを越えて狭まれば 1 区画に戻る"),
    ] {
        resize(width);
        frames_until(|| panes.is_collapsed() == collapsed).await;
        next_frames().await;
        assert_eq!(panes.is_collapsed(), collapsed, "{message} (幅 {width})");
    }

    panes.set_breakpoint(400.0);
    assert!(
        !panes.is_collapsed(),
        "しきい値を変えるといまの幅ですぐ決め直すこと"
    );
    panes.set_list_width(200.0);
    let l = list.native_element().get_bounding_client_rect();
    assert!(
        (l.width() - 200.0).abs() < 0.5,
        "一覧の幅を変えられること: {}",
        l.width()
    );
}

/// The breakpoint is measured on the stack's border box, so growing only its
/// padding (the content keeps its width) still crosses it.
#[wasm_bindgen_test]
async fn stack_breakpoint_follows_its_padding() {
    let mut kept = None;
    with_ui(|ui| {
        // 横の Stack の子は中身の幅になるので、パディングの分だけ外幅が変わる。
        let outer = ui.stack(Orientation::Horizontal)?;
        let form = ui.stack(Orientation::Vertical)?;
        form.set_padding(Padding::all(10.0));
        let item = ui.button("項目")?;
        item.set_sizing(Sizing::fixed(480.0, 30.0));
        form.append(&item);
        outer.append(&form);
        form.set_breakpoint(Some(naui_core::Breakpoint::new(
            510.0,
            Orientation::Horizontal,
        )));
        let mounted = Mounted::new(&outer);
        kept = Some((mounted, outer, form, item));
        Ok(())
    });
    let (_mounted, _outer, form, _item) = kept.expect("組み立て");
    next_frames().await;
    let width = form.native_element().get_bounding_client_rect().width();
    assert!(
        (width - 500.0).abs() < 0.5,
        "外幅は中身とパディング: {width}"
    );
    assert_eq!(form.current_orientation(), Orientation::Vertical);

    form.set_padding(Padding::all(20.0));
    frames_until(|| form.current_orientation() == Orientation::Horizontal).await;
    assert_eq!(
        form.current_orientation(),
        Orientation::Horizontal,
        "パディングだけで外幅がしきい値を越えても横に並ぶこと"
    );
}

/// Switching a `Stack`'s orientation rewrites `flex-direction` and what each child
/// set for the previous main axis: `Fill` on the width stretches a child across a
/// column and hands it the leftover width of a row. Later `set_sizing` calls follow
/// the new direction.
#[wasm_bindgen_test]
fn stack_switches_orientation() {
    with_ui(|ui| {
        let stack = ui.stack(Orientation::Vertical)?;
        stack.set_sizing(Sizing::fixed(400.0, 200.0));
        stack.set_spacing(8.0);
        let first = ui.button("前")?;
        let wide = ui.button("広がる")?;
        wide.set_sizing(Sizing::fill_width());
        stack.append(&first);
        stack.append(&wide);
        let _mounted = Mounted::new(&stack);
        let rect = |widget: &dyn Widget| widget.native_element().get_bounding_client_rect();
        let style = |widget: &dyn Widget| {
            widget
                .native_element()
                .unchecked_into::<HtmlElement>()
                .style()
        };
        let outer = rect(&stack);
        assert!(
            (rect(&wide).width() - 400.0).abs() < 0.5,
            "縦並びでは幅の Fill がスタックの幅になること"
        );

        stack.set_orientation(Orientation::Horizontal);
        assert_eq!(stack.orientation(), Orientation::Horizontal);
        let (a, b) = (rect(&first), rect(&wide));
        assert!(
            (b.left() - a.right() - 8.0).abs() < 0.5 && (a.top() - b.top()).abs() < 1.0,
            "間隔を空けて横に並ぶこと: {} / {}",
            a.right(),
            b.left()
        );
        assert!(
            (b.right() - outer.right()).abs() < 0.5,
            "幅の Fill が横の余りを受け取ること: {} / {}",
            b.right(),
            outer.right()
        );
        assert_eq!(
            style(&wide)
                .get_property_value("align-self")
                .ok()
                .as_deref(),
            Some(""),
            "前の交差軸のための stretch を持ち越さないこと"
        );
        first.set_sizing(Sizing::fill_height());
        let tall = rect(&first).height();
        assert!(
            (tall - 200.0).abs() < 0.5,
            "向きを変えたあとの set_sizing が新しい交差軸に効くこと: {tall}"
        );
        first.set_sizing(Sizing::AUTO);

        stack.set_orientation(Orientation::Vertical);
        let (a, b) = (rect(&first), rect(&wide));
        assert!(
            (b.top() - a.bottom() - 8.0).abs() < 0.5,
            "縦へ戻すと間隔を空けて縦に並ぶこと: {} / {}",
            a.bottom(),
            b.top()
        );
        assert!(
            (b.width() - 400.0).abs() < 0.5,
            "縦へ戻すと幅の Fill がスタックの幅へ戻ること: {}",
            b.width()
        );
        assert_eq!(
            style(&wide).get_property_value("flex-grow").ok().as_deref(),
            Some(""),
            "前の主軸のための flex-grow を持ち越さないこと"
        );
        Ok(())
    });
}

/// A child moved from a `Grid` into a `Stack` follows the stack, with nothing left
/// over from the grid cell.
#[wasm_bindgen_test]
fn moved_child_follows_the_new_parent() {
    with_ui(|ui| {
        let grid = ui.grid()?;
        let child = ui.button("移る")?;
        child.set_sizing(Sizing::fill().max_height(30.0));
        grid.attach(&child, GridCell::new(0, 0));
        grid.remove(GridCell::new(0, 0));

        let stack = ui.stack(Orientation::Vertical)?;
        stack.set_sizing(Sizing::fixed(300.0, 200.0));
        stack.append(&child);
        stack.append(&ui.button("下")?);
        let _mounted = Mounted::new(&stack);

        let style = child
            .native_element()
            .unchecked_into::<HtmlElement>()
            .style();
        for property in ["justify-self", "height"] {
            assert_eq!(
                style.get_property_value(property).ok().as_deref(),
                Some(""),
                "グリッドのための {property} を持ち越さないこと"
            );
        }
        let rect = child.native_element().get_bounding_client_rect();
        assert!(
            (rect.width() - 300.0).abs() < 0.5,
            "新しい親の幅になること: {}",
            rect.width()
        );
        Ok(())
    });
}

/// Inserting into or removing from a `Stack` does not move the focus out of a field
/// that has it.
#[wasm_bindgen_test]
fn stack_insert_keeps_focus() {
    with_ui(|ui| {
        let stack = ui.stack(Orientation::Vertical)?;
        let input = ui.text_input("")?;
        stack.append(&input);
        let _mounted = Mounted::new(&stack);
        assert!(input.request_focus());

        stack.insert(0, &ui.label("見出し")?);
        stack.append(&ui.label("末尾")?);
        stack.remove(0);
        let active = active_element().expect("activeElement");
        assert!(
            input.native_element().contains(Some(&active)),
            "ほかの子を出し入れしても入力欄がフォーカスを持つこと"
        );
        stack.set_orientation(Orientation::Horizontal);
        let active = active_element().expect("activeElement");
        assert!(
            input.native_element().contains(Some(&active)),
            "向きを変えても入力欄がフォーカスを持つこと"
        );
        Ok(())
    });
}

/// A widget moved to another container keeps the scroll position of the `Scroll`s
/// inside, as on the other platforms (the browser alone would show them from the
/// top again), while the focus leaves it.
#[wasm_bindgen_test]
fn moved_widget_keeps_its_scroll_position() {
    with_ui(|ui| {
        let root = ui.stack(Orientation::Vertical)?;
        let first = ui.stack(Orientation::Vertical)?;
        let second = ui.stack(Orientation::Vertical)?;
        root.append(&first);
        root.append(&second);
        let field = ui.text_input("")?;
        first.append(&field);
        let scroll = ui.scroll()?;
        scroll.set_sizing(Sizing::fixed(200.0, 80.0));
        let tall = ui.stack(Orientation::Vertical)?;
        for index in 0..30 {
            tall.append(&ui.label(&format!("行 {index}"))?);
        }
        scroll.set_child(&tall);
        let pane = ui.stack(Orientation::Vertical)?;
        pane.append(&scroll);
        first.append(&pane);
        let _mounted = Mounted::new(&root);
        assert!(field.request_focus());
        scroll.scroll_to(0.0, 200.0);
        assert_eq!(scroll.metrics().y, 200.0);

        // Scroll そのものと、Scroll を含む入れ物の両方を移す。
        first.remove(1);
        first.remove(0);
        second.append(&field);
        second.append(&pane);
        assert_eq!(
            scroll.metrics().y,
            200.0,
            "入れ物ごと移してもスクロール位置が残ること"
        );
        pane.remove(0);
        second.append(&scroll);
        assert_eq!(
            scroll.metrics().y,
            200.0,
            "Scroll そのものを移してもスクロール位置が残ること"
        );
        // 文書に載っていない入れ物を経由しても、載った時点で位置が戻る。
        // 大きさは移す前と同じなので、ResizeObserver は呼ばれない。
        second.remove(1);
        let detached = ui.stack(Orientation::Vertical)?;
        detached.append(&scroll);
        second.append(&detached);
        assert_eq!(
            scroll.metrics().y,
            200.0,
            "載っていない入れ物を経由して戻してもスクロール位置が残ること"
        );
        let focused =
            active_element().is_some_and(|active| field.native_element().contains(Some(&active)));
        assert!(!focused, "移した入力欄からはフォーカスが外れること");
        Ok(())
    });
}

/// A pane moved into a tab keeps the scroll position of the `Scroll` inside. The
/// tab's area is put into the document after the pane is placed in it, so the
/// position has to be restored once the tab is in the document; with the same size
/// before and after, `ResizeObserver` does not report anything.
#[wasm_bindgen_test]
fn moved_into_a_tab_keeps_its_scroll_position() {
    with_ui(|ui| {
        let root = ui.stack(Orientation::Vertical)?;
        let pane = ui.stack(Orientation::Vertical)?;
        let (scroll, _content) = tall_scroll(ui, 40)?;
        pane.append(&scroll);
        root.append(&pane);
        let tabs = ui.tabs()?;
        let _root = Mounted::new(&root);
        let _tabs = Mounted::new(&tabs);
        scroll.scroll_to(0.0, 200.0);
        assert_eq!(scroll.metrics().y, 200.0);

        root.remove(0);
        tabs.add_tab("移した", &pane);
        assert_eq!(
            scroll.metrics().y,
            200.0,
            "タブへ移してもスクロール位置が残ること"
        );
        Ok(())
    });
}

#[wasm_bindgen_test]
fn stack_inserts_and_removes_children() {
    with_ui(|ui| {
        let stack = ui.stack(Orientation::Vertical)?;
        let first = ui.label("A")?;
        stack.append(&first);
        stack.append(&ui.label("C")?);
        stack.insert(1, &ui.label("B")?);
        let _mounted = Mounted::new(&stack);

        assert_eq!(stack.len(), 3);
        assert_eq!(child_texts(&stack), ["A", "B", "C"]);

        // 範囲外の index は末尾へ足す。
        stack.insert(99, &ui.label("D")?);
        assert_eq!(child_texts(&stack), ["A", "B", "C", "D"]);

        stack.remove(1);
        assert_eq!(stack.len(), 3);
        assert_eq!(child_texts(&stack), ["A", "C", "D"]);

        // 範囲外の index は何もしない。
        stack.remove(9);
        assert_eq!(stack.len(), 3);

        stack.clear();
        assert!(stack.is_empty());
        assert!(child_texts(&stack).is_empty());
        assert!(
            first.native_element().parent_element().is_none(),
            "外した子は DOM からも抜けること"
        );

        // 空にした後もふつうに積める。
        stack.append(&ui.label("F")?);
        assert_eq!(child_texts(&stack), ["F"]);
        Ok(())
    });
}

/// Children of a `Grid` that fit their content stay at the start of the cell, and
/// only `Fill` children stretch to the cell's width.
///
/// CSS Grid stretches children by default (`justify-items: stretch`), which on the
/// Web alone would widen buttons and drop-downs to the whole cell. A `Fill` child
/// must also stretch when the browser's own stylesheet gives it a width
/// (`<progress>`).
#[wasm_bindgen_test]
fn grid_keeps_auto_children_at_the_start() {
    with_ui(|ui| {
        let grid = ui.grid()?;
        grid.set_column_track(0, naui_core::Track::Auto);
        grid.set_column_track(1, naui_core::Track::FILL);
        grid.set_column_track(2, naui_core::Track::Auto);
        grid.set_spacing(16.0, 8.0);
        grid.set_sizing(Sizing::fixed(600.0, 200.0));

        let name = ui.label("Button")?;
        let button = ui.button("Click")?;
        grid.attach(&name, GridCell::new(0, 0));
        grid.attach(&button, GridCell::new(1, 0).span(2, 1));

        let progress = ui.progress_bar()?;
        progress.set_sizing(Sizing::fill_width());
        let value = ui.label("40%")?;
        grid.attach(&ui.label("ProgressBar")?, GridCell::new(0, 1));
        grid.attach(&progress, GridCell::new(1, 1));
        grid.attach(&value, GridCell::new(2, 1));

        let combo = ui.combo_box()?;
        combo.set_items(&["System", "Light", "Dark"]);
        grid.attach(&combo, GridCell::new(1, 2));
        let _mounted = Mounted::new(&grid);

        let grid_rect = grid.native_element().get_bounding_client_rect();
        let button_rect = button.native_element().get_bounding_client_rect();
        let progress_rect = progress.native_element().get_bounding_client_rect();
        let value_rect = value.native_element().get_bounding_client_rect();
        let combo_rect = combo.native_element().get_bounding_client_rect();

        assert!(
            button_rect.width() < 200.0,
            "中身に合わせるボタンがマスの幅まで伸びないこと: {}",
            button_rect.width()
        );
        assert!(
            combo_rect.width() < 300.0,
            "中身に合わせる選択欄がマスの幅まで伸びないこと: {}",
            combo_rect.width()
        );
        assert!(
            (button_rect.left() - progress_rect.left()).abs() < 1.0,
            "伸びない子もマスの先頭に置かれること: {} / {}",
            button_rect.left(),
            progress_rect.left()
        );
        // Fill の子は、名前と値の列を除いた残りを受け取る。
        assert!(
            progress_rect.width() > 300.0,
            "Fill の子がマスの幅まで伸びること: {}",
            progress_rect.width()
        );
        assert!(
            progress_rect.right() <= value_rect.left() + 0.5
                && value_rect.right() <= grid_rect.right() + 0.5,
            "Fill の子が値の列と重ならないこと: {} / {}",
            progress_rect.right(),
            value_rect.left()
        );
        Ok(())
    });
}

/// A `Grid` can remove children by cell.
/// A `Fill` child with a maximum grows up to it and sits in the middle of a taller
/// cell vertically, as on GTK, WinUI and macOS. A `stretch` stopped by `max-height`
/// would sit at the top. Horizontally it stays at the start, as on the others.
#[wasm_bindgen_test]
fn grid_places_a_capped_fill_child_in_the_middle() {
    with_ui(|ui| {
        let grid = ui.grid()?;
        grid.set_column_track(0, naui_core::Track::FILL);
        grid.set_row_track(0, naui_core::Track::FILL);
        grid.set_sizing(Sizing::fixed(400.0, 300.0));
        let capped = ui.stack(Orientation::Vertical)?;
        capped.set_sizing(Sizing::fill().max_width(200.0).max_height(100.0));
        grid.attach(&capped, GridCell::new(0, 0));
        let _mounted = Mounted::new(&grid);

        let grid_rect = grid.native_element().get_bounding_client_rect();
        let rect = capped.native_element().get_bounding_client_rect();
        assert_eq!(
            (rect.width(), rect.height()),
            (200.0, 100.0),
            "上限まで広がる"
        );
        assert_eq!(rect.left(), grid_rect.left(), "横はマスの先頭");
        assert_eq!(rect.top() - grid_rect.top(), 100.0, "縦はマスの中央");

        // マスが上限より低ければ、マスに合わせて縮む。
        grid.set_sizing(Sizing::fixed(400.0, 60.0));
        let rect = capped.native_element().get_bounding_client_rect();
        assert_eq!(rect.height(), 60.0, "マスより大きくならない");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn grid_removes_children_by_cell() {
    with_ui(|ui| {
        let grid = ui.grid()?;
        let name = ui.label("名前")?;
        let field = ui.text_input("")?;
        grid.attach(&name, GridCell::new(0, 0));
        grid.attach(&field, GridCell::new(1, 0));
        let _mounted = Mounted::new(&grid);
        assert_eq!(grid.len(), 2);

        grid.remove(GridCell::new(0, 0));
        assert_eq!(grid.len(), 1);
        assert!(name.native_element().parent_element().is_none());
        assert!(field.native_element().parent_element().is_some());

        // 何も無いマスを指定しても何も起きない。
        grid.remove(GridCell::new(0, 0));
        assert_eq!(grid.len(), 1);

        // replace は「そのマスだけ」差し替える。
        grid.attach(&ui.label("表示名")?, GridCell::new(0, 0));
        grid.replace(&ui.label("別名")?, GridCell::new(0, 0));
        assert_eq!(grid.len(), 2, "replace は他のマスの子を外さないこと");

        grid.clear();
        assert!(grid.is_empty());
        assert!(field.native_element().parent_element().is_none());
        Ok(())
    });
}

/// Removing tabs does not shift the indices reported when the remaining tabs are
/// clicked.
#[wasm_bindgen_test]
fn tabs_remove_and_clear() {
    with_ui(|ui| {
        let tabs = ui.tabs()?;
        let first = ui.label("1 枚目")?;
        tabs.add_tab("A", &first);
        tabs.add_tab("B", &ui.label("2 枚目")?);
        tabs.add_tab("C", &ui.label("3 枚目")?);
        let _mounted = Mounted::new(&tabs);
        assert_eq!(tabs.len(), 3);

        let seen = Rc::new(RefCell::new(Vec::new()));
        tabs.on_select({
            let seen = seen.clone();
            move |index| seen.borrow_mut().push(index)
        });

        tabs.set_selected(2);
        assert_eq!(tabs.selected(), Some(2));

        // 選択より前のタブを外すと、選択は同じタブへ付いていく。
        tabs.remove_tab(0);
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs.selected(), Some(1));
        assert!(seen.borrow().is_empty(), "外したことは通知しないこと");
        assert!(
            !first.native_element().is_connected(),
            "外したタブの中身は (囲みの <div> ごと) ページから抜けること"
        );

        // 残ったタブ (もとの B) をブラウザから押すと、詰めた後の位置が届く。
        let tablist = tabs.native_element().children().item(0).expect("tablist");
        let head: HtmlElement = tablist
            .children()
            .item(0)
            .expect("1 枚目のタブ")
            .unchecked_into();
        head.click();
        assert_eq!(*seen.borrow(), vec![0]);
        assert_eq!(tabs.selected(), Some(0));

        tabs.clear();
        assert!(tabs.is_empty());
        assert_eq!(tabs.selected(), None);
        assert_eq!(tablist.children().length(), 0);

        // 空にした後もふつうに足せる。
        tabs.add_tab("D", &ui.label("新しい 1 枚目")?);
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs.selected(), Some(0));
        Ok(())
    });
}

// ---------------------------------------------------------------- List

/// `ListItem::detail` appears small and dim on a second line.
///
/// Row contents are built from naui's `Stack` and `Label`, so their smallness and
/// dimness are decided by [`TextStyle::Caption`] and [`TextColor::Secondary`]. This
/// checks, with the values after the browser has resolved them, that those take
/// effect.
#[wasm_bindgen_test]
fn list_detail_makes_a_second_line() {
    with_ui(|ui| {
        let list = ui.list()?;
        list.set_items(&[
            ListItem::new("東京"),
            ListItem::new("大阪").detail("2,750,000 人"),
        ]);
        let _mounted = Mounted::new(&list);

        // `detail` がある行が 1 つでもあれば、一覧は `<ul role="listbox">` になる。
        let listbox = list
            .native_element()
            .children()
            .item(0)
            .expect("listbox の枠");
        assert_eq!(listbox.tag_name(), "UL");
        let plain = listbox.children().item(0).expect("1 行目");
        let detailed = listbox.children().item(1).expect("2 行目");

        let texts = |row: &Element| -> Vec<HtmlElement> {
            let found = row.get_elements_by_tag_name("span");
            (0..found.length())
                .filter_map(|i| found.item(i))
                .map(|element| element.unchecked_into())
                .collect()
        };
        let plain_texts = texts(&plain);
        let detailed_texts = texts(&detailed);
        assert_eq!(plain_texts.len(), 1, "detail が無い行は文字 1 本");
        assert_eq!(detailed_texts.len(), 2, "detail がある行は文字 2 本");
        assert_eq!(
            detailed_texts[1].text_content().unwrap_or_default(),
            "2,750,000 人"
        );

        let resolved = |element: &HtmlElement, property: &str| {
            web_sys::window()
                .and_then(|w| w.get_computed_style(element).ok().flatten())
                .and_then(|style| style.get_property_value(property).ok())
                .unwrap_or_default()
        };
        let size = |element: &HtmlElement| {
            resolved(element, "font-size")
                .trim_end_matches("px")
                .parse::<f64>()
                .expect("font-size が px で返ること")
        };
        assert!(
            size(&detailed_texts[1]) < size(&detailed_texts[0]),
            "補助の文字のほうが小さいこと: {} / {}",
            size(&detailed_texts[0]),
            size(&detailed_texts[1])
        );
        assert_ne!(
            resolved(&detailed_texts[1], "color"),
            resolved(&detailed_texts[0], "color"),
            "補助の文字のほうが淡いこと"
        );
        Ok(())
    });
}

/// Fires a mouse event (`dblclick` / `contextmenu`) at the element's center. **The
/// return value is whether the default action was prevented.**
fn mouse_at_center(target: &Element, kind: &str, button: i16) -> bool {
    let rect = target.get_bounding_client_rect();
    mouse_at(
        target,
        kind,
        button,
        rect.left() + rect.width() / 2.0,
        rect.top() + rect.height() / 2.0,
    )
}

/// Fires a mouse event at viewport `(x, y)`. **The return value is whether the
/// default action was prevented.**
fn mouse_at(target: &Element, kind: &str, button: i16, x: f64, y: f64) -> bool {
    let init = MouseEventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_button(button);
    init.set_client_x(x as i32);
    init.set_client_y(y as i32);
    let event =
        MouseEvent::new_with_mouse_event_init_dict(kind, &init).expect("マウスイベントの生成");
    !target.dispatch_event(&event).expect("イベントの配送")
}

/// `target`'s center, in coordinates from `list`'s top left.
fn center_within(list: &Element, target: &Element) -> (f64, f64) {
    let outer = list.get_bounding_client_rect();
    let rect = target.get_bounding_client_rect();
    (
        (rect.left() + rect.width() / 2.0).trunc() - outer.left(),
        (rect.top() + rect.height() / 2.0).trunc() - outer.top(),
    )
}

/// With text-only rows (`<select size>`), double-clicks and Enter become row
/// activations, and right-clicks pass the row and position. Not called for rows
/// that cannot be selected.
#[wasm_bindgen_test]
fn list_select_row_activate_and_context_menu() {
    with_ui(|ui| {
        let list = ui.list()?;
        list.set_items(&[
            ListItem::new("札幌"),
            ListItem::new("仙台"),
            ListItem::new("東京"),
            ListItem::new("大阪").enabled(false),
        ]);
        let _mounted = Mounted::new(&list);
        let activated = Rc::new(RefCell::new(Vec::new()));
        list.on_row_activate({
            let activated = activated.clone();
            move |index| activated.borrow_mut().push(index)
        });
        let menus: Rc<RefCell<Vec<(usize, Point)>>> = Rc::new(RefCell::new(Vec::new()));
        list.on_context_menu({
            let menus = menus.clone();
            move |index, at| menus.borrow_mut().push((index, at))
        });

        let select = list.native_select().expect("文字だけなら <select>");
        let option =
            |index: u32| -> Element { select.options().item(index).expect("行").unchecked_into() };

        mouse_at_center(&option(2), "dblclick", 0);
        assert_eq!(
            *activated.borrow(),
            vec![2],
            "ダブルクリックした行が届くこと"
        );
        mouse_at_center(&option(3), "dblclick", 0);
        assert_eq!(*activated.borrow(), vec![2], "選べない行では起動しないこと");

        list.set_selected(1);
        press_enter(select.as_ref(), false);
        assert_eq!(
            *activated.borrow(),
            vec![2, 1],
            "Enter で選ばれている行が届くこと"
        );
        press_enter(select.as_ref(), true);
        assert_eq!(activated.borrow().len(), 2, "変換中の Enter は数えないこと");

        // WebKit は `<option>` に矩形を持たせない (0 になる) ので、押す位置は
        // `<select>` の左上から決める。
        let rect = select.get_bounding_client_rect();
        let (client_x, client_y) = (rect.left().trunc() + 12.0, rect.top().trunc() + 6.0);
        let prevented = mouse_at(&option(0), "contextmenu", 2, client_x, client_y);
        assert!(prevented, "行の上ではブラウザ既定のメニューを抑止すること");
        {
            let menus = menus.borrow();
            assert_eq!(menus.len(), 1);
            assert_eq!(menus[0].0, 0, "右クリックした行のインデックスが届くこと");
            let outer = list.native_element().get_bounding_client_rect();
            let (x, y) = (client_x - outer.left(), client_y - outer.top());
            assert!(
                (menus[0].1.x - x).abs() < 1.0 && (menus[0].1.y - y).abs() < 1.0,
                "位置がリストの左上からの座標であること: {:?} / ({x}, {y})",
                menus[0].1
            );
        }
        assert_eq!(
            list.selection(),
            vec![1],
            "右クリックでは選択が変わらないこと"
        );
        Ok(())
    });
}

/// The composed list (`<ul role="listbox">`) receives them the same way, and
/// double-clicks over a control inside a row do not activate the row.
#[wasm_bindgen_test]
fn list_listbox_row_activate_and_context_menu() {
    with_ui(|ui| {
        let content = ui.stack(Orientation::Horizontal)?;
        content.append(&ui.label("Wi-Fi")?);
        let button = ui.button("設定…")?;
        content.append(&button);
        let list = ui.list()?;
        list.set_rows(&[
            ListRow::new(&ui.label("札幌")?),
            ListRow::new(&content),
            ListRow::new(&ui.label("東京")?),
        ]);
        let _mounted = Mounted::new(&list);
        let activated = Rc::new(RefCell::new(Vec::new()));
        list.on_row_activate({
            let activated = activated.clone();
            move |index| activated.borrow_mut().push(index)
        });
        let menus: Rc<RefCell<Vec<(usize, Point)>>> = Rc::new(RefCell::new(Vec::new()));
        list.on_context_menu({
            let menus = menus.clone();
            move |index, at| menus.borrow_mut().push((index, at))
        });

        let listbox = list
            .native_element()
            .children()
            .item(0)
            .expect("listbox の枠");
        assert_eq!(listbox.tag_name(), "UL");
        let row = |index: u32| listbox.children().item(index).expect("行");

        mouse_at_center(&row(2), "dblclick", 0);
        assert_eq!(
            *activated.borrow(),
            vec![2],
            "ダブルクリックした行が届くこと"
        );
        mouse_at_center(&button.native_element(), "dblclick", 0);
        assert_eq!(
            *activated.borrow(),
            vec![2],
            "行内のボタンのダブルクリックでは起動しないこと"
        );

        // クリックで指した行を、Enter で起動する。
        row(0).unchecked_ref::<HtmlElement>().click();
        press_enter(listbox.as_ref(), false);
        assert_eq!(
            *activated.borrow(),
            vec![2, 0],
            "Enter で指している行が届くこと"
        );

        // 行内のボタンの上の右クリックも、その行の文脈メニューになる。
        let prevented = mouse_at_center(&button.native_element(), "contextmenu", 2);
        assert!(prevented, "行の上ではブラウザ既定のメニューを抑止すること");
        {
            let menus = menus.borrow();
            assert_eq!(menus.len(), 1);
            assert_eq!(menus[0].0, 1, "ボタンを含む行のインデックスが届くこと");
            let (x, y) = center_within(&list.native_element(), &button.native_element());
            assert!(
                (menus[0].1.x - x).abs() < 1.0 && (menus[0].1.y - y).abs() < 1.0,
                "位置がリストの左上からの座標であること: {:?} / ({x}, {y})",
                menus[0].1
            );
        }

        // 余白 (行の外) では通知せず、ブラウザ既定のメニューも止めない。
        let prevented = mouse_at_center(&listbox, "contextmenu", 2);
        assert!(
            !prevented,
            "行の無いところではブラウザ既定のメニューを残すこと"
        );
        assert_eq!(menus.borrow().len(), 1, "行の無いところでは通知しないこと");
        Ok(())
    });
}

/// A row's text spreads to the full width of the row, and overflow is cut off with
/// an ellipsis.
///
/// `Label` is `nowrap` + `ellipsis` by default, but **it cannot be cut without a
/// decided width**. At the content's width, a long label would stick out of the row
/// horizontally.
#[wasm_bindgen_test]
fn list_long_text_stays_inside_the_row() {
    with_ui(|ui| {
        let list = ui.list()?;
        let long = "これは行の幅にはとても収まらない、ずいぶん長いラベルの文字列です";
        list.set_items(&[ListItem::new(long).detail(long)]);
        let element: HtmlElement = list.native_element().unchecked_into();
        let _mounted = Mounted::new(&list);
        // 一覧の幅を決める。行の幅はここから決まる。
        let _ = element.style().set_property("width", "200px");

        let listbox = element.children().item(0).expect("listbox の枠");
        let row = listbox.children().item(0).expect("1 行目");
        let row_width = row.get_bounding_client_rect().width();
        assert!(row_width > 0.0, "行に幅が配られていること");

        let found = row.get_elements_by_tag_name("span");
        assert_eq!(found.length(), 2, "文字が 2 本あること");
        for index in 0..found.length() {
            let text: HtmlElement = found.item(index).expect("行の中の文字").unchecked_into();
            let width = text.get_bounding_client_rect().width();
            assert!(
                width <= row_width,
                "{index} 本目の文字が行からはみ出している: 文字 {width} / 行 {row_width}"
            );
        }
        Ok(())
    });
}

/// `Ui` can be cloned, so widgets can be created even inside callbacks.
///
/// A screen that adds a row on each click builds the row's content inside the
/// notification and passes it to `set_rows`.
#[wasm_bindgen_test]
fn ui_clone_builds_rows_from_a_callback() {
    with_ui(|ui| {
        let list = ui.list()?;
        // 行は積み上げていくので、アプリ側で並びを持つ。
        let rows: Rc<RefCell<Vec<ListRow>>> = Rc::new(RefCell::new(Vec::new()));
        let add = ui.button("行を足す")?;
        add.on_click({
            // コールバックへ持ち込むのは clone した `Ui`。中身は同じ。
            let ui = ui.clone();
            let list = list.clone();
            let rows = rows.clone();
            move || {
                let index = rows.borrow().len() + 1;
                let content = ui.stack(Orientation::Horizontal).expect("行の中身");
                content.append(&ui.label(&format!("行 {index}")).expect("行のラベル"));
                rows.borrow_mut().push(ListRow::new(&content));
                list.set_rows(&rows.borrow());
            }
        });
        let stack = ui.stack(Orientation::Vertical)?;
        stack.append(&add);
        stack.append(&list);
        let _mounted = Mounted::new(&stack);

        assert_eq!(list.len(), 0);
        add.click();
        add.click();
        assert_eq!(list.len(), 2, "コールバックの中で作った行が載ること");

        // 任意内容の行は `<ul role="listbox">` の合成になる。
        let element = list.native_element();
        let listbox = element.children().item(0).expect("listbox の枠");
        assert_eq!(listbox.tag_name(), "UL");
        assert_eq!(
            listbox.children().length(),
            2,
            "後から足した行が DOM にも出ること"
        );
        assert!(
            listbox.text_content().unwrap_or_default().contains("行 2"),
            "後から作ったラベルが行の中に入ること"
        );
        Ok(())
    });
}

// -------------------------------------------------------------- Dialog

// ------------------------------------------------------------- Table

/// The rows in the table's `<tbody>` that are not spacers.
fn table_rows(table: &dyn Widget) -> Vec<HtmlElement> {
    let body = table
        .native_element()
        .query_selector("tbody")
        .expect("tbody の検索")
        .expect("tbody");
    let rows = body
        .query_selector_all("tr[aria-rowindex]")
        .expect("行の検索");
    (0..rows.length())
        .filter_map(|i| rows.get(i))
        .map(|node| node.unchecked_into::<HtmlElement>())
        .collect()
}

/// The row's index (`aria-rowindex` is offset by the headings).
fn table_row_index(row: &HtmlElement) -> usize {
    row.get_attribute("aria-rowindex")
        .expect("aria-rowindex")
        .parse::<usize>()
        .expect("数値")
        - 2
}

/// In a table with many rows, `<tr>`s are created only for the rows on screen. The
/// scrollable height still covers all rows, and scrolling swaps the contents.
#[wasm_bindgen_test]
fn table_builds_only_the_visible_rows() {
    with_ui(|ui| {
        const ROWS: usize = 20_000;
        let table = ui.table()?;
        table.set_columns(&TableColumn::list(["番号", "名前"]));
        table.set_sizing(
            Sizing::new()
                .width(Length::Fill)
                .height(Length::Fixed(200.0)),
        );
        let mounted = Mounted::new(&table);

        table.set_rows(
            &(0..ROWS)
                .map(|i| TableRow::new([i.to_string(), format!("行 {i}")]))
                .collect::<Vec<_>>(),
        );
        assert_eq!(table.len(), ROWS);

        let realized = table_rows(&table);
        assert!(
            !realized.is_empty() && realized.len() < 200,
            "見えている分だけを作ること: {} 行",
            realized.len()
        );
        assert_eq!(table_row_index(&realized[0]), 0, "はじめは先頭から");
        assert_eq!(
            mounted.0.get_attribute("aria-rowcount").as_deref(),
            None,
            "行数は <table> 側に書く"
        );
        let native: HtmlElement = table
            .native_element()
            .query_selector("table")
            .expect("table の検索")
            .expect("table")
            .unchecked_into();
        assert_eq!(
            native.get_attribute("aria-rowcount").as_deref(),
            Some("20001"),
            "読み上げには全行数を伝えること"
        );

        // 詰め物のぶん、スクロールできる高さは全行分ある。
        let height = table.row_height();
        assert!(height > 0.0, "行の高さを測れていること: {height}");
        let scroll_height = mounted.0.scroll_height() as f64;
        let expected = ROWS as f64 * height;
        assert!(
            (scroll_height - expected).abs() < expected * 0.05,
            "全行分の高さがあること: {scroll_height} / {expected}"
        );

        // 途中までスクロールすると、その辺りの行に入れ替わる。
        let root: HtmlElement = mounted.0.clone().unchecked_into();
        root.set_scroll_top((10_000.0 * height) as i32);
        dispatch(root.as_ref(), "scroll");
        let realized = table_rows(&table);
        let first = table_row_index(&realized[0]);
        assert!(
            (9_900..=10_000).contains(&first),
            "スクロール先の行を作ること: {first} 行目から"
        );
        assert!(realized.len() < 200, "作る量は増えないこと");
        let text = realized[0].text_content().unwrap_or_default();
        assert!(
            text.contains(&first.to_string()),
            "中身も入れ替わること: {text}"
        );
        Ok(())
    });
}

/// Even when a row outside the window is selected, the selection is remembered, and
/// the row looks selected when scrolled back into view.
#[wasm_bindgen_test]
fn table_keeps_the_selection_outside_the_window() {
    with_ui(|ui| {
        let table = ui.table()?;
        table.set_columns(&TableColumn::list(["番号"]));
        table.set_sizing(
            Sizing::new()
                .width(Length::Fill)
                .height(Length::Fixed(200.0)),
        );
        let mounted = Mounted::new(&table);
        table.set_rows(
            &(0..5_000)
                .map(|i| TableRow::new([i.to_string()]))
                .collect::<Vec<_>>(),
        );

        table.set_selection(&[4_000]);
        assert_eq!(table.selection(), vec![4_000]);
        assert!(
            table_rows(&table)
                .iter()
                .all(|row| row.get_attribute("aria-selected").as_deref() == Some("false")),
            "まだ画面の外なので、選ばれた行は出ていない"
        );

        let root: HtmlElement = mounted.0.clone().unchecked_into();
        root.set_scroll_top((4_000.0 * table.row_height()) as i32);
        dispatch(root.as_ref(), "scroll");
        let selected: Vec<usize> = table_rows(&table)
            .iter()
            .filter(|row| row.get_attribute("aria-selected").as_deref() == Some("true"))
            .map(table_row_index)
            .collect();
        assert_eq!(
            selected,
            vec![4_000],
            "戻ってきたら選ばれた見た目になること"
        );
        Ok(())
    });
}

/// Even in a table with many rows, sorting by clicking a heading still works.
///
/// Sorting is the app's job, so this checks that what is passed again with
/// `set_rows` after the notification is reflected in the rows on screen.
#[wasm_bindgen_test]
fn table_sorting_works_while_windowed() {
    with_ui(|ui| {
        const ROWS: usize = 10_000;
        let table = ui.table()?;
        table.set_columns(&[
            TableColumn::new("番号").sortable(true),
            TableColumn::new("名前"),
        ]);
        table.set_sizing(
            Sizing::new()
                .width(Length::Fill)
                .height(Length::Fixed(200.0)),
        );
        let mounted = Mounted::new(&table);

        let rows: Vec<TableRow> = (0..ROWS)
            .map(|i| TableRow::new([i.to_string(), format!("項目 {i}")]))
            .collect();
        table.set_rows(&rows);

        let seen: Rc<RefCell<Vec<(usize, bool)>>> = Rc::new(RefCell::new(Vec::new()));
        table.on_sort({
            let seen = seen.clone();
            let table = table.clone();
            let rows = rows.clone();
            move |column, order| {
                seen.borrow_mut().push((column, order.is_ascending()));
                let sorted: Vec<TableRow> = match order.is_ascending() {
                    true => rows.clone(),
                    false => rows.iter().rev().cloned().collect(),
                };
                table.set_rows(&sorted);
            }
        });

        let first_cell = || {
            table_rows(&table)
                .first()
                .and_then(|row| row.query_selector("td").ok().flatten())
                .and_then(|cell| cell.text_content())
                .unwrap_or_default()
        };
        assert_eq!(first_cell(), "0");

        // 見出しの `<button>` を押す (利用者の操作と同じ経路)。
        let header: HtmlElement = mounted
            .0
            .query_selector("thead th button")
            .expect("見出しの検索")
            .expect("押せる見出し")
            .unchecked_into();
        header.click();
        assert_eq!(*seen.borrow(), vec![(0, true)]);
        header.click();
        assert_eq!(*seen.borrow(), vec![(0, true), (0, false)]);
        assert_eq!(
            first_cell(),
            (ROWS - 1).to_string(),
            "並べ替えた結果が画面の行に出ること"
        );

        // 指標は見出しに出る (読み上げ向けの aria-sort と、目に見える矢印)。
        let cell: HtmlElement = mounted
            .0
            .query_selector("thead th")
            .expect("見出しの検索")
            .expect("見出し")
            .unchecked_into();
        assert_eq!(
            cell.get_attribute("aria-sort").as_deref(),
            Some("descending")
        );
        assert!(
            cell.text_content().unwrap_or_default().contains('▼'),
            "向きの矢印が出ること"
        );
        Ok(())
    });
}

/// The headings stay when scrolling, and the rows' contents do not show through.
///
/// With collapsed borders (`border-collapse: collapse`), the table rather than the
/// cells draws the borders, so the borders do not follow sticky headings. Some
/// browsers do not draw the background either, letting the rows show through.
/// Buttons and fields in rows create their own stacking layers, so the headings need
/// a layer (`z-index`) too.
#[wasm_bindgen_test]
fn table_header_stays_above_the_rows() {
    with_ui(|ui| {
        let table = ui.table()?;
        table.set_columns(&TableColumn::list(["番号", "操作"]));
        table.set_sizing(
            Sizing::new()
                .width(Length::Fill)
                .height(Length::Fixed(120.0)),
        );
        let mounted = Mounted::new(&table);
        // セルにウィジェットを置いた行で確かめる (透けたのはこの形)。
        table.set_row_builder(50, {
            let ui = ui.clone();
            move |index| {
                let open = ui.button("開く")?;
                Ok(TableCells::new().text(index.to_string()).cell(&open))
            }
        });

        let native: Element = mounted
            .0
            .query_selector("table")
            .expect("table の検索")
            .expect("table");
        assert_ne!(
            computed(&native, "border-collapse"),
            "collapse",
            "枠を重ねると、留めた見出しに枠が付いてこない"
        );
        let header: Element = mounted
            .0
            .query_selector("thead th")
            .expect("見出しの検索")
            .expect("見出し");
        assert_eq!(computed(&header, "position"), "sticky");
        assert_ne!(
            computed(&header, "z-index"),
            "auto",
            "行の中のコントロールより上に置くこと"
        );
        // 地色は**不透明**でなければならない。行がこの下を流れるため。
        // Safari (WebKit) の暗い配色では入力欄の地色 `Field` が半透明なので、
        // 不透明な下地を敷いた上へ重ねている。
        let background = computed(&header, "background-color");
        let opaque = !background.contains("rgba(") || background.ends_with(", 1)");
        assert!(opaque, "見出しの地色が透けないこと: {background}");
        assert_ne!(
            computed(&header, "background-image"),
            "none",
            "表の地色を下地の上へ重ねること"
        );

        // スクロールしても見出しは動かない (`top: 0` に留まる)。
        let root: HtmlElement = mounted.0.clone().unchecked_into();
        let before = header.get_bounding_client_rect().y() - root.get_bounding_client_rect().y();
        root.set_scroll_top(60);
        let after = header.get_bounding_client_rect().y() - root.get_bounding_client_rect().y();
        assert!(
            (before - after).abs() < 1.0,
            "見出しが枠の上端に留まること: {before} → {after}"
        );

        Ok(())
    });
}

/// Even with built rows, whether a row can be selected is answered without creating
/// it.
#[wasm_bindgen_test]
fn table_row_selectable_answers_without_building() {
    with_ui(|ui| {
        const ROWS: usize = 10_000;
        const LOCKED: usize = 5_000;
        let built = Rc::new(Cell::new(0usize));

        let table = ui.table()?;
        table.set_columns(&TableColumn::list(["番号"]));
        table.set_sizing(
            Sizing::new()
                .width(Length::Fill)
                .height(Length::Fixed(120.0)),
        );
        let _mounted = Mounted::new(&table);
        table.set_row_builder(ROWS, {
            let built = built.clone();
            move |index| {
                built.set(built.get() + 1);
                Ok(TableCells::new()
                    .text(index.to_string())
                    .selectable(index != LOCKED))
            }
        });
        // 画面に出る分はここまでで組み立てられている。以降は増えないはず。
        let realized = built.get();

        // 述語が無いと、まだ作っていない行は選べる扱いになる。
        table.set_selection(&[LOCKED]);
        assert_eq!(table.selection(), vec![LOCKED], "述語が無ければ選べる扱い");

        // 述語を渡すと、行を組み立てずに落とせる。
        table.clear_selection();
        table.set_row_selectable(|index| index != LOCKED);
        table.set_selection(&[LOCKED]);
        assert!(table.selection().is_empty(), "選べない行は取り除かれること");
        table.set_selection(&[LOCKED - 1]);
        assert_eq!(table.selection(), vec![LOCKED - 1]);
        assert_eq!(
            built.get(),
            realized,
            "選択のために行を組み立て直さないこと"
        );
        Ok(())
    });
}

/// Even with built rows (`TableCells`), only the visible rows are created when
/// there are many, and sorting by heading still works.
#[wasm_bindgen_test]
fn table_builder_rows_scale_and_sort() {
    with_ui(|ui| {
        const ROWS: usize = 20_000;
        let built = Rc::new(Cell::new(0usize));
        let descending = Rc::new(Cell::new(false));

        let table = ui.table()?;
        table.set_columns(&[
            TableColumn::new("番号").sortable(true),
            TableColumn::new("操作"),
        ]);
        table.set_sizing(
            Sizing::new()
                .width(Length::Fill)
                .height(Length::Fixed(200.0)),
        );
        let mounted = Mounted::new(&table);

        table.set_row_builder(ROWS, {
            let ui = ui.clone();
            let built = built.clone();
            let descending = descending.clone();
            move |index| {
                built.set(built.get() + 1);
                // 並べ替えはアプリの仕事。ここでは向きを反転するだけ。
                let value = match descending.get() {
                    true => ROWS - 1 - index,
                    false => index,
                };
                let open = ui.button("開く")?;
                Ok(TableCells::new().text(value.to_string()).cell(&open))
            }
        });

        let realized = table_rows(&table).len();
        assert!(
            realized > 0 && realized < 200,
            "見えている分だけを組み立てること: {realized} 行"
        );
        assert!(
            built.get() < 400,
            "組み立てを呼ぶのも見えている行の分だけであること: {} 回",
            built.get()
        );
        assert!(
            table_rows(&table)[0]
                .query_selector("button")
                .expect("検索")
                .is_some(),
            "セルの中がボタンであること"
        );

        // 見出しを押すと、組み立てる中身のほうが入れ替わる。
        table.on_sort({
            let table = table.clone();
            let descending = descending.clone();
            move |_column, order| {
                descending.set(!order.is_ascending());
                // 行数は変わらないので `refresh` で組み立て直す。
                table.refresh();
            }
        });
        let header: HtmlElement = mounted
            .0
            .query_selector("thead th button")
            .expect("見出しの検索")
            .expect("押せる見出し")
            .unchecked_into();
        header.click();
        header.click();

        let first = table_rows(&table)
            .first()
            .and_then(|row| row.query_selector("td").ok().flatten())
            .and_then(|cell| cell.text_content())
            .unwrap_or_default();
        assert_eq!(first, (ROWS - 1).to_string(), "並べ替えた結果が出ること");
        assert!(
            built.get() < 800,
            "並べ替えでも作り直すのは見えている行だけであること: {} 回",
            built.get()
        );
        Ok(())
    });
}

/// Widgets can be placed in cells. Clicking a row becomes an activation, and
/// clicking a button in a cell does not become the row's activation.
#[wasm_bindgen_test]
fn table_widget_cells_separate_the_row_and_its_controls() {
    with_ui(|ui| {
        let activated: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
        let pressed = Rc::new(Cell::new(0));

        let table = ui.table()?;
        table.set_columns(&TableColumn::list(["都市", "操作"]));
        let mounted = Mounted::new(&table);
        {
            let ui = ui.clone();
            let activated = activated.clone();
            let pressed = pressed.clone();
            table.set_row_builder(3, move |index| {
                let button = ui.button("開く")?;
                button.on_click({
                    let pressed = pressed.clone();
                    move || pressed.set(pressed.get() + 1)
                });
                let cells = TableCells::new()
                    .cell(&ui.label(&format!("都市 {index}"))?)
                    .cell(&button)
                    .selectable(index != 1);
                cells.on_activate({
                    let activated = activated.clone();
                    move || activated.borrow_mut().push(index)
                });
                Ok(cells)
            });
        }

        let rows = table_rows(&table);
        assert_eq!(rows.len(), 3);
        assert!(!rows[1].has_attribute("aria-disabled"));
        let label_color = |index: usize| {
            computed(
                &rows[index].query_selector("span").unwrap().unwrap(),
                "color",
            )
        };
        let normal_color = label_color(0);
        assert_eq!(
            label_color(1),
            normal_color,
            "選択不可でも通常の文字色を使うこと"
        );
        assert_eq!(
            rows[0].query_selector("button").expect("検索").is_some(),
            true,
            "セルの中がボタンになっていること"
        );

        // 行そのものを押すと activation と選択が起きる。
        rows[0].click();
        assert_eq!(*activated.borrow(), vec![0]);
        assert_eq!(table.selection(), vec![0]);

        // セルの中のボタンを押しても、行の activation にはならない。
        let button: HtmlElement = rows[2]
            .query_selector("button")
            .expect("検索")
            .expect("ボタン")
            .unchecked_into();
        button.click();
        assert_eq!(pressed.get(), 1, "ボタン自身は押せること");
        assert_eq!(
            *activated.borrow(),
            vec![0],
            "行の activation は起きないこと"
        );

        // 選べない行は、選択だけが起きない (activation は起きる)。
        rows[1].click();
        assert_eq!(*activated.borrow(), vec![0, 1]);
        assert_eq!(table.selection(), vec![0], "選べない行は選ばれないこと");
        assert_eq!(
            label_color(1),
            normal_color,
            "選択の再描画でも文字色を保つこと"
        );
        drop(mounted);
        Ok(())
    });
}

/// Fixing the row height redraws the spacers at that height too.
#[wasm_bindgen_test]
fn table_row_height_can_be_fixed() {
    with_ui(|ui| {
        let table = ui.table()?;
        table.set_columns(&TableColumn::list(["番号"]));
        table.set_sizing(
            Sizing::new()
                .width(Length::Fill)
                .height(Length::Fixed(200.0)),
        );
        let mounted = Mounted::new(&table);
        table.set_rows(
            &(0..1_000)
                .map(|i| TableRow::new([i.to_string()]))
                .collect::<Vec<_>>(),
        );

        table.set_row_height(40.0);
        assert_eq!(table.row_height(), 40.0);
        let rows = table_rows(&table);
        let height = rows[0].get_bounding_client_rect().height();
        assert!(
            (height - 40.0).abs() < 1.0,
            "指定した高さになること: {height}"
        );
        let scroll_height = mounted.0.scroll_height() as f64;
        assert!(
            (scroll_height - 1_000.0 * 40.0).abs() < 200.0,
            "全行分の高さになること: {scroll_height}"
        );
        Ok(())
    });
}

// ------------------------------------------------------------- Popover

/// A button with room on every side to open popovers from, and a popover holding a
/// label. Counts `on_close` calls.
fn popover_fixture(
    ui: &Ui,
) -> Result<(
    naui_web::Stack,
    naui_web::Button,
    naui_web::Popover,
    Rc<Cell<usize>>,
)> {
    let root = ui.stack(Orientation::Vertical)?;
    root.set_padding(Padding::all(160.0));
    root.set_align(Align::Start);
    let button = ui.button("設定")?;
    root.append(&button);
    let popover = ui.popover()?;
    popover.set_child(&ui.label("中身")?);
    let closed = Rc::new(Cell::new(0));
    popover.on_close({
        let closed = closed.clone();
        move || closed.set(closed.get() + 1)
    });
    Ok((root, button, popover, closed))
}

/// Places the widget on the page, pinned to the top left of the window.
///
/// Elements earlier tests left in `<body>` make the document scroll, and WebKit
/// chooses the popover's side as if it had not scrolled (a widget scrolled into the
/// middle of the window still gets its popover above). A naui window does not scroll
/// the document, so the widget is placed the same way.
fn mount_in_view(widget: &dyn Widget) -> Mounted {
    let mounted = Mounted::new(widget);
    let style = mounted.0.unchecked_ref::<HtmlElement>().style();
    for (property, value) in [("position", "fixed"), ("top", "0"), ("left", "0")] {
        style.set_property(property, value).expect("位置の指定");
    }
    mounted
}

/// The names in the element's `anchor-name`.
fn anchor_names(element: &Element) -> Vec<String> {
    element
        .unchecked_ref::<HtmlElement>()
        .style()
        .get_property_value("anchor-name")
        .unwrap_or_default()
        .split(',')
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

/// The `position-anchor` the popover points at.
fn popover_anchor(popover: &naui_web::Popover) -> String {
    popover
        .native_element()
        .unchecked_ref::<HtmlElement>()
        .style()
        .get_property_value("position-anchor")
        .unwrap_or_default()
}

/// Whether the browser places elements with CSS anchor positioning.
fn supports_anchor_positioning() -> bool {
    web_sys::window()
        .and_then(|window| js_sys::Reflect::get(&window, &JsValue::from_str("CSS")).ok())
        .and_then(|css| {
            let supports = js_sys::Reflect::get(&css, &JsValue::from_str("supports")).ok()?;
            let supports: &js_sys::Function = supports.dyn_ref()?;
            supports
                .call1(&css, &JsValue::from_str("position-area: bottom"))
                .ok()
        })
        .is_some_and(|value| value.is_truthy())
}

#[wasm_bindgen_test]
async fn popover_opens_beside_the_widget_and_closes_quietly() {
    let mut kept = None;
    with_ui(|ui| {
        kept = Some(popover_fixture(ui)?);
        Ok(())
    });
    let (root, button, popover, closed) = kept.expect("組み立て");
    let _mounted = mount_in_view(&root);
    assert!(!popover.is_open(), "作っただけでは閉じていること");
    let element = popover.native_element();
    assert_eq!(
        element.get_attribute("popover").as_deref(),
        Some("auto"),
        "外側を押すと閉じる種類のポップオーバーであること"
    );

    popover.open(&button);
    assert!(popover.is_open());
    assert!(element.matches(":popover-open").unwrap_or(false));
    // アンカー配置の無いブラウザは `anchor-name` などを読み捨てる。
    if supports_anchor_positioning() {
        let name = popover_anchor(&popover);
        assert!(name.starts_with("--"), "アンカーの名前があること: {name}");
        assert_eq!(
            anchor_names(&button.native_element()),
            vec![name.clone()],
            "押したウィジェットがアンカーになること"
        );
    }
    assert!(
        element.text_content().unwrap_or_default().contains("中身"),
        "中身がポップオーバーの中に入ること"
    );

    if supports_anchor_positioning() {
        next_frames().await;
        let anchor = button.native_element().get_bounding_client_rect();
        let rect = element.get_bounding_client_rect();
        assert!(
            rect.top() >= anchor.bottom() - 0.5,
            "ウィジェットの下に出ること: {} / {}",
            rect.top(),
            anchor.bottom()
        );
        let center = |r: &web_sys::DomRect| r.left() + r.width() / 2.0;
        assert!(
            (center(&rect) - center(&anchor)).abs() < 1.0,
            "ウィジェットの中央にそろうこと: {} / {}",
            center(&rect),
            center(&anchor)
        );
    }

    popover.close();
    assert!(!popover.is_open());
    next_frames().await;
    assert_eq!(closed.get(), 0, "アプリが閉じたときは通知しないこと");
    assert!(
        anchor_names(&button.native_element()).is_empty(),
        "閉じたらアンカーの名前を外すこと"
    );
    element.remove();
}

#[wasm_bindgen_test]
async fn popover_dismissed_by_the_user_notifies() {
    let mut kept = None;
    with_ui(|ui| {
        kept = Some(popover_fixture(ui)?);
        Ok(())
    });
    let (root, button, popover, closed) = kept.expect("組み立て");
    let _mounted = Mounted::new(&root);

    popover.open(&button);
    popover.dismiss();
    assert!(!popover.is_open(), "閉じてすぐ閉じた扱いになること");
    next_frames().await;
    assert_eq!(closed.get(), 1, "利用者が閉じたら 1 度だけ通知すること");

    // 閉じてすぐ開き直しても、閉じた知らせで外れないこと。
    popover.open(&button);
    popover.close();
    popover.open(&button);
    next_frames().await;
    assert!(popover.is_open());
    assert_eq!(closed.get(), 1);
    if supports_anchor_positioning() {
        assert_eq!(anchor_names(&button.native_element()).len(), 1);
    }
    popover.close();
    popover.native_element().remove();
}

#[wasm_bindgen_test]
async fn popover_follows_the_position() {
    if !supports_anchor_positioning() {
        return;
    }
    let mut kept = None;
    with_ui(|ui| {
        kept = Some(popover_fixture(ui)?);
        Ok(())
    });
    let (root, button, popover, _closed) = kept.expect("組み立て");
    let _mounted = mount_in_view(&root);
    let anchor = button.native_element().get_bounding_client_rect();
    let element = popover.native_element();

    popover.open(&button);
    for position in [
        naui_core::PopoverPosition::Top,
        naui_core::PopoverPosition::Left,
        naui_core::PopoverPosition::Right,
        naui_core::PopoverPosition::Bottom,
    ] {
        // 開いたまま変えても置き直されること。
        popover.set_position(position);
        assert_eq!(popover.position(), position);
        next_frames().await;
        let rect = element.get_bounding_client_rect();
        let placed = match position {
            naui_core::PopoverPosition::Top => rect.bottom() <= anchor.top() + 0.5,
            naui_core::PopoverPosition::Bottom => rect.top() >= anchor.bottom() - 0.5,
            naui_core::PopoverPosition::Left => rect.right() <= anchor.left() + 0.5,
            naui_core::PopoverPosition::Right => rect.left() >= anchor.right() - 0.5,
        };
        assert!(
            placed,
            "{position:?} の側に出ること: ({}, {}, {}, {}) / ({}, {}, {}, {})",
            rect.left(),
            rect.top(),
            rect.right(),
            rect.bottom(),
            anchor.left(),
            anchor.top(),
            anchor.right(),
            anchor.bottom()
        );
    }
    popover.close();
    element.remove();
}

#[wasm_bindgen_test]
fn popover_needs_a_widget_in_the_document() {
    with_ui(|ui| {
        let popover = ui.popover()?;
        let loose = ui.button("どこにもない")?;
        popover.open(&loose);
        assert!(!popover.is_open(), "文書に無いウィジェットには開かないこと");
        assert!(anchor_names(&loose.native_element()).is_empty());
        popover.native_element().remove();
        Ok(())
    });
}

#[wasm_bindgen_test]
fn popovers_share_an_anchor_and_move_between_widgets() {
    if !supports_anchor_positioning() {
        return;
    }
    with_ui(|ui| {
        let (root, first, popover, closed) = popover_fixture(ui)?;
        let second = ui.button("表示")?;
        root.append(&second);
        let other = ui.popover()?;
        let _mounted = Mounted::new(&root);

        popover.open(&first);
        let name = popover_anchor(&popover);
        // `auto` のポップオーバーは、別のものを開くと閉じる (ブラウザの決まり)。
        // 名前の付け外しだけを見るため、もう一方は手で付ける。
        other.open(&first);
        let both = anchor_names(&first.native_element());
        assert_eq!(both.len(), 2, "2 つの名前が並ぶこと: {both:?}");

        popover.open(&second);
        assert!(popover.is_open(), "開いたまま移せること");
        assert_eq!(anchor_names(&second.native_element()), vec![name.clone()]);
        assert!(
            !anchor_names(&first.native_element()).contains(&name),
            "前のウィジェットから名前を外すこと"
        );
        assert_eq!(closed.get(), 0);

        popover.close();
        other.close();
        popover.native_element().remove();
        other.native_element().remove();
        Ok(())
    });
}

#[wasm_bindgen_test]
async fn popover_stays_inside_the_window() {
    let mut kept = None;
    with_ui(|ui| {
        kept = Some(popover_fixture(ui)?);
        Ok(())
    });
    let (root, button, popover, _closed) = kept.expect("組み立て");
    let _mounted = mount_in_view(&root);
    let element = popover.native_element();
    let inset = element
        .unchecked_ref::<HtmlElement>()
        .style()
        .get_property_value("inset")
        .unwrap_or_default();
    // アンカー配置が無いと、中央に置く既定を外すだけになるので残すこと。
    if supports_anchor_positioning() {
        assert_eq!(inset, "auto");
    } else {
        assert_eq!(
            inset, "",
            "アンカー配置が無いときは既定の中央のままにすること"
        );
    }

    popover.open(&button);
    next_frames().await;
    let window = web_sys::window().expect("window");
    let width = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let height = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let rect = element.get_bounding_client_rect();
    assert!(
        rect.left() >= 0.0 && rect.top() >= 0.0 && rect.right() <= width && rect.bottom() <= height,
        "画面の中に出ること: ({}, {}, {}, {}) / {width}x{height}",
        rect.left(),
        rect.top(),
        rect.right(),
        rect.bottom()
    );
    popover.close();
    element.remove();
}

#[wasm_bindgen_test]
fn popover_needs_a_shown_window() {
    with_ui(|ui| {
        let window = ui.window("ポップオーバー", 400.0, 300.0)?;
        let button = ui.button("設定")?;
        window.set_child(&button);
        let popover = ui.popover()?;
        popover.set_child(&ui.label("中身")?);

        popover.open(&button);
        assert!(popover.is_open(), "出ているウィンドウでは開くこと");
        popover.close();

        // 閉じたウィンドウは文書に残るが、そこからは開かないこと。
        window.close();
        popover.open(&button);
        assert!(!popover.is_open(), "閉じたウィンドウからは開かないこと");
        assert!(anchor_names(&button.native_element()).is_empty());

        window.show();
        popover.open(&button);
        assert!(popover.is_open(), "出し直したウィンドウでは開くこと");
        popover.close();
        window.close();
        popover.native_element().remove();
        Ok(())
    });
}

#[wasm_bindgen_test]
async fn popover_escape_in_a_dialog_closes_only_the_popover() {
    let mut kept = None;
    with_ui(|ui| {
        let popover = ui.popover()?;
        let field = ui.text_input("")?;
        popover.set_child(&field);
        let dialog = ui.dialog("確認")?;
        let inside = ui.button("詳細")?;
        dialog.set_child(&inside);
        let closed = Rc::new(Cell::new(0));
        popover.on_close({
            let closed = closed.clone();
            move || closed.set(closed.get() + 1)
        });
        let responses = Rc::new(RefCell::new(Vec::new()));
        dialog.on_response({
            let responses = responses.clone();
            move |response| responses.borrow_mut().push(response)
        });
        kept = Some((popover, field, dialog, inside, closed, responses));
        Ok(())
    });
    let (popover, field, dialog, inside, closed, responses) = kept.expect("組み立て");

    dialog.open();
    popover.open(&inside);
    let prevented = press_escape(field.native_element().as_ref(), false);
    assert!(prevented, "Esc の既定動作を止めていること");
    assert!(
        !popover.is_open(),
        "ポップオーバーの中の Esc でそれが閉じること"
    );
    assert!(dialog.is_open(), "親のダイアログは閉じないこと");
    next_frames().await;
    assert_eq!(closed.get(), 1, "利用者が閉じたものとして知らせること");
    assert!(responses.borrow().is_empty());

    // ダイアログのほかの場所の Esc でも、先にポップオーバーを閉じること。
    popover.open(&inside);
    press_escape(inside.native_element().as_ref(), false);
    assert!(!popover.is_open());
    assert!(dialog.is_open());

    // ポップオーバーが無ければ、ダイアログが閉じること。
    press_escape(inside.native_element().as_ref(), false);
    assert!(!dialog.is_open());
    assert_eq!(*responses.borrow(), vec![DialogResponse::Cancel]);
    popover.native_element().remove();
    dialog.native_element().remove();
}

#[wasm_bindgen_test]
async fn popover_opened_from_another_keeps_both_open() {
    let mut kept = None;
    with_ui(|ui| {
        let (root, button, parent, parent_closed) = popover_fixture(ui)?;
        let inner = ui.button("詳細")?;
        parent.set_child(&inner);
        let child = ui.popover()?;
        child.set_child(&ui.label("子")?);
        let child_closed = Rc::new(Cell::new(0));
        child.on_close({
            let closed = child_closed.clone();
            move || closed.set(closed.get() + 1)
        });
        kept = Some((
            root,
            button,
            parent,
            parent_closed,
            inner,
            child,
            child_closed,
        ));
        Ok(())
    });
    let (root, button, parent, parent_closed, inner, child, child_closed) = kept.expect("組み立て");
    let _mounted = mount_in_view(&root);
    let element = child.native_element();

    parent.open(&button);
    child.open(&inner);
    assert!(child.is_open());
    assert!(parent.is_open(), "子を開いても親は閉じないこと");
    assert_eq!(
        element.parent_element().as_ref(),
        Some(&parent.native_element()),
        "子は親の中に置くこと (ブラウザが入れ子と分かるように)"
    );
    next_frames().await;
    assert!(parent.is_open() && child.is_open());
    assert_eq!((parent_closed.get(), child_closed.get()), (0, 0));
    if supports_anchor_positioning() {
        let anchor = inner.native_element().get_bounding_client_rect();
        let rect = element.get_bounding_client_rect();
        assert!(
            anchor.height() > 0.0 && rect.top() >= anchor.bottom() - 0.5,
            "子は親の中のウィジェットの下に出ること: {} / {}",
            rect.top(),
            anchor.bottom()
        );
    }

    // 子だけ閉じると、子は `<body>` へ戻り、親は残ること。
    child.close();
    assert!(parent.is_open());
    assert_eq!(
        element.parent_element().as_ref(),
        Some(&body().unchecked_into())
    );

    // 親の中身を替えても、中で開いている子の要素は消さないこと。
    child.open(&inner);
    with_ui(|ui| {
        parent.set_child(&ui.label("別の中身")?);
        Ok(())
    });
    assert!(element.is_connected());
    child.close();

    // 親を閉じると、子もブラウザが一緒に閉じること。
    parent.set_child(&inner);
    child.open(&inner);
    parent.close();
    assert!(!parent.is_open() && !child.is_open());
    next_frames().await;
    assert_eq!(parent_closed.get(), 0, "アプリが閉じた親は知らせないこと");
    assert_eq!(
        element.parent_element().as_ref(),
        Some(&body().unchecked_into()),
        "閉じた子は `<body>` へ戻ること"
    );
    parent.native_element().remove();
    element.remove();
}

#[wasm_bindgen_test]
async fn popover_moving_a_parent_closes_its_children() {
    let mut kept = None;
    with_ui(|ui| {
        let (root, outside, parent, parent_closed) = popover_fixture(ui)?;
        let dialog = ui.dialog("確認")?;
        let opener = ui.button("設定")?;
        dialog.set_child(&opener);
        let inner = ui.button("詳細")?;
        parent.set_child(&inner);
        let child = ui.popover()?;
        child.set_child(&ui.label("子")?);
        let child_closed = Rc::new(Cell::new(0));
        child.on_close({
            let closed = child_closed.clone();
            move || closed.set(closed.get() + 1)
        });
        kept = Some((
            root,
            outside,
            parent,
            parent_closed,
            dialog,
            opener,
            inner,
            child,
            child_closed,
        ));
        Ok(())
    });
    let (root, outside, parent, parent_closed, dialog, opener, inner, child, child_closed) =
        kept.expect("組み立て");
    let _mounted = Mounted::new(&root);

    dialog.open();
    parent.open(&opener);
    child.open(&inner);
    assert!(parent.is_open() && child.is_open());

    // 親をダイアログの外のウィジェットへ移すと、要素が動くので中の子も閉じる。
    parent.open(&outside);
    assert!(parent.is_open(), "親は移った先で開いていること");
    assert!(!child.is_open(), "中の子は閉じること");
    next_frames().await;
    assert_eq!(child_closed.get(), 1, "子が閉じたことを知らせること");
    assert_eq!(parent_closed.get(), 0, "移しただけの親は知らせないこと");
    assert!(
        anchor_names(&inner.native_element()).is_empty(),
        "子のアンカーの名前を外すこと"
    );
    assert_eq!(
        child.native_element().parent_element().as_ref(),
        Some(&body().unchecked_into()),
        "閉じた子は `<body>` へ戻ること"
    );

    // 子はまた開けること。
    child.open(&inner);
    assert!(child.is_open() && parent.is_open());

    child.close();
    parent.close();
    dialog.close();
    next_frames().await;
    parent.native_element().remove();
    child.native_element().remove();
    dialog.native_element().remove();
}

#[wasm_bindgen_test]
async fn popover_escape_in_a_dialog_closes_the_topmost_first() {
    let mut kept = None;
    with_ui(|ui| {
        let dialog = ui.dialog("確認")?;
        let opener = ui.button("設定")?;
        dialog.set_child(&opener);
        let parent = ui.popover()?;
        let inner = ui.button("詳細")?;
        parent.set_child(&inner);
        let child = ui.popover()?;
        let field = ui.text_input("")?;
        child.set_child(&field);
        kept = Some((dialog, opener, parent, inner, child, field));
        Ok(())
    });
    let (dialog, opener, parent, inner, child, field) = kept.expect("組み立て");

    dialog.open();
    parent.open(&opener);
    child.open(&inner);
    assert!(parent.is_open() && child.is_open());

    press_escape(field.native_element().as_ref(), false);
    assert!(!child.is_open(), "いちばん上の子が閉じること");
    assert!(parent.is_open(), "親は残ること");
    assert!(dialog.is_open());

    press_escape(inner.native_element().as_ref(), false);
    assert!(!parent.is_open());
    assert!(dialog.is_open());

    press_escape(opener.native_element().as_ref(), false);
    assert!(!dialog.is_open());
    next_frames().await;
    parent.native_element().remove();
    child.native_element().remove();
    dialog.native_element().remove();
}

#[wasm_bindgen_test]
async fn popover_closes_with_its_window_or_dialog() {
    let mut kept = None;
    with_ui(|ui| {
        let window = ui.window("ポップオーバー", 400.0, 300.0)?;
        let button = ui.button("設定")?;
        window.set_child(&button);
        let dialog = ui.dialog("確認")?;
        let inside = ui.button("詳細")?;
        dialog.set_child(&inside);
        let popover = ui.popover()?;
        popover.set_child(&ui.label("中身")?);
        let closed = Rc::new(Cell::new(0));
        popover.on_close({
            let closed = closed.clone();
            move || closed.set(closed.get() + 1)
        });
        kept = Some((window, button, dialog, inside, popover, closed));
        Ok(())
    });
    let (window, button, dialog, inside, popover, closed) = kept.expect("組み立て");

    popover.open(&button);
    assert!(popover.is_open());
    window.close();
    assert!(!popover.is_open(), "ウィンドウを閉じたら一緒に閉じること");
    next_frames().await;
    assert_eq!(closed.get(), 1, "外側を押したときと同じく知らせること");

    // 別のウィンドウのものは閉じないこと。
    dialog.open();
    popover.open(&inside);
    window.close();
    assert!(popover.is_open(), "ほかのウィンドウを閉じても残ること");

    dialog.close();
    assert!(!popover.is_open(), "ダイアログを閉じたら一緒に閉じること");
    // WebKit は `position-try-fallbacks` を持つ要素を中に残したままモーダルを
    // 閉じると描画が止まるので、閉じる前に `<body>` へ戻っていること。
    assert_eq!(
        popover.native_element().parent_element().as_ref(),
        Some(&body().unchecked_into())
    );
    next_frames().await;
    assert_eq!(closed.get(), 2);
    popover.native_element().remove();
    dialog.native_element().remove();
}

#[wasm_bindgen_test]
async fn popover_in_a_modal_dialog_can_be_used() {
    let mut kept = None;
    with_ui(|ui| {
        let (root, outside, popover, closed) = popover_fixture(ui)?;
        let field = ui.text_input("")?;
        popover.set_child(&field);
        let dialog = ui.dialog("確認")?;
        let inside = ui.button("詳細")?;
        dialog.set_child(&inside);
        kept = Some((root, outside, popover, closed, field, dialog, inside));
        Ok(())
    });
    let (root, outside, popover, closed, field, dialog, inside) = kept.expect("組み立て");
    let _mounted = Mounted::new(&root);
    let element = popover.native_element();

    dialog.open();
    popover.open(&inside);
    assert!(popover.is_open());
    assert_eq!(
        element.parent_element().as_ref(),
        Some(&dialog.native_element()),
        "ウィジェットと同じダイアログの中に置くこと"
    );
    // モーダルの外 (inert) に置かれていると、フォーカスを受け取れない。
    let input = field.native_element();
    input
        .unchecked_ref::<HtmlElement>()
        .focus()
        .expect("フォーカス");
    assert_eq!(
        body().owner_document().and_then(|doc| doc.active_element()),
        Some(input.clone()),
        "ダイアログの中のポップオーバーを操作できること"
    );

    // ダイアログの外のウィジェットへ開いたまま移しても、閉じた知らせは出ないこと。
    popover.open(&outside);
    assert!(popover.is_open(), "開いたまま移せること");
    assert_eq!(
        element.parent_element().as_ref(),
        Some(&body().unchecked_into())
    );
    next_frames().await;
    assert!(popover.is_open());
    assert_eq!(closed.get(), 0, "移しただけで通知しないこと");

    popover.close();
    dialog.close();
    element.remove();
    dialog.native_element().remove();
}

#[wasm_bindgen_test]
fn dialog_escape_closes_and_stops_the_browser_default() {
    with_ui(|ui| {
        let dialog = ui.dialog("確認")?;
        let seen = Rc::new(RefCell::new(Vec::new()));
        dialog.on_response({
            let seen = seen.clone();
            move |response| seen.borrow_mut().push(response)
        });

        dialog.open();
        assert!(dialog.is_open(), "open() でモーダルが出ること");

        let prevented = press_escape(dialog.native_element().as_ref(), false);
        assert!(!dialog.is_open(), "Esc で閉じること");
        assert_eq!(*seen.borrow(), vec![DialogResponse::Cancel]);
        // 既定動作を残すと、Safari は全画面のとき Esc をまず全画面の解除に
        // 使ってしまい、ダイアログを閉じるのに 2 回押すことになる。
        assert!(prevented, "Esc の既定動作を止めていること");

        dialog.native_element().remove();
        Ok(())
    });
}

#[wasm_bindgen_test]
fn dialog_leaves_escape_to_the_ime_while_composing() {
    with_ui(|ui| {
        let dialog = ui.dialog("確認")?;
        dialog.open();

        let prevented = press_escape(dialog.native_element().as_ref(), true);
        assert!(dialog.is_open(), "変換中の Esc では閉じないこと");
        assert!(!prevented, "変換中の Esc は IME へ渡すこと");

        dialog.close();
        dialog.native_element().remove();
        Ok(())
    });
}

// --------------------------------------------------------------- Sheet

/// The labels of a sheet's buttons, from the left.
fn sheet_button_labels(sheet: &naui_web::Sheet) -> Vec<String> {
    sheet_button_elements(sheet)
        .iter()
        .map(|button| button.text_content().unwrap_or_default())
        .collect()
}

fn sheet_button_elements(sheet: &naui_web::Sheet) -> Vec<HtmlElement> {
    let list = sheet
        .native_element()
        .query_selector_all("button")
        .expect("ボタンの検索");
    (0..list.length())
        .filter_map(|i| list.get(i))
        .map(|node| node.unchecked_into::<HtmlElement>())
        .collect()
}

#[wasm_bindgen_test]
fn sheet_lays_out_title_content_and_buttons() {
    with_ui(|ui| {
        let sheet = ui.sheet("新しいフォルダ")?;
        let field = ui.text_input("名称未設定")?;
        sheet.set_child(&field);
        sheet.set_buttons(
            DialogButtons::new()
                .primary("作成")
                .secondary("あとで")
                .cancel("キャンセル"),
        );
        let element = sheet.native_element();
        assert_eq!(element.tag_name(), "DIALOG");
        let heading = element
            .query_selector("h2")
            .expect("見出しの検索")
            .expect("見出し");
        assert_eq!(heading.text_content().as_deref(), Some("新しいフォルダ"));
        assert!(
            element.contains(Some(&field.native_element())),
            "中身が <dialog> の中に入ること"
        );
        let message: HtmlElement = element
            .query_selector("p")
            .expect("本文の検索")
            .expect("本文")
            .unchecked_into();
        assert!(message.hidden(), "本文は出さないこと");
        assert_eq!(
            sheet_button_labels(&sheet),
            ["あとで", "キャンセル", "作成"]
        );
        let buttons = sheet_button_elements(&sheet);
        assert_eq!(
            buttons[2].get_attribute("data-naui-role").as_deref(),
            Some("primary"),
            "主ボタンに Button::set_role と同じ印"
        );
        assert_eq!(buttons[1].get_attribute("data-naui-role"), None);
        let min_width = element
            .unchecked_ref::<HtmlElement>()
            .style()
            .get_property_value("min-width")
            .unwrap_or_default();
        assert!(
            min_width.contains("320px"),
            "最小の幅を持つこと: {min_width}"
        );

        sheet.set_title("");
        assert!(
            heading.unchecked_ref::<HtmlElement>().hidden(),
            "空の見出しは出さない"
        );

        sheet.open();
        assert!(sheet.is_open());
        sheet.close();
        assert!(!sheet.is_open());
        element.remove();
        Ok(())
    });
}

#[wasm_bindgen_test]
fn sheet_buttons_respond_and_can_be_disabled() {
    with_ui(|ui| {
        let sheet = ui.sheet("確認")?;
        sheet.set_buttons(DialogButtons::new().primary("作成").cancel("やめる"));
        let seen = Rc::new(RefCell::new(Vec::new()));
        sheet.on_response({
            let seen = seen.clone();
            move |response| seen.borrow_mut().push(response)
        });

        sheet.set_button_enabled(DialogResponse::Primary, false);
        sheet.set_button_enabled(DialogResponse::Cancel, false);
        assert!(!sheet.is_button_enabled(DialogResponse::Primary));
        assert!(sheet.is_button_enabled(DialogResponse::Cancel));
        let disabled = |sheet: &naui_web::Sheet| -> Vec<bool> {
            sheet_button_elements(sheet)
                .iter()
                .map(|button| button.has_attribute("disabled"))
                .collect()
        };
        assert_eq!(disabled(&sheet), [false, true]);
        sheet.set_buttons(DialogButtons::new().primary("保存").cancel("やめる"));
        assert_eq!(disabled(&sheet), [false, true], "差し替えても残ること");
        sheet.set_button_enabled(DialogResponse::Primary, true);
        assert_eq!(disabled(&sheet), [false, false]);

        sheet.open();
        sheet_button_elements(&sheet)[1].click();
        assert_eq!(*seen.borrow(), [DialogResponse::Primary]);
        assert!(!sheet.is_open(), "押したら閉じること");
        sheet.native_element().remove();
        Ok(())
    });
}

#[wasm_bindgen_test]
fn sheet_enter_in_a_text_field_presses_the_default_button() {
    with_ui(|ui| {
        let sheet = ui.sheet("名前")?;
        let column = ui.stack(Orientation::Vertical)?;
        let field = ui.text_input("")?;
        let check = ui.checkbox("あとで開く")?;
        column.append(&field);
        column.append(&check);
        sheet.set_child(&column);
        sheet.set_buttons(DialogButtons::new().primary("作成").cancel("やめる"));
        let seen = Rc::new(RefCell::new(Vec::new()));
        sheet.on_response({
            let seen = seen.clone();
            move |response| seen.borrow_mut().push(response)
        });
        sheet.open();
        let input = field.native_element();

        assert!(
            !press_enter(input.as_ref(), true),
            "変換中は IME へ渡すこと"
        );
        assert!(sheet.is_open());
        // Safari は変換を確定する Enter を isComposing なし・keyCode 229 で送る。
        assert!(
            !key_down(input.as_ref(), "Enter", false, false, 229),
            "Safari の変換確定の Enter も IME へ渡すこと"
        );
        assert!(sheet.is_open(), "変換確定の Enter では押さないこと");
        assert!(
            !key_down(input.as_ref(), "Escape", false, false, 229),
            "Safari の変換取り消しの Esc も IME へ渡すこと"
        );
        assert!(sheet.is_open(), "変換取り消しの Esc では閉じないこと");
        assert!(seen.borrow().is_empty());

        let checkbox = check
            .native_element()
            .query_selector("input")
            .expect("チェックボックスの検索")
            .unwrap_or_else(|| check.native_element());
        press_enter(checkbox.as_ref(), false);
        assert!(sheet.is_open(), "チェックボックスの Enter では押さないこと");

        sheet.set_button_enabled(DialogResponse::Primary, false);
        assert!(!press_enter(input.as_ref(), false));
        assert!(sheet.is_open(), "無効な主ボタンは Enter でも押せないこと");

        sheet.set_button_enabled(DialogResponse::Primary, true);
        assert!(press_enter(input.as_ref(), false), "既定の動作を止めること");
        assert_eq!(*seen.borrow(), [DialogResponse::Primary]);
        assert!(!sheet.is_open());

        // 主ボタンが無ければ、OK (取り消し) が既定のボタン。
        sheet.set_buttons(DialogButtons::new());
        sheet.open();
        press_enter(input.as_ref(), false);
        assert_eq!(
            *seen.borrow(),
            [DialogResponse::Primary, DialogResponse::Cancel]
        );

        // Esc は取り消し。
        sheet.open();
        press_escape(input.as_ref(), false);
        assert_eq!(seen.borrow().last(), Some(&DialogResponse::Cancel));
        assert!(!sheet.is_open());
        sheet.native_element().remove();
        Ok(())
    });
}

// ----------------------------------------------------------- PopupMenu

#[wasm_bindgen_test]
fn popup_menu_escape_closes_and_stops_the_browser_default() {
    with_ui(|ui| {
        let menu = ui.popup_menu()?;
        menu.set_items(&[PopupItem::new("先頭を選択")]);
        let anchor = ui.label("ここを右クリック")?;
        let _mounted = Mounted::new(&anchor);

        menu.open_at(&anchor, 10.0, 10.0);
        let element = menu.native_element();
        assert_ne!(computed(&element, "display"), "none", "メニューが出ること");

        // 購読しているのは document なので、`<body>` から上がってくる。
        let prevented = press_escape(body().as_ref(), false);
        assert_eq!(computed(&element, "display"), "none", "Esc で閉じること");
        assert!(prevented, "Esc の既定動作を止めていること");
        Ok(())
    });
}

/// A popup's submenu opens beside its heading, and a check item flips before the
/// notification.
#[wasm_bindgen_test]
fn popup_menu_submenus_and_checks() {
    with_ui(|ui| {
        let menu = ui.popup_menu()?;
        menu.set_items(&[
            PopupItem::new("折り返す").checked(true),
            PopupItem::separator(),
            PopupItem::submenu("並べ替え", [PopupItem::new("名前"), PopupItem::new("日付")]),
        ]);
        assert_eq!(menu.len(), 3);
        let anchor = ui.label("ここを右クリック")?;
        let _mounted = Mounted::new(&anchor);

        let seen = Rc::new(RefCell::new(Vec::new()));
        menu.on_select({
            let seen = seen.clone();
            let menu = menu.clone();
            move |path| {
                seen.borrow_mut()
                    .push((path.to_vec(), menu.is_item_checked(path)))
            }
        });

        menu.open_at(&anchor, 10.0, 10.0);
        let sort = menu.native_item(&[2]).expect("サブメニューの見出し");
        let submenu = menu.native_submenu(&[2]).expect("サブメニュー");
        assert_eq!(computed(&submenu, "display"), "none");
        hover(&sort);
        assert_ne!(computed(&submenu, "display"), "none", "乗ると開く");
        menu.native_item(&[2, 1]).expect("日付").click();
        assert_eq!(*seen.borrow(), vec![(vec![2, 1], false)]);
        assert_eq!(
            computed(&menu.native_element(), "display"),
            "none",
            "選ぶと閉じる"
        );
        assert_eq!(computed(&submenu, "display"), "none");

        menu.select(&[0]);
        menu.select(&[2]);
        assert_eq!(
            *seen.borrow(),
            vec![(vec![2, 1], false), (vec![0], false)],
            "印は通知の前に反転し、見出しは選べない"
        );
        let wrap = menu.native_item(&[0]).expect("項目");
        assert_eq!(wrap.get_attribute("aria-checked").as_deref(), Some("false"));
        Ok(())
    });
}

// ------------------------------------------------------------- MenuBar

#[wasm_bindgen_test]
fn menu_bar_builds_aria_roles_for_each_menu() {
    with_ui(|ui| {
        let menu_bar = ui.menu_bar()?;
        assert!(menu_bar.is_empty());

        menu_bar.set_menus(&[
            MenuSpec::new(
                "ファイル",
                [
                    MenuItem::new("新規").shortcut(MenuShortcut::new('n')),
                    MenuItem::separator(),
                    MenuItem::new("保存").enabled(false),
                ],
            ),
            MenuSpec::new("表示", ["拡大", "縮小"]),
        ]);

        assert_eq!(menu_bar.len(), 2);
        assert_eq!(menu_bar.menu_len(0), 3, "区切り線も 1 項目として数える");
        assert_eq!(menu_bar.menu_len(9), 0, "範囲外は 0");

        let bar = menu_bar.native_element();
        assert_eq!(bar.get_attribute("role").as_deref(), Some("menubar"));

        let title = menu_bar.native_title(0).expect("見出し");
        assert_eq!(title.get_attribute("role").as_deref(), Some("menuitem"));
        assert_eq!(
            title.get_attribute("aria-haspopup").as_deref(),
            Some("true")
        );
        assert_eq!(title.text_content().as_deref(), Some("ファイル"));

        let menu = menu_bar.native_menu(0).expect("メニュー");
        assert_eq!(menu.get_attribute("role").as_deref(), Some("menu"));

        let first = menu_bar.native_item(&[0, 0]).expect("先頭は項目");
        assert_eq!(first.get_attribute("role").as_deref(), Some("menuitem"));
        assert_eq!(
            first.get_attribute("aria-keyshortcuts").as_deref(),
            Some("Control+N"),
            "ショートカットは読み上げへも伝える"
        );
        assert!(
            first.text_content().unwrap_or_default().contains("Ctrl+N"),
            "ブラウザは右端の表示を持たないので naui が添える"
        );
        assert!(
            menu_bar.native_item(&[0, 1]).is_none(),
            "区切り線にボタンは無い"
        );
        assert!(menu_bar
            .native_item(&[0, 2])
            .expect("3 番目は項目")
            .has_attribute("disabled"));
        assert!(menu_bar.native_item(&[9, 0]).is_none(), "範囲外は None");

        assert!(menu_bar.is_item_enabled(&[0, 0]));
        assert!(!menu_bar.is_item_enabled(&[0, 1]), "区切り線は押せない");
        assert!(!menu_bar.is_item_enabled(&[0, 2]));

        // 項目ごとの指定と全体の指定は AND を取る。
        menu_bar.set_item_enabled(&[0, 2], true);
        assert!(!menu_bar
            .native_item(&[0, 2])
            .expect("項目")
            .has_attribute("disabled"));
        menu_bar.set_enabled(false);
        assert!(!menu_bar.is_item_enabled(&[0, 0]));
        assert!(menu_bar
            .native_title(0)
            .expect("見出し")
            .has_attribute("disabled"));
        menu_bar.set_enabled(true);
        assert!(
            menu_bar.is_item_enabled(&[0, 2]),
            "全体を戻すと項目ごとの指定が残る"
        );

        // 区切り線への set_item_enabled は無視する。
        menu_bar.set_item_enabled(&[0, 1], true);
        assert!(!menu_bar.is_item_enabled(&[0, 1]));

        menu_bar.set_menus(&[]);
        assert!(menu_bar.is_empty());
        assert!(menu_bar.native_title(0).is_none());
        Ok(())
    });
}

#[wasm_bindgen_test]
fn menu_bar_opens_on_the_title_and_closes_after_choosing() {
    with_ui(|ui| {
        let window = ui.window("メニューバー", 400.0, 300.0)?;
        let menu_bar = ui.menu_bar()?;
        menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規", "開く"])]);
        window.set_menu_bar(&menu_bar);

        let seen = Rc::new(RefCell::new(Vec::new()));
        menu_bar.on_activate({
            let seen = seen.clone();
            move |path| seen.borrow_mut().push(path.to_vec())
        });

        let title = menu_bar.native_title(0).expect("見出し");
        let menu = menu_bar.native_menu(0).expect("メニュー");
        assert_eq!(computed(&menu, "display"), "none", "はじめは閉じている");

        title.click();
        assert_ne!(computed(&menu, "display"), "none", "見出しを押すと開く");
        assert_eq!(
            title.get_attribute("aria-expanded").as_deref(),
            Some("true")
        );

        // 項目を押すと通知が届き、メニューは閉じる。
        menu_bar.native_item(&[0, 1]).expect("項目").click();
        assert_eq!(*seen.borrow(), vec![vec![0, 1]]);
        assert_eq!(computed(&menu, "display"), "none");
        assert_eq!(
            title.get_attribute("aria-expanded").as_deref(),
            Some("false")
        );

        // もう一度開いて、Esc で閉じる (購読しているのは document)。
        title.click();
        assert_ne!(computed(&menu, "display"), "none");
        let prevented = press_escape(body().as_ref(), false);
        assert_eq!(computed(&menu, "display"), "none", "Esc で閉じること");
        assert!(prevented, "Esc の既定動作を止めていること");

        window.clear_menu_bar();
        window.close();
        Ok(())
    });
}

#[wasm_bindgen_test]
fn menu_bar_shortcuts_reach_the_closure() {
    with_ui(|ui| {
        let menu_bar = ui.menu_bar()?;
        menu_bar.set_menus(&[MenuSpec::new(
            "ファイル",
            [
                MenuItem::new("保存").shortcut(MenuShortcut::new('s')),
                MenuItem::new("別名で保存").shortcut(MenuShortcut::new('s').shift(true)),
                MenuItem::new("閉じる")
                    .shortcut(MenuShortcut::new('w'))
                    .enabled(false),
            ],
        )]);
        let window = ui.window("メニューバー", 400.0, 300.0)?;
        window.set_menu_bar(&menu_bar);

        let seen = Rc::new(RefCell::new(Vec::new()));
        menu_bar.on_activate({
            let seen = seen.clone();
            move |path| seen.borrow_mut().push(path.to_vec())
        });

        let prevented = press_shortcut(body().as_ref(), "s", true, false, false);
        assert_eq!(*seen.borrow(), vec![vec![0, 0]]);
        assert!(prevented, "ブラウザの既定のショートカットは起こさない");

        // ⇧ の有無で別の項目になる。
        press_shortcut(body().as_ref(), "S", true, true, false);
        assert_eq!(*seen.borrow(), vec![vec![0, 0], vec![0, 1]]);

        // 修飾キーが足りない・余っているときは合わない。
        press_shortcut(body().as_ref(), "s", false, false, false);
        press_shortcut(body().as_ref(), "s", true, false, true);
        // 押せない項目のショートカットは効かない。
        press_shortcut(body().as_ref(), "w", true, false, false);
        assert_eq!(*seen.borrow(), vec![vec![0, 0], vec![0, 1]]);

        menu_bar.set_item_enabled(&[0, 2], true);
        press_shortcut(body().as_ref(), "w", true, false, false);
        assert_eq!(*seen.borrow(), vec![vec![0, 0], vec![0, 1], vec![0, 2]]);

        menu_bar.set_enabled(false);
        press_shortcut(body().as_ref(), "s", true, false, false);
        assert_eq!(
            *seen.borrow(),
            vec![vec![0, 0], vec![0, 1], vec![0, 2]],
            "無効なメニューバーは通知しない"
        );

        window.clear_menu_bar();
        window.close();
        Ok(())
    });
}

/// Shortcuts work only while the menu bar is attached to a window that is on screen.
#[wasm_bindgen_test]
fn menu_bar_shortcuts_follow_the_attachment() {
    with_ui(|ui| {
        let menu_bar = ui.menu_bar()?;
        menu_bar.set_menus(&[MenuSpec::new(
            "ファイル",
            [MenuItem::new("保存").shortcut(MenuShortcut::new('s'))],
        )]);
        let seen = Rc::new(Cell::new(0));
        menu_bar.on_activate({
            let seen = seen.clone();
            move |_path| seen.set(seen.get() + 1)
        });

        // 取り付ける前は、キーを見張っていない。
        let prevented = press_shortcut(body().as_ref(), "s", true, false, false);
        assert_eq!(seen.get(), 0, "取り付ける前は反応しない");
        assert!(!prevented, "ブラウザの既定動作も止めない");

        let window = ui.window("メニューバー", 400.0, 300.0)?;
        window.set_menu_bar(&menu_bar);
        press_shortcut(body().as_ref(), "s", true, false, false);
        assert_eq!(seen.get(), 1, "取り付けると効く");

        // 閉じた (隠した) ウィンドウのメニューバーは反応しない。
        window.close();
        let prevented = press_shortcut(body().as_ref(), "s", true, false, false);
        assert_eq!(seen.get(), 1, "閉じたウィンドウでは反応しない");
        assert!(!prevented);
        window.show();
        press_shortcut(body().as_ref(), "s", true, false, false);
        assert_eq!(seen.get(), 2, "出し直すとまた効く");

        // 外したメニューバーは、もう反応しない。
        window.clear_menu_bar();
        let prevented = press_shortcut(body().as_ref(), "s", true, false, false);
        assert_eq!(seen.get(), 2, "外したあとは反応しない");
        assert!(!prevented);

        // 付け直せば、また効く。
        window.set_menu_bar(&menu_bar);
        press_shortcut(body().as_ref(), "s", true, false, false);
        assert_eq!(seen.get(), 3);

        window.clear_menu_bar();
        window.close();
        Ok(())
    });
}

/// Menu bars of hidden windows or other windows do not prevent Escape's default
/// action (they are not the ones whose open menu should close).
#[wasm_bindgen_test]
fn menu_bar_escape_is_left_alone_when_it_is_not_ours() {
    with_ui(|ui| {
        let window = ui.window("メニューバー", 400.0, 300.0)?;
        let menu_bar = ui.menu_bar()?;
        menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規"])]);
        window.set_menu_bar(&menu_bar);
        let title = menu_bar.native_title(0).expect("見出し");
        let menu = menu_bar.native_menu(0).expect("メニュー");

        // 閉じたウィンドウでは、開いていたメニューも一緒に閉じ、Esc は拾わない。
        title.click();
        assert_ne!(computed(&menu, "display"), "none");
        window.close();
        assert_eq!(
            title.get_attribute("aria-expanded").as_deref(),
            Some("false"),
            "ウィンドウを閉じるとメニューも閉じる"
        );
        let prevented = press_escape(body().as_ref(), false);
        assert!(!prevented, "隠れたウィンドウは Esc の既定動作を止めない");

        // 別のウィンドウの中から上がってきた Esc も拾わない。
        window.show();
        let other = ui.window("別のウィンドウ", 400.0, 300.0)?;
        let input = ui.text_input("")?;
        other.set_child(&input);
        title.click();
        assert_ne!(computed(&menu, "display"), "none");
        let prevented = press_escape(input.native_element().as_ref(), false);
        assert!(!prevented, "別のウィンドウの Esc の既定動作は止めない");
        assert_ne!(computed(&menu, "display"), "none", "メニューは開いたまま");

        // 自分のウィンドウ (や、どのウィンドウにも属さないところ) からなら閉じる。
        let prevented = press_escape(body().as_ref(), false);
        assert!(prevented);
        assert_eq!(computed(&menu, "display"), "none");

        window.clear_menu_bar();
        window.close();
        other.close();
        Ok(())
    });
}

/// Even with two windows having the same shortcut, it notifies only once.
#[wasm_bindgen_test]
fn menu_bar_shortcuts_reach_only_one_window() {
    with_ui(|ui| {
        let spec = [MenuSpec::new(
            "ファイル",
            [MenuItem::new("保存").shortcut(MenuShortcut::new('s'))],
        )];

        let first_window = ui.window("1 つ目", 400.0, 300.0)?;
        let first_input = ui.text_input("")?;
        first_window.set_child(&first_input);
        let first = ui.menu_bar()?;
        first.set_menus(&spec);
        first_window.set_menu_bar(&first);

        let second_window = ui.window("2 つ目", 400.0, 300.0)?;
        let second_input = ui.text_input("")?;
        second_window.set_child(&second_input);
        let second = ui.menu_bar()?;
        second.set_menus(&spec);
        second_window.set_menu_bar(&second);

        let seen = Rc::new(RefCell::new(Vec::new()));
        first.on_activate({
            let seen = seen.clone();
            move |_path| seen.borrow_mut().push(1)
        });
        second.on_activate({
            let seen = seen.clone();
            move |_path| seen.borrow_mut().push(2)
        });

        // キーが上がってきたウィンドウのメニューバーだけが受け取る。
        press_shortcut(
            second_input.native_element().as_ref(),
            "s",
            true,
            false,
            false,
        );
        assert_eq!(*seen.borrow(), vec![2], "2 つ目のウィンドウの中から");
        press_shortcut(
            first_input.native_element().as_ref(),
            "s",
            true,
            false,
            false,
        );
        assert_eq!(*seen.borrow(), vec![2, 1], "1 つ目のウィンドウの中から");

        // どのウィンドウにも属さないところからのキーは、1 つだけが受け取る。
        press_shortcut(body().as_ref(), "s", true, false, false);
        assert_eq!(seen.borrow().len(), 3, "二重に通知しない");

        first_window.clear_menu_bar();
        second_window.clear_menu_bar();
        first_window.close();
        second_window.close();
        Ok(())
    });
}

/// Fires `pointerenter` on an element (the pointer moving onto it).
fn hover(element: &HtmlElement) {
    let event = PointerEvent::new("pointerenter").expect("ポインタイベントの生成");
    element.dispatch_event(&event).expect("イベントの配送");
}

/// A check item becomes `menuitemcheckbox` with a visible ✓, and a submenu opens
/// beside its heading on click or hover.
#[wasm_bindgen_test]
fn menu_bar_submenus_and_checks() {
    with_ui(|ui| {
        let window = ui.window("メニューバー", 400.0, 300.0)?;
        let menu_bar = ui.menu_bar()?;
        menu_bar.set_menus(&[MenuSpec::new(
            "表示",
            [
                MenuItem::new("サイドバー").checked(true),
                MenuItem::submenu(
                    "拡大",
                    [MenuItem::new("拡大"), MenuItem::submenu("深い", ["さらに"])],
                ),
                MenuItem::new("全画面"),
                MenuItem::submenu("閉じた", ["中"]).enabled(false),
            ],
        )]);
        window.set_menu_bar(&menu_bar);

        let sidebar = menu_bar.native_item(&[0, 0]).expect("項目");
        assert_eq!(
            sidebar.get_attribute("role").as_deref(),
            Some("menuitemcheckbox")
        );
        assert_eq!(
            sidebar.get_attribute("aria-checked").as_deref(),
            Some("true")
        );
        assert!(sidebar.text_content().unwrap_or_default().starts_with('✓'));
        let full = menu_bar.native_item(&[0, 2]).expect("項目");
        assert_eq!(full.get_attribute("role").as_deref(), Some("menuitem"));
        assert_eq!(
            full.first_element_child()
                .and_then(|m| m.text_content())
                .as_deref(),
            Some(""),
            "印の無い項目にも印の列を空けて、文字の頭をそろえる"
        );

        let zoom = menu_bar.native_item(&[0, 1]).expect("サブメニューの見出し");
        assert_eq!(zoom.get_attribute("aria-haspopup").as_deref(), Some("menu"));
        let submenu = menu_bar.native_submenu(&[0, 1]).expect("サブメニュー");
        assert_eq!(submenu.get_attribute("role").as_deref(), Some("menu"));
        let deep = menu_bar.native_submenu(&[0, 1, 1]).expect("3 段目");
        assert!(menu_bar.native_submenu(&[0, 0]).is_none());

        let seen = Rc::new(RefCell::new(Vec::new()));
        menu_bar.on_activate({
            let seen = seen.clone();
            let menu_bar = menu_bar.clone();
            move |path| {
                seen.borrow_mut()
                    .push((path.to_vec(), menu_bar.is_item_checked(path)))
            }
        });

        let title = menu_bar.native_title(0).expect("見出し");
        title.click();
        assert_eq!(
            computed(&submenu, "display"),
            "none",
            "開いた直後は閉じている"
        );
        zoom.click();
        assert_ne!(computed(&submenu, "display"), "none", "見出しを押すと開く");
        assert_eq!(zoom.get_attribute("aria-expanded").as_deref(), Some("true"));
        let zoom_rect = zoom.get_bounding_client_rect();
        let submenu_rect = submenu.get_bounding_client_rect();
        assert!(
            submenu_rect.left() >= zoom_rect.right() - 1.0
                || submenu_rect.right() <= zoom_rect.left() + 1.0,
            "見出しの横に出る"
        );
        hover(&menu_bar.native_item(&[0, 1, 1]).expect("深い"));
        assert_ne!(computed(&deep, "display"), "none", "乗るだけでも開く");

        // 兄弟の普通の項目に乗ると、サブメニューは閉じる。
        hover(&full);
        assert_eq!(computed(&submenu, "display"), "none");
        assert_eq!(computed(&deep, "display"), "none");
        assert_eq!(
            zoom.get_attribute("aria-expanded").as_deref(),
            Some("false")
        );
        // 押せないサブメニューは開かない。
        hover(&menu_bar.native_item(&[0, 3]).expect("閉じた"));
        let closed = menu_bar.native_submenu(&[0, 3]).expect("サブメニュー");
        assert_eq!(computed(&closed, "display"), "none");
        assert!(menu_bar
            .native_item(&[0, 3, 0])
            .expect("中")
            .has_attribute("disabled"));

        // 奥の項目を押すと、経路で届き、メニューは全部閉じる。
        zoom.click();
        hover(&menu_bar.native_item(&[0, 1, 1]).expect("深い"));
        menu_bar.native_item(&[0, 1, 1, 0]).expect("さらに").click();
        assert_eq!(*seen.borrow(), vec![(vec![0, 1, 1, 0], false)]);
        assert_eq!(
            computed(&menu_bar.native_menu(0).expect("メニュー"), "display"),
            "none"
        );
        assert_eq!(computed(&submenu, "display"), "none");
        assert_eq!(computed(&deep, "display"), "none");

        // 印のある項目を押すと、通知の前に反転する。
        title.click();
        sidebar.click();
        assert_eq!(seen.borrow()[1], (vec![0, 0], false));
        assert_eq!(
            sidebar.get_attribute("aria-checked").as_deref(),
            Some("false")
        );
        assert!(!sidebar.text_content().unwrap_or_default().contains('✓'));
        menu_bar.set_item_checked(&[0, 0], true);
        assert_eq!(
            sidebar.get_attribute("aria-checked").as_deref(),
            Some("true")
        );
        assert_eq!(seen.borrow().len(), 2, "set_item_checked は通知しない");

        // サブメニューを止めると中も止まる。
        menu_bar.set_item_enabled(&[0, 1], false);
        assert!(zoom.has_attribute("disabled"));
        assert!(menu_bar
            .native_item(&[0, 1, 1, 0])
            .expect("さらに")
            .has_attribute("disabled"));

        window.clear_menu_bar();
        window.close();
        Ok(())
    });
}

/// Function keys work on their own, inside submenus too, and show their names.
#[wasm_bindgen_test]
fn menu_bar_function_keys_reach_the_closure() {
    with_ui(|ui| {
        let menu_bar = ui.menu_bar()?;
        menu_bar.set_menus(&[MenuSpec::new(
            "表示",
            [
                MenuItem::new("再読み込み").shortcut(MenuShortcut::function(5)),
                MenuItem::submenu(
                    "開発",
                    [MenuItem::new("調べる")
                        .shortcut(MenuShortcut::function(12).primary(true).shift(true))],
                ),
                MenuItem::new("記号").shortcut(MenuShortcut::new('/')),
            ],
        )]);
        let window = ui.window("メニューバー", 400.0, 300.0)?;
        window.set_menu_bar(&menu_bar);

        let reload = menu_bar.native_item(&[0, 0]).expect("項目");
        assert_eq!(
            reload.get_attribute("aria-keyshortcuts").as_deref(),
            Some("F5")
        );
        assert!(reload.text_content().unwrap_or_default().contains("F5"));
        assert_eq!(
            menu_bar
                .native_item(&[0, 1, 0])
                .expect("項目")
                .get_attribute("aria-keyshortcuts")
                .as_deref(),
            Some("Control+Shift+F12")
        );
        let symbol = menu_bar.native_item(&[0, 2]).expect("項目");
        assert!(
            !symbol.has_attribute("aria-keyshortcuts"),
            "記号のショートカットはどの環境でも登録しない"
        );

        let seen = Rc::new(RefCell::new(Vec::new()));
        menu_bar.on_activate({
            let seen = seen.clone();
            move |path| seen.borrow_mut().push(path.to_vec())
        });

        let prevented = press_shortcut(body().as_ref(), "F5", false, false, false);
        assert_eq!(*seen.borrow(), vec![vec![0, 0]]);
        assert!(prevented, "F5 の再読み込みは起こさない");
        // 主修飾キーが余っていれば合わない。
        let prevented = press_shortcut(body().as_ref(), "F5", true, false, false);
        assert!(!prevented);
        press_shortcut(body().as_ref(), "F12", true, true, false);
        assert_eq!(*seen.borrow(), vec![vec![0, 0], vec![0, 1, 0]]);
        press_shortcut(body().as_ref(), "/", true, false, false);
        assert_eq!(seen.borrow().len(), 2);

        window.clear_menu_bar();
        window.close();
        Ok(())
    });
}

#[wasm_bindgen_test]
fn menu_bar_attaches_to_the_top_of_the_window() {
    with_ui(|ui| {
        let window = ui.window("メニューバー", 400.0, 300.0)?;
        let content = ui.label("中身")?;
        window.set_child(&content);

        let menu_bar = ui.menu_bar()?;
        menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規"])]);
        window.set_menu_bar(&menu_bar);

        let bar = menu_bar.native_element();
        let parent = bar.parent_element().expect("親要素");
        assert_eq!(
            parent.first_element_child().as_ref(),
            Some(&bar),
            "ウィンドウの先頭に入る"
        );

        // 中身を差し替えても先頭に残る。
        let other = ui.label("別の中身")?;
        window.set_child(&other);
        assert_eq!(
            bar.parent_element()
                .and_then(|p| p.first_element_child())
                .as_ref(),
            Some(&bar),
            "中身を入れ替えても付いたまま"
        );

        window.clear_menu_bar();
        assert!(bar.parent_element().is_none(), "外すとページから消える");
        window.close();
        Ok(())
    });
}

#[wasm_bindgen_test]
fn menu_bar_callback_is_reentrant_and_replaceable() {
    with_ui(|ui| {
        let menu_bar = ui.menu_bar()?;
        menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規", "開く"])]);

        let seen = Rc::new(RefCell::new(Vec::new()));
        menu_bar.on_activate({
            let seen = seen.clone();
            let menu_bar = menu_bar.clone();
            move |path| {
                seen.borrow_mut().push(path.to_vec());
                // 通知の中でメニューを組み替え、コールバックも差し替える。
                menu_bar.set_menus(&[MenuSpec::new("編集", ["元に戻す"])]);
                menu_bar.on_activate({
                    let seen = seen.clone();
                    move |path| {
                        seen.borrow_mut()
                            .push(path.iter().map(|i| i + 10).collect())
                    }
                });
            }
        });

        menu_bar.activate(&[0, 1]);
        assert_eq!(*seen.borrow(), vec![vec![0, 1]]);
        assert_eq!(menu_bar.len(), 1);

        menu_bar.activate(&[0, 0]);
        assert_eq!(
            *seen.borrow(),
            vec![vec![0, 1], vec![10, 10]],
            "差し替えたコールバックが呼ばれる"
        );
        Ok(())
    });
}

// --------------------------------------------------------------- Theme

#[wasm_bindgen_test]
fn theme_switches_the_documents_color_scheme() {
    with_ui(|ui| {
        let root: Element = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
            .expect("html 要素");

        ui.set_theme(Theme::Dark)?;
        assert_eq!(ui.theme(), Theme::Dark);
        assert_eq!(computed(&root, "color-scheme"), "dark");

        ui.set_theme(Theme::Light)?;
        assert_eq!(computed(&root, "color-scheme"), "light");

        // ページの見た目をテスト前へ戻す。
        ui.set_theme(Theme::System)?;
        assert_eq!(computed(&root, "color-scheme"), "light dark");
        Ok(())
    });
}

// ------------------------------------------------------- Window / 構造

#[wasm_bindgen_test]
fn checkbox_is_a_label_around_a_native_input() {
    with_ui(|ui| {
        let checkbox = ui.checkbox("印")?;
        let _mounted = Mounted::new(&checkbox);
        let element = checkbox.native_element();
        assert_eq!(element.tag_name(), "LABEL");

        // 見た目を作り込まず、ブラウザ標準のチェックボックスを使っている。
        let input = first_input(&element);
        assert_eq!(input.type_(), "checkbox");
        assert_eq!(computed(&element, "display"), "inline-flex");
        Ok(())
    });
}

// --------------------------------------------------------- Breadcrumbs

/// Takes out the breadcrumbs' `<a>`s in order.
fn crumb_links(nav: &Element) -> Vec<HtmlElement> {
    let links = nav.query_selector_all("a").expect("a の検索");
    (0..links.length())
        .filter_map(|index| links.item(index))
        .map(|node| node.unchecked_into())
        .collect()
}

/// Only ancestors are links (`href`); the current position and items that cannot be
/// selected are plain text. The separator is `›`, and item text is not interpreted
/// as HTML.
#[wasm_bindgen_test]
fn breadcrumbs_link_only_the_ancestors() {
    with_ui(|ui| {
        let breadcrumbs = ui.breadcrumbs()?;
        let mut items = NavItem::list(["<b>&</b>", "書類", "2026"]);
        items[1].enabled = false;
        breadcrumbs.set_items(&items);
        let _mounted = Mounted::new(&breadcrumbs);
        let nav = breadcrumbs.native_element();

        let items = nav.query_selector_all("li").expect("li の検索");
        let texts: Vec<String> = (0..items.length())
            .filter_map(|index| items.item(index))
            .map(|li| li.text_content().unwrap_or_default())
            .collect();
        assert_eq!(texts, ["<b>&</b>", "›書類", "›2026"]);

        let links = crumb_links(&nav);
        assert!(links[0].query_selector("b").expect("b の検索").is_none());
        assert!(links[0].has_attribute("href"), "祖先はリンク");
        assert!(
            !links[1].has_attribute("href"),
            "選べない項目はリンクにしない"
        );
        assert!(!links[2].has_attribute("href"), "現在地はリンクにしない");
        assert_eq!(
            links[2].get_attribute("aria-current").as_deref(),
            Some("page")
        );
        assert_eq!(links[0].get_attribute("aria-current"), None);
        Ok(())
    });
}

/// Clicking an ancestor notifies; clicking the current position or an item that
/// cannot be selected does not. Even when the current position moves, each item's
/// width does not change (it is not made bold).
#[wasm_bindgen_test]
fn breadcrumbs_click_moves_the_current_place() {
    with_ui(|ui| {
        let breadcrumbs = ui.breadcrumbs()?;
        let mut items = NavItem::list(["ホーム", "書類", "2026"]);
        items[1].enabled = false;
        breadcrumbs.set_items(&items);
        let log = Rc::new(RefCell::new(Vec::new()));
        breadcrumbs.on_select({
            let log = log.clone();
            move |index| log.borrow_mut().push(index)
        });
        let _mounted = Mounted::new(&breadcrumbs);
        let links = crumb_links(&breadcrumbs.native_element());
        let widths = || -> Vec<f64> {
            links
                .iter()
                .map(|link| link.get_bounding_client_rect().width())
                .collect()
        };
        let before = widths();

        links[2].click();
        links[1].click();
        assert!(log.borrow().is_empty(), "現在地と選べない項目は通知しない");
        assert_eq!(breadcrumbs.selected(), Some(2));

        links[0].click();
        assert_eq!(log.borrow().as_slice(), [0]);
        assert_eq!(breadcrumbs.selected(), Some(0));
        assert!(
            !links[0].has_attribute("href"),
            "新しい現在地はリンクでなくなる"
        );
        assert!(links[2].has_attribute("href"), "前の現在地はリンクに戻る");
        assert_eq!(widths(), before, "現在地が移っても幅は変わらない");
        Ok(())
    });
}

// ------------------------------------------------------------- サイドバー

fn sidebar_fixture() -> Vec<SidebarSection> {
    vec![
        SidebarSection::untitled([
            SidebarItem::new("一般").icon(ToolbarIcon::Settings),
            SidebarItem::new("検索"),
        ]),
        SidebarSection::new(
            "場所",
            [
                SidebarItem::new("書類").icon(ToolbarIcon::Open),
                SidebarItem::new("共有").enabled(false),
            ],
        ),
    ]
}

/// The sidebar's item buttons (in running-number order).
fn sidebar_buttons(aside: &Element) -> Vec<HtmlElement> {
    let found = aside
        .query_selector_all("li > button")
        .expect("ボタンの検索");
    (0..found.length())
        .filter_map(|i| found.item(i))
        .map(|node| node.unchecked_into::<HtmlElement>())
        .collect()
}

/// The preferred languages are `navigator.languages`, and `languagechange` reads
/// them again. Only a real change reaches the closure, and naui's own strings stay
/// as chosen at startup.
#[wasm_bindgen_test]
fn preferred_languages_follow_languagechange() {
    with_ui(|ui| {
        let window = web_sys::window().expect("window");
        let navigator = window.navigator();
        let read = || -> Vec<String> {
            let languages: Vec<String> = navigator
                .languages()
                .iter()
                .filter_map(|tag| tag.as_string())
                .collect();
            if languages.is_empty() {
                navigator.language().into_iter().collect()
            } else {
                languages
            }
        };
        let browser = read();
        assert_eq!(ui.preferred_languages(), browser);
        let strings = naui_core::ui_strings();

        let seen = Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
        let sink = seen.clone();
        ui.on_preferred_languages_change(move |tags| sink.borrow_mut().push(tags.to_vec()));
        let fire = || {
            let event = web_sys::Event::new("languagechange").unwrap();
            window.dispatch_event(&event).unwrap();
        };

        fire();
        assert!(seen.borrow().is_empty(), "変わっていなければ呼ばれない");

        // `navigator.languages` は書き換えられないので、インスタンスに同名の
        // 値を置いて原型のゲッターを隠す。
        let changed = ["fr-FR", "ja-JP"];
        let array: js_sys::Array = changed.iter().map(|tag| JsValue::from_str(tag)).collect();
        let descriptor = js_sys::Object::new();
        js_sys::Reflect::set(&descriptor, &"value".into(), &array).unwrap();
        js_sys::Reflect::set(&descriptor, &"configurable".into(), &JsValue::TRUE).unwrap();
        js_sys::Object::define_property(&navigator, &"languages".into(), &descriptor);
        fire();
        let restored = js_sys::Reflect::delete_property(&navigator, &"languages".into());
        assert_eq!(*seen.borrow(), [changed.map(String::from).to_vec()]);
        assert_eq!(ui.preferred_languages(), changed);
        assert!(
            std::ptr::eq(naui_core::ui_strings(), strings),
            "naui の文字列は起動時の言語のまま"
        );

        assert_eq!(restored, Ok(true));
        fire();
        assert_eq!(seen.borrow().last(), Some(&browser));
        assert_eq!(ui.preferred_languages(), browser);
        Ok(())
    });
}

/// The labels naui chooses itself come from the table for `navigator.languages`.
#[wasm_bindgen_test]
fn own_labels_follow_the_browser_language() {
    with_ui(|ui| {
        let navigator = web_sys::window().expect("window").navigator();
        let preferred: Vec<String> = navigator
            .languages()
            .iter()
            .filter_map(|tag| tag.as_string())
            .collect();
        let text = naui_core::UiStrings::for_languages(&preferred);
        assert_eq!(
            naui_core::ui_strings(),
            text,
            "起動時にブラウザの言語 {preferred:?} から表を選んでいること"
        );

        let label = |element: &web_sys::Element| element.get_attribute("aria-label");
        let breadcrumbs = ui.breadcrumbs()?;
        assert_eq!(
            label(&breadcrumbs.native_element()).as_deref(),
            Some(text.breadcrumbs)
        );
        let pagination = ui.pagination(3)?;
        assert_eq!(
            label(&pagination.native_element()).as_deref(),
            Some(text.pagination)
        );
        let sidebar = ui.sidebar()?;
        let nav = sidebar
            .native_element()
            .query_selector("nav")
            .ok()
            .flatten()
            .expect("<nav>");
        assert_eq!(label(&nav).as_deref(), Some(text.sidebar));
        Ok(())
    });
}

#[wasm_bindgen_test]
fn sidebar_builds_a_nav_with_headed_lists() {
    with_ui(|ui| {
        let sidebar = ui.sidebar()?;
        assert!(sidebar.is_empty());
        sidebar.set_sections(&sidebar_fixture());
        assert_eq!(sidebar.len(), 4);

        let aside = sidebar.native_element();
        assert_eq!(aside.tag_name(), "ASIDE");
        let nav = aside.query_selector("nav").ok().flatten().expect("<nav>");
        assert!(nav.get_attribute("aria-label").is_some(), "読み上げの名前");

        let lists = nav.query_selector_all("ul").expect("一覧の検索");
        assert_eq!(lists.length(), 2, "まとまりごとに <ul>");
        let heading = nav
            .query_selector("[role=heading]")
            .ok()
            .flatten()
            .expect("見出し");
        assert_eq!(heading.text_content().as_deref(), Some("場所"));
        let second: Element = lists.item(1).expect("2 つ目の一覧").unchecked_into();
        assert_eq!(
            second.get_attribute("aria-labelledby"),
            heading.get_attribute("id"),
            "見出しが一覧の名前になる"
        );

        let buttons = sidebar_buttons(&aside);
        let labels: Vec<String> = buttons
            .iter()
            .map(|b| b.text_content().unwrap_or_default())
            .collect();
        assert_eq!(labels, ["一般", "検索", "書類", "共有"]);
        assert!(buttons[0].query_selector("svg").ok().flatten().is_some());
        assert!(buttons[1].query_selector("svg").ok().flatten().is_none());
        assert!(buttons[3].has_attribute("disabled"), "選べない項目は無効");
        let _mounted = Mounted::new_element(&aside);
        let label = buttons[0]
            .query_selector("span")
            .ok()
            .flatten()
            .expect("文字");
        assert_eq!(
            computed(&label, "text-overflow"),
            "ellipsis",
            "入りきらない文字は末尾を省略記号にする"
        );
        assert_eq!(computed(&label, "white-space"), "nowrap", "折り返さない");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn sidebar_click_selects_and_notifies() {
    with_ui(|ui| {
        let sidebar = ui.sidebar()?;
        sidebar.set_sections(&sidebar_fixture());
        let seen = Rc::new(RefCell::new(Vec::new()));
        sidebar.on_select({
            let seen = seen.clone();
            move |index| seen.borrow_mut().push(index)
        });
        let aside = sidebar.native_element();
        let _mounted = Mounted::new_element(&aside);
        let buttons = sidebar_buttons(&aside);

        buttons[2].click();
        assert_eq!(sidebar.selected(), Some(2));
        assert_eq!(*seen.borrow(), [2]);
        assert_eq!(
            buttons[2].get_attribute("aria-current").as_deref(),
            Some("page")
        );
        buttons[3].click();
        assert_eq!(sidebar.selected(), Some(2), "無効な項目は押せない");

        sidebar.set_selected(0);
        assert_eq!(sidebar.selected(), Some(0));
        assert!(!buttons[2].has_attribute("aria-current"));
        assert_eq!(seen.borrow().len(), 1, "set_selected は通知しない");
        sidebar.set_selected(3);
        sidebar.set_selected(9);
        assert_eq!(sidebar.selected(), Some(0), "選べない・範囲外は無視");

        sidebar.select(1);
        assert_eq!(*seen.borrow(), [2, 1], "select は通知する");

        sidebar.set_sections(&sidebar_fixture());
        assert_eq!(sidebar.selected(), Some(1), "同じ番号が選べれば残る");
        sidebar.set_items(&SidebarItem::list(["ひとつ"]));
        assert_eq!(sidebar.selected(), None);
        sidebar.set_selected(0);
        sidebar.clear_selection();
        assert_eq!(sidebar.selected(), None);
        assert_eq!(seen.borrow().len(), 2, "差し替えと解除は通知しない");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn sidebar_attaches_to_the_left_of_the_window() {
    with_ui(|ui| {
        let window = ui.window("サイドバー", 600.0, 300.0)?;
        let content = ui.label("中身")?;
        window.set_child(&content);
        let menu_bar = ui.menu_bar()?;
        menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規"])]);
        window.set_menu_bar(&menu_bar);

        let sidebar = ui.sidebar()?;
        sidebar.set_items(&SidebarItem::list(["一般"]));
        window.set_sidebar(&sidebar);

        let root = window.native_element();
        let aside = sidebar.native_element();
        assert!(root.contains(Some(&aside)), "ウィンドウの中に入る");
        // サイドバーと中身は SplitView の start と end に入り、間が仕切り。
        let start = aside.parent_element().expect("start 側の区画");
        let divider: Element = sidebar.native_divider().unchecked_into();
        assert_eq!(start.next_element_sibling(), Some(divider.clone()));
        assert_eq!(divider.get_attribute("role").as_deref(), Some("separator"));
        let end = divider.next_element_sibling().expect("end 側の区画");
        let element = content.native_element();
        assert!(
            end.contains(Some(&element)),
            "子はサイドバーの右の中身の側へ移る"
        );
        // 開閉ボタンはツールバーの行の先頭 (メニューバーの直下)。
        let row = sidebar
            .native_toggle_button()
            .parent_element()
            .expect("ツールバーの行");
        assert_eq!(row.parent_element().as_ref(), Some(&root));
        assert_eq!(
            menu_bar.native_element().next_element_sibling(),
            Some(row.clone()),
            "ツールバーの行はメニューバーの直下"
        );
        assert_eq!(
            row.first_element_child(),
            Some(sidebar.native_toggle_button())
        );
        assert_eq!(
            root.first_element_child(),
            Some(menu_bar.native_element()),
            "メニューバーは先頭に残る"
        );

        // 表示枠の幅はランナーによって 0 になることがあるので、実際の幅では
        // なく、区画へ書いた幅と、余りを受け取る指定を見る。
        let start_html: &HtmlElement = start.unchecked_ref();
        let width_of =
            |pane: &HtmlElement| pane.style().get_property_value("width").unwrap_or_default();
        assert_eq!(
            width_of(start_html),
            format!("{DEFAULT_SIDEBAR_WIDTH}px"),
            "既定の幅"
        );
        assert_eq!(computed(&end, "flex-grow"), "1", "中身が残りの幅を受け取る");

        sidebar.set_width(160.0);
        assert_eq!(width_of(start_html), "160px");
        assert_eq!(sidebar.width(), 160.0);
        sidebar.set_width(40.0);
        assert_eq!(
            sidebar.width(),
            naui_core::SIDEBAR_MIN_WIDTH,
            "下限より狭くはならない"
        );
        sidebar.set_width(160.0);
        sidebar.set_collapsed(true);
        assert!(sidebar.is_collapsed());
        assert!(start_html.hidden(), "閉じるとサイドバーの区画が隠れる");
        // 属性だけでなく、実際に描かれなくなっていること (インラインの
        // `display` が `[hidden]` より強いので、属性だけでは消えない)。
        assert_eq!(computed(&start, "display"), "none", "区画が実際に消える");
        assert_eq!(
            computed(&divider, "display"),
            "none",
            "仕切りも実際に消える"
        );
        assert!(
            divider.unchecked_ref::<HtmlElement>().hidden(),
            "仕切りも隠れる"
        );
        assert_eq!(sidebar.width(), 160.0, "閉じても幅は覚えている");
        sidebar.set_collapsed(false);
        assert!(!start_html.hidden());
        assert_eq!(
            computed(&start, "display"),
            "flex",
            "開き直すと元の並びに戻る"
        );

        // 付けたまま子を差し替えても中身の側に入る。
        let other = ui.label("別の中身")?;
        window.set_child(&other);
        assert!(end.contains(Some(&other.native_element())));

        window.clear_sidebar();
        assert!(!root.contains(Some(&aside)), "外すとページから消える");
        assert_eq!(
            other.native_element().parent_element().as_ref(),
            Some(&root),
            "子はウィンドウの直下へ戻る"
        );
        window.clear_menu_bar();
        window.close();
        Ok(())
    });
}

#[wasm_bindgen_test]
fn sidebar_callback_is_reentrant_and_replaceable() {
    with_ui(|ui| {
        let sidebar = ui.sidebar()?;
        sidebar.set_items(&SidebarItem::list(["春", "夏"]));
        let seen = Rc::new(RefCell::new(Vec::new()));
        sidebar.on_select({
            let sidebar = sidebar.clone();
            let seen = seen.clone();
            move |index| {
                seen.borrow_mut().push(index);
                sidebar.set_items(&SidebarItem::list(["冬"]));
                let seen = seen.clone();
                sidebar.on_select(move |index| seen.borrow_mut().push(100 + index));
            }
        });
        sidebar.select(1);
        assert_eq!(sidebar.len(), 1);
        sidebar.select(0);
        assert_eq!(*seen.borrow(), [1, 100]);
        Ok(())
    });
}

#[wasm_bindgen_test]
fn sidebar_toggle_button_collapses_and_notifies() {
    with_ui(|ui| {
        let window = ui.window("サイドバーの開閉", 600.0, 300.0)?;
        let content = ui.label("中身")?;
        window.set_child(&content);
        let menu_bar = ui.menu_bar()?;
        menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規"])]);
        window.set_menu_bar(&menu_bar);
        let toolbar = ui.toolbar()?;
        toolbar.set_items(&[ToolbarItem::new(ToolbarIcon::New, "新規")]);
        window.set_toolbar(&toolbar);

        let sidebar = ui.sidebar()?;
        sidebar.set_items(&SidebarItem::list(["一般"]));
        let seen = Rc::new(RefCell::new(Vec::new()));
        sidebar.on_collapse({
            let seen = seen.clone();
            move |collapsed| seen.borrow_mut().push(collapsed)
        });
        window.set_sidebar(&sidebar);

        let aside = sidebar.native_element();
        let toggle: HtmlElement = sidebar.native_toggle_button().unchecked_into();
        assert_eq!(
            toggle.get_attribute("aria-controls"),
            aside.get_attribute("id"),
            "ボタンがどの区画を開閉するかを読み上げに伝える"
        );
        assert_eq!(
            toggle.get_attribute("aria-expanded").as_deref(),
            Some("true")
        );
        // ツールバーの行に [開閉ボタン][ツールバー] の順で並ぶ (macOS と同じ)。
        let row = toggle.parent_element().expect("ツールバーの行");
        assert_eq!(
            toggle.next_element_sibling(),
            Some(toolbar.native_element()),
            "開閉ボタンの右隣がツールバー"
        );

        let start = aside.parent_element().expect("start 側の区画");
        let element = content.native_element();
        let before_toggle = toggle.get_bounding_client_rect();
        let before_content = element.get_bounding_client_rect().top();
        toggle.click();
        assert!(sidebar.is_collapsed());
        assert_eq!(computed(&start, "display"), "none", "ボタンで実際に消える");
        assert_eq!(
            toggle.parent_element(),
            Some(row.clone()),
            "ボタンは動かない"
        );
        let after_toggle = toggle.get_bounding_client_rect();
        assert_eq!(
            (after_toggle.top(), after_toggle.left()),
            (before_toggle.top(), before_toggle.left()),
            "開閉してもボタンの位置は変わらない"
        );
        assert_eq!(
            element.get_bounding_client_rect().top(),
            before_content,
            "開閉しても中身が上下しない"
        );
        assert_eq!(
            toggle.get_attribute("aria-expanded").as_deref(),
            Some("false")
        );
        assert_eq!(*seen.borrow(), [true]);
        toggle.click();
        assert!(!sidebar.is_collapsed());
        assert_eq!(*seen.borrow(), [true, false]);

        sidebar.set_collapsed(true);
        assert_eq!(
            toggle.get_attribute("aria-expanded").as_deref(),
            Some("false")
        );
        sidebar.set_collapsed(false);
        assert_eq!(seen.borrow().len(), 2, "set_collapsed では通知しない");

        // ツールバーを外してもボタンだけの行が残り、サイドバーを外すと消える。
        window.clear_toolbar();
        assert_eq!(toggle.parent_element(), Some(row.clone()));
        assert_eq!(
            menu_bar.native_element().next_element_sibling(),
            Some(row.clone()),
            "組み直してもメニューバーの直下"
        );
        window.clear_sidebar();
        assert!(toggle.parent_element().is_none(), "外すとボタンも消える");
        assert!(row.parent_element().is_none(), "空の行は置かない");
        window.clear_menu_bar();
        window.close();
        Ok(())
    });
}

/// When the window gets too narrow for the sidebar and
/// `SIDEBAR_COLLAPSE_CONTENT_WIDTH` of content, the sidebar collapses before the
/// paint and expands again when it widens. What the user chose is kept.
#[wasm_bindgen_test]
async fn sidebar_collapses_when_the_window_narrows() {
    let mut kept = None;
    with_ui(|ui| {
        let window = ui.window("サイドバーの自動の開閉", 760.0, 300.0)?;
        window.set_child(&ui.label("中身")?);
        let sidebar = ui.sidebar()?;
        sidebar.set_items(&SidebarItem::list(["一般"]));
        assert!(sidebar.auto_collapse(), "既定で自動で畳む");
        let seen = Rc::new(RefCell::new(Vec::new()));
        sidebar.on_collapse({
            let seen = seen.clone();
            move |collapsed| seen.borrow_mut().push(collapsed)
        });
        window.set_sidebar(&sidebar);
        window.show();
        kept = Some((window, sidebar, seen));
        Ok(())
    });
    let (window, sidebar, seen) = kept.expect("window");
    next_frames().await;
    assert!(!sidebar.is_collapsed(), "広いうちは開いている");
    assert!(seen.borrow().is_empty());
    let toggle: HtmlElement = sidebar.native_toggle_button().unchecked_into();
    let narrow = DEFAULT_SIDEBAR_WIDTH + naui_core::SIDEBAR_COLLAPSE_CONTENT_WIDTH - 60.0;

    window.set_size(narrow, 300.0);
    frames_until(|| !seen.borrow().is_empty()).await;
    assert!(sidebar.is_collapsed(), "狭めると畳まれる");
    assert_eq!(*seen.borrow(), [true], "畳んだことが届く");
    assert_eq!(
        toggle.get_attribute("aria-expanded").as_deref(),
        Some("false"),
        "ボタンも閉じた側"
    );

    window.set_size(760.0, 300.0);
    frames_until(|| seen.borrow().len() >= 2).await;
    assert!(!sidebar.is_collapsed(), "広げると開き直す");
    assert_eq!(*seen.borrow(), [true, false]);
    assert_eq!(sidebar.width(), DEFAULT_SIDEBAR_WIDTH, "幅は変わらない");

    // 狭いうちに利用者が開いたら開いたまま。
    window.set_size(narrow, 300.0);
    frames_until(|| seen.borrow().len() >= 3).await;
    toggle.click();
    assert!(!sidebar.is_collapsed());
    window.set_size(narrow - 20.0, 300.0);
    next_frames().await;
    assert!(!sidebar.is_collapsed(), "狭いまま動かしても閉じない");

    // 利用者が閉じたものは、狭めて広げても開かない。
    window.set_size(760.0, 300.0);
    next_frames().await;
    toggle.click();
    window.set_size(narrow, 300.0);
    next_frames().await;
    window.set_size(760.0, 300.0);
    next_frames().await;
    assert!(sidebar.is_collapsed(), "利用者が閉じたまま");
    assert_eq!(*seen.borrow(), [true, false, true, false, true]);

    // 切ると畳まない。
    sidebar.set_collapsed(false);
    sidebar.set_auto_collapse(false);
    assert!(!sidebar.auto_collapse());
    window.set_size(narrow, 300.0);
    next_frames().await;
    assert!(!sidebar.is_collapsed(), "切っていれば畳まない");
    assert_eq!(seen.borrow().len(), 5);
    window.close();
}

#[wasm_bindgen_test]
fn sidebar_divider_resizes_and_notifies() {
    with_ui(|ui| {
        let sidebar = ui.sidebar()?;
        sidebar.set_items(&SidebarItem::list(["一般"]));
        let seen = Rc::new(RefCell::new(Vec::new()));
        sidebar.on_resize({
            let seen = seen.clone();
            move |width| seen.borrow_mut().push(width)
        });
        sidebar.set_width(200.0);
        assert!(seen.borrow().is_empty(), "set_width では通知しない");

        // 仕切りは SplitView と同じく矢印キーでも動く (10 px ずつ)。
        let divider = sidebar.native_divider();
        let key = |name: &str| {
            let init = KeyboardEventInit::new();
            init.set_key(name);
            init.set_bubbles(true);
            let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init)
                .expect("キーイベント");
            divider.dispatch_event(&event).expect("配送");
        };
        key("ArrowRight");
        assert_eq!(sidebar.width(), 210.0);
        assert_eq!(*seen.borrow(), [210.0], "利用者が動かしたら通知する");
        for _ in 0..20 {
            key("ArrowLeft");
        }
        assert_eq!(
            sidebar.width(),
            naui_core::SIDEBAR_MIN_WIDTH,
            "下限で止まる"
        );

        // 閉じている間に置いた幅が、開き直したときに反映され、その後の操作も
        // 通知される。
        sidebar.set_collapsed(true);
        sidebar.set_width(300.0);
        sidebar.set_collapsed(false);
        let start: HtmlElement = sidebar
            .native_element()
            .parent_element()
            .expect("start 側の区画")
            .unchecked_into();
        assert_eq!(
            start
                .style()
                .get_property_value("width")
                .unwrap_or_default(),
            "300px",
            "開き直すと閉じている間に置いた幅"
        );
        let count = seen.borrow().len();
        key("ArrowRight");
        assert_eq!(sidebar.width(), 310.0);
        assert_eq!(
            seen.borrow().len(),
            count + 1,
            "開き直したあとの操作も通知する"
        );
        Ok(())
    });
}

// ------------------------------------------------------------- Scroll

/// A scroll view 100 high holding tall content.
fn tall_scroll(ui: &Ui, rows: usize) -> Result<(naui_web::Scroll, naui_web::Stack)> {
    let pane = ui.stack(Orientation::Vertical)?;
    for i in 0..rows {
        pane.append(&ui.label(&format!("行 {i}"))?);
    }
    let scroll = ui.scroll()?;
    scroll.set_child(&pane);
    scroll.set_sizing(Sizing::fixed(200.0, 100.0));
    Ok((scroll, pane))
}

#[wasm_bindgen_test]
fn scroll_measures_and_moves() {
    with_ui(|ui| {
        let (scroll, _pane) = tall_scroll(ui, 40)?;
        let _mounted = Mounted::new(&scroll);

        let m = scroll.metrics();
        assert_eq!((m.x, m.y), (0.0, 0.0), "最初は先頭: {m:?}");
        assert_eq!(m.viewport_height, 100.0, "{m:?}");
        assert!(
            m.content_height > m.viewport_height,
            "はみ出している: {m:?}"
        );

        scroll.scroll_to(0.0, 30.0);
        assert_eq!(scroll.metrics().y, 30.0);

        scroll.scroll_to_end();
        let end = scroll.metrics();
        assert_eq!(end.y, end.max_y(), "末尾へ: {end:?}");
        assert_eq!(end.distance_to_end(), 0.0);

        scroll.scroll_to(-50.0, 1e9);
        let clamped = scroll.metrics();
        assert_eq!((clamped.x, clamped.y), (0.0, clamped.max_y()));
        Ok(())
    });
}

/// Short content still covers the visible area, so a `Fill` child takes the
/// leftover; tall content grows past it and scrolls. Matches GtkViewport, WinUI's
/// ScrollViewer and the clip-view constraints on macOS.
#[wasm_bindgen_test]
fn scroll_content_covers_the_viewport() {
    with_ui(|ui| {
        let pane = ui.stack(Orientation::Vertical)?;
        pane.append(&ui.label("見出し")?);
        let filler = ui.stack(Orientation::Vertical)?;
        filler.set_sizing(Sizing::fill());
        pane.append(&filler);
        let scroll = ui.scroll()?;
        scroll.set_child(&pane);
        scroll.set_sizing(Sizing::fixed(200.0, 300.0));
        let _mounted = Mounted::new(&scroll);

        let height =
            |widget: &dyn Widget| widget.native_element().get_bounding_client_rect().height();
        assert_eq!(height(&pane), 300.0, "短い中身も表示領域の高さになる");
        assert!(
            height(&filler) > 250.0,
            "Fill の子が余りを受け取る: {}",
            height(&filler)
        );
        let m = scroll.metrics();
        assert_eq!(m.max_y(), 0.0, "はみ出さないので送れない: {m:?}");

        for i in 0..40 {
            pane.append(&ui.label(&format!("行 {i}"))?);
        }
        assert!(height(&pane) > 300.0, "長い中身は表示領域を越えて伸びる");
        let m = scroll.metrics();
        assert!(m.max_y() > 0.0, "はみ出した分を送れる: {m:?}");
        Ok(())
    });
}

/// A `Fill` image asks for no size and fits into what it is given, so a large one
/// does not push a vertical `Scroll` out. Asking for its pixel size, it pushed the
/// content (and the rows after it) to the image's height.
#[wasm_bindgen_test]
async fn fill_image_does_not_push_a_scroll() {
    // 3000x2000 の PNG。SVG だと自然な大きさが画素の大きさにならない (WebKit)。
    let canvas: web_sys::HtmlCanvasElement = web_sys::window()
        .and_then(|window| window.document())
        .expect("document")
        .create_element("canvas")
        .expect("canvas の生成")
        .unchecked_into();
    canvas.set_width(3000);
    canvas.set_height(2000);
    let source = canvas.to_data_url().expect("PNG への書き出し");
    let mut kept = None;
    with_ui(|ui| {
        let pane = ui.stack(Orientation::Vertical)?;
        pane.append(&ui.label("見出し")?);
        let image = ui.image(&source)?;
        image.set_sizing(Sizing::fill());
        pane.append(&image);
        pane.append(&ui.label("下端")?);
        let scroll = ui.scroll()?;
        scroll.set_child(&pane);
        scroll.set_sizing(Sizing::fixed(200.0, 300.0));
        kept = Some((scroll, pane, image));
        Ok(())
    });
    let (scroll, _pane, image) = kept.expect("組み立て");
    let _mounted = Mounted::new(&scroll);
    let element: web_sys::HtmlImageElement = image.native_element().unchecked_into();
    for _ in 0..40 {
        if element.complete() && element.natural_height() > 0 {
            break;
        }
        wait_millis(50).await;
    }
    next_frames().await;
    assert_eq!(element.natural_height(), 2000, "画像が読み込まれていること");
    let m = scroll.metrics();
    assert_eq!(
        m.content_height, m.viewport_height,
        "中身は表示領域に収まる: {m:?}"
    );
    let height = element.get_bounding_client_rect().height();
    assert!(
        height > 100.0 && height < 300.0,
        "画像は余りを受け取る: {height}"
    );

    // `Auto` の軸が残るときは、縦横比から大きさを決めるので封じ込めない。
    image.set_sizing(Sizing::fill_width());
    assert_eq!(computed(element.as_ref(), "contain"), "none");
}

/// A `Fill` video does not ask for the height its width and aspect ratio give,
/// the same as an image, so it fits into the leftover space of a vertical `Scroll`.
#[wasm_bindgen_test]
fn fill_video_does_not_ask_for_its_size() {
    with_ui(|ui| {
        let video = ui.video("")?;
        let element = video.native_element();
        let _mounted = Mounted::new(&video);
        assert_eq!(
            computed(&element, "contain"),
            "none",
            "Auto は大きさを求める"
        );

        video.set_sizing(Sizing::fill());
        assert_eq!(
            computed(&element, "contain"),
            "size",
            "Fill は大きさを求めない"
        );

        // `Auto` の軸が残るときは、縦横比から大きさを決めるので封じ込めない。
        video.set_sizing(Sizing::fill_width());
        assert_eq!(computed(&element, "contain"), "none");
        Ok(())
    });
}

/// A `Fill` image with a maximum keeps a size in an `Auto` row even when a `Fill`
/// row takes the rest: its pixel size, held to the maximum. With its natural size
/// contained to zero, it collapsed there and could not be seen (in Firefox even
/// when the maximum was given with `contain-intrinsic-height`).
///
/// In a `Fill` row it grows up to the maximum in the middle of the cell, and in a
/// cell lower than the maximum it shrinks to the cell. A height fixed at the maximum
/// would keep the row from shrinking.
#[wasm_bindgen_test]
async fn capped_fill_image_asks_for_its_maximum() {
    let canvas: web_sys::HtmlCanvasElement = web_sys::window()
        .and_then(|window| window.document())
        .expect("document")
        .create_element("canvas")
        .expect("canvas の生成")
        .unchecked_into();
    // 画素の大きさが上限を超える画像。Auto の行では上限で抑えた高さになる。
    canvas.set_width(400);
    canvas.set_height(300);
    let source = canvas.to_data_url().expect("PNG への書き出し");
    let mut kept = None;
    with_ui(|ui| {
        let image = ui.image(&source)?;
        image.set_sizing(Sizing::fill().max_height(100.0));
        let rest = ui.stack(Orientation::Vertical)?;
        rest.set_sizing(Sizing::fill());
        let grid = ui.grid()?;
        grid.set_column_track(0, naui_core::Track::FILL);
        grid.set_row_track(0, naui_core::Track::Auto);
        grid.set_row_track(1, naui_core::Track::FILL);
        grid.attach(&image, GridCell::new(0, 0));
        grid.attach(&rest, GridCell::new(0, 1));
        grid.set_sizing(Sizing::fixed(300.0, 300.0));
        kept = Some((grid, image, rest));
        Ok(())
    });
    let (grid, image, rest) = kept.expect("組み立て");
    let _mounted = Mounted::new(&grid);
    let element: web_sys::HtmlImageElement = image.native_element().unchecked_into();
    for _ in 0..40 {
        if element.complete() && element.natural_height() > 0 {
            break;
        }
        wait_millis(50).await;
    }
    next_frames().await;
    assert_eq!(
        computed(element.as_ref(), "contain"),
        "none",
        "上限が自然な大きさを抑えるので封じ込めない"
    );
    let height = element.get_bounding_client_rect().height();
    assert_eq!(height, 100.0, "Auto 行でも上限の高さを保つ");
    let rest_height = rest.native_element().get_bounding_client_rect().height();
    assert_eq!(rest_height, 200.0, "残りは Fill の行が受け取る");

    // Fill の行に置けば、上限まで広がってマスの縦中央に来る。
    grid.set_row_track(0, naui_core::Track::FILL);
    grid.set_row_track(1, naui_core::Track::Auto);
    grid.remove(GridCell::new(0, 1));
    next_frames().await;
    let grid_rect = grid.native_element().get_bounding_client_rect();
    let rect = element.get_bounding_client_rect();
    assert_eq!(rect.height(), 100.0, "上限まで広がる");
    assert_eq!(rect.top() - grid_rect.top(), 100.0, "縦はマスの中央");

    // マスが上限より低ければ、マスに合わせて縮む。
    grid.set_sizing(Sizing::fixed(300.0, 60.0));
    next_frames().await;
    assert_eq!(
        element.get_bounding_client_rect().height(),
        60.0,
        "マスより大きくならない"
    );
    assert_eq!(
        grid.native_element().get_bounding_client_rect().height(),
        60.0,
        "行もマスの高さに収まる"
    );
}

/// Calls "append, then scroll to the end", as in a chat, back to back.
#[wasm_bindgen_test]
fn scroll_to_end_after_appending() {
    with_ui(|ui| {
        let (scroll, pane) = tall_scroll(ui, 20)?;
        let _mounted = Mounted::new(&scroll);
        scroll.scroll_to_end();
        let before = scroll.metrics();
        for i in 0..20 {
            pane.append(&ui.label(&format!("追加 {i}"))?);
        }
        scroll.scroll_to_end();
        let after = scroll.metrics();
        assert!(
            after.content_height > before.content_height,
            "中身が伸びている: {before:?} → {after:?}"
        );
        assert_eq!(after.distance_to_end(), 0.0, "{after:?}");
        Ok(())
    });
}

/// A scroll-to-end requested before being placed in the document takes effect once
/// it is placed and has a size.
#[wasm_bindgen_test]
async fn scroll_to_end_before_mounted() {
    let mut kept = None;
    with_ui(|ui| {
        let (scroll, pane) = tall_scroll(ui, 40)?;
        scroll.scroll_to_end();
        kept = Some((scroll, pane));
        Ok(())
    });
    let (scroll, _pane) = kept.expect("組み立て");
    let _mounted = Mounted::new(&scroll);
    next_frames().await;
    let m = scroll.metrics();
    assert!(m.max_y() > 0.0, "{m:?}");
    assert_eq!(m.distance_to_end(), 0.0, "載った後に末尾にいる: {m:?}");
}

/// Switching tabs keeps the scroll position of a `Scroll` in a tab, as on the other
/// platforms. The browser forgets the position of a hidden (`display: none`)
/// element.
#[wasm_bindgen_test]
async fn hidden_tab_keeps_its_scroll_position() {
    let mut kept = None;
    with_ui(|ui| {
        let (scroll, _pane) = tall_scroll(ui, 40)?;
        // タブの中身はタブいっぱいに広がるので、Scroll の大きさは入れ物の中で保つ。
        let page = ui.stack(Orientation::Vertical)?;
        page.append(&scroll);
        let tabs = ui.tabs()?;
        tabs.add_tab("一覧", &page);
        tabs.add_tab("別", &ui.label("別のタブ")?);
        kept = Some((scroll, page, tabs));
        Ok(())
    });
    let (scroll, _pane, tabs) = kept.expect("組み立て");
    let _mounted = Mounted::new(&tabs);
    next_frames().await;
    scroll.scroll_to(0.0, 120.0);
    assert_eq!(scroll.metrics().y, 120.0);

    tabs.set_selected(1);
    next_frames().await;
    tabs.set_selected(0);
    frames_until(|| scroll.metrics().y == 120.0).await;
    assert_eq!(
        scroll.metrics().y,
        120.0,
        "タブを切り替えて戻ってもスクロール位置が残ること"
    );
}

#[wasm_bindgen_test]
fn scroll_notifies_only_moves() {
    with_ui(|ui| {
        let (scroll, _pane) = tall_scroll(ui, 40)?;
        let mounted = Mounted::new(&scroll);
        let seen = Rc::new(RefCell::new(Vec::new()));
        scroll.on_scroll({
            let seen = seen.clone();
            move |m: ScrollMetrics| seen.borrow_mut().push(m.y)
        });

        // ブラウザの `scroll` イベントは非同期なので、ここでは自分で起こす。
        let root: HtmlElement = mounted.0.clone().unchecked_into();
        scroll.scroll_to(0.0, 25.0);
        dispatch(root.as_ref(), "scroll");
        dispatch(root.as_ref(), "scroll"); // 位置が同じなら届かない
        assert_eq!(*seen.borrow(), vec![25.0]);

        // 利用者の操作 (ブラウザ側での移動) でも届く。
        root.set_scroll_top(40);
        dispatch(root.as_ref(), "scroll");
        assert_eq!(*seen.borrow(), vec![25.0, 40.0]);
        Ok(())
    });
}

// ------------------------------------------------- 表示・フォーカス・ツールチップ

fn height_of(element: &Element) -> f64 {
    element.get_bounding_client_rect().height()
}

fn active_element() -> Option<Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
}

#[wasm_bindgen_test]
fn hidden_widget_leaves_the_stack_layout() {
    with_ui(|ui| {
        let stack = ui.stack(Orientation::Vertical)?;
        let middle = ui.button("真ん中")?;
        // インラインの `display` を書くウィジェットでも隠れること。
        let switch = ui.toggle("スイッチ")?;
        stack.append(&ui.label("1 行目")?);
        stack.append(&middle);
        stack.append(&switch);
        stack.append(&ui.label("3 行目")?);
        let mounted = Mounted::new(&stack);
        let shown = height_of(&mounted.0);
        assert!(middle.is_visible(), "既定は表示");

        middle.set_visible(false);
        switch.set_visible(false);
        assert!(!middle.is_visible());
        assert_eq!(computed(&middle.native_element(), "display"), "none");
        assert_eq!(computed(&switch.native_element(), "display"), "none");
        let hidden = height_of(&mounted.0);
        assert!(
            hidden + 10.0 < shown,
            "隠すと場所を空けない: {shown} -> {hidden}"
        );

        middle.set_visible(true);
        switch.set_visible(true);
        assert!(middle.is_visible());
        assert_ne!(computed(&switch.native_element(), "display"), "none");
        assert_eq!(height_of(&mounted.0), shown, "戻すと元の高さ");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn request_focus_reaches_the_editor() {
    with_ui(|ui| {
        let root = ui.stack(Orientation::Vertical)?;
        let label = ui.label("見出し")?;
        let input = ui.text_input("")?;
        let area = ui.text_area("")?;
        let inner = ui.stack(Orientation::Vertical)?;
        let nested = ui.text_input("")?;
        inner.append(&ui.label("中")?);
        inner.append(&nested);
        root.append(&label);
        root.append(&input);
        root.append(&area);
        root.append(&inner);

        assert!(!input.request_focus(), "文書に載る前は移せない");
        let _mounted = Mounted::new(&root);

        assert!(!label.request_focus(), "ラベルは受け取らない");

        assert!(input.request_focus());
        let active = active_element().expect("activeElement");
        assert!(
            input.native_element().contains(Some(&active)),
            "入力欄が受け取る"
        );

        assert!(area.request_focus());
        let active = active_element().expect("activeElement");
        assert_eq!(active.tag_name(), "TEXTAREA");

        assert!(inner.request_focus(), "コンテナは中の最初の入力欄へ");
        let active = active_element().expect("activeElement");
        assert!(nested.native_element().contains(Some(&active)));

        // 隠した直後 (スタイルの計算を挟まない) でも移せないこと。
        input.set_visible(false);
        assert!(!input.request_focus(), "隠れていると移せない");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn tooltip_becomes_the_title() {
    with_ui(|ui| {
        let button = ui.button("保存")?;
        button.set_tooltip(Some("変更を保存します"));
        assert_eq!(
            button.native_element().get_attribute("title").as_deref(),
            Some("変更を保存します")
        );
        button.set_tooltip(None);
        assert_eq!(button.native_element().get_attribute("title"), None);
        Ok(())
    });
}

// ------------------------------------------------------------- on_key_down

/// Fires a `keydown`. **The return value is whether the default action was
/// prevented.**
fn key_down(target: &EventTarget, key: &str, shift: bool, composing: bool, key_code: u32) -> bool {
    let init = KeyboardEventInit::new();
    init.set_key(key);
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_shift_key(shift);
    init.set_is_composing(composing);
    init.set_key_code(key_code);
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init)
        .expect("キーイベントの生成");
    !target.dispatch_event(&event).expect("イベントの配送")
}

/// Focus entering and leaving arrives only when the owning widget changes.
#[wasm_bindgen_test]
fn focus_change_follows_the_owner() {
    with_ui(|ui| {
        let root = ui.stack(Orientation::Vertical)?;
        let name = ui.text_input("")?;
        let memo = ui.text_area("")?;
        let count = ui.number_input(1.0)?;
        let other = ui.text_input("")?;
        root.append(&name);
        root.append(&memo);
        root.append(&count);
        root.append(&other);
        let _mounted = Mounted::new(&root);

        // 通知先を置く前からフォーカスがある欄も、そこから出たことは伝わる。
        assert!(name.request_focus());
        let log = Rc::new(RefCell::new(Vec::new()));
        let record = |label: &'static str| {
            let log = log.clone();
            move |focused: bool| log.borrow_mut().push(format!("{label}:{focused}"))
        };
        name.on_focus_change(record("名前"));
        memo.on_focus_change(record("メモ"));
        count.on_focus_change(record("個数"));
        assert!(log.borrow().is_empty(), "置いただけでは通知しない");

        assert!(memo.request_focus());
        assert!(count.request_focus());
        assert_eq!(
            *log.borrow(),
            ["名前:false", "メモ:true", "メモ:false", "個数:true"]
        );

        // 通知先の無い欄へ移れば、出たことだけが伝わる。
        assert!(other.request_focus());
        assert_eq!(log.borrow()[4..], ["個数:false"]);

        // 同じ欄へ移し直しても、持ち主は変わらない。
        assert!(name.request_focus());
        assert!(name.request_focus());
        assert_eq!(log.borrow()[5..], ["名前:true"]);

        // どこにもフォーカスが無くなっても、出たことが伝わる。
        name.native_element()
            .dyn_into::<HtmlElement>()
            .expect("入力欄は HTML の要素")
            .blur()
            .expect("blur");
        assert_eq!(log.borrow()[6..], ["名前:false"]);
        Ok(())
    });
}

/// Focus is moved back during validation on leaving. `focus()` synchronously fires
/// the next `focusin`, so even when the document subscription is called
/// re-entrantly, no notification is lost.
#[wasm_bindgen_test]
fn focus_change_may_move_the_focus_back() {
    with_ui(|ui| {
        let root = ui.stack(Orientation::Vertical)?;
        let mail = ui.text_input("")?;
        let other = ui.text_input("")?;
        root.append(&mail);
        root.append(&other);
        let _mounted = Mounted::new(&root);

        let log = Rc::new(RefCell::new(Vec::new()));
        let record = |label: &'static str| {
            let log = log.clone();
            move |focused: bool| log.borrow_mut().push(format!("{label}:{focused}"))
        };
        other.on_focus_change(record("ほか"));
        mail.on_focus_change({
            let mail = mail.downgrade();
            let record = record("メール");
            let returned = Cell::new(false);
            move |focused| {
                record(focused);
                // 1 度だけ欄へ戻す。
                if !focused && !returned.replace(true) {
                    if let Some(mail) = mail.upgrade() {
                        assert!(mail.request_focus());
                    }
                }
            }
        });

        assert!(mail.request_focus());
        assert!(!other.request_focus(), "通知の中で戻されたので移っていない");
        assert_eq!(
            *log.borrow(),
            [
                "メール:true",
                "メール:false",
                "ほか:true",
                "ほか:false",
                "メール:true"
            ]
        );
        let active = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.active_element());
        assert_eq!(active.as_ref(), Some(&mail.native_element()));

        // 持ち主も戻っているので、次に出たときも伝わる。
        assert!(other.request_focus());
        assert_eq!(log.borrow()[5..], ["メール:false", "ほか:true"]);
        Ok(())
    });
}

/// Neither the handler nor the element of a discarded field remains in the
/// registry.
#[wasm_bindgen_test]
fn focus_change_entry_is_freed_with_the_widget() {
    with_ui(|ui| {
        let root = ui.stack(Orientation::Vertical)?;
        let kept = ui.text_input("")?;
        root.append(&kept);
        let _mounted = Mounted::new(&root);
        let before = naui_web::registered_focus_elements();

        // 通知先が捕まえている状態。解放されれば数が 1 へ戻る。
        let state = Rc::new(());
        let capture = || {
            let state = state.clone();
            move |_: bool| {
                let _ = &state;
            }
        };

        // 表示中の木へ入れてフォーカスを持たせてから外す。
        let input = ui.text_input("")?;
        root.append(&input);
        input.on_focus_change(capture());
        // 差し替えても、前の通知先は残らず、表にも 1 つしか載らない。
        input.on_focus_change(capture());
        assert!(input.request_focus());
        assert_eq!(Rc::strong_count(&state), 2);
        assert_eq!(naui_web::registered_focus_elements(), before + 1);
        assert!(kept.request_focus());
        root.remove(1);
        drop(input);
        assert_eq!(Rc::strong_count(&state), 1, "外して捨てた欄の通知先");
        assert_eq!(
            naui_web::registered_focus_elements(),
            before,
            "外して捨てた欄の要素"
        );

        // 木へ入れないまま捨てる。
        let area = ui.text_area("")?;
        area.on_focus_change(capture());
        drop(area);
        assert_eq!(Rc::strong_count(&state), 1, "木へ入れていない欄の通知先");
        assert_eq!(
            naui_web::registered_focus_elements(),
            before,
            "木へ入れていない欄の要素"
        );
        Ok(())
    });
}

/// Uses only Enter without Shift for sending, as in a chat input.
#[wasm_bindgen_test]
fn text_area_key_down_can_take_enter() {
    with_ui(|ui| {
        let area = ui.text_area("")?;
        let mounted = Mounted::new(&area);
        let seen = Rc::new(RefCell::new(Vec::new()));
        area.on_key_down({
            let seen = seen.clone();
            move |event: &KeyEvent| {
                seen.borrow_mut().push((event.key, event.modifiers.shift));
                if event.key == Key::Enter && !event.modifiers.shift {
                    EventResponse::Handled
                } else {
                    EventResponse::Continue
                }
            }
        });
        let target: &EventTarget = mounted.0.as_ref();

        assert!(
            key_down(target, "Enter", false, false, 13),
            "Enter は止める"
        );
        assert!(
            !key_down(target, "Enter", true, false, 13),
            "Shift+Enter は改行させる"
        );
        // 修飾キーを単独で押しても届かない。
        assert!(!key_down(target, "Shift", true, false, 16));
        assert!(!key_down(target, "A", true, false, 65));
        assert_eq!(
            *seen.borrow(),
            vec![
                (Key::Enter, false),
                (Key::Enter, true),
                (Key::Character('a'), true)
            ]
        );

        // 変換中のキーは IME のものなので届かない。
        assert!(!key_down(target, "Enter", false, true, 13));
        assert!(!key_down(target, "Process", false, false, 229));
        assert_eq!(seen.borrow().len(), 3, "変換中のキーは数えない");
        Ok(())
    });
}

/// Returning `Handled` also prevents naui's own Enter handling (confirming the
/// search).
#[wasm_bindgen_test]
fn handled_key_stops_the_search() {
    with_ui(|ui| {
        let search = ui.search_input()?;
        let mounted = Mounted::new(&search);
        let searched = Rc::new(Cell::new(0));
        search.on_search({
            let searched = searched.clone();
            move |_| searched.set(searched.get() + 1)
        });
        let handle = Rc::new(Cell::new(true));
        search.on_key_down({
            let handle = handle.clone();
            move |_| {
                if handle.get() {
                    EventResponse::Handled
                } else {
                    EventResponse::Continue
                }
            }
        });
        let input: &EventTarget = mounted.0.as_ref();
        key_down(input, "Enter", false, false, 13);
        assert_eq!(searched.get(), 0, "Handled なら検索しない");
        handle.set(false);
        key_down(input, "Enter", false, false, 13);
        assert_eq!(searched.get(), 1, "Continue なら検索する");
        Ok(())
    });
}

/// Single-line input and password input report confirmation on Enter. Enters that
/// confirm a conversion, and those while read-only, do not count.
#[wasm_bindgen_test]
fn text_inputs_submit_on_enter() {
    with_ui(|ui| {
        let input = ui.text_input("ねこ")?;
        let password = ui.password_input()?;
        let _input = Mounted::new(&input);
        let _password = Mounted::new(&password);
        let submitted = Rc::new(RefCell::new(Vec::new()));
        input.on_submit({
            let submitted = submitted.clone();
            move |text| submitted.borrow_mut().push(format!("入力:{text}"))
        });
        password.on_submit({
            let submitted = submitted.clone();
            move |text| submitted.borrow_mut().push(format!("伏せ字:{text}"))
        });
        password.set_text("ひみつ");

        let input_target: EventTarget = input.native_element().into();
        let password_target: EventTarget = password.native_element().into();
        key_down(&input_target, "a", false, false, 65);
        assert!(submitted.borrow().is_empty(), "Enter 以外では確定しない");
        key_down(&input_target, "Enter", false, true, 13);
        key_down(&input_target, "Process", false, false, 229);
        key_down(&input_target, "Enter", false, false, 229);
        assert!(submitted.borrow().is_empty(), "変換の確定は数えない");

        key_down(&input_target, "Enter", false, false, 13);
        key_down(&password_target, "Enter", false, false, 13);
        assert_eq!(*submitted.borrow(), vec!["入力:ねこ", "伏せ字:ひみつ"]);

        input.set_read_only(true);
        key_down(&input_target, "Enter", false, false, 13);
        assert_eq!(submitted.borrow().len(), 2, "読み取り専用なら確定しない");
        Ok(())
    });
}

/// Returning `Handled` also prevents Enter's confirmation.
#[wasm_bindgen_test]
fn handled_key_stops_the_submit() {
    with_ui(|ui| {
        let input = ui.text_input("")?;
        let _mounted = Mounted::new(&input);
        let submitted = Rc::new(Cell::new(0));
        input.on_submit({
            let submitted = submitted.clone();
            move |_| submitted.set(submitted.get() + 1)
        });
        input.on_key_down(|_| EventResponse::Handled);
        let target: EventTarget = input.native_element().into();
        key_down(&target, "Enter", false, false, 13);
        assert_eq!(submitted.get(), 0, "Handled なら確定しない");
        Ok(())
    });
}

/// Enter in number input reads what has been typed into the value first, then
/// reports it.
#[wasm_bindgen_test]
fn number_input_commits_before_submitting() {
    with_ui(|ui| {
        let number = ui.number_input(5.0)?;
        number.set_range(None, Some(100.0));
        let _mounted = Mounted::new(&number);
        let input = number.native_input();
        let log = Rc::new(RefCell::new(Vec::new()));
        number.on_change({
            let log = log.clone();
            move |value| log.borrow_mut().push(format!("変更:{value}"))
        });
        number.on_submit({
            let log = log.clone();
            move |value| log.borrow_mut().push(format!("確定:{value}"))
        });
        let target: EventTarget = input.clone().into();

        // `input` を起こさずに表示だけ変えても、Enter で値になる。
        input.set_value("999");
        key_down(&target, "Enter", false, false, 13);
        assert_eq!(*log.borrow(), vec!["変更:100", "確定:100"]);
        assert_eq!(input.value(), "100", "範囲の端へ寄せた表示に直す");

        // 読めない表示は元の値へ戻してから知らせる。
        input.set_value("");
        key_down(&target, "Enter", false, false, 13);
        assert_eq!(log.borrow()[2..], ["確定:100"]);
        assert_eq!(input.value(), "100");
        Ok(())
    });
}

/// Only keys for which the widget returned `Continue` reach the window.
#[wasm_bindgen_test]
fn window_key_down_gets_what_the_widget_left() {
    with_ui(|ui| {
        let window = ui.window("キー", 400.0, 300.0)?;
        let input = ui.text_input("")?;
        window.set_child(&input);
        window.show();
        input.on_key_down(|event: &KeyEvent| {
            if event.key == Key::Enter {
                EventResponse::Handled
            } else {
                EventResponse::Continue
            }
        });
        let seen = Rc::new(RefCell::new(Vec::new()));
        window.on_key_down({
            let seen = seen.clone();
            move |event: &KeyEvent| {
                seen.borrow_mut().push(event.key);
                EventResponse::Continue
            }
        });
        let target: EventTarget = input.native_element().into();
        key_down(&target, "x", false, false, 88);
        key_down(&target, "Enter", false, false, 13);
        key_down(&target, "Escape", false, false, 27);
        assert_eq!(*seen.borrow(), vec![Key::Character('x'), Key::Escape]);

        // どのウィンドウにも属さないところ (<body>) からのキーも受ける。
        key_down(body().as_ref(), "F5", false, false, 116);
        assert_eq!(seen.borrow().last(), Some(&Key::F(5)));
        window.close();
        Ok(())
    });
}

// ------------------------------------------------- アプリ向けの基本機能

/// Waits `millis` (the browser's `setTimeout`).
async fn wait_millis(millis: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        web_sys::window()
            .expect("window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, millis)
            .expect("setTimeout");
    });
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .expect("待ち");
}

#[wasm_bindgen_test]
async fn timers_fire_after_the_delay() {
    use std::time::Duration;
    let mut kept = None;
    with_ui(|ui| {
        kept = Some(ui.tasks());
        Ok(())
    });
    let tasks = kept.expect("tasks");
    let once = Rc::new(Cell::new(0));
    tasks.after(Duration::from_millis(20), {
        let once = once.clone();
        move || once.set(once.get() + 1)
    });
    let ticks = Rc::new(Cell::new(0));
    let timer = tasks.every(Duration::from_millis(10), {
        let ticks = ticks.clone();
        move || ticks.set(ticks.get() + 1)
    });
    let cancelled = Rc::new(Cell::new(false));
    tasks
        .after(Duration::from_millis(10), {
            let cancelled = cancelled.clone();
            move || cancelled.set(true)
        })
        .cancel();
    assert_eq!(once.get(), 0, "その場では呼ばない");
    wait_millis(120).await;
    assert_eq!(once.get(), 1);
    assert!(ticks.get() >= 3, "繰り返し呼ばれる: {}", ticks.get());
    timer.cancel();
    let stopped_at = ticks.get();
    wait_millis(50).await;
    assert_eq!(ticks.get(), stopped_at, "止めた後は呼ばれない");
    assert!(!cancelled.get());
}

#[wasm_bindgen_test]
fn progress_bar_switches_to_indeterminate() {
    with_ui(|ui| {
        let progress = ui.progress_bar()?;
        let _mounted = Mounted::new(&progress);
        progress.set_value(0.4);
        progress.set_indeterminate(true);
        let element = progress.native_element();
        assert!(element.matches(":indeterminate").expect("matches"));
        progress.set_value(0.7);
        progress.set_indeterminate(false);
        assert!(!element.matches(":indeterminate").expect("matches"));
        assert_eq!(progress.value(), 0.7, "不確定の間に置いた値が出る");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn spinner_is_an_indeterminate_progress() {
    with_ui(|ui| {
        let spinner = ui.spinner()?;
        let _mounted = Mounted::new(&spinner);
        assert!(spinner.is_spinning(), "作った時点で回っている");
        let element = spinner.native_element();
        assert_eq!(element.tag_name(), "PROGRESS");
        assert!(element.matches(":indeterminate").expect("matches"));
        let width = element.get_bounding_client_rect().width();
        assert!(width > 0.0);

        spinner.set_spinning(false);
        assert!(!spinner.is_spinning());
        assert_eq!(
            computed(&element, "visibility"),
            "hidden",
            "止めたら描かない"
        );
        assert_eq!(
            element.get_bounding_client_rect().width(),
            width,
            "場所は残る"
        );
        spinner.set_spinning(true);
        assert_eq!(computed(&element, "visibility"), "visible");
        Ok(())
    });
}

#[wasm_bindgen_test]
fn label_is_not_selectable_until_asked() {
    with_ui(|ui| {
        let label = ui.label("コピーできる文字")?;
        let mounted = Mounted::new(&label);
        assert!(!label.is_selectable(), "他の環境と同じく既定は選べない");
        let user_select = |element: &Element| {
            let value = computed(element, "user-select");
            if value.is_empty() {
                computed(element, "-webkit-user-select")
            } else {
                value
            }
        };
        assert_eq!(user_select(&mounted.0), "none");
        label.set_selectable(true);
        assert!(label.is_selectable());
        assert_ne!(user_select(&mounted.0), "none");
        Ok(())
    });
}

/// Permission prompts can leave a real read pending indefinitely. Use a resolved
/// promise to check callback timing without reading the user's clipboard.
#[wasm_bindgen_test]
async fn clipboard_read_calls_back_later() {
    clipboard_read_with_response(
        "return Promise.resolve('naui のテスト')",
        Some("naui のテスト"),
    )
    .await;
}

#[wasm_bindgen_test]
async fn clipboard_read_reports_a_refusal() {
    clipboard_read_with_response(
        "return Promise.reject(new DOMException('断りました', 'NotAllowedError'))",
        None,
    )
    .await;
}

#[wasm_bindgen_test]
async fn clipboard_read_empty_text_is_none() {
    clipboard_read_with_response("return Promise.resolve('')", None).await;
}

async fn clipboard_read_with_response(body: &str, expected: Option<&str>) {
    let mut kept = None;
    with_ui(|ui| {
        kept = Some(ui.clipboard());
        Ok(())
    });
    let clipboard = kept.expect("clipboard");
    let native: wasm_bindgen::JsValue = web_sys::window()
        .expect("window")
        .navigator()
        .clipboard()
        .into();
    let response = js_sys::Function::new_no_args(body);
    js_sys::Reflect::set(&native, &"readText".into(), &response).expect("差し替え");
    let seen = Rc::new(RefCell::new(None));
    clipboard.read_text({
        let seen = seen.clone();
        move |text| *seen.borrow_mut() = Some(text)
    });
    assert!(seen.borrow().is_none(), "その場では呼ばない");
    for _ in 0..50 {
        if seen.borrow().is_some() {
            break;
        }
        wait_millis(20).await;
    }
    js_sys::Reflect::delete_property(native.unchecked_ref::<js_sys::Object>(), &"readText".into())
        .expect("元に戻す");
    let text = seen.borrow_mut().take().expect("コールバックが呼ばれる");
    assert_eq!(text.as_deref(), expected);
}

/// When writing is refused, `Err` arrives. So as not to wipe the user's clipboard
/// when run locally, `writeText` is replaced with a fake that refuses.
#[wasm_bindgen_test]
async fn clipboard_write_reports_a_refusal() {
    let mut kept = None;
    with_ui(|ui| {
        kept = Some(ui.clipboard());
        Ok(())
    });
    let clipboard = kept.expect("clipboard");
    let native: wasm_bindgen::JsValue = web_sys::window()
        .expect("window")
        .navigator()
        .clipboard()
        .into();
    let refuse = js_sys::Function::new_no_args(
        "return Promise.reject(new DOMException('断りました', 'NotAllowedError'))",
    );
    js_sys::Reflect::set(&native, &"writeText".into(), &refuse).expect("差し替え");

    let written = Rc::new(RefCell::new(None));
    clipboard.set_text("naui のテスト ✂", {
        let written = written.clone();
        move |result| *written.borrow_mut() = Some(result)
    });
    assert!(written.borrow().is_none(), "その場では呼ばない");
    for _ in 0..50 {
        if written.borrow().is_some() {
            break;
        }
        wait_millis(20).await;
    }
    js_sys::Reflect::delete_property(
        native.unchecked_ref::<js_sys::Object>(),
        &"writeText".into(),
    )
    .expect("元に戻す");
    let result = written.borrow_mut().take().expect("コールバックが呼ばれる");
    let error = result.expect_err("断られたら Err");
    assert!(error.to_string().contains("断りました"), "{error}");
}

#[wasm_bindgen_test]
async fn window_reports_its_size() {
    let mut kept = None;
    with_ui(|ui| {
        let window = ui.window("大きさ", 400.0, 300.0)?;
        window.set_child(&ui.label("中身")?);
        window.show();
        kept = Some(window);
        Ok(())
    });
    let window = kept.expect("window");
    let seen = Rc::new(RefCell::new(Vec::new()));
    window.on_resize({
        let seen = seen.clone();
        move |size| seen.borrow_mut().push(size)
    });
    next_frames().await;
    let (width, height) = window.size();
    assert!(width > 0.0 && height > 0.0, "{width} x {height}");
    assert_eq!(
        seen.borrow().last(),
        Some(&(width, height)),
        "載った直後にも届く"
    );
    // Web では閉じる確認は呼ばれないが、受け付けはする。
    window.on_close_request(|| naui_core::CloseResponse::KeepOpen);
    window.close();
}

/// Web windows have no position, and the only state is fullscreen.
///
/// Ordinary browsers allow fullscreen only inside a user action, so
/// `set_state(Fullscreen)` from a test is refused and the state does not change.
/// Some automated browsers allow it (Playwright's WebKit), in which case the round
/// trip and notification are checked. Neither case throws.
///
/// Playwright's WebKit allows it **only the first time per page** and refuses from
/// the second on. So entering fullscreen is done once in this test, and the cleanup
/// when another state is requested while entering is checked in that same once.
#[wasm_bindgen_test]
async fn window_state_is_only_fullscreen() {
    let mut kept = None;
    with_ui(|ui| {
        let window = ui.window("状態", 400.0, 300.0)?;
        window.set_child(&ui.label("中身")?);
        window.show();
        kept = Some(window);
        Ok(())
    });
    let window = kept.expect("window");
    let document = web_sys::window()
        .and_then(|w| w.document())
        .expect("document");
    let seen = Rc::new(RefCell::new(Vec::new()));
    window.on_state_change({
        let seen = seen.clone();
        let document = document.clone();
        move |state| {
            let page = document.document_element();
            let page_is_fullscreen = page.is_some() && document.fullscreen_element() == page;
            seen.borrow_mut().push((state, page_is_fullscreen));
        }
    });

    assert_eq!(window.position(), None);
    window.set_position(120.0, 140.0);
    assert_eq!(window.position(), None, "動かしても取れないまま");

    // 全画面でなければ、最大化と最小化は何もしない。
    assert_eq!(window.state(), WindowState::Normal);
    window.set_state(WindowState::Maximized);
    window.set_state(WindowState::Minimized);
    next_frames().await;
    assert_eq!(window.state(), WindowState::Normal);
    assert!(document.fullscreen_element().is_none());

    // 入る途中に最大化を頼む。`fullscreenElement` は入り終わるまで空なので、
    // その場では判断できない。入り終わってから、最後の頼みどおり全画面を出る
    // (Web の最大化は全画面を出るだけ)。
    window.set_state(WindowState::Fullscreen);
    assert!(
        document.fullscreen_element().is_none(),
        "全画面に入るのは非同期"
    );
    window.set_state(WindowState::Maximized);
    for _ in 0..10 {
        next_frames().await;
    }
    frames_until(|| seen.borrow().len() >= 2).await;
    if seen.borrow().is_empty() {
        // 断られた。状態は変わらず、知らせもしない。
        assert!(document.fullscreen_element().is_none());
        assert_eq!(window.state(), WindowState::Normal);
    } else {
        // 許された。いったん全画面に入り (ページ全体)、続けて出る。
        assert_eq!(
            seen.borrow().as_slice(),
            [
                (WindowState::Fullscreen, true),
                (WindowState::Normal, false)
            ],
            "(状態, ページ全体が全画面か)"
        );
        assert!(
            document.fullscreen_element().is_none(),
            "最後の頼みどおり全画面を出ている"
        );
        assert_eq!(window.state(), WindowState::Normal);
    }

    // 全画面の出入り (`fullscreenchange`) で状態を読み直すが、変わって
    // いなければ知らせない。
    let before = seen.borrow().len();
    let event = web_sys::Event::new("fullscreenchange").expect("イベント");
    document.dispatch_event(&event).expect("dispatchEvent");
    assert_eq!(seen.borrow().len(), before, "{:?}", seen.borrow());

    // 大きさを変えられるかどうかは、効かないが値は往復する。
    assert!(window.is_resizable());
    window.set_resizable(false);
    assert!(!window.is_resizable());
    window.close();
}

/// `placement` keeps the requested size rather than the measured one, and never a
/// position. Fullscreen is left out: Playwright's WebKit allows it only once per
/// page, and `window_state_is_only_fullscreen` uses that once.
#[wasm_bindgen_test]
async fn window_placement_keeps_the_requested_size() {
    let mut kept = None;
    with_ui(|ui| {
        let window = ui.window("保存する枠", 100_000.0, 300.0)?;
        window.set_child(&ui.label("中身")?);
        window.show();
        kept = Some(window);
        Ok(())
    });
    let window = kept.expect("window");
    next_frames().await;
    assert!(window.size().0 < 100_000.0, "ブラウザの幅に収まる");
    assert_eq!(
        window.placement(),
        WindowPlacement {
            size: (100_000.0, 300.0),
            position: None,
            state: WindowState::Normal,
        }
    );
    let saved =
        WindowPlacement::parse("size=640x480 position=150,170 state=normal").expect("読める");
    window.set_placement(&saved);
    next_frames().await;
    assert_eq!(
        window.placement(),
        WindowPlacement {
            position: None,
            ..saved
        }
    );
    assert_eq!(window.position(), None);
    window.close();
}

/// Advances frames until the condition is met (giving up after 30 rounds of two
/// frames).
async fn frames_until(mut ready: impl FnMut() -> bool) {
    for _ in 0..30 {
        if ready() {
            return;
        }
        next_frames().await;
    }
}

/// The content size excludes the menu bar and toolbar rows, as on the other
/// platforms. The window's height follows the browser's visible area (the browser
/// window plays the part of the window the user resizes), and the width given to
/// `ui.window` and `set_size` is its maximum. The rows are added to the minimum
/// height given to `set_min_size`.
#[wasm_bindgen_test]
async fn window_size_excludes_the_menu_bar() {
    let mut kept = None;
    with_ui(|ui| {
        let window = ui.window("大きさ", 400.0, 300.0)?;
        window.set_child(&ui.label("中身")?);
        let menu_bar = ui.menu_bar()?;
        menu_bar.set_menus(&[MenuSpec::new("ファイル", ["新規"])]);
        window.set_menu_bar(&menu_bar);
        window.show();
        kept = Some((window, menu_bar));
        Ok(())
    });
    let (window, menu_bar) = kept.expect("window");
    next_frames().await;
    let bar = menu_bar.native_element().unchecked_into::<HtmlElement>();
    let bar_height = f64::from(bar.offset_height());
    assert!(bar_height > 0.0, "メニューバーの行に高さが付く");
    let element = window.native_element();
    let (width, height) = window.size();
    assert_eq!(
        height,
        f64::from(element.client_height()) - bar_height,
        "size はメニューバーの行を除く"
    );
    let viewport = web_sys::window()
        .and_then(|window| window.inner_height().ok())
        .and_then(|height| height.as_f64())
        .expect("表示領域の高さ");
    assert_eq!(
        f64::from(element.client_height()),
        viewport - page_inset(),
        "ウィンドウの高さはブラウザの表示領域 (ページの余白を除く) に合わせる"
    );
    assert!(width <= 400.0, "中身の幅は 400 まで: {width}");

    window.set_size(360.0, 200.0);
    next_frames().await;
    let style = window_element_style(&window);
    assert!(style.contains("max-width: 360px"), "{style}");
    assert!(
        style.contains("height: 100dvh") || style.contains("height: calc(100dvh - "),
        "高さは表示領域: {style}"
    );
    assert!(
        !style.contains("min-height"),
        "高さの指定は下限にしない: {style}"
    );

    window.set_min_size(240.0, 120.0);
    next_frames().await;
    let style = window_element_style(&window);
    assert!(
        style.contains(&format!("min-height: {}px", 120.0 + bar_height)),
        "高さの下限に行の高さを足す: {style}"
    );
    window.close();
}

/// The window's height follows the browser, so content taller than it scrolls in
/// the `Scroll` under the root instead of stretching the window. The root (a `Fill`
/// given by the window) may become smaller than its content; with the Flexbox
/// default minimum it stayed as tall as the `Scroll`'s content and was cut off.
#[wasm_bindgen_test]
async fn window_root_scroll_fits_the_window() {
    let mut kept = None;
    with_ui(|ui| {
        let window = ui.window("中で送る", 400.0, 300.0)?;
        let pane = ui.stack(Orientation::Vertical)?;
        for i in 0..200 {
            pane.append(&ui.label(&format!("行 {i}"))?);
        }
        let scroll = ui.scroll()?;
        scroll.set_child(&pane);
        scroll.set_sizing(Sizing::fill());
        let root = ui.grid()?;
        root.set_column_track(0, naui_core::Track::FILL);
        root.set_row_track(1, naui_core::Track::FILL);
        root.attach(&ui.label("見出し")?, GridCell::new(0, 0));
        root.attach(&scroll, GridCell::new(0, 1));
        window.set_child(&root);
        window.show();
        kept = Some((window, scroll));
        Ok(())
    });
    let (window, scroll) = kept.expect("window");
    next_frames().await;
    let viewport = web_sys::window()
        .and_then(|window| window.inner_height().ok())
        .and_then(|height| height.as_f64())
        .expect("表示領域の高さ");
    let element = window.native_element();
    assert_eq!(
        f64::from(element.client_height()),
        viewport - page_inset(),
        "ウィンドウは表示領域の高さ (ページの余白を除く) のまま"
    );
    let m = scroll.metrics();
    assert!(
        m.viewport_height < viewport && m.max_y() > 0.0,
        "中身は Scroll の中で送れる: {m:?}"
    );
    window.close();
}

/// The vertical margins, borders and padding of `<html>` and `<body>`.
fn page_inset() -> f64 {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .expect("document");
    [
        document.document_element(),
        document.body().map(|body| body.unchecked_into::<Element>()),
    ]
    .into_iter()
    .flatten()
    .map(|element| {
        [
            "margin-top",
            "margin-bottom",
            "border-top-width",
            "border-bottom-width",
            "padding-top",
            "padding-bottom",
        ]
        .iter()
        .map(|property| {
            computed(&element, property)
                .trim_end_matches("px")
                .parse::<f64>()
                .unwrap_or(0.0)
        })
        .sum::<f64>()
    })
    .sum()
}

/// A page that keeps the browser's 8 px margin on `<body>` (one without a style
/// sheet) does not make the window stick out at the bottom: its height is the
/// visible area less the page's margins. Otherwise the whole page would scroll even
/// when the content fits in the window.
#[wasm_bindgen_test]
async fn window_leaves_room_for_the_page_margin() {
    let body = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.body())
        .expect("body");
    let saved = body
        .style()
        .get_property_value("margin")
        .unwrap_or_default();
    let _ = body.style().set_property("margin", "8px");
    let mut kept = None;
    with_ui(|ui| {
        let window = ui.window("余白", 400.0, 300.0)?;
        window.set_child(&ui.label("中身")?);
        window.show();
        kept = Some(window);
        Ok(())
    });
    let window = kept.expect("window");
    next_frames().await;
    let viewport = web_sys::window()
        .and_then(|window| window.inner_height().ok())
        .and_then(|height| height.as_f64())
        .expect("表示領域の高さ");
    let inset = page_inset();
    let height = f64::from(window.native_element().client_height());
    window.close();
    let _ = body.style().set_property("margin", &saved);
    assert!(inset >= 16.0, "body の上下の余白を数える: {inset}");
    assert_eq!(height, viewport - inset, "ページの余白の分だけ低くなる");
}

#[wasm_bindgen_test]
fn accessible_label_read_only_and_min_size() {
    with_ui(|ui| {
        let button = ui.button("⚙")?;
        button.set_accessible_label(Some("設定"));
        assert_eq!(
            button
                .native_element()
                .get_attribute("aria-label")
                .as_deref(),
            Some("設定")
        );
        button.set_accessible_label(None);
        assert_eq!(button.native_element().get_attribute("aria-label"), None);

        let input = ui.text_input("ログ")?;
        input.set_read_only(true);
        assert!(input.is_read_only());
        assert!(input.native_element().has_attribute("readonly"));
        let area = ui.text_area("ログ")?;
        area.set_read_only(true);
        assert!(area.native_element().has_attribute("readonly"));

        let window = ui.window("最小", 320.0, 200.0)?;
        window.set_min_size(240.0, 120.0);
        let style = window_element_style(&window);
        assert!(style.contains("min-width: 240px"), "{style}");
        window.close();
        Ok(())
    });
}

/// The window element's inline style.
fn window_element_style(window: &naui_web::Window) -> String {
    window
        .native_element()
        .get_attribute("style")
        .unwrap_or_default()
}

// ------------------------------------------------------------- 行へ送る

/// Whether `row` is within `container`'s visible range.
fn is_within(container: &Element, row: &Element) -> bool {
    let view = container.get_bounding_client_rect();
    let rect = row.get_bounding_client_rect();
    rect.top() >= view.top() - 1.0 && rect.bottom() <= view.bottom() + 1.0
}

#[wasm_bindgen_test]
fn list_scrolls_a_row_into_view() {
    with_ui(|ui| {
        let labels: Vec<String> = (0..100).map(|i| format!("行 {i}")).collect();
        // 文字だけなら `<select size>`、ウィジェットの行なら `role="listbox"`。
        for widget_rows in [false, true] {
            let list = ui.list()?;
            list.set_sizing(Sizing::fixed(160.0, 200.0));
            if widget_rows {
                let rows: Vec<ListRow> = labels
                    .iter()
                    .map(|label| ui.label(label).map(|l| ListRow::new(&l)))
                    .collect::<Result<_>>()?;
                list.set_rows(&rows);
            } else {
                list.set_items(&ListItem::list(labels.iter().map(String::as_str)));
            }
            let mounted = Mounted::new(&list);
            let scroller = mounted
                .0
                .query_selector("select, [role=listbox]")
                .expect("検索")
                .expect("スクロールする要素");
            let row = |index: usize| -> Element {
                scroller
                    .query_selector_all("option, [role=option]")
                    .expect("行")
                    .item(index as u32)
                    .expect("行がある")
                    .unchecked_into()
            };
            list.scroll_to_row(80);
            assert!(
                is_within(&scroller, &row(80)),
                "80 行目が見える (widget_rows={widget_rows})"
            );
            let top = scroller.scroll_top();
            list.scroll_to_row(79);
            assert_eq!(scroller.scroll_top(), top, "見えていれば動かさない");
            assert!(list.selected().is_none(), "選択は変わらない");
        }
        Ok(())
    });
}

#[wasm_bindgen_test]
fn table_scrolls_to_a_row_it_has_not_built_yet() {
    with_ui(|ui| {
        let table = ui.table()?;
        table.set_columns(&[TableColumn::new("番号")]);
        let rows: Vec<String> = (0..20_000).map(|i| i.to_string()).collect();
        table.set_rows(&TableRow::list(rows.iter().map(|r| [r.as_str()])));
        table.set_sizing(Sizing::fixed(200.0, 240.0));
        let mounted = Mounted::new(&table);
        table.scroll_to_row(15_000);
        let built: Vec<usize> = table_rows(&table).iter().map(table_row_index).collect();
        assert!(
            built.contains(&15_000),
            "送った先の行を組み立てる: {built:?}"
        );
        let _ = mounted;
        Ok(())
    });
}

#[wasm_bindgen_test]
fn tree_opens_ancestors_and_scrolls_to_an_item() {
    with_ui(|ui| {
        let tree = ui.tree()?;
        let labels: Vec<String> = (0..100).map(|i| format!("項目 {i}")).collect();
        tree.set_items(&[naui_core::TreeItem::new("上")
            .children(naui_core::TreeItem::list(labels.iter().map(String::as_str)))]);
        tree.set_sizing(Sizing::fixed(200.0, 200.0));
        let mounted = Mounted::new(&tree);
        let expanded = Rc::new(Cell::new(0));
        tree.on_expand({
            let expanded = expanded.clone();
            move |_, _| expanded.set(expanded.get() + 1)
        });
        tree.scroll_to_item(&[0, 70]);
        assert!(tree.is_expanded(&[0]), "祖先を開く");
        assert_eq!(expanded.get(), 0, "開閉は通知しない");
        let item = mounted
            .0
            .query_selector_all("[role=treeitem]")
            .expect("項目")
            .item(71)
            .expect("71 番目の項目")
            .unchecked_into::<Element>();
        assert!(is_within(&mounted.0, &item), "項目が見える");
        Ok(())
    });
}

// ------------------------------------------------------------- WebWidgetExt

#[wasm_bindgen_test]
fn web_ext_adds_classes_and_attributes() {
    with_ui(|ui| {
        let button = ui.button("保存")?;
        button.add_class("primary");
        button.add_class("wide");
        assert!(button.has_class("primary"));
        button.remove_class("wide");
        assert!(!button.has_class("wide"));
        assert_eq!(
            button.native_element().get_attribute("class").as_deref(),
            Some("primary")
        );

        assert!(button.set_attribute("data-testid", "save"));
        assert_eq!(button.attribute("data-testid").as_deref(), Some("save"));
        assert!(button.remove_attribute("data-testid"));
        assert_eq!(button.attribute("data-testid"), None);

        // naui が使う属性は書き換えない。
        button.set_sizing(Sizing::fixed(120.0, 32.0));
        let style = button.attribute("style");
        assert!(!button.set_attribute("style", "width: 1px"));
        assert!(!button.remove_attribute("STYLE"));
        assert!(!button.set_attribute("data-naui-hidden", ""));
        assert_eq!(button.attribute("style"), style, "大きさの指定は残る");
        button.set_visible(false);
        assert!(!button.remove_attribute("data-naui-hidden"));
        assert!(!button.is_visible(), "隠したものは隠れたまま");

        // `title` は `set_tooltip` と同じ属性で、後から書いたほうが残る。
        button.set_tooltip(Some("変更を保存します"));
        assert_eq!(
            button.attribute("title").as_deref(),
            Some("変更を保存します")
        );
        assert!(button.set_attribute("title", "保存"));
        assert_eq!(button.attribute("title").as_deref(), Some("保存"));

        // コンテナにも使える。
        let stack = ui.stack(Orientation::Vertical)?;
        stack.add_class("toolbar");
        assert!(stack.has_class("toolbar"));
        Ok(())
    });
}

// ------------------------------------------------------------- ファイルの受け取り

/// Makes the drag event the browser fires when files from the OS are dragged in.
/// `files` is a list of names; each file holds its own name as its contents. With
/// no names, it is a drag of selected text instead.
fn file_drag_event(kind: &str, files: &[&str]) -> web_sys::Event {
    let make = js_sys::Function::new_with_args(
        "kind, names",
        "const data = new DataTransfer();
         if (names.length === 0) data.setData('text/plain', '文字');
         for (const name of names) data.items.add(new File([name], name, { type: 'text/plain' }));
         return new DragEvent(kind, { dataTransfer: data, bubbles: true, cancelable: true });",
    );
    let names: js_sys::Array = files.iter().map(|name| JsValue::from_str(name)).collect();
    make.call2(&JsValue::NULL, &JsValue::from_str(kind), &names)
        .expect("ドラッグのイベントの生成")
        .unchecked_into()
}

/// Fires a drag event. **The return value is whether the default action was
/// prevented**, which for `dragover` means the element accepts the drop.
fn fire_drag(target: &EventTarget, kind: &str, files: &[&str]) -> bool {
    !target
        .dispatch_event(&file_drag_event(kind, files))
        .expect("イベントの配送")
}

/// Files dropped from the OS go to the innermost widget with `on_file_drop`, and
/// otherwise to the window's. The browser hands over contents, so they arrive as
/// `blob:` URLs without paths.
#[wasm_bindgen_test]
async fn file_drop_reaches_the_innermost_target() {
    let mut kept = None;
    with_ui(|ui| {
        let window = ui.window("ファイルの受け取り", 400.0, 300.0)?;
        let stack = ui.stack(Orientation::Vertical)?;
        let outer = ui.stack(Orientation::Vertical)?;
        let target = ui.label("ここへ落とす")?;
        let plain = ui.button("受け取らない")?;
        outer.append(&target);
        stack.append(&outer);
        stack.append(&plain);
        window.set_child(&stack);
        window.show();
        kept = Some((window, outer, target, plain));
        Ok(())
    });
    let (window, outer, target, plain) = kept.expect("widgets");

    let to_target = Rc::new(RefCell::new(Vec::new()));
    let to_outer = Rc::new(Cell::new(0));
    target.on_file_drop({
        let seen = to_target.clone();
        move |files| seen.borrow_mut().extend(files.iter().cloned())
    });
    outer.on_file_drop({
        let seen = to_outer.clone();
        move |_| seen.set(seen.get() + 1)
    });

    let on_target = target.native_element();
    let on_plain = plain.native_element();
    assert!(
        fire_drag(&on_target, "dragover", &["メモ.txt"]),
        "受け取るウィジェットの上では落とせる"
    );
    assert!(
        !fire_drag(&on_target, "dragover", &[]),
        "ファイルでないドラッグには応えない"
    );
    assert!(
        fire_drag(&on_plain, "dragover", &["メモ.txt"]),
        "受け取り手のいない所では、文書が既定の動作 (ファイルを開く) を止める"
    );

    assert!(fire_drag(&on_target, "drop", &["メモ.txt", "写真.png"]));
    let names: Vec<String> = to_target
        .borrow()
        .iter()
        .map(|f| f.name().to_string())
        .collect();
    assert_eq!(names, ["メモ.txt", "写真.png"]);
    let first = to_target.borrow()[0].clone();
    assert_eq!(first.path(), None, "ブラウザはパスを渡さない");
    let url = first.source().expect("中身を指す URL").to_string();
    assert!(url.starts_with("blob:"), "{url}");
    assert_eq!(to_outer.get(), 0, "内側が受け取ったら外側へは届かない");

    // 渡した URL から中身を読める。
    assert_eq!(read_blob(&url).await.as_deref(), Some("メモ.txt"));

    // 受け取り手のいない所へ落としたものは、窓へ届く。
    let to_window = Rc::new(RefCell::new(Vec::new()));
    window.on_file_drop({
        let seen = to_window.clone();
        move |files| {
            seen.borrow_mut()
                .extend(files.iter().map(|f| f.name().to_string()))
        }
    });
    assert!(fire_drag(&on_plain, "drop", &["別.txt"]));
    assert_eq!(*to_window.borrow(), ["別.txt"]);
    assert_eq!(
        to_target.borrow().len(),
        2,
        "ほかのウィジェットへは届かない"
    );

    // 設定し直すと前のクロージャは外れる。
    let replaced = Rc::new(Cell::new(0));
    target.on_file_drop({
        let replaced = replaced.clone();
        move |_| replaced.set(replaced.get() + 1)
    });
    assert!(fire_drag(&on_target, "drop", &["メモ.txt"]));
    assert_eq!(replaced.get(), 1);
    assert_eq!(to_target.borrow().len(), 2, "前のクロージャは呼ばれない");
    assert_eq!(
        read_blob(&url).await,
        None,
        "設定し直しても、前のクロージャへ渡した URL は次のドロップで取り消す"
    );
    window.close();
}

/// Drops onto text fields do not reach `on_file_drop`, even the parent's, as on the
/// other platforms. The document refuses them instead.
#[wasm_bindgen_test]
fn file_drop_skips_text_fields() {
    let mut kept = None;
    with_ui(|ui| {
        let window = ui.window("入力欄へのドロップ", 400.0, 300.0)?;
        let outer = ui.stack(Orientation::Vertical)?;
        let input = ui.text_input("")?;
        let area = ui.text_area("")?;
        let plain = ui.label("受け取る")?;
        outer.append(&input);
        outer.append(&area);
        outer.append(&plain);
        window.set_child(&outer);
        window.show();
        kept = Some((window, outer, input, area, plain));
        Ok(())
    });
    let (window, outer, input, area, plain) = kept.expect("widgets");
    let dropped = Rc::new(Cell::new(0));
    outer.on_file_drop({
        let dropped = dropped.clone();
        move |_| dropped.set(dropped.get() + 1)
    });
    // 入力欄自身に設定しても、入力欄へのドロップは届かない。
    input.on_file_drop({
        let dropped = dropped.clone();
        move |_| dropped.set(dropped.get() + 1)
    });

    let area_element = area
        .native_element()
        .query_selector("textarea")
        .expect("検索")
        .unwrap_or_else(|| area.native_element());
    for field in [input.native_element(), area_element] {
        assert!(
            fire_drag(&field, "drop", &["メモ.txt"]),
            "ブラウザがファイルを開かないよう、文書が断る"
        );
    }
    assert_eq!(dropped.get(), 0, "入力欄へのドロップは親へも届かない");

    assert!(fire_drag(&plain.native_element(), "drop", &["メモ.txt"]));
    assert_eq!(dropped.get(), 1, "入力欄の外なら親が受け取る");
    window.close();
}

/// The URLs handed out are revoked when the widget that received them goes away,
/// not only on the next drop.
#[wasm_bindgen_test]
async fn file_drop_revokes_urls_when_the_widget_goes_away() {
    let mut kept = None;
    with_ui(|ui| {
        let window = ui.window("受け口の破棄", 400.0, 300.0)?;
        let stack = ui.stack(Orientation::Vertical)?;
        let target = ui.label("ここへ落とす")?;
        stack.append(&target);
        window.set_child(&stack);
        window.show();
        kept = Some((window, stack, target));
        Ok(())
    });
    let (window, stack, target) = kept.expect("widgets");
    let urls = Rc::new(RefCell::new(Vec::new()));
    target.on_file_drop({
        let urls = urls.clone();
        move |files| {
            urls.borrow_mut()
                .extend(files.iter().filter_map(|f| f.source()).map(str::to_string))
        }
    });
    let element = target.native_element();
    assert!(fire_drag(&element, "drop", &["メモ.txt"]));
    let url = urls.borrow()[0].clone();
    assert_eq!(read_blob(&url).await.as_deref(), Some("メモ.txt"));

    stack.clear();
    drop(target);
    assert_eq!(
        read_blob(&url).await,
        None,
        "ウィジェットが無くなったら、最後に渡した URL も取り消す"
    );
    window.close();
}

/// Reads the text behind a `blob:` URL. `None` once the URL has been revoked.
async fn read_blob(url: &str) -> Option<String> {
    let response: web_sys::Response = wasm_bindgen_futures::JsFuture::from(
        web_sys::window().expect("window").fetch_with_str(url),
    )
    .await
    .ok()?
    .unchecked_into();
    let text = wasm_bindgen_futures::JsFuture::from(response.text().ok()?)
        .await
        .ok()?;
    text.as_string()
}
