//! # miui-gtk (骨組み・未実装)
//!
//! miui の Linux バックエンド。**まだ実装されていない。**
//!
//! 他のバックエンドと同じ API の形だけを定義してあり、
//! 呼ぶと必ず「未実装」のエラーを返す。ビルドは通るが動作はしない。
//!
//! ## 未実装である理由
//!
//! GTK4 / libadwaita のバインディング (`gtk4` / `libadwaita` クレート) は
//! ビルドに GTK4 の開発用システムライブラリと pkg-config を要求するため、
//! この実装を書いた環境 (macOS) では **コンパイル確認すらできない**。
//! 動作未確認のコードを「実装済み」として置くより、
//! 空であることを明示するほうが誠実だと判断した。
//!
//! ## 実装するときの対応表
//!
//! | miui | GTK4 / libadwaita |
//! | --- | --- |
//! | `run` | `gtk::Application` + `connect_activate` (コールバック内で UI 構築) |
//! | `Window` | `adw::ApplicationWindow` |
//! | `Stack` | `gtk::Box` (`Orientation::Vertical` / `Horizontal`) |
//! | `Label` | `gtk::Label` |
//! | `Button` | `gtk::Button` + `connect_clicked` |
//! | `Checkbox` | `gtk::CheckButton` + `connect_toggled` |
//! | `TextInput` | `gtk::Entry` + `connect_changed` |
//! | `Slider` | `gtk::Scale` + `connect_value_changed` |
//! | `ProgressBar` | `gtk::ProgressBar` |
//!
//! GTK のシグナルハンドラは `'static` なクロージャを受けるので、
//! macOS/Web と同じ `Rc<Inner>` + クロージャ保持の形がそのまま使える
//! (Windows のような `Send + Sync` 制約は無い)。

#![cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
#![forbid(unsafe_code)]

use std::cell::RefCell;

use miui_core::{Align, Error, Orientation, Padding, Result, Settings};

fn unimplemented_error(what: &'static str) -> Error {
    Error::new(
        what,
        "Linux (GTK4) バックエンドは未実装です。crates/miui-gtk のドキュメントを参照してください",
    )
}

/// GTK4 の実ウィジェットに対応する予定のハンドル。現状は中身を持たない。
macro_rules! placeholder_widget {
    ($name:ident) => {
        #[derive(Clone)]
        pub struct $name(std::rc::Rc<()>);

        impl Widget for $name {
            fn boxed_clone(&self) -> Box<dyn Widget> {
                Box::new(self.clone())
            }
        }
    };
}

/// miui のウィジェットが実装する共通インタフェース。
pub trait Widget: 'static {
    #[doc(hidden)]
    fn boxed_clone(&self) -> Box<dyn Widget>;
}

placeholder_widget!(Label);
placeholder_widget!(Button);
placeholder_widget!(Checkbox);
placeholder_widget!(TextInput);
placeholder_widget!(Slider);
placeholder_widget!(ProgressBar);
placeholder_widget!(Stack);

impl Label {
    pub fn text(&self) -> String {
        String::new()
    }
    pub fn set_text(&self, _text: &str) {}
}

impl Button {
    pub fn set_text(&self, _text: &str) {}
    pub fn set_enabled(&self, _enabled: bool) {}
    pub fn on_click(&self, _f: impl FnMut() + 'static) {}
}

impl Checkbox {
    pub fn is_checked(&self) -> bool {
        false
    }
    pub fn set_checked(&self, _checked: bool) {}
    pub fn set_enabled(&self, _enabled: bool) {}
    pub fn on_toggle(&self, _f: impl FnMut(bool) + 'static) {}
}

impl TextInput {
    pub fn text(&self) -> String {
        String::new()
    }
    pub fn set_text(&self, _text: &str) {}
    pub fn set_placeholder(&self, _text: &str) {}
    pub fn set_enabled(&self, _enabled: bool) {}
    pub fn on_change(&self, _f: impl FnMut(&str) + 'static) {}
}

impl Slider {
    pub fn value(&self) -> f64 {
        0.0
    }
    pub fn set_value(&self, _value: f64) {}
    pub fn set_enabled(&self, _enabled: bool) {}
    pub fn on_change(&self, _f: impl FnMut(f64) + 'static) {}
}

impl ProgressBar {
    pub fn value(&self) -> f64 {
        0.0
    }
    pub fn set_value(&self, _value: f64) {}
}

impl Stack {
    pub fn set_spacing(&self, _spacing: f64) {}
    pub fn set_padding(&self, _padding: Padding) {}
    pub fn set_align(&self, _align: Align) {}
    pub fn append(&self, _child: &dyn Widget) {}
    pub fn len(&self) -> usize {
        0
    }
    pub fn is_empty(&self) -> bool {
        true
    }
}

/// トップレベルウィンドウ (未実装)。
#[derive(Clone)]
pub struct Window(std::rc::Rc<()>);

impl Window {
    pub fn set_title(&self, _title: &str) {}
    pub fn title(&self) -> String {
        String::new()
    }
    pub fn set_size(&self, _width: f64, _height: f64) {}
    pub fn set_child(&self, _child: &dyn Widget) {}
    pub fn show(&self) {}
    pub fn close(&self) {}
    pub fn is_visible(&self) -> bool {
        false
    }
}

/// ウィジェットを生成するための入り口 (未実装)。
pub struct Ui {
    _private: RefCell<()>,
}

impl Ui {
    pub fn window(&self, _title: &str, _width: f64, _height: f64) -> Result<Window> {
        Err(unimplemented_error("ウィンドウの生成"))
    }
    pub fn stack(&self, _orientation: Orientation) -> Result<Stack> {
        Err(unimplemented_error("Stack の生成"))
    }
    pub fn label(&self, _text: &str) -> Result<Label> {
        Err(unimplemented_error("Label の生成"))
    }
    pub fn button(&self, _text: &str) -> Result<Button> {
        Err(unimplemented_error("Button の生成"))
    }
    pub fn checkbox(&self, _label: &str) -> Result<Checkbox> {
        Err(unimplemented_error("Checkbox の生成"))
    }
    pub fn text_input(&self, _text: &str) -> Result<TextInput> {
        Err(unimplemented_error("TextInput の生成"))
    }
    pub fn slider(&self, _min: f64, _max: f64) -> Result<Slider> {
        Err(unimplemented_error("Slider の生成"))
    }
    pub fn progress_bar(&self) -> Result<ProgressBar> {
        Err(unimplemented_error("ProgressBar の生成"))
    }
    pub fn quit(&self) {}
}

/// 未実装。呼ぶと必ずエラーを返す。
pub fn run<F>(_settings: Settings, _build: F) -> Result<()>
where
    F: FnOnce(&Ui) -> Result<()> + 'static,
{
    Err(unimplemented_error("Linux でのアプリ起動"))
}
