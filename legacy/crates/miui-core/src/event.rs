//! プラットフォーム非依存の入力イベント。
//!
//! 各バックエンド (winit / Web) はネイティブイベントをここへ正規化する。

use crate::geometry::Point;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// 修飾キーの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Windows キー / Command キー。
    pub meta: bool,
}

impl Modifiers {
    /// 各 OS における「コマンド修飾」 (macOS は Cmd、それ以外は Ctrl)。
    pub fn command(&self) -> bool {
        if cfg!(target_os = "macos") {
            self.meta
        } else {
            self.ctrl
        }
    }
}

/// テキスト編集やフォーカス移動に使う論理キー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Enter,
    Escape,
    Backspace,
    Delete,
    Tab,
    Space,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    /// 上記以外 (文字入力は `Event::Text` として届く)。
    Other,
}

/// UI ツリーへ配送される入力イベント。
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    PointerMoved(Point),
    PointerPressed {
        position: Point,
        button: MouseButton,
    },
    PointerReleased {
        position: Point,
        button: MouseButton,
    },
    /// ウィンドウ外へポインタが出た。
    PointerLeft,
    /// ホイール / トラックパッドのスクロール量 (論理ピクセル)。
    Scrolled {
        position: Point,
        delta_x: f32,
        delta_y: f32,
    },
    KeyPressed {
        key: Key,
        modifiers: Modifiers,
    },
    /// 確定した文字入力 (IME 変換確定後を含む)。
    Text(String),
    /// IME の未確定文字列。`cursor` は未確定文字列内のバイト範囲。
    ImePreedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
}

impl Event {
    /// ポインタ位置を持つイベントならその座標。
    pub fn position(&self) -> Option<Point> {
        match self {
            Event::PointerMoved(p) => Some(*p),
            Event::PointerPressed { position, .. } => Some(*position),
            Event::PointerReleased { position, .. } => Some(*position),
            Event::Scrolled { position, .. } => Some(*position),
            _ => None,
        }
    }
}
