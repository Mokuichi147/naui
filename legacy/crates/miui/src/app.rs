//! アプリケーション側が実装するインタフェース。

use miui_core::painter::{FontFamily, FontWeight};
use miui_core::theme::{ColorMode, PlatformStyle, Theme};
use miui_core::widget::Element;

/// ランタイムが検出した実行環境。テーマの自動選択に使う。
#[derive(Debug, Clone, PartialEq)]
pub struct Environment {
    /// OS が示している配色 (ライト / ダーク)。
    pub color_mode: ColorMode,
    /// 実行中のプラットフォームに対応するデザイン言語。
    pub style: PlatformStyle,
    /// 論理ピクセル → 物理ピクセルの倍率。
    pub scale_factor: f32,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            color_mode: ColorMode::Light,
            style: PlatformStyle::detect(),
            scale_factor: 1.0,
        }
    }
}

/// miui のアプリケーション。
///
/// 状態は [`Application::update`] でのみ変化し、[`Application::view`] は
/// その時点の状態から UI ツリーを組み立てる。ツリーは毎フレーム作り直されるが、
/// ホバーやフォーカス、キャレット位置はランタイムが保持する。
pub trait Application: 'static {
    /// UI から送られるメッセージ。
    type Message: Clone + 'static;

    /// 現在の状態から UI を組み立てる。
    fn view(&self) -> Element<Self::Message>;

    /// メッセージを受けて状態を更新する。
    fn update(&mut self, message: Self::Message);

    /// 使用するテーマ。既定では、ビルド対象のプラットフォームのデザイン言語を
    /// 模したトークンを、OS のライト / ダーク設定に合わせて構築する。
    fn theme(&self, env: &Environment) -> Theme {
        miui_theme::for_target(env.color_mode)
    }

    /// 状態に応じてウィンドウタイトルを変えたい場合に返す。
    /// `None` なら [`Settings::title`] を使う。
    fn title(&self) -> Option<String> {
        None
    }
}

/// 起動時の設定。
pub struct Settings {
    pub title: String,
    /// 初期ウィンドウサイズ (論理ピクセル)。
    ///
    /// **ネイティブでのみ有効。** Web ではブラウザのビューポート全体を使い、
    /// リサイズにも追従する (キャンバスを固定サイズに縛らない)。
    pub size: (f64, f64),
    /// 最小ウィンドウサイズ。`size` と同様、ネイティブでのみ有効。
    pub min_size: Option<(f64, f64)>,
    pub resizable: bool,
    /// 追加で登録するフォント。Web ではフォントをファイルから読めないため、
    /// ここへ埋め込みバイト列を渡す必要がある。
    pub fonts: Vec<FontSpec>,
    /// OS 標準フォントの探索を行うか。
    pub load_system_fonts: bool,
}

/// 追加登録するフォント 1 つ分の指定。
pub struct FontSpec {
    pub bytes: Vec<u8>,
    pub family: FontFamily,
    pub weight: FontWeight,
    /// 他に該当が無いときだけ使う (CJK フォールバックなど)。
    pub fallback: bool,
    /// TrueType Collection 内のインデックス。
    pub collection_index: u32,
}

impl FontSpec {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            fallback: false,
            collection_index: 0,
        }
    }

    pub fn family(mut self, family: FontFamily) -> Self {
        self.family = family;
        self
    }
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }
    pub fn fallback(mut self, fallback: bool) -> Self {
        self.fallback = fallback;
        self
    }
    pub fn collection_index(mut self, index: u32) -> Self {
        self.collection_index = index;
        self
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            title: "miui".to_string(),
            size: (960.0, 680.0),
            min_size: Some((360.0, 320.0)),
            resizable: true,
            fonts: Vec::new(),
            load_system_fonts: true,
        }
    }
}

impl Settings {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn size(mut self, width: f64, height: f64) -> Self {
        self.size = (width, height);
        self
    }

    pub fn font(mut self, font: FontSpec) -> Self {
        self.fonts.push(font);
        self
    }
}
