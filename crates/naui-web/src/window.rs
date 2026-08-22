//! ブラウザには OS のウィンドウが無いため、`<body>` 直下のブロック要素を
//! ウィンドウとして扱う。タイトルは `document.title` に反映する。

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use naui_core::{Error, Result, Theme};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::apply_theme;
use crate::to_error;
use crate::toolbar::Toolbar;
use crate::widgets::{create, Widget};

struct WindowInner {
    element: HtmlElement,
    document: Document,
    title: RefCell<String>,
    child: RefCell<Option<Box<dyn Widget>>>,
    /// 上端に差し込んだツールバー。通知先ごと生かしておく。
    toolbar: RefCell<Option<Toolbar>>,
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

        let this = Self(Rc::new(WindowInner {
            element,
            document: document.clone(),
            title: RefCell::new(String::new()),
            child: RefCell::new(None),
            toolbar: RefCell::new(None),
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
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.element.set_inner_html("");
        let element = child.native_element();
        if self.0.element.append_child(&element).is_ok() {
            crate::layout::fill_parent(&element);
            crate::layout::apply_child_layout(
                &element,
                crate::layout::ParentLayout::Flex(naui_core::Orientation::Vertical),
            );
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
        // 中身を入れ替えると差し込んだ要素も消えるので、付け直す。
        self.mount_toolbar();
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
    }

    /// ツールバーをウィンドウの先頭へ置き直す。
    fn mount_toolbar(&self) {
        let toolbar = self.0.toolbar.borrow();
        let Some(toolbar) = toolbar.as_ref() else {
            return;
        };
        let mount = toolbar.mount();
        let first = self.0.element.first_element_child();
        let _ = self
            .0
            .element
            .insert_before(&mount, first.as_ref().map(|e| e.as_ref()));
    }

    /// 表示する。Web では最初から表示されているため、隠していた場合に戻す。
    pub fn show(&self) {
        // 中身を縦に積む flex コンテナへ戻す (`none` からの復帰)。
        let _ = self.0.element.style().set_property("display", "flex");
    }

    pub fn close(&self) {
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
