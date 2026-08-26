//! 自由に入力できるコンボボックス (WinUI 3 の編集できる `ComboBox`)。
//!
//! [`ComboBox`](crate::ComboBox) との違いは `IsEditable` だけで、コントロール
//! そのものは同じ WinUI 3 の `ComboBox`。候補は文字列のまま入れる
//! (`ComboBoxItem` に包むと、編集欄へ出る文字が型名になってしまう)。
//!
//! 1 文字ごとの通知は、テンプレートの中にある入力欄 (`EditableText`) の
//! `TextChanged` から拾う。`ComboBox` 自身は文字の変化を表に出さないため。
//! テンプレートが差し替えられていて入力欄が見つからないときは、候補の選択と
//! Enter での確定だけが通知される。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::Result;
use windows::Foundation::{PropertyValue, TypedEventHandler};
use windows_core::{Interface, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::{
    ComboBox as XamlComboBox, ComboBoxTextSubmittedEventArgs, SelectionChangedEventHandler,
    TextBox, TextChangedEventHandler,
};
use winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

/// WinUI 3 の `ComboBox` テンプレートが持つ入力欄の名前。
const EDITABLE_TEXT_PART: &str = "EditableText";

/// 文字列を受け取る通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するので [`UiThreadCell`] に載せる。
/// 呼び出しの間だけ取り出すため、通知の中から同じコンボボックスを操作しても
/// 二重借用にならない。
#[derive(Clone)]
struct TextHandler(Arc<UiThreadCell<Option<Box<dyn FnMut(&str)>>>>);

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
    /// 候補の控え。`selected` の一致判定と `set_selected` の書き込みに使う。
    items: RefCell<Vec<String>>,
    handler: TextHandler,
    /// 最後に通知した文字列。WinUI は同じ変更を複数の経路で伝えてくるので、
    /// 同じ値の二重通知をここで落とす。
    last: RefCell<String>,
    /// プログラムから書き換えている間だけ WinUI の通知を止める。
    silent: Cell<bool>,
    /// テンプレートの入力欄。見つかるまでは `None`。
    editable: RefCell<Option<TextBox>>,
}

/// 候補から選ぶことも、自由に打ち込むこともできる入力欄
/// (`IsEditable` な `ComboBox`)。
///
/// 値は文字列で、作った直後は空。
#[derive(Clone)]
pub struct EditableComboBox(Rc<EditableComboBoxInner>);
impl_widget!(EditableComboBox, native);

impl EditableComboBox {
    pub(crate) fn new() -> Result<Self> {
        let native = XamlComboBox::new().map_err(|e| to_error("ComboBox の生成", e))?;
        native
            .SetIsEditable(true)
            .map_err(|e| to_error("ComboBox の編集可否の設定", e))?;
        native
            .SetSelectedIndex(-1)
            .map_err(|e| to_error("ComboBox の選択解除", e))?;

        let this = Self(Rc::new(EditableComboBoxInner {
            native,
            items: RefCell::new(Vec::new()),
            handler: TextHandler::new(),
            last: RefCell::new(String::new()),
            silent: Cell::new(false),
            editable: RefCell::new(None),
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
            .map_err(|e| to_error("ComboBox の選択購読", e))?;

        // Enter で確定したとき。**受け取ったことにしないと**、WinUI は候補に
        // 無い文字列を捨てて選択中の候補へ戻してしまう。
        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let submitted = TypedEventHandler::<XamlComboBox, ComboBoxTextSubmittedEventArgs>::new(
            move |_sender, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                args.SetHandled(true)?;
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
            .map_err(|e| to_error("ComboBox の確定購読", e))?;

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
            .map_err(|e| to_error("ComboBox の読み込み購読", e))?;

        Ok(this)
    }

    /// 候補を作り直す。**入力されている文字列は変わらず**、通知も出ない。
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
                .map_err(|e| to_error("ComboBox の項目取得", e))?;
            children
                .Clear()
                .map_err(|e| to_error("ComboBox の項目消去", e))?;
            for item in this.0.items.borrow().iter() {
                let value = PropertyValue::CreateString(&HSTRING::from(item.as_str()))
                    .map_err(|e| to_error("候補の生成", e))?;
                children
                    .Append(&value)
                    .map_err(|e| to_error("候補の追加", e))?;
            }
            this.write_selection(&text);
            Ok(())
        })
    }

    /// 候補の数。
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 入力されている文字列。
    pub fn text(&self) -> String {
        self.0
            .native
            .Text()
            .map(|text| text.to_string())
            .unwrap_or_default()
    }

    /// プログラムから文字列を差し替える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        self.without_notifying(|this| this.write_selection(text));
        *self.0.last.borrow_mut() = text.to_string();
    }

    /// 入力されている文字列と**そのまま一致する**候補の位置。
    ///
    /// 打ち込まれた文字列がどの候補とも一致しなければ `None`。
    pub fn selected(&self) -> Option<usize> {
        let text = self.text();
        self.0.items.borrow().iter().position(|item| *item == text)
    }

    /// 範囲内の候補を通知せずに選ぶ。範囲外なら何もしない。
    pub fn set_selected(&self, index: usize) {
        let Some(text) = self.0.items.borrow().get(index).cloned() else {
            return;
        };
        self.set_text(&text);
    }

    /// 通知せずに文字列を空にする。
    pub fn clear(&self) {
        self.set_text("");
    }

    /// 利用者が候補を選んだのと同じように、範囲内の候補を選んで通知する。
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

    /// 文字列が変わるたびに、その時点の中身で呼ばれる。
    /// 設定し直すと以前のコールバックは置き換わる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.handler.set(f);
    }

    /// テンプレートの入力欄を見つけて、1 文字ごとの変化を購読する。
    ///
    /// 見つからないときは何もしない (候補の選択と Enter は別経路で届く)。
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

    /// 選ばれている候補の文字列。
    fn selected_item_text(&self) -> Option<String> {
        let index = usize::try_from(self.0.native.SelectedIndex().ok()?).ok()?;
        self.0.items.borrow().get(index).cloned()
    }

    /// 重複を落としてから 1 回だけ通知する。
    fn emit(&self, text: String) {
        if self.0.silent.get() || *self.0.last.borrow() == text {
            return;
        }
        *self.0.last.borrow_mut() = text.clone();
        self.0.handler.emit(&text);
    }

    /// 文字列と、それに対応する候補の選択をまとめて書く。
    ///
    /// `SelectedIndex` を書くと WinUI が `Text` を上書きするので、順番は
    /// 「選択 → 文字列」で固定する。
    fn write_selection(&self, text: &str) {
        let index = self
            .0
            .items
            .borrow()
            .iter()
            .position(|item| item == text)
            .and_then(|index| i32::try_from(index).ok())
            .unwrap_or(-1);
        let _ = self.0.native.SetSelectedIndex(index);
        self.write_text(text);
    }

    /// 編集欄の文字列だけを書く。
    fn write_text(&self, text: &str) {
        let _ = self.0.native.SetText(&HSTRING::from(text));
    }

    /// WinUI の通知を止めたまま操作する。
    fn without_notifying<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        let previous = self.0.silent.replace(true);
        let result = f(self);
        self.0.silent.set(previous);
        result
    }
}
