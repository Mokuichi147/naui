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

use naui_core::{Align, Color, GridCell, Orientation, Padding, Result, Theme};
use naui_web::{run_for_test, ListRow, Ui, Widget};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{Element, EventTarget, HtmlElement, HtmlInputElement, HtmlSelectElement};

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
