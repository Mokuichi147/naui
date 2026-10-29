//! Receiving files dropped from the OS (Finder and other apps).
//!
//! AppKit hands a drag to the deepest view registered with
//! `registerForDraggedTypes:`, and that view's class has to implement
//! `NSDraggingDestination` to answer. naui's widgets are stock AppKit classes, so
//! the views cannot answer themselves. Instead every naui window registers for file
//! URLs, `NSWindow` passes the messages to its delegate, and the delegate asks this
//! module which widget is under the pointer.
//!
//! Views that register for drags themselves (text fields and text views, which take
//! dropped files as text) are deeper than the window, so AppKit gives the drop to
//! them, and it does not reach naui.

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::FileEntry;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::ClassType;
use objc2_app_kit::{
    NSDragOperation, NSDraggingInfo, NSPasteboard, NSPasteboardTypeFileURL,
    NSPasteboardURLReadingFileURLsOnlyKey, NSView, NSWindow,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};

type DropCallback = Box<dyn FnMut(&[FileEntry])>;

/// The closure notified with dropped files.
///
/// So that setting it again from inside the notification does not double-borrow,
/// the closure is taken out only for the duration of the call.
#[derive(Default)]
pub(crate) struct FileDropHandler(RefCell<Option<DropCallback>>);

impl FileDropHandler {
    pub(crate) fn set(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub(crate) fn is_set(&self) -> bool {
        self.0.borrow().is_some()
    }

    fn emit(&self, files: &[FileEntry]) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(files);
        // 呼び出し中に差し替えられていたら、新しいほうを残す。
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

thread_local! {
    /// The widgets that take file drops, with their closures.
    static TARGETS: RefCell<Vec<(Weak<NSView>, Rc<FileDropHandler>)>> =
        const { RefCell::new(Vec::new()) };
}

/// Lets the widget whose view is `view` take file drops. Setting it again replaces
/// the previous closure.
pub(crate) fn set_handler(view: &NSView, f: impl FnMut(&[FileEntry]) + 'static) {
    let handler = Rc::new(FileDropHandler::default());
    handler.set(f);
    TARGETS.with(|targets| {
        let mut targets = targets.borrow_mut();
        // 消えたビューの分もここで片づける。
        targets.retain(|(weak, _)| {
            weak.load()
                .is_some_and(|other| !std::ptr::eq(&*other, view))
        });
        targets.push((Weak::from(view), handler));
    });
}

/// The closure registered for `view` itself.
fn handler_of(view: &NSView) -> Option<Rc<FileDropHandler>> {
    TARGETS.with(|targets| {
        targets.borrow().iter().find_map(|(weak, handler)| {
            weak.load()
                .filter(|other| std::ptr::eq(&**other, view))
                .map(|_| handler.clone())
        })
    })
}

/// Makes the window a destination for file drags. Called once when it is made.
pub(crate) fn register_window(window: &NSWindow) {
    let types = NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]);
    window.registerForDraggedTypes(&types);
}

/// Who takes a drop at a point: the nearest widget under it that has a closure, or
/// the window's own closure.
fn target(
    window: &NSWindow,
    sender: &ProtocolObject<dyn NSDraggingInfo>,
    window_handler: &Rc<FileDropHandler>,
) -> Option<Rc<FileDropHandler>> {
    if !has_files(&sender.draggingPasteboard()) {
        return None;
    }
    let location = sender.draggingLocation();
    // 窓の座標で引けるよう、タイトルバーまで含む枠のビューから探す。
    let frame_view = window
        .contentView()
        .map(|content| unsafe { content.superview() }.unwrap_or(content));
    let mut view: Option<Retained<NSView>> = frame_view.and_then(|frame| frame.hitTest(location));
    while let Some(current) = view {
        if let Some(handler) = handler_of(&current) {
            return Some(handler);
        }
        view = unsafe { current.superview() };
    }
    window_handler.is_set().then(|| window_handler.clone())
}

/// The answer to `draggingEntered:` / `draggingUpdated:`. Copy where something
/// takes the files, otherwise refuse (the pointer shows no badge).
pub(crate) fn operation(
    window: &NSWindow,
    sender: &ProtocolObject<dyn NSDraggingInfo>,
    window_handler: &Rc<FileDropHandler>,
) -> NSDragOperation {
    if target(window, sender, window_handler).is_some() {
        NSDragOperation::Copy
    } else {
        NSDragOperation::None
    }
}

/// `performDragOperation:`. Hands the files to whoever is under the pointer.
pub(crate) fn perform(
    window: &NSWindow,
    sender: &ProtocolObject<dyn NSDraggingInfo>,
    window_handler: &Rc<FileDropHandler>,
) -> bool {
    let Some(handler) = target(window, sender, window_handler) else {
        return false;
    };
    let files = read_files(&sender.draggingPasteboard());
    if files.is_empty() {
        return false;
    }
    handler.emit(&files);
    true
}

/// Reading options that take only `file:` URLs (not web links).
fn file_url_options() -> Retained<NSDictionary<NSString, AnyObject>> {
    let key: &NSString = unsafe { NSPasteboardURLReadingFileURLsOnlyKey };
    let yes = NSNumber::new_bool(true);
    let value: &AnyObject = &yes;
    NSDictionary::from_slices(&[key], &[value])
}

fn has_files(pasteboard: &NSPasteboard) -> bool {
    let classes = NSArray::from_slice(&[NSURL::class()]);
    unsafe { pasteboard.canReadObjectForClasses_options(&classes, Some(&file_url_options())) }
}

/// Reads the dropped file URLs as entries carrying absolute paths.
fn read_files(pasteboard: &NSPasteboard) -> Vec<FileEntry> {
    let classes = NSArray::from_slice(&[NSURL::class()]);
    let Some(objects) =
        (unsafe { pasteboard.readObjectsForClasses_options(&classes, Some(&file_url_options())) })
    else {
        return Vec::new();
    };
    objects
        .iter()
        .filter_map(|object| object.downcast::<NSURL>().ok())
        .filter_map(|url| {
            url.path()
                .map(|path| FileEntry::from_path(path.to_string()))
        })
        .collect()
}
