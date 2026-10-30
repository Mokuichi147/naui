//! The items inside a `<div role="menu">`, shared by [`crate::MenuBar`] and
//! [`crate::PopupMenu`].
//!
//! | naui | DOM |
//! | --- | --- |
//! | Item | `<button role="menuitem">` |
//! | Item with a check mark | `<button role="menuitemcheckbox" aria-checked>` |
//! | Submenu | `<button role="menuitem" aria-haspopup="menu">` + `<div role="menu">` beside it |
//! | Separator | `<div role="separator">` |
//!
//! Browsers draw neither check marks nor submenu arrows for these roles, so the
//! check mark (`✓`) and the arrow (`▸`) are text in front of and after the label,
//! hidden from screen readers (which read `aria-checked` and `aria-haspopup`
//! instead). When a menu has check items, every item in it gets the column for the
//! mark so the labels line up.
//!
//! A submenu opens when its heading is clicked or hovered, beside the heading
//! (`position: absolute`), and to the left when there is no room on the right.
//! Hovering an ordinary item closes the submenus of its siblings.

use std::collections::HashMap;

use naui_core::{MenuItem, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::to_error;
use crate::widgets::{create, set_disabled, Listener};

pub(crate) fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

fn append(parent: &Element, child: &Element) -> Result<()> {
    parent
        .append_child(child)
        .map(|_| ())
        .map_err(|e| to_error("add to the DOM", e))
}

/// The look that differs between the menu bar and popup menus.
pub(crate) struct Look {
    /// Styles a `<div role="menu">` (the box itself).
    pub(crate) surface: fn(&HtmlElement),
    /// Styles an item's `<button>`.
    pub(crate) item: fn(&HtmlElement),
    /// Styles a separator.
    pub(crate) separator: fn(&HtmlElement),
}

/// What happens when an item is used. Each returns the handler for one path.
pub(crate) struct Actions<'a> {
    /// An ordinary item or check item was chosen.
    pub(crate) choose: &'a dyn Fn(Vec<usize>) -> Box<dyn FnMut()>,
    /// The submenus down to this path should be open (and the others closed).
    pub(crate) open: &'a dyn Fn(Vec<usize>) -> Box<dyn FnMut()>,
}

/// The built DOM, looked up by path.
#[derive(Default)]
pub(crate) struct MenuDom {
    /// The `<button>` per item, submenu headings included. Separators are not in it.
    pub(crate) buttons: HashMap<Vec<usize>, HtmlElement>,
    /// The `<div role="menu">` per submenu, by the path of its heading.
    pub(crate) submenus: HashMap<Vec<usize>, HtmlElement>,
    /// The subscriptions of the buttons.
    pub(crate) listeners: Vec<Listener>,
}

impl MenuDom {
    /// Builds `items` into `menu`, recursing into submenus.
    ///
    /// `path` is the path of `menu` itself, and `enabled` is whether everything leading
    /// to it is enabled.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build(
        &mut self,
        doc: &Document,
        menu: &HtmlElement,
        items: &[MenuItem],
        path: &mut Vec<usize>,
        enabled: bool,
        look: &Look,
        actions: &Actions,
    ) -> Result<()> {
        let has_checks = items.iter().any(MenuItem::is_checkable);
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                let separator: HtmlElement = create(doc, "div")?.unchecked_into();
                let _ = separator.set_attribute("role", "separator");
                (look.separator)(&separator);
                append(menu, &separator)?;
                continue;
            }
            path.push(index);
            let here = item.enabled && enabled;

            let button: HtmlElement = create(doc, "button")?.unchecked_into();
            let _ = button.set_attribute("type", "button");
            (look.item)(&button);
            style(&button, "display", "flex");
            style(&button, "align-items", "center");
            style(&button, "width", "100%");
            style(&button, "text-align", "start");
            if has_checks {
                // 印の列。印の無い項目も同じ幅を空けて、文字の頭をそろえる。
                let mark: HtmlElement = create(doc, "span")?.unchecked_into();
                let _ = mark.set_attribute("aria-hidden", "true");
                mark.set_text_content(Some(if item.checked == Some(true) {
                    "✓"
                } else {
                    ""
                }));
                style(&mark, "flex", "0 0 1.4em");
                append(&button, &mark)?;
            }
            let label: HtmlElement = create(doc, "span")?.unchecked_into();
            label.set_text_content(Some(&item.label));
            style(&label, "flex", "1 1 auto");
            append(&button, &label)?;

            if let Some(children) = &item.submenu {
                let _ = button.set_attribute("role", "menuitem");
                let _ = button.set_attribute("aria-haspopup", "menu");
                let _ = button.set_attribute("aria-expanded", "false");
                trailing(doc, &button, "▸")?;

                // 見出しの横へ出すため、見出しとサブメニューを位置決めの入れ物へ入れる。
                let holder: HtmlElement = create(doc, "div")?.unchecked_into();
                let _ = holder.set_attribute("role", "none");
                style(&holder, "position", "relative");
                append(&holder, &button)?;

                let submenu: HtmlElement = create(doc, "div")?.unchecked_into();
                let _ = submenu.set_attribute("role", "menu");
                let _ = submenu.set_attribute("aria-label", &item.label);
                (look.surface)(&submenu);
                style(&submenu, "position", "absolute");
                // 枠と余白の分だけ上へずらし、最初の項目を見出しの高さにそろえる。
                style(&submenu, "top", "-5px");
                style(&submenu, "left", "100%");
                style(&submenu, "display", "none");
                self.build(doc, &submenu, children, path, here, look, actions)?;
                append(&holder, &submenu)?;
                append(menu, &holder)?;

                for event in ["click", "pointerenter"] {
                    self.listeners.push(Listener::attach(
                        button.as_ref(),
                        event,
                        (actions.open)(path.clone()),
                    )?);
                }
                self.submenus.insert(path.clone(), submenu);
            } else {
                match item.checked {
                    Some(checked) => {
                        let _ = button.set_attribute("role", "menuitemcheckbox");
                        let _ = button.set_attribute("aria-checked", &checked.to_string());
                    }
                    None => {
                        let _ = button.set_attribute("role", "menuitem");
                    }
                }
                if let Some(shortcut) = item.shortcut.filter(|s| s.is_valid()) {
                    // ブラウザはショートカットの表示を持たないので、naui が添える。
                    // 読み上げには aria-keyshortcuts で伝える。
                    trailing(doc, &button, &shortcut.label())?;
                    let _ =
                        button.set_attribute("aria-keyshortcuts", &shortcut.aria_key_shortcuts());
                }
                append(menu, &button)?;
                self.listeners.push(Listener::attach(
                    button.as_ref(),
                    "click",
                    (actions.choose)(path.clone()),
                )?);
                // 普通の項目に乗ったら、兄弟のサブメニューは閉じる。
                let parent = path[..path.len() - 1].to_vec();
                self.listeners.push(Listener::attach(
                    button.as_ref(),
                    "pointerenter",
                    (actions.open)(parent),
                )?);
            }
            set_disabled(&button, !here);
            self.buttons.insert(path.clone(), button);
            path.pop();
        }
        Ok(())
    }

    /// Shows exactly the submenus whose headings lead to `open`, and hides the
    /// others. An empty path closes every submenu.
    ///
    /// A disabled submenu heading does not open.
    pub(crate) fn open_to(&self, open: &[usize]) {
        if self.submenus.contains_key(open)
            && self
                .buttons
                .get(open)
                .is_some_and(|button| button.has_attribute("disabled"))
        {
            return;
        }
        // 浅いほうから開くと、深いほうの置き場所を親の位置から決められる。
        let mut paths: Vec<&Vec<usize>> = self.submenus.keys().collect();
        paths.sort_by_key(|path| path.len());
        for path in paths {
            let submenu = &self.submenus[path];
            let visible = open.starts_with(path);
            style(submenu, "display", if visible { "block" } else { "none" });
            if let Some(button) = self.buttons.get(path) {
                let _ = button.set_attribute("aria-expanded", &visible.to_string());
            }
            if visible {
                keep_beside(submenu);
            }
        }
    }

    /// Shows the check mark of the button at `path`.
    pub(crate) fn set_checked(&self, path: &[usize], checked: bool) {
        let Some(button) = self.buttons.get(path) else {
            return;
        };
        let _ = button.set_attribute("aria-checked", &checked.to_string());
        if let Some(mark) = button.first_element_child() {
            mark.set_text_content(Some(if checked { "✓" } else { "" }));
        }
    }
}

/// Adds text at the right edge of an item (a shortcut or the submenu arrow).
fn trailing(doc: &Document, button: &HtmlElement, text: &str) -> Result<()> {
    let hint: HtmlElement = create(doc, "span")?.unchecked_into();
    hint.set_text_content(Some(text));
    // 読み上げでは操作名だけを読ませる (ショートカットは aria-keyshortcuts で伝える)。
    let _ = hint.set_attribute("aria-hidden", "true");
    style(&hint, "margin-inline-start", "2em");
    style(&hint, "opacity", "0.6");
    append(button, &hint)
}

/// Opens a submenu to the left of its heading when it would overflow the right edge
/// of the viewport.
fn keep_beside(submenu: &HtmlElement) {
    style(submenu, "left", "100%");
    style(submenu, "right", "auto");
    let Some(limit) = web_sys::window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|value| value.as_f64())
    else {
        return;
    };
    if submenu.get_bounding_client_rect().right() > limit {
        style(submenu, "left", "auto");
        style(submenu, "right", "100%");
    }
}
