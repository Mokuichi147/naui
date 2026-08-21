//! ツールバー (`AdwHeaderBar` に載せる `GtkButton` の並び)。
//!
//! GNOME では、よく使う操作はウィンドウ上端の `AdwHeaderBar` へ入れるのが
//! 作法で、GTK3 の `GtkToolbar` は GTK4 で廃止された。そのため naui の
//! ツールバーは [`Widget`](crate::Widget) ではなく、
//! [`Window::set_toolbar`](crate::Window::set_toolbar) でヘッダーバーへ
//! 取り付ける。macOS の `NSToolbar` がタイトルバーと一体になるのと同じ位置に
//! 出る。
//!
//! アイコンは [`ToolbarIcon`](naui_core::ToolbarIcon) をアイコンテーマの
//! 名前 (freedesktop の標準名) へ写したもので、`label` はツールチップと
//! 読み上げに使う。区切りは `GtkSeparator` で表す。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use naui_core::ToolbarItem;

use crate::callback::Notifier;

pub(crate) struct ToolbarInner {
    /// ヘッダーバーへ差し込む入れ物。項目はこの中に並ぶ。
    native: gtk::Box,
    items: RefCell<Vec<ToolbarItem>>,
    /// 項目と同じ並び。区切りのところは `None`。
    buttons: RefCell<Vec<Option<gtk::Button>>>,
    on_activate: Notifier<usize>,
    /// ツールバー全体の有効・無効。項目ごとの指定と AND を取る。
    enabled: Cell<bool>,
}

/// ウィンドウの上端に付く、よく使う操作の並び。
///
/// [`Widget`](crate::Widget) ではない。
/// [`Window::set_toolbar`](crate::Window::set_toolbar) で取り付ける。
/// ナビゲーションと違い**選ばれている項目を持たず**、押されるたびに
/// そのインデックスで [`on_activate`](Self::on_activate) が呼ばれる。
/// インデックスは区切りを含めた並びの位置で、区切りが返ることはない。
#[derive(Clone)]
pub struct Toolbar(Rc<ToolbarInner>);

impl Toolbar {
    pub(crate) fn new() -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        Self(Rc::new(ToolbarInner {
            native,
            items: RefCell::new(Vec::new()),
            buttons: RefCell::new(Vec::new()),
            on_activate: Notifier::default(),
            enabled: Cell::new(true),
        }))
    }

    /// ヘッダーバーへ差し込む入れ物。[`crate::Window`] だけが使う。
    pub(crate) fn mount(&self) -> gtk::Box {
        self.0.native.clone()
    }

    /// 項目を作り直す。以前の項目は取り除かれる。
    ///
    /// インデックスは区切りを含めた並びの位置。
    pub fn set_items(&self, items: &[ToolbarItem]) {
        while let Some(child) = self.0.native.first_child() {
            self.0.native.remove(&child);
        }
        self.0.buttons.borrow_mut().clear();

        let whole = self.0.enabled.get();
        let mut buttons = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                let separator = gtk::Separator::new(gtk::Orientation::Vertical);
                self.0.native.append(&separator);
                buttons.push(None);
                continue;
            }

            // 見た目はアイコンテーマの絵。無い名前ならテーマの
            // 「画像がありません」の絵が出る (GTK4 が面倒を見る)。
            let button = gtk::Button::from_icon_name(item.icon.icon_name());
            // ヘッダーバーのボタンは枠なしが GNOME の既定。
            button.add_css_class("flat");
            button.set_tooltip_text(Some(&item.label));
            // アイコンだけでは読み上げに出ないので、名前を持たせる。
            button.update_property(&[gtk::accessible::Property::Label(&item.label)]);
            button.set_sensitive(item.enabled && whole);
            // ハンドルを強く持つとシグナルとの間で循環するため、弱参照にする。
            let weak = Rc::downgrade(&self.0);
            button.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_activate.emit(index);
                }
            });
            self.0.native.append(&button);
            buttons.push(Some(button));
        }

        *self.0.buttons.borrow_mut() = buttons;
        self.0.items.borrow_mut().clear();
        self.0.items.borrow_mut().extend_from_slice(items);
    }

    /// 区切りを含めた項目数。
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 項目 1 つの有効・無効を変える。区切りと範囲外は何もしない。
    pub fn set_item_enabled(&self, index: usize, enabled: bool) {
        let mut items = self.0.items.borrow_mut();
        let Some(item) = items.get_mut(index) else {
            return;
        };
        if item.is_separator() {
            return;
        }
        item.enabled = enabled;
        drop(items);
        self.apply_enabled();
    }

    /// いま押せる項目か。区切りと範囲外は `false`。
    pub fn is_item_enabled(&self, index: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .items
                .borrow()
                .get(index)
                .is_some_and(|item| !item.is_separator() && item.enabled)
    }

    /// ツールバー全体の有効・無効を変える。項目ごとの指定は残る。
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// 項目ごとの指定と全体の指定をネイティブへ反映する。
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        let items = self.0.items.borrow();
        for (button, item) in self.0.buttons.borrow().iter().zip(items.iter()) {
            if let Some(button) = button {
                button.set_sensitive(item.enabled && whole);
            }
        }
    }

    /// 利用者が押したのと同じように項目を実行する。
    ///
    /// 区切り・押せない項目・範囲外は何もしない。
    pub fn activate(&self, index: usize) {
        if self.is_item_enabled(index) {
            self.0.on_activate.emit(index);
        }
    }

    /// 項目が押されたときに、そのインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_activate.set(f);
    }

    /// 項目を並べている `GtkBox`。バックエンド固有の脱出口として公開している。
    pub fn native_box(&self) -> gtk::Box {
        self.0.native.clone()
    }

    /// 項目に対応する GTK4 のボタン。区切りと範囲外は `None`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_button(&self, index: usize) -> Option<gtk::Button> {
        self.0.buttons.borrow().get(index)?.clone()
    }
}
