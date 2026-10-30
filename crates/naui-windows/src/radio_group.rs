//! A radio group that lays out options and lets the user choose exactly one
//! (WinUI 3).
//!
//! `RadioButton`s are laid out in a `StackPanel` with the same `GroupName`. WinUI
//! handles exclusivity, and the `GroupName` is a unique string created per group, so
//! several radio groups on the same screen do not mix.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Orientation, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Orientation as XamlOrientation, RadioButton, StackPanel, TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement};
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{bool_ref, impl_widget, SelectHandler, Widget};

/// The gap between items laid out horizontally. Same as WinUI's
/// `RadioButtonsColumnSpacing`.
const HORIZONTAL_SPACING: f64 = 7.0;

thread_local! {
    /// A counter for creating a unique `GroupName` per group.
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
    /// The `GroupName` shared by the radios. WinUI uses it to make them exclusive.
    group_name: HSTRING,
    buttons: RefCell<Vec<RadioButton>>,
    handler: SelectHandler,
    /// Stops WinUI's notifications only while the selection or items are being changed
    /// from code.
    silent: Cell<bool>,
    /// The setting from [`set_enabled`](RadioGroup::set_enabled). `StackPanel` has no
    /// `IsEnabled` (it belongs to `Control`), so it is written on each button and
    /// remembered so the buttons recreated by `set_items` get it too.
    enabled: Cell<bool>,
}

/// A radio group that lays out options and lets the user choose exactly one
/// (`RadioButton`).
///
/// Nothing is selected right after creation and after
/// [`set_items`](Self::set_items).
#[derive(Clone)]
pub struct RadioGroup(Rc<RadioGroupInner>);
impl_widget!(RadioGroup, native);

impl RadioGroup {
    pub(crate) fn new() -> Result<Self> {
        let native = StackPanel::new().map_err(|e| to_error("create the StackPanel", e))?;
        native
            .SetOrientation(XamlOrientation::Vertical)
            .map_err(|e| to_error("set the radio group orientation", e))?;
        Ok(Self(Rc::new(RadioGroupInner {
            native,
            group_name: next_group_name(),
            buttons: RefCell::new(Vec::new()),
            handler: SelectHandler::new(),
            silent: Cell::new(false),
            enabled: Cell::new(true),
        })))
    }

    /// Rebuilds the options and clears the selection. No selection notification is
    /// sent.
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        let _ = self.rebuild(items);
    }

    fn rebuild<S: AsRef<str>>(&self, items: &[S]) -> Result<()> {
        self.without_notifying(|this| {
            let children = this
                .0
                .native
                .Children()
                .map_err(|e| to_error("get the radio group items", e))?;
            children
                .Clear()
                .map_err(|e| to_error("clear the radio group items", e))?;
            this.0.buttons.borrow_mut().clear();

            let enabled = this.0.enabled.get();
            let mut buttons = Vec::with_capacity(items.len());
            for (index, text) in items.iter().enumerate() {
                let button =
                    RadioButton::new().map_err(|e| to_error("create the RadioButton", e))?;
                let label = TextBlock::new().map_err(|e| to_error("create the item label", e))?;
                label
                    .SetText(&HSTRING::from(text.as_ref()))
                    .map_err(|e| to_error("set the item label", e))?;
                button
                    .SetContent(&label)
                    .map_err(|e| to_error("set the RadioButton content", e))?;
                button
                    .SetGroupName(&this.0.group_name)
                    .map_err(|e| to_error("set the RadioButton group", e))?;
                button
                    .SetIsChecked(&bool_ref(false)?)
                    .map_err(|e| to_error("initialize the RadioButton", e))?;
                button
                    .SetIsEnabled(enabled)
                    .map_err(|e| to_error("enable the RadioButton", e))?;

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
                    .map_err(|e| to_error("subscribe to RadioButton selection", e))?;

                let element = button
                    .cast::<UIElement>()
                    .map_err(|e| to_error("convert the RadioButton to UIElement", e))?;
                children
                    .Append(&element)
                    .map_err(|e| to_error("add the RadioButton", e))?;
                buttons.push(button);
            }
            *this.0.buttons.borrow_mut() = buttons;
            Ok(())
        })
    }

    /// The number of options.
    pub fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The currently selected option. `None` if nothing is selected.
    pub fn selected(&self) -> Option<usize> {
        self.0.buttons.borrow().iter().position(is_checked)
    }

    /// Selects an in-range option without notifying. Does nothing if out of range.
    pub fn set_selected(&self, index: usize) {
        let _ = self.write_selected(index);
    }

    /// Clears the selection without notifying.
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

    /// Selects an in-range option and notifies, as if the user had chosen it.
    pub fn select(&self, index: usize) {
        if self.write_selected(index) {
            // 既に点いているものを選び直すと `Checked` は起きないため、通知は
            // WinUI に任せず、抑止した書き換えの後で必ず 1 回だけ送る。
            self.0.handler.emit(index);
        }
    }

    /// The direction the options are laid out in. Vertical by default.
    pub fn set_orientation(&self, orientation: Orientation) {
        let vertical = orientation.is_vertical();
        let _ = self.0.native.SetOrientation(if vertical {
            XamlOrientation::Vertical
        } else {
            XamlOrientation::Horizontal
        });
        // RadioButton の右には余白が無いので、横に並べると項目の文字が次の
        // 丸に付く。WinUI の RadioButtons が横の間に置く幅
        // (`RadioButtonsColumnSpacing`) と同じだけ空ける。縦は RadioButton
        // の MinHeight で間が空くので足さない。
        let _ = self
            .0
            .native
            .SetSpacing(if vertical { 0.0 } else { HORIZONTAL_SPACING });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        for button in self.0.buttons.borrow().iter() {
            let _ = button.SetIsEnabled(enabled);
        }
    }

    /// Called with the index when an option is chosen. Setting it again replaces the
    /// previous callback.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// The real WinUI controls. Public as a backend-specific escape hatch.
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

    /// Operates with WinUI's `Checked` stopped.
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
