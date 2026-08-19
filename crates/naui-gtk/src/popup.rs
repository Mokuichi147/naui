//! ポップアップ (コンテキスト) メニュー (`GtkPopoverMenu` + `GMenu`)。
//!
//! GTK4 のメニューは「モデル (`GMenu`) と操作 (`GAction`)」でできている。
//! naui の項目は「文字・選べるかどうか・区切り線」だけなので、
//! **項目 1 つにつき `GSimpleAction` を 1 つ**作り、選べるかどうかは
//! その操作の有効・無効で表す (`GMenuItem` 自体は有効・無効を持たない)。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use naui_core::PopupItem;

use crate::callback::Notifier;
use crate::widgets::Widget;

/// メニューの操作をまとめて置く名前空間。
const GROUP: &str = "naui-popup";

struct PopupInner {
    native: gtk::PopoverMenu,
    actions: gio::SimpleActionGroup,
    items: RefCell<Vec<PopupItem>>,
    on_select: Notifier<usize>,
    /// 右クリックを拾うために取り付けたウィジェット。
    attached: RefCell<Vec<gtk::Widget>>,
}

/// 右クリックで出るポップアップ (コンテキスト) メニュー。
///
/// 画面に並ぶウィジェットではないので [`Widget`] ではない。
#[derive(Clone)]
pub struct PopupMenu(Rc<PopupInner>);

impl PopupMenu {
    pub(crate) fn new() -> Self {
        let native = gtk::PopoverMenu::from_model(None::<&gio::Menu>);
        native.set_has_arrow(false);
        native.set_halign(gtk::Align::Start);
        let actions = gio::SimpleActionGroup::new();
        native.insert_action_group(GROUP, Some(&actions));
        Self(Rc::new(PopupInner {
            native,
            actions,
            items: RefCell::new(Vec::new()),
            on_select: Notifier::default(),
            attached: RefCell::new(Vec::new()),
        }))
    }

    /// 対応する `GtkPopoverMenu`。バックエンド固有の脱出口として公開している。
    pub fn native_popover(&self) -> gtk::PopoverMenu {
        self.0.native.clone()
    }

    /// 項目を作り直す。インデックスは**区切り線を含めた並び**で数える。
    pub fn set_items(&self, items: &[PopupItem]) {
        for name in self.0.actions.list_actions() {
            self.0.actions.remove_action(&name);
        }

        let menu = gio::Menu::new();
        let mut section = gio::Menu::new();
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                // 区切り線は「ここで節を切る」ことで表す。
                if section.n_items() > 0 {
                    menu.append_section(None, &section);
                    section = gio::Menu::new();
                }
                continue;
            }
            let name = format!("item{index}");
            let action = gio::SimpleAction::new(&name, None);
            action.set_enabled(item.enabled);
            {
                let weak = Rc::downgrade(&self.0);
                action.connect_activate(move |_, _| {
                    if let Some(inner) = weak.upgrade() {
                        inner.on_select.emit(index);
                    }
                });
            }
            self.0.actions.add_action(&action);
            section.append(Some(&item.label), Some(&format!("{GROUP}.{name}")));
        }
        if section.n_items() > 0 {
            menu.append_section(None, &section);
        }
        self.0.native.set_menu_model(Some(&menu));

        let mut stored = self.0.items.borrow_mut();
        stored.clear();
        stored.extend_from_slice(items);
    }

    /// 区切り線を含めた項目数。
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 右クリックでこのメニューが出るようにする。いくつでも取り付けられる。
    pub fn attach(&self, widget: &dyn Widget) {
        let target = widget.native_widget();
        let gesture = gtk::GestureClick::new();
        // 3 は副ボタン (右クリック)。
        gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
        {
            let weak = Rc::downgrade(&self.0);
            let target = target.clone();
            gesture.connect_pressed(move |_, _, x, y| {
                if let Some(inner) = weak.upgrade() {
                    PopupMenu(inner).show_at(&target, x, y);
                }
            });
        }
        target.add_controller(gesture);
        self.0.attached.borrow_mut().push(target);
    }

    /// プログラムから出す。位置は `widget` の左上からの論理ピクセル。
    pub fn open_at(&self, widget: &dyn Widget, x: f64, y: f64) {
        self.show_at(&widget.native_widget(), x, y);
    }

    /// 取り付け先を差し替えてから出す。
    ///
    /// GTK4 のポップオーバーは親を 1 つしか持てないので、出すたびに
    /// そのときのウィジェットへ付け替える。
    fn show_at(&self, target: &gtk::Widget, x: f64, y: f64) {
        let native = &self.0.native;
        if native.parent().as_ref() != Some(target) {
            if native.parent().is_some() {
                native.unparent();
            }
            native.set_parent(target);
        }
        native.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
            x.round() as i32,
            y.round() as i32,
            1,
            1,
        )));
        native.popup();
    }

    pub fn close(&self) {
        self.0.native.popdown();
    }

    /// ユーザーが選んだのと同じ経路で通知する。
    ///
    /// 区切り線と選べない項目は無視する。
    pub fn select(&self, index: usize) {
        let selectable = self
            .0
            .items
            .borrow()
            .get(index)
            .is_some_and(|item| item.enabled && !item.is_separator());
        if selectable {
            self.0.on_select.emit(index);
        }
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }
}

impl Drop for PopupInner {
    fn drop(&mut self) {
        // GTK4 では、親を持ったまま壊すと警告が出る。
        if self.native.parent().is_some() {
            self.native.unparent();
        }
    }
}
