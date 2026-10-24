//! A ComboBox that shows its options collapsed (WinUI 3).

use std::cell::Cell;
use std::rc::Rc;

use naui_core::Result;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    ComboBox as XamlComboBox, ComboBoxItem, SelectionChangedEventHandler, TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::UIElement;
use windows_core::{IInspectable, Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, SelectHandler, Widget};

struct ComboBoxInner {
    native: XamlComboBox,
    handler: SelectHandler,
    /// Stops WinUI's notifications only while the selection or items are being changed
    /// from code.
    silent: Cell<bool>,
}

/// A combo box that shows its options collapsed (`ComboBox`).
///
/// Nothing is selected right after creation and after `set_items`.
#[derive(Clone)]
pub struct ComboBox(Rc<ComboBoxInner>);
impl_widget!(ComboBox, native);

impl ComboBox {
    pub(crate) fn new() -> Result<Self> {
        let native = XamlComboBox::new().map_err(|e| to_error("ComboBox の生成", e))?;
        native
            .SetIsEditable(false)
            .map_err(|e| to_error("ComboBox の編集可否の設定", e))?;
        native
            .SetSelectedIndex(-1)
            .map_err(|e| to_error("ComboBox の選択解除", e))?;

        let this = Self(Rc::new(ComboBoxInner {
            native,
            handler: SelectHandler::new(),
            silent: Cell::new(false),
        }));

        // WinRT のデリゲートは Send + Sync を要求する一方、XAML のイベントは
        // UI スレッドで届く。弱参照を UiThreadCell に載せ、購読との循環も避ける。
        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let handler = SelectionChangedEventHandler::new(move |_sender, _args| {
            // borrow 中にユーザーコールバックを呼ばないことで再入を許す。
            let Some(inner) = state.try_with_mut(|weak| weak.upgrade()).flatten() else {
                return Ok(());
            };
            if !inner.silent.get() {
                let combo_box = ComboBox(inner);
                if let Some(index) = combo_box.selected() {
                    combo_box.0.handler.emit(index);
                }
            }
            Ok(())
        });
        this.0
            .native
            .SelectionChanged(&handler)
            .map_err(|e| to_error("ComboBox の選択購読", e))?;

        Ok(this)
    }

    /// Rebuilds the items. Indices change meaning, so the selection is cleared.
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        let _ = self.rebuild(items);
    }

    fn rebuild<S: AsRef<str>>(&self, items: &[S]) -> Result<()> {
        self.without_notifying(|this| {
            let children = this
                .0
                .native
                .Items()
                .map_err(|e| to_error("ComboBox の項目取得", e))?;
            children
                .Clear()
                .map_err(|e| to_error("ComboBox の項目消去", e))?;

            for text in items {
                let item = ComboBoxItem::new().map_err(|e| to_error("ComboBoxItem の生成", e))?;
                let label = TextBlock::new().map_err(|e| to_error("項目ラベルの生成", e))?;
                label
                    .SetText(&HSTRING::from(text.as_ref()))
                    .map_err(|e| to_error("項目ラベルの設定", e))?;
                item.SetContent(&label)
                    .map_err(|e| to_error("ComboBoxItem への内容設定", e))?;
                let element = item
                    .cast::<IInspectable>()
                    .map_err(|e| to_error("ComboBoxItem の要素化", e))?;
                children
                    .Append(&element)
                    .map_err(|e| to_error("ComboBoxItem の追加", e))?;
            }

            // WinUI が先頭項目を暗黙に選ぶことに依存せず、常に未選択へ戻す。
            this.0
                .native
                .SetSelectedIndex(-1)
                .map_err(|e| to_error("ComboBox の選択解除", e))
        })
    }

    /// The number of items.
    pub fn len(&self) -> usize {
        self.0
            .native
            .Items()
            .and_then(|items| items.Size())
            .unwrap_or(0) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The currently selected item.
    pub fn selected(&self) -> Option<usize> {
        let index = usize::try_from(self.0.native.SelectedIndex().ok()?).ok()?;
        (index < self.len()).then_some(index)
    }

    /// Selects an item without notifying. Does nothing if out of range.
    pub fn set_selected(&self, index: usize) {
        let _ = self.write_selected(index);
    }

    /// Clears the selection without notifying.
    pub fn clear_selection(&self) {
        let _ = self.without_notifying(|this| this.0.native.SetSelectedIndex(-1));
    }

    /// Selects an item through the same path as a user's choice (notifies).
    pub fn select(&self, index: usize) {
        if self.write_selected(index) {
            // 同じ項目を選び直すと SelectionChanged は起きないため、通知は
            // WinUI に任せず、抑止した書き換えの後で必ず 1 回だけ送る。
            self.0.handler.emit(index);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// Called with the index when an item is chosen. Setting it again replaces the
    /// previous callback.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    fn write_selected(&self, index: usize) -> bool {
        if index >= self.len() {
            return false;
        }
        let Ok(index) = i32::try_from(index) else {
            return false;
        };
        self.without_notifying(|this| this.0.native.SetSelectedIndex(index))
            .is_ok()
    }

    /// Operates with WinUI's `SelectionChanged` stopped.
    fn without_notifying<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        let previous = self.0.silent.replace(true);
        let result = f(self);
        self.0.silent.set(previous);
        result
    }
}
