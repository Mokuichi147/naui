//! Receiving files dropped from the OS (the DOM's `dragover` / `drop`).
//!
//! The browser hands over file contents, not paths, so dropped files are passed the
//! same way as with [`FilePicker`](crate::FilePicker):
//! [`FileEntry::source`](naui_core::FileEntry::source) is a `blob:` URL made with
//! `URL.createObjectURL`, and [`FileEntry::path`](naui_core::FileEntry::path) is
//! always `None`. **The URLs stay valid until the next drop onto the same target**,
//! which revokes them. A dropped folder comes as one entry with only its name, as
//! with the folder mode of `FilePicker`.
//!
//! The listeners are owned by the element (the closures are handed to JS), so they
//! go away with it and need no bookkeeping on the Rust side. Once any target
//! exists, the document refuses file drops that no target takes; otherwise a drop
//! that misses a target would make the browser open the file in place of the app.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::FileEntry;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{DataTransfer, DragEvent, EventTarget};

/// The property of the target element holding its listeners, so that setting the
/// closure again can take the previous ones off.
const LISTENERS_KEY: &str = "__nauiFileDrop";

/// The events a target answers. `dragenter` and `dragover` have to be cancelled
/// for the element to count as a drop target.
const ACCEPT_EVENTS: [&str; 2] = ["dragenter", "dragover"];

type DropCallback = Box<dyn FnMut(&[FileEntry])>;

struct DropTarget {
    handler: RefCell<Option<DropCallback>>,
    /// The `blob:` URLs handed out with the last drop. Revoked on the next one.
    object_urls: RefCell<Vec<String>>,
}

impl DropTarget {
    /// So that setting the closure again from inside the notification does not
    /// double-borrow, the closure is taken out only for the duration of the call.
    fn emit(&self, files: &[FileEntry]) {
        let Some(mut f) = self.handler.borrow_mut().take() else {
            return;
        };
        f(files);
        let mut slot = self.handler.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }

    /// Maps the dropped items into entries. Has to run inside the `drop` event;
    /// the browser empties the `DataTransfer` afterwards.
    fn read(&self, data: &DataTransfer) -> Vec<FileEntry> {
        let mut urls = self.object_urls.borrow_mut();
        for url in urls.drain(..) {
            let _ = web_sys::Url::revoke_object_url(&url);
        }
        let items = data.items();
        (0..items.length())
            .filter_map(|i| items.get(i))
            .filter(|item| item.kind() == "file")
            .filter_map(|item| {
                let is_folder = item
                    .webkit_get_as_entry()
                    .ok()
                    .flatten()
                    .is_some_and(|entry| entry.is_directory());
                let file = item.get_as_file().ok().flatten()?;
                let name = file.name();
                if is_folder {
                    // フォルダーの中身は読めないので、名前だけを渡す。
                    return Some(FileEntry::from_name(name));
                }
                Some(match web_sys::Url::create_object_url_with_blob(&file) {
                    Ok(url) => {
                        urls.push(url.clone());
                        FileEntry::from_name_and_source(name, url)
                    }
                    Err(_) => FileEntry::from_name(name),
                })
            })
            .collect()
    }
}

/// Whether the drag carries files (and not, say, selected text).
fn carries_files(event: &DragEvent) -> bool {
    event
        .data_transfer()
        .is_some_and(|data| data.types().includes(&JsValue::from_str("Files"), 0))
}

/// Lets `target` take file drops. Setting it again replaces the previous closure.
pub(crate) fn set_handler(target: &EventTarget, f: impl FnMut(&[FileEntry]) + 'static) {
    guard_document();
    remove_listeners(target);

    let state = Rc::new(DropTarget {
        handler: RefCell::new(Some(Box::new(f))),
        object_urls: RefCell::new(Vec::new()),
    });
    let accept = Closure::<dyn Fn(DragEvent)>::new(|event: DragEvent| {
        if !carries_files(&event) {
            return;
        }
        // 内側が受け取ったら、外側 (窓や文書) へは回さない。
        event.prevent_default();
        event.stop_propagation();
        if let Some(data) = event.data_transfer() {
            data.set_drop_effect("copy");
        }
    })
    .into_js_value();
    let drop = Closure::<dyn Fn(DragEvent)>::new(move |event: DragEvent| {
        if !carries_files(&event) {
            return;
        }
        event.prevent_default();
        event.stop_propagation();
        let Some(data) = event.data_transfer() else {
            return;
        };
        let files = state.read(&data);
        if !files.is_empty() {
            state.emit(&files);
        }
    })
    .into_js_value();

    for event in ACCEPT_EVENTS {
        let _ = target.add_event_listener_with_callback(event, accept.unchecked_ref());
    }
    let _ = target.add_event_listener_with_callback("drop", drop.unchecked_ref());

    let listeners = js_sys::Array::of2(&accept, &drop);
    let _ = js_sys::Reflect::set(target, &JsValue::from_str(LISTENERS_KEY), &listeners);
}

/// Takes off the listeners a previous [`set_handler`] added.
fn remove_listeners(target: &EventTarget) {
    let Ok(previous) = js_sys::Reflect::get(target, &JsValue::from_str(LISTENERS_KEY)) else {
        return;
    };
    let Some(previous) = previous.dyn_ref::<js_sys::Array>() else {
        return;
    };
    let accept = previous.get(0);
    let drop = previous.get(1);
    for event in ACCEPT_EVENTS {
        let _ = target.remove_event_listener_with_callback(event, accept.unchecked_ref());
    }
    let _ = target.remove_event_listener_with_callback("drop", drop.unchecked_ref());
}

thread_local! {
    static DOCUMENT_GUARDED: Cell<bool> = const { Cell::new(false) };
}

/// Makes the document refuse file drops no target takes, once per page.
///
/// Targets stop the events they take, so only the rest reach here. Without this, a
/// file dropped beside a target would replace the page with the file.
fn guard_document() {
    if DOCUMENT_GUARDED.with(|guarded| guarded.replace(true)) {
        return;
    }
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let refuse = Closure::<dyn Fn(DragEvent)>::new(|event: DragEvent| {
        if !carries_files(&event) {
            return;
        }
        event.prevent_default();
        if let Some(data) = event.data_transfer() {
            data.set_drop_effect("none");
        }
    })
    .into_js_value();
    for event in ["dragover", "drop"] {
        let _ = document.add_event_listener_with_callback(event, refuse.unchecked_ref());
    }
}
