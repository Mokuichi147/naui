//! 折りたたみ (見出しの `ToggleButton` + 中身の `StackPanel`)。
//!
//! WinUI 3 には `Expander` があるが、`winio-winui3` がこの型を投影して
//! いないため、`Tabs` や `Navbar` と同じように**標準コントロールを組んで**
//! 同じ形を作る。見出しは押すたびに入り切りが変わる `ToggleButton` で、
//! 山形 (`ChevronRight` / `ChevronDown`) は Segoe Fluent Icons の字を入れる
//! (`Tree` の開閉ボタンと同じ)。
//!
//! たたむときは中身を `Visibility::Collapsed` にする。`StackPanel` は
//! 隠れた子の場所を空けないので、見出しの高さまで縮む。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::Result;
use windows_core::{Interface, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::Primitives::ToggleButton;
use winui3::Microsoft::UI::Xaml::Controls::{
    Orientation as XamlOrientation, StackPanel, TextBlock,
};
use winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement, Visibility};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

/// 見出しのボタン。押せる場所を行いっぱいに広げ、中身は左詰めにする。
const HEADER_XAML: &str = r#"<ToggleButton
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    HorizontalAlignment="Stretch" HorizontalContentAlignment="Left"
    Padding="12,8,12,8"/>"#;

/// 山形の字 (Segoe Fluent Icons)。`Tree` の開閉ボタンと同じもの。
const GLYPH_XAML: &str = r#"<TextBlock
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    FontFamily="Segoe Fluent Icons" FontSize="10" VerticalAlignment="Center"/>"#;

/// 閉じているときの山形 (ChevronRight)。
const GLYPH_COLLAPSED: &str = "\u{E76C}";
/// 開いているときの山形 (ChevronDown)。
const GLYPH_EXPANDED: &str = "\u{E70D}";

/// 開閉が変わったことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するので [`UiThreadCell`] に
/// 載せる。呼び出しの間だけクロージャを取り出すため、通知の中から同じ
/// 折りたたみを操作しても二重借用にならない。
#[derive(Clone)]
struct ToggleHandler(Arc<UiThreadCell<Option<Box<dyn FnMut(bool)>>>>);

impl ToggleHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(bool) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, expanded: bool) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(expanded);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct ExpanderInner {
    native: StackPanel,
    header: ToggleButton,
    glyph: TextBlock,
    label: TextBlock,
    /// 中身のハンドルを保持し、コールバックごと生かしておく。
    child: RefCell<Option<Box<dyn Widget>>>,
    handler: ToggleHandler,
}

/// 見出しを押して中身を出し入れするコンテナ。
#[derive(Clone)]
pub struct Expander(Rc<ExpanderInner>);
impl_widget!(Expander, native);

impl Expander {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = StackPanel::new().map_err(|e| to_error("StackPanel の生成", e))?;
        native
            .SetOrientation(XamlOrientation::Vertical)
            .map_err(|e| to_error("折りたたみの向き設定", e))?;

        let header = header_button()?;
        let glyph = glyph_block()?;
        let label = TextBlock::new().map_err(|e| to_error("見出しの生成", e))?;
        label
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("見出しの設定", e))?;

        // 山形と見出しの字を横に並べて、ボタンの内容にする。
        let content = StackPanel::new().map_err(|e| to_error("見出しの組み立て", e))?;
        content
            .SetOrientation(XamlOrientation::Horizontal)
            .map_err(|e| to_error("見出しの向き設定", e))?;
        let _ = content.SetSpacing(8.0);
        let children = content
            .Children()
            .map_err(|e| to_error("見出しの子の取得", e))?;
        children
            .Append(&glyph)
            .map_err(|e| to_error("見出しへの追加", e))?;
        children
            .Append(&label)
            .map_err(|e| to_error("見出しへの追加", e))?;
        header
            .SetContent(&content)
            .map_err(|e| to_error("見出しへの内容設定", e))?;
        native
            .Children()
            .and_then(|children| children.Append(&header))
            .map_err(|e| to_error("折りたたみへの追加", e))?;

        let this = Self(Rc::new(ExpanderInner {
            native,
            header,
            glyph,
            label,
            child: RefCell::new(None),
            handler: ToggleHandler::new(),
        }));
        this.write_glyph(false);
        this.connect();
        Ok(this)
    }

    /// 見出しの押し下げを Rust のクロージャへつなぐ。
    ///
    /// `Click` は利用者の操作でしか飛ばない (`SetIsChecked` では呼ばれない)
    /// ので、プログラムからの開閉と混ざらない。ハンドルを強く持つと購読との
    /// 間で循環するため、弱参照にする。
    fn connect(&self) {
        let handler = self.0.handler.clone();
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let delegate = RoutedEventHandler::new(move |_sender, _args| {
            let Some(expanded) = state.try_with_mut(|weak| {
                let inner = weak.upgrade()?;
                let this = Expander(inner);
                let expanded = this.is_expanded();
                this.write_state(expanded);
                Some(expanded)
            }) else {
                return Ok(());
            };
            if let Some(expanded) = expanded {
                handler.emit(expanded);
            }
            Ok(())
        });
        let _ = self.0.header.Click(&delegate);
    }

    /// 見出しの文字。
    pub fn text(&self) -> String {
        self.0
            .label
            .Text()
            .map(|text| text.to_string())
            .unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        let _ = self.0.label.SetText(&HSTRING::from(text));
    }

    /// 開いているかどうか。
    pub fn is_expanded(&self) -> bool {
        self.0
            .header
            .IsChecked()
            .and_then(|value| value.Value())
            .unwrap_or(false)
    }

    /// プログラムから開閉する。`on_toggle` は呼ばれない。
    pub fn set_expanded(&self, expanded: bool) {
        let _ = self
            .0
            .header
            .SetIsChecked(crate::widgets::bool_ref(expanded).ok().as_ref());
        self.write_state(expanded);
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.header.SetIsEnabled(enabled);
    }

    /// 折りたたむ中身。呼ぶたびに置き換わる。
    pub fn set_child(&self, child: &dyn Widget) {
        let Ok(children) = self.0.native.Children() else {
            return;
        };
        if self.0.child.borrow().is_some() {
            // 先頭は見出しなので、中身だけを外す。
            let _ = children.RemoveAt(1);
            *self.0.child.borrow_mut() = None;
        }
        let element = child.native_element();
        if children.Append(&element).is_ok() {
            set_visible(&element, self.is_expanded());
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// 利用者が開閉するたびに、変わった後の状態で呼ばれる。
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.handler.set(f);
    }

    /// 山形と中身の出し入れを、開閉の状態へそろえる。
    fn write_state(&self, expanded: bool) {
        self.write_glyph(expanded);
        if let Some(child) = self.0.child.borrow().as_ref() {
            set_visible(&child.native_element(), expanded);
        }
    }

    fn write_glyph(&self, expanded: bool) {
        let glyph = if expanded {
            GLYPH_EXPANDED
        } else {
            GLYPH_COLLAPSED
        };
        let _ = self.0.glyph.SetText(&HSTRING::from(glyph));
    }
}

fn set_visible(element: &UIElement, visible: bool) {
    let _ = element.SetVisibility(if visible {
        Visibility::Visible
    } else {
        Visibility::Collapsed
    });
}

/// 見出しのボタンを作る。XAML を読めない環境では素のボタンに落とす。
fn header_button() -> Result<ToggleButton> {
    match XamlReader::Load(&HSTRING::from(HEADER_XAML))
        .and_then(|element| element.cast::<ToggleButton>())
    {
        Ok(button) => Ok(button),
        Err(error) => {
            eprintln!("naui-windows: 折りたたみの見出しの生成に失敗: {error}");
            ToggleButton::new().map_err(|e| to_error("見出しの生成", e))
        }
    }
}

/// 山形を出す `TextBlock` を作る。字体を指定できなければ素のものに落とす。
fn glyph_block() -> Result<TextBlock> {
    match XamlReader::Load(&HSTRING::from(GLYPH_XAML))
        .and_then(|element| element.cast::<TextBlock>())
    {
        Ok(block) => Ok(block),
        Err(error) => {
            eprintln!("naui-windows: 折りたたみの山形の生成に失敗: {error}");
            TextBlock::new().map_err(|e| to_error("山形の生成", e))
        }
    }
}
