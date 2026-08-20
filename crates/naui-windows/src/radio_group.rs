//! 選択肢を並べて 1 つだけ選ばせるラジオグループ (WinUI 3)。
//!
//! `RadioButton` を `StackPanel` へ並べ、同じ `GroupName` を持たせる。
//! 排他は WinUI が行い、`GroupName` はグループごとに作った一意の文字列なので、
//! 同じ画面に複数のラジオグループを置いても混ざらない。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Orientation, Result};
use windows_core::{Interface, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::{
    Orientation as XamlOrientation, RadioButton, StackPanel, TextBlock,
};
use winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{bool_ref, impl_widget, SelectHandler, Widget};

thread_local! {
    /// グループごとに一意な `GroupName` を作るための連番。
    static NEXT_GROUP: Cell<u64> = const { Cell::new(0) };
}

fn next_group_name() -> HSTRING {
    NEXT_GROUP.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1));
        HSTRING::from(format!("naui-radio-{id}"))
    })
}

struct RadioGroupInner {
    native: StackPanel,
    /// 同じ `GroupName` を共有するラジオ。WinUI はこれを見て排他にする。
    group_name: HSTRING,
    buttons: RefCell<Vec<RadioButton>>,
    handler: SelectHandler,
    /// プログラムから選択や項目を変えている間だけ WinUI の通知を止める。
    silent: Cell<bool>,
    /// [`set_enabled`](RadioGroup::set_enabled) の指定。`StackPanel` は
    /// `IsEnabled` を持たない (`Control` の持ち物) ので、ボタンごとに書き、
    /// `set_items` で作り直すぶんにも引き継げるよう覚えておく。
    enabled: Cell<bool>,
}

/// 選択肢を並べて 1 つだけ選ばせるラジオグループ (`RadioButton`)。
///
/// 生成直後と [`set_items`](Self::set_items) の後は何も選ばれていない。
#[derive(Clone)]
pub struct RadioGroup(Rc<RadioGroupInner>);
impl_widget!(RadioGroup, native);

impl RadioGroup {
    pub(crate) fn new() -> Result<Self> {
        let native = StackPanel::new().map_err(|e| to_error("StackPanel の生成", e))?;
        native
            .SetOrientation(XamlOrientation::Vertical)
            .map_err(|e| to_error("ラジオグループの向きの設定", e))?;
        Ok(Self(Rc::new(RadioGroupInner {
            native,
            group_name: next_group_name(),
            buttons: RefCell::new(Vec::new()),
            handler: SelectHandler::new(),
            silent: Cell::new(false),
            enabled: Cell::new(true),
        })))
    }

    /// 選択肢を作り直し、選択を外す。選択通知は発生しない。
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        let _ = self.rebuild(items);
    }

    fn rebuild<S: AsRef<str>>(&self, items: &[S]) -> Result<()> {
        self.without_notifying(|this| {
            let children = this
                .0
                .native
                .Children()
                .map_err(|e| to_error("ラジオグループの項目取得", e))?;
            children
                .Clear()
                .map_err(|e| to_error("ラジオグループの項目消去", e))?;
            this.0.buttons.borrow_mut().clear();

            let enabled = this.0.enabled.get();
            let mut buttons = Vec::with_capacity(items.len());
            for (index, text) in items.iter().enumerate() {
                let button = RadioButton::new().map_err(|e| to_error("RadioButton の生成", e))?;
                let label = TextBlock::new().map_err(|e| to_error("項目ラベルの生成", e))?;
                label
                    .SetText(&HSTRING::from(text.as_ref()))
                    .map_err(|e| to_error("項目ラベルの設定", e))?;
                button
                    .SetContent(&label)
                    .map_err(|e| to_error("RadioButton への内容設定", e))?;
                button
                    .SetGroupName(&this.0.group_name)
                    .map_err(|e| to_error("RadioButton の組の設定", e))?;
                button
                    .SetIsChecked(&bool_ref(false)?)
                    .map_err(|e| to_error("RadioButton の初期化", e))?;
                button
                    .SetIsEnabled(enabled)
                    .map_err(|e| to_error("RadioButton の有効化", e))?;

                // WinRT のデリゲートは Send + Sync を要求する一方、XAML の
                // イベントは UI スレッドで届く。弱参照を UiThreadCell に載せ、
                // 購読との循環も避ける。
                let state = UiThreadCell::new(Rc::downgrade(&this.0));
                let handler = RoutedEventHandler::new(move |_sender, _args| {
                    // borrow 中にユーザーコールバックを呼ばないことで再入を許す。
                    let Some(inner) = state.try_with_mut(|weak| weak.upgrade()).flatten() else {
                        return Ok(());
                    };
                    if !inner.silent.get() {
                        // 外れた側は `Unchecked` なので、ここには点いたものだけ来る。
                        inner.handler.emit(index);
                    }
                    Ok(())
                });
                button
                    .Checked(&handler)
                    .map_err(|e| to_error("RadioButton の選択購読", e))?;

                let element = button
                    .cast::<UIElement>()
                    .map_err(|e| to_error("RadioButton の要素化", e))?;
                children
                    .Append(&element)
                    .map_err(|e| to_error("RadioButton の追加", e))?;
                buttons.push(button);
            }
            *this.0.buttons.borrow_mut() = buttons;
            Ok(())
        })
    }

    /// 選択肢の数。
    pub fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 現在選ばれている選択肢。未選択なら `None`。
    pub fn selected(&self) -> Option<usize> {
        self.0.buttons.borrow().iter().position(is_checked)
    }

    /// 範囲内の選択肢を通知せずに選ぶ。範囲外なら何もしない。
    pub fn set_selected(&self, index: usize) {
        let _ = self.write_selected(index);
    }

    /// 選択を通知せずに外す。
    pub fn clear_selection(&self) {
        self.without_notifying(|this| {
            let Ok(off) = bool_ref(false) else {
                return;
            };
            for button in this.0.buttons.borrow().iter() {
                let _ = button.SetIsChecked(&off);
            }
        });
    }

    /// ユーザーが選んだのと同じように、範囲内の選択肢を選んで通知する。
    pub fn select(&self, index: usize) {
        if self.write_selected(index) {
            // 既に点いているものを選び直すと `Checked` は起きないため、通知は
            // WinUI に任せず、抑止した書き換えの後で必ず 1 回だけ送る。
            self.0.handler.emit(index);
        }
    }

    /// 選択肢の並ぶ向き。既定は縦。
    pub fn set_orientation(&self, orientation: Orientation) {
        let _ = self.0.native.SetOrientation(if orientation.is_vertical() {
            XamlOrientation::Vertical
        } else {
            XamlOrientation::Horizontal
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        for button in self.0.buttons.borrow().iter() {
            let _ = button.SetIsEnabled(enabled);
        }
    }

    /// 選択肢が選ばれたときに、そのインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは置き換わる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// WinUI の実コントロール。バックエンド固有の脱出口として公開している。
    pub fn native_buttons(&self) -> Vec<RadioButton> {
        self.0.buttons.borrow().clone()
    }

    fn write_selected(&self, index: usize) -> bool {
        self.without_notifying(|this| {
            let buttons = this.0.buttons.borrow();
            let Some(button) = buttons.get(index) else {
                return false;
            };
            let Ok(on) = bool_ref(true) else {
                return false;
            };
            // 残りを外すのは WinUI が `GroupName` を見て行う。
            button.SetIsChecked(&on).is_ok()
        })
    }

    /// WinUI の `Checked` を止めたまま操作する。
    fn without_notifying<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        let previous = self.0.silent.replace(true);
        let result = f(self);
        self.0.silent.set(previous);
        result
    }
}

fn is_checked(button: &RadioButton) -> bool {
    button.IsChecked().and_then(|r| r.Value()).unwrap_or(false)
}
