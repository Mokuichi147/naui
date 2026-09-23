//! メニューバー (`GtkPopoverMenuBar` + `GMenu`)。
//!
//! GTK4 のメニューは「モデル (`GMenu`) と操作 (`GAction`)」でできている。
//! メニューバーも同じで、見出しは `GMenu` の submenu、項目は
//! **項目 1 つにつき 1 つの `GSimpleAction`** で表す (`GMenuItem` 自体は
//! 有効・無効を持たないため、選べるかどうかは操作側が持つ)。
//!
//! ショートカットは `GtkApplication` のアクセラレータへ登録する。押されたキーを
//! naui が見張るのではなく、**GTK4 が操作を呼び出す**ので、メニューを開いて
//! いなくても効き、項目の右端の表示も GTK4 が作る。アクセラレータの表は
//! アプリ全体で 1 つなので、登録するのは**ウィンドウへ取り付けている間だけ**。
//!
//! 見出しは `AdwToolbarView` の上段 (ヘッダーバーの下) へ入る。GNOME では
//! メニューバーよりハンバーガーメニューが好まれるが、naui は 4 環境で同じ
//! API を出すため、GTK4 が持つ本物のメニューバーをそのまま使う。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use naui_core::MenuSpec;

use crate::callback::ActivateNotifier;

/// メニューの操作をまとめて置く名前空間の頭。後ろにメニューバーごとの番号が付く。
///
/// アクセラレータは `GtkApplication` に**アプリ全体で 1 つの表**として
/// 登録されるので、名前空間を共有すると、あとから作ったメニューバーが
/// 前のメニューバーの登録を上書きしてしまう。メニューバーごとに分けておけば、
/// 同じキーを複数のウィンドウで使っても、GTK4 がフォーカスのあるウィンドウに
/// 入っている操作だけを呼ぶ。
const GROUP_PREFIX: &str = "naui-menubar-";

thread_local! {
    /// 次に作るメニューバーの番号。GTK4 はメインスレッドでしか触れない。
    static NEXT_GROUP: Cell<u64> = const { Cell::new(0) };
}

struct MenuBarInner {
    native: gtk::PopoverMenuBar,
    app: adw::Application,
    /// このメニューバーの操作の名前空間 (`naui-menubar-<番号>`)。
    group: String,
    actions: gio::SimpleActionGroup,
    menus: RefCell<Vec<MenuSpec>>,
    /// 見出しと項目のインデックスから引ける操作。区切り線のところは `None`。
    items: RefCell<Vec<Vec<Option<gio::SimpleAction>>>>,
    on_activate: ActivateNotifier,
    /// メニューバー全体の有効・無効。項目ごとの指定と AND を取る。
    enabled: Cell<bool>,
    /// 取り付けているウィンドウの数。アクセラレータはこれが 1 以上の間だけ
    /// アプリへ登録する。
    attachments: Cell<usize>,
}

impl MenuBarInner {
    /// 操作の名前。`GMenu` から参照するときは名前空間を頭に付ける。
    fn action_name(menu: usize, item: usize) -> String {
        format!("m{menu}i{item}")
    }

    /// 名前空間を付けた操作の名前 (`naui-menubar-3.m0i1` の形)。
    fn detailed_name(&self, menu: usize, item: usize) -> String {
        format!("{}.{}", self.group, Self::action_name(menu, item))
    }

    /// いまの項目のショートカットを、アプリのアクセラレータへ登録する。
    fn register_accels(&self) {
        for (menu, spec) in self.menus.borrow().iter().enumerate() {
            for (item, entry) in spec.items.iter().enumerate() {
                if entry.is_separator() {
                    continue;
                }
                if let Some(shortcut) = entry.shortcut {
                    // 右端の表示も、押されたキーの受け取りも GTK4 が行う。
                    self.app.set_accels_for_action(
                        &self.detailed_name(menu, item),
                        &[&shortcut.accelerator()],
                    );
                }
            }
        }
    }

    /// 登録したアクセラレータを 1 つずつ外す。
    fn clear_accels(&self) {
        for (menu, spec) in self.menus.borrow().iter().enumerate() {
            for (item, entry) in spec.items.iter().enumerate() {
                if entry.shortcut.is_some() {
                    self.app
                        .set_accels_for_action(&self.detailed_name(menu, item), &[]);
                }
            }
        }
    }
}

/// ウィンドウの上端に付く、OS のアプリケーションメニュー。
///
/// [`Widget`](crate::Widget) ではない。
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar) で取り付ける。
/// 項目が押されるたびに、その **(見出しのインデックス, 項目のインデックス)**
/// で [`on_activate`](Self::on_activate) が呼ばれる。項目のインデックスは
/// 区切り線を含めた並びの位置で、区切り線が返ることはない。
#[derive(Clone)]
pub struct MenuBar(Rc<MenuBarInner>);

impl MenuBar {
    pub(crate) fn new(app: &adw::Application) -> Self {
        let native = gtk::PopoverMenuBar::from_model(None::<&gio::Menu>);
        let actions = gio::SimpleActionGroup::new();
        let group = NEXT_GROUP.with(|next| {
            let id = next.get();
            next.set(id + 1);
            format!("{GROUP_PREFIX}{id}")
        });
        // 取り付ける前でもメニューから操作を引けるようにしておく。
        // 取り付け先のウィンドウへも同じ組を入れる (アクセラレータの解決は
        // フォーカスのあるウィジェットからたどるため)。
        native.insert_action_group(&group, Some(&actions));
        Self(Rc::new(MenuBarInner {
            native,
            app: app.clone(),
            group,
            actions,
            menus: RefCell::new(Vec::new()),
            items: RefCell::new(Vec::new()),
            on_activate: ActivateNotifier::default(),
            enabled: Cell::new(true),
            attachments: Cell::new(0),
        }))
    }

    /// メニューを作り直す。以前のメニューは取り除かれる。
    ///
    /// 見出しのインデックスは渡した並びの位置、項目のインデックスは
    /// 区切り線を含めた並びの位置。
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        self.0.clear_accels();
        for name in self.0.actions.list_actions() {
            self.0.actions.remove_action(&name);
        }

        let whole = self.0.enabled.get();
        let model = gio::Menu::new();
        let mut built = Vec::with_capacity(menus.len());
        for (menu_index, spec) in menus.iter().enumerate() {
            let submenu = gio::Menu::new();
            // 区切り線は「ここで節を切る」ことで表す。
            let mut section = gio::Menu::new();
            let mut actions = Vec::with_capacity(spec.items.len());
            for (item_index, item) in spec.items.iter().enumerate() {
                if item.is_separator() {
                    if section.n_items() > 0 {
                        submenu.append_section(None, &section);
                        section = gio::Menu::new();
                    }
                    actions.push(None);
                    continue;
                }

                let name = MenuBarInner::action_name(menu_index, item_index);
                let action = gio::SimpleAction::new(&name, None);
                action.set_enabled(item.enabled && whole);
                // ハンドルを強く持つとシグナルとの間で循環するため、弱参照にする。
                let weak = Rc::downgrade(&self.0);
                action.connect_activate(move |_, _| {
                    if let Some(inner) = weak.upgrade() {
                        inner.on_activate.emit(menu_index, item_index);
                    }
                });
                self.0.actions.add_action(&action);

                let detailed = self.0.detailed_name(menu_index, item_index);
                section.append(Some(&item.label), Some(&detailed));
                actions.push(Some(action));
            }
            if section.n_items() > 0 {
                submenu.append_section(None, &section);
            }
            model.append_submenu(Some(&spec.title), &submenu);
            built.push(actions);
        }

        self.0.native.set_menu_model(Some(&model));
        *self.0.items.borrow_mut() = built;
        let mut stored = self.0.menus.borrow_mut();
        stored.clear();
        stored.extend_from_slice(menus);
        drop(stored);
        // ショートカットは、ウィンドウへ取り付けている間だけ登録する。
        if self.0.attachments.get() > 0 {
            self.0.register_accels();
        }
    }

    /// 見出しの数。
    pub fn len(&self) -> usize {
        self.0.menus.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 見出し 1 つが持つ、区切り線を含めた項目数。範囲外は `0`。
    pub fn menu_len(&self, menu: usize) -> usize {
        self.0
            .menus
            .borrow()
            .get(menu)
            .map_or(0, |spec| spec.items.len())
    }

    /// 項目 1 つの有効・無効を変える。区切り線と範囲外は何もしない。
    pub fn set_item_enabled(&self, menu: usize, item: usize, enabled: bool) {
        let mut menus = self.0.menus.borrow_mut();
        let Some(entry) = menus
            .get_mut(menu)
            .and_then(|spec| spec.items.get_mut(item))
        else {
            return;
        };
        if entry.is_separator() {
            return;
        }
        entry.enabled = enabled;
        drop(menus);
        self.apply_enabled();
    }

    /// いま押せる項目か。区切り線と範囲外は `false`。
    pub fn is_item_enabled(&self, menu: usize, item: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .menus
                .borrow()
                .get(menu)
                .and_then(|spec| spec.items.get(item))
                .is_some_and(|entry| !entry.is_separator() && entry.enabled)
    }

    /// メニューバー全体の有効・無効を変える。項目ごとの指定は残る。
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// 項目ごとの指定と全体の指定をネイティブへ反映する。
    ///
    /// 見出しを押せなくするのは `GtkWidget` の sensitive で足りるが、
    /// アクセラレータは操作を直に呼ぶので、操作側も合わせて落とす。
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        self.0.native.set_sensitive(whole);
        let menus = self.0.menus.borrow();
        for (spec, actions) in menus.iter().zip(self.0.items.borrow().iter()) {
            for (entry, action) in spec.items.iter().zip(actions.iter()) {
                if let Some(action) = action {
                    action.set_enabled(entry.enabled && whole);
                }
            }
        }
    }

    /// 利用者が選んだのと同じように項目を実行する。
    ///
    /// 区切り線・押せない項目・範囲外は何もしない。
    pub fn activate(&self, menu: usize, item: usize) {
        if self.is_item_enabled(menu, item) {
            self.0.on_activate.emit(menu, item);
        }
    }

    /// 項目が押されたときに、その見出しと項目のインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_activate(&self, f: impl FnMut(usize, usize) + 'static) {
        self.0.on_activate.set(f);
    }

    /// 対応する `GtkPopoverMenuBar`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_menu_bar(&self) -> gtk::PopoverMenuBar {
        self.0.native.clone()
    }

    /// 項目に対応する `GSimpleAction`。区切り線と範囲外は `None`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_action(&self, menu: usize, item: usize) -> Option<gio::SimpleAction> {
        self.0.items.borrow().get(menu)?.get(item)?.clone()
    }

    /// 項目の操作を、名前空間を付けた名前 (`naui-menubar-3.m0i1` の形) で返す。
    /// 区切り線と範囲外は `None`。
    ///
    /// `gtk_widget_activate_action` や `gtk_application_set_accels_for_action`
    /// に渡す名前で、バックエンド固有の脱出口として公開している。
    pub fn native_action_name(&self, menu: usize, item: usize) -> Option<String> {
        self.native_action(menu, item)?;
        Some(self.0.detailed_name(menu, item))
    }

    /// ウィンドウの上段へ差し込む widget。[`crate::Window`] だけが使う。
    pub(crate) fn mount(&self) -> gtk::PopoverMenuBar {
        self.0.native.clone()
    }

    /// 取り付け先のウィンドウへ入れる操作の組。[`crate::Window`] だけが使う。
    pub(crate) fn action_group(&self) -> gio::SimpleActionGroup {
        self.0.actions.clone()
    }

    /// ウィンドウへ取り付けたときに呼ぶ。[`crate::Window`] だけが使う。
    ///
    /// 最初の取り付けで、ショートカットをアプリのアクセラレータへ登録する。
    pub(crate) fn attach(&self) {
        let count = self.0.attachments.get();
        self.0.attachments.set(count + 1);
        if count == 0 {
            self.0.register_accels();
        }
    }

    /// ウィンドウから外したときに呼ぶ。[`crate::Window`] だけが使う。
    ///
    /// どのウィンドウにも付いていなくなったら、アクセラレータの登録も外す。
    /// 外したメニューバーのショートカットが、アプリの表に残らないようにする。
    pub(crate) fn detach(&self) {
        let count = self.0.attachments.get();
        if count == 0 {
            return;
        }
        self.0.attachments.set(count - 1);
        if count == 1 {
            self.0.clear_accels();
        }
    }

    /// 操作の名前空間。[`crate::Window`] だけが使う。
    pub(crate) fn group(&self) -> String {
        self.0.group.clone()
    }
}

impl Drop for MenuBarInner {
    fn drop(&mut self) {
        // アプリへ登録したアクセラレータは、メニューバーが無くなっても残る。
        self.clear_accels();
    }
}
