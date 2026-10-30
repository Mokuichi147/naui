//! A combo box that accepts free input (WinUI 3's editable `ComboBox`).
//!
//! The only difference from [`ComboBox`](crate::ComboBox) is `IsEditable`; the
//! control itself is the same WinUI 3 `ComboBox`. Suggestions are inserted as plain
//! strings (wrapping them in `ComboBoxItem` makes the edit field show the type name).
//!
//! Per-keystroke notifications are picked up from the `TextChanged` of the input
//! field inside the template (`EditableText`), since `ComboBox` itself does not
//! expose text changes. If the template has been replaced and the input field
//! cannot be found, only choosing a suggestion and confirming with Enter are
//! notified.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::Result;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    ComboBox as XamlComboBox, ComboBoxTextSubmittedEventArgs, SelectionChangedEventHandler,
    TextBox, TextChangedEventHandler,
};
use naui_winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement};
use windows::Foundation::{PropertyValue, TypedEventHandler};
use windows_core::{IInspectable, Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// The name of the input field in WinUI 3's `ComboBox` template.
const EDITABLE_TEXT_PART: &str = "EditableText";

/// A handler that receives a string.
///
/// WinRT delegates require `Send + Sync`, so it is placed in a [`UiThreadCell`]. It
/// is taken out only for the duration of the call, so operating on the same combo
/// box from inside the notification does not double-borrow.
#[derive(Clone)]
struct TextHandler(HandlerCell<dyn FnMut(&str)>);

impl TextHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(&str) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, text: &str) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(text);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct EditableComboBoxInner {
    native: XamlComboBox,
    /// A copy of the suggestions. Used to check matches for `selected` and to write for
    /// `set_selected`.
    items: RefCell<Vec<String>>,
    handler: TextHandler,
    /// The string last notified. WinUI reports the same change through several paths,
    /// so duplicate notifications of the same value are dropped here.
    last: RefCell<String>,
    /// Stops WinUI's notifications only while rewriting from code.
    silent: Cell<bool>,
    /// The template's input field. `None` until found.
    editable: RefCell<Option<TextBox>>,
    /// The `on_focus_change` handler.
    focus: crate::focus::Focus,
}

/// A field where the user can either choose from suggestions or type freely (an
/// `IsEditable` `ComboBox`).
///
/// The value is a string, empty right after creation.
#[derive(Clone)]
pub struct EditableComboBox(Rc<EditableComboBoxInner>);
crate::keys::impl_key_down!(EditableComboBox);
crate::focus::impl_focus_change!(EditableComboBox);
impl_widget!(EditableComboBox, native);

impl EditableComboBox {
    pub(crate) fn new() -> Result<Self> {
        let native = XamlComboBox::new().map_err(|e| to_error("create the ComboBox", e))?;
        native
            .SetIsEditable(true)
            .map_err(|e| to_error("set whether the ComboBox is editable", e))?;
        native
            .SetSelectedIndex(-1)
            .map_err(|e| to_error("clear the ComboBox selection", e))?;

        let this = Self(Rc::new(EditableComboBoxInner {
            native,
            items: RefCell::new(Vec::new()),
            handler: TextHandler::new(),
            last: RefCell::new(String::new()),
            silent: Cell::new(false),
            editable: RefCell::new(None),
            focus: Default::default(),
        }));

        // 候補が選ばれたとき。編集欄への反映は WinUI より先に自分で行い、
        // 通知の最中でも `text()` が選ばれた文字列を返すようにする。
        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let selected = SelectionChangedEventHandler::new(move |_sender, _args| {
            let Some(inner) = state.try_with_mut(|weak| weak.upgrade()).flatten() else {
                return Ok(());
            };
            let combo = EditableComboBox(inner);
            let Some(text) = combo.selected_item_text() else {
                return Ok(());
            };
            combo.write_text(&text);
            combo.emit(text);
            Ok(())
        });
        this.0
            .native
            .SelectionChanged(&selected)
            .map_err(|e| to_error("subscribe to ComboBox selection", e))?;

        // Enter (またはフォーカスが外れたこと) で、候補に無い文字列が確定した
        // とき。**`Handled` は立てない**。立てると WinUI は既定の処理 (その
        // 文字列を `SelectedItem` に入れる) を飛ばし、編集欄を選択中の項目で
        // 書き直す。打ち始めた時点で選択は外れているので、打った文字が消えて
        // 空になる。既定の処理に任せれば `SelectedIndex` は -1 のまま文字が
        // 残り、続く `SelectionChanged` は候補を指さないので上で無視される。
        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let submitted = TypedEventHandler::<XamlComboBox, ComboBoxTextSubmittedEventArgs>::new(
            move |_sender, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let text = args.Text()?.to_string();
                let Some(inner) = state.try_with_mut(|weak| weak.upgrade()).flatten() else {
                    return Ok(());
                };
                EditableComboBox(inner).emit(text);
                Ok(())
            },
        );
        this.0
            .native
            .TextSubmitted(&submitted)
            .map_err(|e| to_error("subscribe to ComboBox text submission", e))?;

        // テンプレートが展開されてから入力欄を探す。
        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let loaded = RoutedEventHandler::new(move |_sender, _args| {
            if let Some(inner) = state.try_with_mut(|weak| weak.upgrade()).flatten() {
                EditableComboBox(inner).watch_editable_text();
            }
            Ok(())
        });
        this.0
            .native
            .Loaded(&loaded)
            .map_err(|e| to_error("subscribe to ComboBox Loaded", e))?;

        Ok(this)
    }

    /// Rebuilds the suggestions. **The entered text does not change**, and no
    /// notification is sent.
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        *self.0.items.borrow_mut() = items.iter().map(|s| s.as_ref().to_string()).collect();
        let _ = self.rebuild();
    }

    fn rebuild(&self) -> Result<()> {
        // 候補を入れ替えると WinUI は選択も文字列も落とすので、書き戻す。
        let text = self.text();
        self.without_notifying(|this| {
            let children = this
                .0
                .native
                .Items()
                .map_err(|e| to_error("get the ComboBox items", e))?;
            children
                .Clear()
                .map_err(|e| to_error("clear the ComboBox items", e))?;
            for item in this.0.items.borrow().iter() {
                let value = PropertyValue::CreateString(&HSTRING::from(item.as_str()))
                    .map_err(|e| to_error("create the suggestion", e))?;
                children
                    .Append(&value)
                    .map_err(|e| to_error("add the suggestion", e))?;
            }
            this.write_selection(&text);
            Ok(())
        })
    }

    /// The number of suggestions.
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entered text.
    pub fn text(&self) -> String {
        self.0
            .native
            .Text()
            .map(|text| text.to_string())
            .unwrap_or_default()
    }

    /// Replaces the text from code. `on_change` is not called.
    pub fn set_text(&self, text: &str) {
        self.without_notifying(|this| this.write_selection(text));
        *self.0.last.borrow_mut() = text.to_string();
    }

    /// The position of the suggestion that **exactly matches** the entered text.
    ///
    /// `None` if the typed text matches no suggestion.
    pub fn selected(&self) -> Option<usize> {
        let text = self.text();
        self.0.items.borrow().iter().position(|item| *item == text)
    }

    /// Chooses an in-range suggestion without notifying. Does nothing if out of range.
    pub fn set_selected(&self, index: usize) {
        let Some(text) = self.0.items.borrow().get(index).cloned() else {
            return;
        };
        self.set_text(&text);
    }

    /// Empties the text without notifying.
    pub fn clear(&self) {
        self.set_text("");
    }

    /// Chooses an in-range suggestion and notifies, as if the user had chosen it.
    pub fn select(&self, index: usize) {
        let Some(text) = self.0.items.borrow().get(index).cloned() else {
            return;
        };
        self.set_text(&text);
        self.0.handler.emit(&text);
    }

    pub fn set_placeholder(&self, text: &str) {
        let _ = self.0.native.SetPlaceholderText(&HSTRING::from(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// Called with the current contents every time the text changes. Setting it again
    /// replaces the previous callback.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.handler.set(f);
    }

    /// Finds the template's input field and subscribes to per-keystroke changes.
    ///
    /// Does nothing if it is not found (choosing suggestions and Enter arrive through
    /// another path).
    fn watch_editable_text(&self) {
        if self.0.editable.borrow().is_some() {
            return;
        }
        let Ok(part) = self
            .0
            .native
            .GetTemplateChild(&HSTRING::from(EDITABLE_TEXT_PART))
        else {
            return;
        };
        let Ok(text_box) = part.cast::<TextBox>() else {
            return;
        };

        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let changed = TextChangedEventHandler::new(move |_sender, _args| {
            let Some(inner) = state.try_with_mut(|weak| weak.upgrade()).flatten() else {
                return Ok(());
            };
            // `ComboBox.Text` はこの後で追いつくので、入力欄から直に読む。
            let text = inner
                .editable
                .borrow()
                .as_ref()
                .and_then(|text_box| text_box.Text().ok())
                .map(|text| text.to_string());
            if let Some(text) = text {
                EditableComboBox(inner).emit(text);
            }
            Ok(())
        });
        if text_box.TextChanged(&changed).is_ok() {
            *self.0.editable.borrow_mut() = Some(text_box);
        }
    }

    /// The text of the selected suggestion.
    fn selected_item_text(&self) -> Option<String> {
        let index = usize::try_from(self.0.native.SelectedIndex().ok()?).ok()?;
        self.0.items.borrow().get(index).cloned()
    }

    /// Drops duplicates and notifies once.
    fn emit(&self, text: String) {
        if self.0.silent.get() || *self.0.last.borrow() == text {
            return;
        }
        *self.0.last.borrow_mut() = text.clone();
        self.0.handler.emit(&text);
    }

    /// Writes the text and the selection of the matching suggestion together.
    ///
    /// Writing `SelectedIndex` makes WinUI overwrite `Text`, so the order is fixed as
    /// "selection → text".
    fn write_selection(&self, text: &str) {
        let index = self
            .0
            .items
            .borrow()
            .iter()
            .position(|item| item == text)
            .and_then(|index| i32::try_from(index).ok());
        match index {
            Some(index) => {
                let _ = self.0.native.SetSelectedIndex(index);
            }
            None => self.write_custom_value(text),
        }
        self.write_text(text);
    }

    /// Gives WinUI a string not among the suggestions, in the same form as when the user
    /// confirms it.
    ///
    /// When WinUI cancels editing with Esc, it reverts to the last non-suggestion string
    /// put into `SelectedItem`. Just writing -1 to `SelectedIndex` (or just emptying
    /// `SelectedItem`) does not change this revert target, so even after `set_text` or
    /// `clear`, a string the user confirmed earlier would come back. So the string is
    /// put in as is, just as WinUI does on confirmation.
    fn write_custom_value(&self, text: &str) {
        if let Ok(value) = PropertyValue::CreateString(&HSTRING::from(text)) {
            let _ = self.0.native.SetSelectedItem(&value);
        }
        // 空のときは戻り先だけ "" に変えて、選択は「なし」へ戻す。空にしても
        // 戻り先は "" のまま残る。
        if text.is_empty() {
            let _ = self.0.native.SetSelectedItem(None::<&IInspectable>);
        }
    }

    /// Writes only the edit field's text.
    fn write_text(&self, text: &str) {
        let _ = self.0.native.SetText(&HSTRING::from(text));
    }

    /// Operates with WinUI's notifications stopped.
    fn without_notifying<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        let previous = self.0.silent.replace(true);
        let result = f(self);
        self.0.silent.set(previous);
        result
    }
}
