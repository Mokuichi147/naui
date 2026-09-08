//! ブラウザの実 DOM に対する動作確認。
//!
//! DOM のイベント (`click` / `input` / `change`) を**実際に起こして**、Rust の
//! クロージャへ届くこと・ブラウザ側の状態が変わることを確かめる。大きさや
//! 位置は `getBoundingClientRect` の結果で測る。
//!
//! Web バックエンドはウィジェットを自前で描かず、ブラウザの挙動 (ラベルを
//! 押すとチェックが入る、同じ `name` のラジオは自動で排他になる、Flexbox が
//! 子を並べる) にそのまま乗っている。そこはブラウザの上でしか確かめられない
//! ので、`wasm_bindgen_test_configure!(run_in_browser)` で実ブラウザへ載せる。
//!
//!   cargo test --target wasm32-unknown-unknown -p naui-web
//!
//! ドライバ (chromedriver / geckodriver) が要る。手元に無いときは
//! `NO_HEADLESS=1` を付けると、ランナーが待ち受ける URL を
//! 表示するので、任意のブラウザで開けば同じテストが走る。

#![cfg(target_arch = "wasm32")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    Align, Color, DialogResponse, GridCell, Length, ListItem, Orientation, Padding, PopupItem,
    Result, Sizing, TableColumn, TableRow, TextColor, TextStyle, Theme,
};
use naui_web::{run_for_test, ListRow, TableCells, Ui, Widget};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{
    Element, EventTarget, HtmlElement, HtmlInputElement, HtmlSelectElement, KeyboardEvent,
    KeyboardEventInit,
};

wasm_bindgen_test_configure!(run_in_browser);

// ------------------------------------------------------------- 補助

/// ウィジェットを作って調べる。組み立てに失敗したらテストを落とす。
fn with_ui(build: impl FnOnce(&Ui) -> Result<()>) {
    run_for_test(build).expect("テスト用の UI を組み立てられませんでした");
}

/// `<body>` へ載せている間だけ生きるガード。
///
/// ドキュメントに載っていない要素は、ブラウザがレイアウトせず、
/// `getComputedStyle` も空を返し、`click()` を呼んでも `change` が飛ばない
/// (`checked` だけが変わる)。位置・算出スタイル・通知を見るテストは、
/// 実際のアプリと同じようにページへ載せてから確かめる。
struct Mounted(Element);

impl Mounted {
    fn new(widget: &dyn Widget) -> Self {
        let element = widget.native_element();
        body().append_child(&element).expect("body への追加");
        Self(element)
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

/// ブラウザ側で起きるのと同じイベントを起こす。
fn dispatch(target: &EventTarget, kind: &str) {
    let event = web_sys::Event::new(kind).expect("イベントの生成");
    target.dispatch_event(&event).expect("イベントの配送");
}

/// Escape の `keydown` を起こす。**戻り値は既定動作を止められたか**
/// (`dispatchEvent` は `preventDefault()` されると `false` を返す)。
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

/// 算出後のスタイル (ブラウザが解釈した結果)。
fn computed(element: &Element, property: &str) -> String {
    web_sys::window()
        .expect("window")
        .get_computed_style(element)
        .expect("算出スタイルの取得")
        .expect("算出スタイル")
        .get_property_value(property)
        .expect("プロパティの取得")
}

/// 要素の内側にある最初の `<input>`。
fn first_input(element: &Element) -> HtmlInputElement {
    element
        .query_selector("input")
        .expect("input の検索")
        .expect("input が見つかりません")
        .unchecked_into()
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

/// `set_style` / `set_color` が、実際に描かれる文字の大きさと色を変える。
///
/// Web にだけ見出しの段階が無いので、naui が CSS で他の 3 環境へそろえている。
/// 指定した値ではなく、ブラウザが解決したあとの値を見る。
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

/// `<div>` に並んだ子のテキストを、並び順で取り出す。
fn child_texts(widget: &dyn Widget) -> Vec<String> {
    let element = widget.native_element();
    let children = element.children();
    (0..children.length())
        .filter_map(|index| children.item(index))
        .map(|child| child.text_content().unwrap_or_default())
        .collect()
}

/// `Stack` は後から子を差し込み、外し、空にできる。
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

/// `Grid` はマスを指定して子を外せる。
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

/// タブを外しても、残ったタブを押したときのインデックスがずれない。
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

/// `ListItem::detail` は 2 行目に小さく淡く出る。
///
/// 行の中身は naui の `Stack` と `Label` で組むので、小ささと淡さを決めるのは
/// [`TextStyle::Caption`] と [`TextColor::Secondary`] のほう。ここでは、
/// ブラウザが解決したあとの値でそれが効いていることを見る。
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

/// 行の文字は行の幅いっぱいに広がり、あふれた分は省略記号で切られる。
///
/// `Label` は既定で `nowrap` + `ellipsis` だが、**幅が決まらないと切れない**。
/// 内容の幅のままだと、長いラベルが行から横へはみ出す。
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

/// `Ui` は clone できるので、コールバックの中からでもウィジェットを作れる。
///
/// 押されたときに 1 行増やす画面では、通知の中で行の中身を組み立てて
/// `set_rows` へ渡すことになる。
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

/// 表の `<tbody>` にある、詰め物ではない行。
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

/// 行のインデックス (`aria-rowindex` は見出しの分だけずれている)。
fn table_row_index(row: &HtmlElement) -> usize {
    row.get_attribute("aria-rowindex")
        .expect("aria-rowindex")
        .parse::<usize>()
        .expect("数値")
        - 2
}

/// 行数が多い表では、`<tr>` を作るのは画面に出ている分だけになる。
/// それでもスクロールできる高さは全行分あり、スクロールすると中身が入れ替わる。
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

/// 窓の外にある行を選んでも、選択は覚えられていて、
/// スクロールで戻ってきたときに選ばれた見た目になる。
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

/// 行が多い表でも、見出しを押した並べ替えはそのまま働く。
///
/// 並べ替えるのはアプリなので、通知を受けて `set_rows` で渡し直したものが、
/// 画面に出ている行へ反映されることまで確かめる。
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

/// 組み立てる行 (`TableCells`) でも、行数が多いときは見えている分だけを作り、
/// 見出しの並べ替えもそのまま働く。
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

/// セルにウィジェットを置ける。行のクリックは activation になり、
/// セルの中のボタンを押したときは行の activation にはならない。
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
                    .text(format!("都市 {index}"))
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
        drop(mounted);
        Ok(())
    });
}

/// 行の高さを決めると、その高さで詰め物も引き直される。
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
