//! どのウィジェットにもある操作 (表示・非表示 / フォーカス / ツールチップ)。
//!
//! 対象は親へ置いている要素 ([`Widget::native_element`](crate::Widget::native_element))。

use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

/// アプリが隠した要素の目印。
///
/// naui はウィジェットごとにインラインの `display` (`flex` など) を書くので、
/// `hidden` 属性は負ける。この属性に `!important` 付きの `display: none` を
/// 当てる規則を文書へ 1 つだけ入れておき、インラインの指定より強くする。
/// naui が内部で使う `hidden` 属性 (タブの中身など) とも別にしておける。
const HIDDEN_ATTR: &str = "data-naui-hidden";
/// その規則を入れた `<style>` の id。
const STYLE_ID: &str = "naui-interaction-style";
const STYLE_RULES: &str = "[data-naui-hidden] { display: none !important; }";

/// フォーカスを受け取れる子孫。最初に受け取れたものへ移す。
const FOCUSABLE: &str = "input:not([type=hidden]), select, textarea, button, a[href], \
                         summary, [contenteditable=true], [tabindex]:not([tabindex='-1'])";

pub(crate) fn set_visible(element: &Element, visible: bool) {
    if visible {
        let _ = element.remove_attribute(HIDDEN_ATTR);
        return;
    }
    if let Some(document) = element.owner_document() {
        ensure_style(&document);
    }
    let _ = element.set_attribute(HIDDEN_ATTR, "");
}

pub(crate) fn is_visible(element: &Element) -> bool {
    !element.has_attribute(HIDDEN_ATTR)
}

fn ensure_style(document: &Document) {
    if document.get_element_by_id(STYLE_ID).is_some() {
        return;
    }
    let Ok(style) = document.create_element("style") else {
        return;
    };
    style.set_id(STYLE_ID);
    style.set_text_content(Some(STYLE_RULES));
    let parent: Option<Element> = document
        .head()
        .map(Into::into)
        .or_else(|| document.document_element());
    if let Some(parent) = parent {
        let _ = parent.append_child(&style);
    }
}

/// キーボードフォーカスを移す。移せたら `true`。
pub(crate) fn request_focus(element: &Element) -> bool {
    if !element.is_connected() {
        return false;
    }
    let Some(document) = element.owner_document() else {
        return false;
    };
    if try_focus(&document, element) {
        return true;
    }
    let Ok(candidates) = element.query_selector_all(FOCUSABLE) else {
        return false;
    };
    (0..candidates.length())
        .filter_map(|index| candidates.item(index))
        .filter_map(|node| node.dyn_into::<Element>().ok())
        .any(|candidate| try_focus(&document, &candidate))
}

/// 移せたかは、ブラウザがその要素を `activeElement` にしたかで見る
/// (隠れている・無効な要素の `focus()` は何も起こさない)。
fn try_focus(document: &Document, element: &Element) -> bool {
    let Some(html) = element.dyn_ref::<HtmlElement>() else {
        return false;
    };
    // WebKit は、隠すための `<style>` を入れた直後だと、まだ計算していない
    // スタイルで「表示されている」と判断してフォーカスを移してしまう。
    // 大きさを読んでスタイルとレイアウトを先に済ませる。
    let _ = html.offset_width();
    let _ = html.focus();
    document
        .active_element()
        .is_some_and(|active| &active == element)
}

pub(crate) fn set_tooltip(element: &Element, text: Option<&str>) {
    match text {
        Some(text) => {
            let _ = element.set_attribute("title", text);
        }
        None => {
            let _ = element.remove_attribute("title");
        }
    }
}

/// 読み上げソフトに伝える名前 (`aria-label`)。`None` で外す。
pub(crate) fn set_accessible_label(element: &Element, text: Option<&str>) {
    match text {
        Some(text) => {
            let _ = element.set_attribute("aria-label", text);
        }
        None => {
            let _ = element.remove_attribute("aria-label");
        }
    }
}
