//! ヘッドレスで実際に操作し、メッセージが流れて状態が変わることを確認する。

use miui::headless::Headless;
use miui::prelude::*;
use miui::theme;

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Pressed,
    Toggled(bool),
    NameChanged(String),
    Volume(f32),
}

#[derive(Default)]
struct TestApp {
    presses: u32,
    checked: bool,
    name: String,
    volume: f32,
}

/// 各ウィジェットを高さ固定の帯に置く。コントロールの高さはテーマごとに
/// 違うため、テスト側で当たり判定の座標を固定できるようにしておく。
const ROW: f32 = 60.0;

fn band(child: impl Widget<Msg> + 'static) -> Container<Msg> {
    Container::new().height(ROW).fill_width().child(child)
}

/// `index` 番目の帯の中で、確実にウィジェットへ当たる座標。
fn at(index: usize, x: f32) -> Point {
    Point::new(x, ROW * index as f32 + 8.0)
}

impl Application for TestApp {
    type Message = Msg;

    fn view(&self) -> Element<Msg> {
        Element::new(
            column()
                .spacing(0.0)
                .align(CrossAxis::Stretch)
                .child(band(
                    Button::new("押す").min_width(120.0).on_press(Msg::Pressed),
                ))
                .child(band(
                    Checkbox::new("有効", self.checked).on_toggle(Msg::Toggled),
                ))
                .child(band(
                    TextInput::new(self.name.clone()).on_change(Msg::NameChanged),
                ))
                .child(band(
                    Slider::new(self.volume, 0.0..=1.0).on_change(Msg::Volume),
                )),
        )
    }

    fn update(&mut self, message: Msg) {
        match message {
            Msg::Pressed => self.presses += 1,
            Msg::Toggled(v) => self.checked = v,
            Msg::NameChanged(v) => self.name = v,
            Msg::Volume(v) => self.volume = v,
        }
    }
}

fn fixture() -> (TestApp, Headless, Theme, Size) {
    (
        TestApp::default(),
        Headless::new(),
        theme::for_target(ColorMode::Light),
        Size::new(400.0, ROW * 4.0),
    )
}

/// ボタンの上で押して離すとメッセージが 1 回だけ飛ぶ。
#[test]
fn clicking_a_button_emits_once() {
    let (mut app, mut h, theme, size) = fixture();
    let at = at(0, 20.0);

    h.dispatch(
        &mut app,
        &theme,
        size,
        &Event::PointerPressed {
            position: at,
            button: MouseButton::Left,
        },
    );
    assert_eq!(app.presses, 0, "押下だけでは発火しない");

    h.dispatch(
        &mut app,
        &theme,
        size,
        &Event::PointerReleased {
            position: at,
            button: MouseButton::Left,
        },
    );
    assert_eq!(app.presses, 1);
}

/// ボタンの外で離した場合は発火しない。
#[test]
fn releasing_outside_cancels_the_press() {
    let (mut app, mut h, theme, size) = fixture();
    h.dispatch(
        &mut app,
        &theme,
        size,
        &Event::PointerPressed {
            position: at(0, 20.0),
            button: MouseButton::Left,
        },
    );
    h.dispatch(
        &mut app,
        &theme,
        size,
        &Event::PointerReleased {
            position: Point::new(380.0, ROW * 4.0 - 5.0),
            button: MouseButton::Left,
        },
    );
    assert_eq!(app.presses, 0);
}

/// ポインタを重ねるとホバー状態になる。
#[test]
fn pointer_move_sets_hover() {
    let (mut app, mut h, theme, size) = fixture();
    h.dispatch(
        &mut app,
        &theme,
        size,
        &Event::PointerMoved(at(0, 20.0)),
    );
    assert!(h.interaction.hovered.is_some());

    h.dispatch(
        &mut app,
        &theme,
        size,
        &Event::PointerMoved(Point::new(390.0, ROW * 4.0 - 5.0)),
    );
    assert!(h.interaction.hovered.is_none());
}

/// チェックボックスをクリックすると反転する。
#[test]
fn checkbox_toggles() {
    let (mut app, mut h, theme, size) = fixture();
    let at = at(1, 10.0);
    h.dispatch(
        &mut app,
        &theme,
        size,
        &Event::PointerPressed {
            position: at,
            button: MouseButton::Left,
        },
    );
    h.dispatch(
        &mut app,
        &theme,
        size,
        &Event::PointerReleased {
            position: at,
            button: MouseButton::Left,
        },
    );
    assert!(app.checked);
}

/// テキスト入力にフォーカスして文字を打つと値が変わる。日本語も通る。
#[test]
fn text_input_accepts_characters_including_japanese() {
    let (mut app, mut h, theme, size) = fixture();
    let at = at(2, 40.0);
    h.dispatch(
        &mut app,
        &theme,
        size,
        &Event::PointerPressed {
            position: at,
            button: MouseButton::Left,
        },
    );
    h.dispatch(
        &mut app,
        &theme,
        size,
        &Event::PointerReleased {
            position: at,
            button: MouseButton::Left,
        },
    );
    assert!(h.interaction.focused.is_some(), "クリックでフォーカスが移る");

    h.dispatch(&mut app, &theme, size, &Event::Text("あ".into()));
    h.dispatch(&mut app, &theme, size, &Event::Text("い".into()));
    assert_eq!(app.name, "あい");

    h.dispatch(
        &mut app,
        &theme,
        size,
        &Event::KeyPressed {
            key: Key::Backspace,
            modifiers: Modifiers::default(),
        },
    );
    assert_eq!(app.name, "あ", "マルチバイト文字を 1 文字単位で削除する");
}

/// スライダーをクリックした位置に値が飛ぶ。
#[test]
fn slider_jumps_to_click_position() {
    let (mut app, mut h, theme, size) = fixture();
    h.dispatch(
        &mut app,
        &theme,
        size,
        &Event::PointerPressed {
            position: at(3, 200.0),
            button: MouseButton::Left,
        },
    );
    assert!(
        app.volume > 0.4 && app.volume < 0.6,
        "中央付近をクリックしたら中央付近の値になる: {}",
        app.volume
    );
}

/// 描画したバッファが背景色以外を含む (= 何かが描かれている)。
#[test]
fn rendering_produces_non_background_pixels() {
    let (app, mut h, theme, _) = fixture();
    let buffer = h.render(&app, &theme, 400, (ROW * 4.0) as u32, 1.0);
    let bg = theme.color.window_bg.to_argb8() & 0x00FF_FFFF;
    let painted = buffer.iter().filter(|p| **p != bg).count();
    assert!(
        painted > 1000,
        "描画されたピクセルが少なすぎる: {painted}"
    );
}
