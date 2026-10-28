//! Receiving files dropped from the OS (`GtkDropTarget` for `GdkFileList`).
//!
//! GTK hands a drop to the innermost widget under the pointer whose drop target
//! accepts the offered formats, so nesting works without naui's help. Text fields
//! have their own drop target for text, and file managers also offer the files as
//! text (`text/uri-list`), so drops onto text fields go into the text instead.

use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;
use naui_core::FileEntry;

use crate::callback::FileNotifier;

/// The name of naui's drop target, so that setting the closure again can find the
/// previous one.
const CONTROLLER_NAME: &str = "naui-file-drop";

/// Lets `widget` take file drops. Setting it again replaces the previous closure.
pub(crate) fn set_handler(widget: &impl IsA<gtk::Widget>, f: impl FnMut(&[FileEntry]) + 'static) {
    let widget = widget.as_ref();
    let controllers = widget.observe_controllers();
    let previous: Vec<gtk::EventController> = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i))
        .filter_map(|object| object.downcast::<gtk::EventController>().ok())
        .filter(|controller| controller.name().as_deref() == Some(CONTROLLER_NAME))
        .collect();
    for controller in previous {
        widget.remove_controller(&controller);
    }

    let notifier = Rc::new(FileNotifier::default());
    notifier.set(f);
    let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    target.set_name(Some(CONTROLLER_NAME));
    target.connect_drop(move |_, value, _, _| {
        let Ok(list) = value.get::<gdk::FileList>() else {
            return false;
        };
        // ローカルに無いもの (パスを持たない URI) は渡せないので除く。
        let files: Vec<FileEntry> = list
            .files()
            .iter()
            .filter_map(|file| file.path())
            .map(FileEntry::from_path)
            .collect();
        if files.is_empty() {
            return false;
        }
        notifier.emit(&files);
        true
    });
    widget.add_controller(target);
}
