//! A radio group that lays out options and lets the user choose exactly one (the
//! DOM's `<input type="radio">`).
//!
//! Exclusivity is left to the browser. Turning on one radio with a given `name`
//! automatically turns off the rest. The `name` is a unique string created per
//! group, so several radio groups on the same screen do not mix.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Orientation, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, HtmlInputElement};

use crate::widgets::{create, impl_widget, Listener, SelectionHandler, Widget};

thread_local! {
    /// A counter for creating a unique `name` per group.
    static NEXT_GROUP: Cell<u64> = const { Cell::new(0) };
}

fn next_group_name() -> String {
    NEXT_GROUP.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1));
        format!("naui-radio-{id}")
    })
}

struct RadioGroupInner {
    /// `<div role="radiogroup">`
    element: HtmlElement,
    document: Document,
    /// The `name` shared by the radios. The browser uses it to make them exclusive.
    name: String,
    inputs: RefCell<Vec<HtmlInputElement>>,
    /// The `change` subscriptions, in the same order as the inputs. Dropping them also
    /// unsubscribes.
    listeners: RefCell<Vec<Listener>>,
    on_select: SelectionHandler,
    /// The setting from [`set_enabled`](RadioGroup::set_enabled). Remembered so the
    /// inputs recreated by `set_items` get the same state.
    enabled: Cell<bool>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A radio group that lays out options and lets the user choose exactly one
/// (`<input type="radio">`).
///
/// Right after creation and right after [`set_items`](Self::set_items), nothing is
/// selected.
#[derive(Clone)]
pub struct RadioGroup(Rc<RadioGroupInner>);
impl_widget!(RadioGroup, element);

impl RadioGroup {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let element: HtmlElement = create(document, "div")?.unchecked_into();
        // 支援技術にもラジオの組として伝わるようにしておく。
        let _ = element.set_attribute("role", "radiogroup");
        let style = element.style();
        let _ = style.set_property("display", "flex");
        let _ = style.set_property("flex-direction", "column");
        let _ = style.set_property("align-items", "flex-start");

        Ok(Self(Rc::new(RadioGroupInner {
            element,
            document: document.clone(),
            name: next_group_name(),
            inputs: RefCell::new(Vec::new()),
            listeners: RefCell::new(Vec::new()),
            on_select: SelectionHandler::default(),
            enabled: Cell::new(true),
            file_drop: Default::default(),
        })))
    }

    /// Rebuilds the options and clears the selection. No selection notification is
    /// sent.
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        while let Some(child) = self.0.element.first_child() {
            let _ = self.0.element.remove_child(&child);
        }
        self.0.inputs.borrow_mut().clear();
        self.0.listeners.borrow_mut().clear();

        let enabled = self.0.enabled.get();
        let mut inputs = Vec::with_capacity(items.len());
        let mut listeners = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let Ok(input) = self.build_item(item.as_ref(), enabled) else {
                continue;
            };
            let listener = Listener::attach(input.as_ref(), "change", {
                let weak = Rc::downgrade(&self.0);
                move || {
                    if let Some(inner) = weak.upgrade() {
                        inner.on_select.emit(index);
                    }
                }
            });
            // `change` は点いたラジオにだけ届くので、外れた側は通知されない。
            if let Ok(listener) = listener {
                listeners.push(listener);
            }
            inputs.push(input);
        }
        *self.0.inputs.borrow_mut() = inputs;
        *self.0.listeners.borrow_mut() = listeners;
    }

    /// The number of options.
    pub fn len(&self) -> usize {
        self.0.inputs.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The currently selected option. `None` if nothing is selected.
    pub fn selected(&self) -> Option<usize> {
        self.0
            .inputs
            .borrow()
            .iter()
            .position(|input| input.checked())
    }

    /// Selects an in-range option without notifying. Does nothing if out of range.
    pub fn set_selected(&self, index: usize) {
        // `checked` を書き換えても `change` は起きないので、購読を止める必要はない。
        if let Some(input) = self.0.inputs.borrow().get(index) {
            input.set_checked(true);
        }
    }

    /// Clears the selection without notifying.
    pub fn clear_selection(&self) {
        for input in self.0.inputs.borrow().iter() {
            input.set_checked(false);
        }
    }

    /// Selects an in-range option and notifies, as if the user had chosen it.
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.set_selected(index);
            self.0.on_select.emit(index);
        }
    }

    /// The direction the options are laid out in. Vertical by default.
    pub fn set_orientation(&self, orientation: Orientation) {
        let _ = self.0.element.style().set_property(
            "flex-direction",
            if orientation.is_vertical() {
                "column"
            } else {
                "row"
            },
        );
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        for input in self.0.inputs.borrow().iter() {
            input.set_disabled(!enabled);
        }
    }

    /// Called with the index when an option is chosen. Setting it again removes the
    /// previous callback.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }

    /// Builds one `<label><input type="radio">text</label>` and adds it.
    fn build_item(&self, text: &str, enabled: bool) -> Result<HtmlInputElement> {
        let label: HtmlElement = create(&self.0.document, "label")?.unchecked_into();
        let input: HtmlInputElement = create(&self.0.document, "input")?.unchecked_into();
        input.set_type("radio");
        input.set_name(&self.0.name);
        input.set_checked(false);
        input.set_disabled(!enabled);
        let caption = create(&self.0.document, "span")?;
        caption.set_text_content(Some(text));
        let style = label.style();
        let _ = style.set_property("display", "inline-flex");
        let _ = style.set_property("align-items", "center");
        let _ = style.set_property("gap", "0.4em");
        label
            .append_child(&input)
            .map_err(|e| crate::to_error("ラジオボタンの組み立て", e))?;
        label
            .append_child(&caption)
            .map_err(|e| crate::to_error("ラジオボタンの組み立て", e))?;
        self.0
            .element
            .append_child(&label)
            .map_err(|e| crate::to_error("ラジオグループの組み立て", e))?;
        Ok(input)
    }
}
