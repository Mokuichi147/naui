//! ブラウザには OS のウィンドウが無いため、`<body>` 直下のブロック要素を
//! ウィンドウとして扱う。タイトルは `document.title` に反映する。

use std::cell::RefCell;
use std::rc::Rc;

use miui_core::{Error, Result, Theme};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::widgets::{create, Widget};
use crate::to_error;
use crate::apply_theme;

struct WindowInner {
    element: HtmlElement,
    document: Document,
    title: RefCell<String>,
    child: RefCell<Option<Box<dyn Widget>>>,
}

/// ページ上のウィンドウ相当。
#[derive(Clone)]
pub struct Window(Rc<WindowInner>);

impl Window {
    pub(crate) fn new(document: &Document, title: &str, width: f64, height: f64) -> Result<Self> {
        let element: HtmlElement = create(document, "div")?.unchecked_into();
        let style = element.style();
        // 指定サイズを上限としつつ、狭い画面では縮む。
        let _ = style.set_property("max-width", &format!("{width}px"));
        let _ = style.set_property("min-height", &format!("{height}px"));
        let _ = style.set_property("margin", "0 auto");
        let _ = style.set_property("box-sizing", "border-box");

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
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.element.set_inner_html("");
        if self.0.element.append_child(&child.native_element()).is_ok() {
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// 表示する。Web では最初から表示されているため、隠していた場合に戻す。
    pub fn show(&self) {
        let _ = self.0.element.style().remove_property("display");
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
