//! ブラウザには OS のウィンドウが無いため、`<body>` 直下のブロック要素を
//! ウィンドウとして扱う。タイトルは `document.title` に反映する。

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use naui_core::{Error, Result, Theme};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::apply_theme;
use crate::menu_bar::MenuBar;
use crate::sidebar::Sidebar;
use crate::to_error;
use crate::toolbar::Toolbar;
use crate::widgets::{create, Widget};

/// ウィンドウ要素に付ける印。
const WINDOW_ATTRIBUTE: &str = "data-naui-window";

/// ウィンドウ要素を `closest` で探すためのセレクタ。
pub(crate) const WINDOW_SELECTOR: &str = "[data-naui-window]";

struct WindowInner {
    element: HtmlElement,
    document: Document,
    title: RefCell<String>,
    child: RefCell<Option<Box<dyn Widget>>>,
    /// 上端に差し込んだツールバー。通知先ごと生かしておく。
    toolbar: RefCell<Option<Toolbar>>,
    /// 上端に差し込んだメニューバー。通知先ごと生かしておく。
    menu_bar: RefCell<Option<MenuBar>>,
    /// 取り付けたサイドバー。通知先ごと生かしておく。
    sidebar: RefCell<Option<Sidebar>>,
    /// ツールバーの行。サイドバーの開閉ボタンとツールバーを横に並べる。
    toolbar_row: HtmlElement,
}

/// ページ上のウィンドウ相当。
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

/// ウィンドウを強く保持せずにイベントハンドラから参照するための弱参照。
#[derive(Clone)]
pub struct WeakWindow(Weak<WindowInner>);

impl WeakWindow {
    /// ウィンドウがまだ生きていれば強参照へ戻す。
    pub fn upgrade(&self) -> Option<Window> {
        self.0.upgrade().map(Window)
    }
}

impl Window {
    /// イベントハンドラなどへ渡しても所有権循環を作らない参照を返す。
    pub fn downgrade(&self) -> WeakWindow {
        WeakWindow(Rc::downgrade(&self.0))
    }

    pub(crate) fn new(document: &Document, title: &str, width: f64, height: f64) -> Result<Self> {
        let element: HtmlElement = create(document, "div")?.unchecked_into();
        // メニューバーが「キーがどのウィンドウから上がってきたか」を見分ける印。
        let _ = element.set_attribute(WINDOW_ATTRIBUTE, "");
        let style = element.style();
        // 指定サイズを上限としつつ、狭い画面では縮む。
        let _ = style.set_property("max-width", &format!("{width}px"));
        let _ = style.set_property("min-height", &format!("{height}px"));
        let _ = style.set_property("margin", "0 auto");
        let _ = style.set_property("box-sizing", "border-box");
        // 中身がウィンドウの高さいっぱいに広がれるようにする。
        let _ = style.set_property("display", "flex");
        let _ = style.set_property("flex-direction", "column");
        crate::layout::mark_parent(
            &element,
            crate::layout::ParentLayout::Flex(naui_core::Orientation::Vertical),
        );

        let body = document
            .body()
            .ok_or_else(|| Error::new("body の取得", "body がありません"))?;
        body.append_child(&element)
            .map_err(|e| to_error("ウィンドウの追加", e))?;

        // ツールバーの行。サイドバーの開閉ボタン (あれば) とツールバーを並べる。
        // macOS でサイドバーボタンがツールバーの先頭に入るのと同じ並び。
        let toolbar_row: HtmlElement = create(document, "div")?.unchecked_into();
        let row_style = toolbar_row.style();
        let _ = row_style.set_property("display", "flex");
        let _ = row_style.set_property("flex-direction", "row");
        let _ = row_style.set_property("align-items", "center");
        let _ = row_style.set_property("gap", "6px");
        let _ = row_style.set_property("flex-shrink", "0");

        let this = Self(Rc::new(WindowInner {
            element,
            document: document.clone(),
            title: RefCell::new(String::new()),
            child: RefCell::new(None),
            toolbar: RefCell::new(None),
            menu_bar: RefCell::new(None),
            sidebar: RefCell::new(None),
            toolbar_row,
        }));
        this.set_title(title);
        Ok(this)
    }

    pub fn set_title(&self, title: &str) {
        *self.0.title.borrow_mut() = title.to_string();
        self.0.document.set_title(title);
    }

    pub fn title(&self) -> String {
        self.0.title.borrow().clone()
    }

    pub fn set_size(&self, width: f64, height: f64) {
        let style = self.0.element.style();
        let _ = style.set_property("max-width", &format!("{width}px"));
        let _ = style.set_property("min-height", &format!("{height}px"));
    }

    /// ルートに置くウィジェット。呼ぶたびに置き換わる。
    ///
    /// ルートはウィンドウいっぱいに広がる (AppKit の contentView と同じ)。
    /// サイドバーを付けているときは、その右の中身の側に置かれる。
    pub fn set_child(&self, child: &dyn Widget) {
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
        self.mount_body();
    }

    /// ウィンドウの左に付けるサイドバー。呼ぶたびに置き換わる。
    ///
    /// ブラウザにはサイドバーのコントロールが無いため、ウィンドウ要素の中を
    /// 「サイドバー (`<aside>`) と中身」の横並びへ組み替え、
    /// [`set_child`](Self::set_child) の子は中身の側へ移す。
    pub fn set_sidebar(&self, sidebar: &Sidebar) {
        *self.0.sidebar.borrow_mut() = Some(sidebar.clone());
        self.mount_body();
    }

    /// 取り付けたサイドバーを外す。付いていなければ何もしない。
    ///
    /// 中身の側にあった子は、ウィンドウの中身へ戻る。
    pub fn clear_sidebar(&self) {
        let old = self.0.sidebar.borrow_mut().take();
        if let Some(old) = old {
            old.set_content(None);
            old.mount().remove();
            self.mount_body();
        }
    }

    /// ウィンドウ要素の中身を組み直す。
    ///
    /// 子 (とサイドバー) を置き、先頭へツールバーとメニューバーを差し込む。
    fn mount_body(&self) {
        self.0.element.set_inner_html("");
        let child = self
            .0
            .child
            .borrow()
            .as_ref()
            .map(|child| child.native_element());
        let sidebar = self.0.sidebar.borrow().clone();
        let body = match &sidebar {
            Some(sidebar) => {
                sidebar.set_content(child.as_ref());
                Some(sidebar.mount().unchecked_into::<Element>())
            }
            None => child,
        };
        if let Some(body) = body {
            if self.0.element.append_child(&body).is_ok() {
                crate::layout::fill_parent(&body);
                crate::layout::apply_child_layout(
                    &body,
                    crate::layout::ParentLayout::Flex(naui_core::Orientation::Vertical),
                );
            }
        }
        // 中身を入れ替えると差し込んだ要素も消えるので、付け直す。
        // メニューバーはツールバーより上に来る (OS のメニューと同じ順)。
        self.mount_toolbar();
        self.mount_menu_bar();
    }

    /// ウィンドウの上端に付けるツールバー。呼ぶたびに置き換わる。
    ///
    /// ブラウザにはタイトルバーが無いため、ウィンドウ要素の先頭に置く。
    pub fn set_toolbar(&self, toolbar: &Toolbar) {
        self.clear_toolbar();
        *self.0.toolbar.borrow_mut() = Some(toolbar.clone());
        self.mount_toolbar();
    }

    /// 取り付けたツールバーを外す。付いていなければ何もしない。
    pub fn clear_toolbar(&self) {
        if let Some(old) = self.0.toolbar.borrow_mut().take() {
            old.mount().remove();
        }
        self.mount_toolbar();
    }

    /// ツールバーの行を組み直して、ウィンドウの先頭へ置く。
    ///
    /// 並びは [サイドバーの開閉ボタン][ツールバー]。どちらも無ければ行ごと
    /// 外す。ツールバーが無くてもサイドバーがあれば、ボタンだけの行になる
    /// (macOS のボタンだけのツールバーと同じ)。
    fn mount_toolbar(&self) {
        let row = &self.0.toolbar_row;
        row.set_inner_html("");
        if let Some(sidebar) = self.0.sidebar.borrow().as_ref() {
            let _ = row.append_child(&sidebar.toggle());
        }
        if let Some(toolbar) = self.0.toolbar.borrow().as_ref() {
            let _ = row.append_child(&toolbar.mount());
        }
        if row.child_element_count() == 0 {
            row.remove();
            return;
        }
        // メニューバーが付いていれば、その直下 (OS のメニューと同じ順)。
        let menu_bar = self.0.menu_bar.borrow().as_ref().map(|m| m.mount());
        match menu_bar.filter(|m| m.parent_element().as_ref() == Some(self.0.element.as_ref())) {
            Some(menu_bar) => {
                let _ = self
                    .0
                    .element
                    .insert_before(row, menu_bar.next_sibling().as_ref());
            }
            None => self.mount_first(row),
        }
    }

    /// ウィンドウの上端に付けるメニューバー。呼ぶたびに置き換わる。
    ///
    /// ブラウザには OS のメニューバーが無いため、ウィンドウ要素の先頭
    /// (ツールバーより上) に置く。
    pub fn set_menu_bar(&self, menu_bar: &MenuBar) {
        self.clear_menu_bar();
        *self.0.menu_bar.borrow_mut() = Some(menu_bar.clone());
        self.mount_menu_bar();
        // ショートカットの購読は、取り付けている間だけ張る。
        menu_bar.attach(self.0.element.as_ref());
    }

    /// 取り付けたメニューバーを外す。付いていなければ何もしない。
    ///
    /// 外したメニューバーはショートカットにも反応しなくなる。
    pub fn clear_menu_bar(&self) {
        let old = self.0.menu_bar.borrow_mut().take();
        if let Some(old) = old {
            old.detach();
            old.mount().remove();
        }
    }

    /// メニューバーをウィンドウの先頭へ置き直す。
    fn mount_menu_bar(&self) {
        let menu_bar = self.0.menu_bar.borrow();
        let Some(menu_bar) = menu_bar.as_ref() else {
            return;
        };
        self.mount_first(&menu_bar.mount());
    }

    /// ウィンドウ要素の先頭へ差し込む。
    fn mount_first(&self, mount: &HtmlElement) {
        let first = self.0.element.first_element_child();
        let _ = self
            .0
            .element
            .insert_before(mount, first.as_ref().map(|e| e.as_ref()));
    }

    /// 表示する。Web では最初から表示されているため、隠していた場合に戻す。
    pub fn show(&self) {
        // 中身を縦に積む flex コンテナへ戻す (`none` からの復帰)。
        let _ = self.0.element.style().set_property("display", "flex");
    }

    pub fn close(&self) {
        // 開いたままのメニューを残さない (隠れたウィンドウの Esc を拾わない)。
        let menu_bar = self.0.menu_bar.borrow().clone();
        if let Some(menu_bar) = menu_bar {
            menu_bar.close();
        }
        let _ = self.0.element.style().set_property("display", "none");
    }

    pub fn is_visible(&self) -> bool {
        self.0
            .element
            .style()
            .get_property_value("display")
            .map(|v| v != "none")
            .unwrap_or(true)
    }

    /// このウィンドウの配色テーマを切り替える。
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        apply_theme(&self.0.document, theme)
    }

    /// DOM 要素。バックエンド固有の脱出口。
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }
}
