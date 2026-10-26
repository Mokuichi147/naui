//! A combo box (`GtkDropDown` + `GtkStringList`).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

struct ComboBoxInner {
    native: gtk::DropDown,
    model: gtk::StringList,
    bin: SizeBin,
    on_select: Notifier<usize>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// A drop-down for choosing one item.
///
/// The native model has a final empty placeholder for no selection. Real items
/// keep their naui indices; the placeholder has no content and cannot be activated
/// in the popup.
#[derive(Clone)]
pub struct ComboBox(Rc<ComboBoxInner>);
impl_widget!(ComboBox);

impl ComboBox {
    pub(crate) fn new() -> Self {
        // Some GTK releases enforce a selected item in a nonempty DropDown and
        // ignore INVALID_LIST_POSITION. A trailing placeholder preserves our
        // optional selection without shifting the indices of real items.
        let model = gtk::StringList::new(&[""]);
        let native = gtk::DropDown::builder().model(&model).build();
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_bind({
            let model = model.clone();
            move |_, object| {
                let item = object.downcast_ref::<gtk::ListItem>().expect("list item");
                let placeholder = item.position() == model.n_items().saturating_sub(1);
                item.set_selectable(!placeholder);
                item.set_activatable(!placeholder);
                if placeholder {
                    item.set_child(None::<&gtk::Widget>);
                    return;
                }
                let text = item
                    .item()
                    .and_downcast::<gtk::StringObject>()
                    .expect("string");
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                let label = gtk::Label::new(Some(&text.string()));
                label.set_xalign(0.0);
                label.set_hexpand(true);
                let mark = gtk::Image::from_icon_name("object-select-symbolic");
                item.bind_property("selected", &mark, "visible")
                    .sync_create()
                    .build();
                row.append(&label);
                row.append(&mark);
                item.set_child(Some(&row));
            }
        });
        native.set_list_factory(Some(&factory));
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(ComboBoxInner {
            native,
            model,
            bin,
            on_select: Notifier::default(),
            handler: RefCell::new(None),
        });

        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_selected_notify(move |native| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let index = native.selected();
                if index < inner.model.n_items().saturating_sub(1) {
                    inner.on_select.emit(index as usize);
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// Rebuilds the items. Indices change meaning, so the selection is cleared. No
    /// notification is sent.
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        let mut additions: Vec<&str> = items.iter().map(AsRef::as_ref).collect();
        additions.push("");
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.model.splice(0, self.0.model.n_items(), &additions);
            self.0.native.set_selected(items.len() as u32);
        });
    }

    pub fn len(&self) -> usize {
        self.0.model.n_items().saturating_sub(1) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        let index = self.0.native.selected();
        (index < self.0.model.n_items().saturating_sub(1)).then_some(index as usize)
    }

    /// Changes the selection without notifying, if the index is in range.
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            without_signal(&self.0.native, &self.0.handler, || {
                self.0.native.set_selected(index as u32);
            });
        }
    }

    /// Clears the selection without notifying.
    pub fn clear_selection(&self) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_selected(self.len() as u32);
        });
    }

    /// Selects an item through the same path as a user's choice and notifies once.
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.set_selected(index);
            self.0.on_select.emit(index);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the index of the chosen item when the selection changes.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select.set(f);
    }
}
