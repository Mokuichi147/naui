//! Receiving files dropped from the OS (`GtkDropTarget` for `GdkFileList`).
//!
//! GTK hands a drop to the innermost widget under the pointer whose drop target
//! accepts the offered formats, so nesting works without naui's help. Text fields
//! have their own drop target for text, and file managers also offer the files as
//! text (`text/uri-list`), so drops onto text fields go into the text instead.
//!
//! [`drop_onto`] (`drop_files`, for tests and automation) emits `drop` on the same
//! drop target a real drop reaches. GTK cannot be asked to pick it, so it is found
//! by walking up from the widget (event propagation in GTK4 follows the same parent
//! chain), and the entries are handed over as given rather than read back from the
//! `GdkFileList`.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use naui_core::FileEntry;

use crate::callback::FileNotifier;

/// The name of naui's drop target, so that setting the closure again can find the
/// previous one.
const CONTROLLER_NAME: &str = "naui-file-drop";

thread_local! {
    /// The entries [`drop_onto`] is handing to the drop target it emits `drop` on.
    static DROPPING: RefCell<Option<Vec<FileEntry>>> = const { RefCell::new(None) };
}

/// Lets `widget` take file drops. Setting it again replaces the previous closure.
pub(crate) fn set_handler(widget: &impl IsA<gtk::Widget>, f: impl FnMut(&[FileEntry]) + 'static) {
    let widget = widget.as_ref();
    for controller in targets_of(widget) {
        widget.remove_controller(&controller);
    }

    let notifier = Rc::new(FileNotifier::default());
    notifier.set(f);
    let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    target.set_name(Some(CONTROLLER_NAME));
    target.connect_drop(move |_, value, _, _| {
        // `drop_onto` からなら、渡された項目をそのまま使う。
        let files = match DROPPING.with(|dropping| dropping.borrow_mut().take()) {
            Some(files) => files,
            None => {
                let Ok(list) = value.get::<gdk::FileList>() else {
                    return false;
                };
                // ローカルに無いもの (パスを持たない URI) は渡せないので除く。
                list.files()
                    .iter()
                    .filter_map(|file| file.path())
                    .map(FileEntry::from_path)
                    .collect()
            }
        };
        if files.is_empty() {
            return false;
        }
        notifier.emit(&files);
        true
    });
    widget.add_controller(target);
}

/// Drops `files` the way a real drop onto `widget` goes: to naui's drop target on
/// `widget`, or else on the nearest ancestor that has one (the window's is on the
/// window itself). Nothing happens if `files` is empty or no drop target is found.
///
/// It emits `drop` on that `GtkDropTarget`, with a `GdkFileList` of the entries that
/// have a path, as GTK does; the entries themselves reach the handler as given.
pub(crate) fn drop_onto(widget: &gtk::Widget, files: &[FileEntry]) {
    if files.is_empty() {
        return;
    }
    let Some((target, at)) = std::iter::successors(Some(widget.clone()), |w| w.parent())
        .find_map(|w| targets_of(&w).into_iter().next().map(|target| (target, w)))
    else {
        return;
    };
    let Ok(target) = target.downcast::<gtk::DropTarget>() else {
        return;
    };
    let paths: Vec<gio::File> = files
        .iter()
        .filter_map(FileEntry::path)
        .map(gio::File::for_path)
        .collect();
    let list = gdk::FileList::from_array(&paths);
    // 落とした位置は受け口のウィジェットの真ん中にしておく (受け取り側は使わない)。
    let (x, y) = (f64::from(at.width()) / 2.0, f64::from(at.height()) / 2.0);
    DROPPING.with(|dropping| *dropping.borrow_mut() = Some(files.to_vec()));
    // シグナルの引数は、値そのものを包んだ GValue。
    let value = glib::BoxedValue(list.to_value());
    target.emit_by_name::<bool>("drop", &[&value, &x, &y]);
    // 受け取り側が読まずに終わっても、次の本物のドロップへ持ち越さない。
    DROPPING.with(|dropping| dropping.borrow_mut().take());
}

/// naui's drop targets on `widget` (at most one, once set up).
fn targets_of(widget: &gtk::Widget) -> Vec<gtk::EventController> {
    let controllers = widget.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|i| controllers.item(i))
        .filter_map(|object| object.downcast::<gtk::EventController>().ok())
        .filter(|controller| controller.name().as_deref() == Some(CONTROLLER_NAME))
        .collect()
}
