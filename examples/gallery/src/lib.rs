//! miui のウィジェットギャラリー。
//!
//! 表示されているコントロールはすべて miui が自前で描いたもので、
//! OS のウィジェットではない。

use miui::prelude::*;
use miui::theme;

/// Web ビルド時に埋め込まれるフォント (`MIUI_WEB_FONT` で指定)。
#[cfg(target_arch = "wasm32")]
const WEB_FONT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/webfont.ttf"));

#[derive(Clone, Debug)]
pub enum Msg {
    SetMode(Option<ColorMode>),
    Increment,
    Decrement,
    ToggleNotify(bool),
    ToggleSync(bool),
    SelectPlan(usize),
    NameChanged(String),
    Submit,
    VolumeChanged(f32),
    Reset,
}

pub struct Gallery {
    /// `None` なら OS のライト / ダーク設定に従う。
    mode: Option<ColorMode>,
    count: i32,
    notify: bool,
    sync: bool,
    plan: usize,
    name: String,
    submitted: Option<String>,
    volume: f32,
}

impl Default for Gallery {
    fn default() -> Self {
        Self {
            mode: None,
            count: 3,
            notify: true,
            sync: false,
            plan: 1,
            name: String::new(),
            submitted: None,
            volume: 0.4,
        }
    }
}

impl Gallery {
    /// 見出し + 中身のカード。
    fn card(&self, title: &str, body: impl Widget<Msg> + 'static) -> Container<Msg> {
        Container::card(
            column()
                .spacing(14.0)
                .align(CrossAxis::Stretch)
                .child(Text::new(title.to_string()).subtitle())
                .child(body),
        )
        .padding(Insets::all(20.0))
    }

    fn header(&self) -> impl Widget<Msg> {
        let dark = matches!(self.mode, Some(ColorMode::Dark));
        column()
            .spacing(6.0)
            .align(CrossAxis::Stretch)
            .child(Text::new("miui ウィジェットギャラリー").title())
            .child(
                Text::new(
                    "Rust だけで書かれた軽量 GUI。OS のウィジェットは使わず、\
                     このプラットフォームのデザイン言語を模して自前で描画しています。",
                )
                .secondary(),
            )
            .child(
                row()
                    .align(CrossAxis::Center)
                    .padding(Insets::new(10.0, 0.0, 0.0, 0.0))
                    .child_flex(Spacer::new(), 1.0)
                    .child(Switch::new("ダークモード", dark).on_toggle(|on| {
                        Msg::SetMode(if on {
                            Some(ColorMode::Dark)
                        } else {
                            Some(ColorMode::Light)
                        })
                    })),
            )
    }

    fn buttons_card(&self) -> impl Widget<Msg> {
        self.card(
            "ボタン",
            column()
                .spacing(12.0)
                .align(CrossAxis::Stretch)
                .child(
                    row()
                        .spacing(10.0)
                        .align(CrossAxis::Center)
                        .child(Button::new("標準").on_press(Msg::Increment))
                        .child(Button::new("アクセント").accent().on_press(Msg::Increment))
                        .child(Button::new("サブトル").subtle().on_press(Msg::Increment))
                        .child(Button::new("破壊的").danger().on_press(Msg::Reset))
                        .child(Button::new("無効").enabled(false)),
                )
                .child(
                    row()
                        .spacing(10.0)
                        .align(CrossAxis::Center)
                        .child(
                            IconButton::new(IconGlyph::Minus)
                                .on_press(Msg::Decrement)
                                .enabled(self.count > 0),
                        )
                        .child(
                            Container::new()
                                .width(56.0)
                                .align(Alignment::Center)
                                .child(Text::new(self.count.to_string()).subtitle()),
                        )
                        .child(IconButton::new(IconGlyph::Plus).on_press(Msg::Increment))
                        .child(Spacer::size(12.0))
                        .child(Text::new("カウンタ").secondary()),
                ),
        )
    }

    fn selection_card(&self) -> impl Widget<Msg> {
        self.card(
            "選択",
            column()
                .spacing(12.0)
                .align(CrossAxis::Start)
                .child(Checkbox::new("通知を受け取る", self.notify).on_toggle(Msg::ToggleNotify))
                .child(Switch::new("バックグラウンド同期", self.sync).on_toggle(Msg::ToggleSync))
                .child(Divider::horizontal())
                .child(Radio::new("無料プラン", self.plan == 0).on_select(Msg::SelectPlan(0)))
                .child(Radio::new("標準プラン", self.plan == 1).on_select(Msg::SelectPlan(1)))
                .child(Radio::new("Pro (準備中)", self.plan == 2).enabled(false)),
        )
    }

    fn input_card(&self) -> impl Widget<Msg> {
        let result = match &self.submitted {
            Some(name) if !name.is_empty() => format!("こんにちは、{name} さん"),
            Some(_) => "名前が空です".to_string(),
            None => "Enter か「送信」で確定します".to_string(),
        };
        self.card(
            "テキスト入力",
            column()
                .spacing(12.0)
                .align(CrossAxis::Stretch)
                .child(
                    TextInput::new(self.name.clone())
                        .placeholder("名前を入力 (日本語入力にも対応)")
                        .on_change(Msg::NameChanged)
                        .on_submit(Msg::Submit),
                )
                .child(
                    row()
                        .spacing(10.0)
                        .align(CrossAxis::Center)
                        .child(Button::new("送信").accent().on_press(Msg::Submit))
                        .child(Text::new(result).secondary()),
                ),
        )
    }

    fn value_card(&self) -> impl Widget<Msg> {
        self.card(
            "値の調整",
            column()
                .spacing(14.0)
                .align(CrossAxis::Stretch)
                .child(
                    row()
                        .spacing(12.0)
                        .align(CrossAxis::Center)
                        .child_flex(
                            Slider::new(self.volume, 0.0..=1.0)
                                .step(0.01)
                                .on_change(Msg::VolumeChanged),
                            1.0,
                        )
                        .child(
                            Container::new().width(52.0).align(Alignment::CenterRight).child(
                                Text::new(format!("{:.0}%", self.volume * 100.0)).strong(),
                            ),
                        ),
                )
                .child(ProgressBar::new(self.volume).height(6.0))
                .child(
                    Text::new("スライダーは ← → キーでも動かせます。Tab でフォーカス移動。")
                        .caption()
                        .secondary(),
                ),
        )
    }
}

impl Application for Gallery {
    type Message = Msg;

    fn view(&self) -> Element<Msg> {
        Element::new(
            Scroll::new().child(
                column()
                    .spacing(20.0)
                    .padding(Insets::all(24.0))
                    .align(CrossAxis::Stretch)
                    .child(self.header())
                    .child(self.buttons_card())
                    .child(self.selection_card())
                    .child(self.input_card())
                    .child(self.value_card())
                    .child(
                        Text::new(format!(
                            "配色: {}",
                            match self.mode {
                                Some(ColorMode::Dark) => "ダーク (手動)",
                                Some(ColorMode::Light) => "ライト (手動)",
                                None => "OS 設定に追従",
                            }
                        ))
                        .caption()
                        .secondary(),
                    ),
            ),
        )
    }

    fn update(&mut self, message: Msg) {
        match message {
            Msg::SetMode(m) => self.mode = m,
            Msg::Increment => self.count += 1,
            Msg::Decrement => self.count -= 1,
            Msg::ToggleNotify(v) => self.notify = v,
            Msg::ToggleSync(v) => self.sync = v,
            Msg::SelectPlan(i) => self.plan = i,
            Msg::NameChanged(v) => self.name = v,
            Msg::Submit => self.submitted = Some(self.name.clone()),
            Msg::VolumeChanged(v) => self.volume = v,
            Msg::Reset => {
                let mode = self.mode;
                *self = Gallery::default();
                self.mode = mode;
            }
        }
    }

    /// ビルド対象のトークンを使い、配色だけアプリ側で上書きできるようにする。
    fn theme(&self, env: &Environment) -> Theme {
        theme::for_target(self.mode.unwrap_or(env.color_mode))
    }

    fn title(&self) -> Option<String> {
        Some(format!("miui gallery — {}", self.count))
    }
}

/// ネイティブ / Web 共通の起動処理。
pub fn start() {
    let mut settings = Settings::new("miui gallery").size(900.0, 720.0);

    #[cfg(target_arch = "wasm32")]
    if !WEB_FONT.is_empty() {
        settings = settings.font(FontSpec::new(WEB_FONT.to_vec()));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        settings.load_system_fonts = true;
    }

    miui::run(Gallery::default(), settings);
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_start() {
    start();
}
