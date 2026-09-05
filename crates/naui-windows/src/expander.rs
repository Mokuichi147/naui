//! 折りたたみ (WinUI 3 のネイティブ `Expander`)。
//!
//! コントロールは [`naui_winui3`] の投影から直に作る。開閉・見出し・
//! 中身のレイアウトは WinUI の標準テンプレートが行う。
//!
//! 見出しと中身は横いっぱいに広げたいので、`HorizontalAlignment` と
//! `HorizontalContentAlignment` だけ `Stretch` にしておく。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::Result;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Control, Expander as XamlExpander, ExpanderCollapsedEventArgs, ExpanderExpandingEventArgs,
    TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::{HorizontalAlignment, UIElement};
use windows::Foundation::TypedEventHandler;
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// 開閉が変わったことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するので [`UiThreadCell`] に
/// 載せる。呼び出しの間だけクロージャを取り出すため、通知の中から同じ
/// 折りたたみを操作しても二重借用にならない。
#[derive(Clone)]
struct ToggleHandler(HandlerCell<dyn FnMut(bool)>);

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
    native: XamlExpander,
    label: TextBlock,
    /// 中身のハンドルを保持し、コールバックごと生かしておく。
    child: RefCell<Option<Box<dyn Widget>>>,
    handler: ToggleHandler,
    /// `set_expanded` による変更では `on_toggle` を呼ばない。
    silent: Cell<bool>,
}

/// 見出しを押して中身を出し入れするコンテナ。
#[derive(Clone)]
pub struct Expander(Rc<ExpanderInner>);
impl_widget!(Expander, native);

impl Expander {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = XamlExpander::new().map_err(|e| to_error("Expander の生成", e))?;
        native
            .SetHorizontalAlignment(HorizontalAlignment::Stretch)
            .and_then(|()| native.SetHorizontalContentAlignment(HorizontalAlignment::Stretch))
            .map_err(|e| to_error("Expander の配置設定", e))?;
        let label = TextBlock::new().map_err(|e| to_error("見出しの生成", e))?;
        label
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("見出しの設定", e))?;
        native
            .SetHeader(&label)
            .map_err(|e| to_error("Expander の見出し設定", e))?;

        let this = Self(Rc::new(ExpanderInner {
            native,
            label,
            child: RefCell::new(None),
            handler: ToggleHandler::new(),
            silent: Cell::new(false),
        }));
        this.connect()?;
        Ok(this)
    }

    /// WinUI の `Expanding` / `Collapsed` を Rust のクロージャへつなぐ。
    fn connect(&self) -> Result<()> {
        let expanding_target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let expanding =
            TypedEventHandler::<XamlExpander, ExpanderExpandingEventArgs>::new(move |_, _| {
                let _ = expanding_target.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        if !inner.silent.get() {
                            inner.handler.emit(true);
                        }
                    }
                });
                Ok(())
            });
        self.0
            .native
            .Expanding(&expanding)
            .map_err(|e| to_error("Expander の展開購読", e))?;

        let collapsed_target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let collapsed =
            TypedEventHandler::<XamlExpander, ExpanderCollapsedEventArgs>::new(move |_, _| {
                let _ = collapsed_target.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        if !inner.silent.get() {
                            inner.handler.emit(false);
                        }
                    }
                });
                Ok(())
            });
        self.0
            .native
            .Collapsed(&collapsed)
            .map_err(|e| to_error("Expander の折りたたみ購読", e))?;
        Ok(())
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
        self.0.native.IsExpanded().unwrap_or(false)
    }

    /// プログラムから開閉する。`on_toggle` は呼ばれない。
    pub fn set_expanded(&self, expanded: bool) {
        self.0.silent.set(true);
        let _ = self.0.native.SetIsExpanded(expanded);
        self.0.silent.set(false);
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(control) = self.0.native.cast::<Control>() {
            let _ = control.SetIsEnabled(enabled);
        }
    }

    /// 折りたたむ中身。呼ぶたびに置き換わる。
    pub fn set_child(&self, child: &dyn Widget) {
        if self.0.native.SetContent(None).is_err() {
            return;
        }
        let element = child.native_element();
        if self.0.native.SetContent(&element).is_ok() {
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// 利用者が開閉するたびに、変わった後の状態で呼ばれる。
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.handler.set(f);
    }
}
