//! Operations every widget has (show/hide / focus / tooltip).
//!
//! The target is the element placed in the parent
//! ([`Widget::native_element`](crate::Widget::native_element)).

use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

/// The marker on elements the app hid.
///
/// naui writes an inline `display` (`flex` and so on) per widget, so the `hidden`
/// attribute loses to it. A single rule applying `display: none` with `!important`
/// to this attribute is added to the document, making it stronger than the inline
/// setting. It also keeps it separate from the `hidden` attribute naui uses
/// internally (tab contents and so on).
const HIDDEN_ATTR: &str = "data-naui-hidden";
/// The id of the `<style>` holding that rule.
const STYLE_ID: &str = "naui-interaction-style";
const STYLE_RULES: &str = "[data-naui-hidden] { display: none !important; }";

/// Descendants that can take focus. Focus moves to the first one that accepts it.
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

/// Moves keyboard focus here. `true` if it moved.
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

/// Whether it moved is judged by whether the browser made that element the
/// `activeElement` (`focus()` on hidden or disabled elements does nothing).
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

/// The name reported to screen readers (`aria-label`). `None` removes it.
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
