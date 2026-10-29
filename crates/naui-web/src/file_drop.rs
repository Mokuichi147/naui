//! Receiving files dropped from the OS (the DOM's `dragover` / `drop`).
//!
//! The browser hands over file contents, not paths, so dropped files are passed the
//! same way as with [`FilePicker`](crate::FilePicker):
//! [`FileEntry::source`](naui_core::FileEntry::source) is a `blob:` URL made with
//! `URL.createObjectURL`, and [`FileEntry::path`](naui_core::FileEntry::path) is
//! always `None`. **The URLs stay valid until the next drop onto the same target**,
//! which revokes them, or until the widget (or window) is dropped. A dropped folder
//! comes as one entry with only its name, as with the folder mode of `FilePicker`.
//!
//! As with `on_key_down`, each widget and window holds a [`FileDrop`], and the
//! listeners and the handed-out URLs go away with it.
//!
//! Drops onto text fields do not reach `on_file_drop`, as on the other platforms:
//! the targets pass them by, and the document refuses them. Once any target
//! exists, the document refuses file drops that no target takes; otherwise a drop
//! that misses a target would make the browser open the file in place of the app.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::FileEntry;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{DataTransfer, DragEvent, Element, Event, EventTarget, HtmlElement};

use crate::widgets::Listener;

/// The events a target answers. `dragenter` and `dragover` have to be cancelled
/// for the element to count as a drop target.
const ACCEPT_EVENTS: [&str; 2] = ["dragenter", "dragover"];

/// The text fields whose drops do not reach `on_file_drop` (`TextInput`,
/// `TextArea`, `NumberInput` and the like). Content-editable elements are checked
/// separately.
const TEXT_FIELDS: &str = "textarea, input:not([type]), input[type=\"text\"], \
input[type=\"search\"], input[type=\"password\"], input[type=\"number\"], \
input[type=\"email\"], input[type=\"url\"], input[type=\"tel\"]";

type DropCallback = Box<dyn FnMut(&[FileEntry])>;

#[derive(Default)]
struct DropTarget {
    handler: RefCell<Option<DropCallback>>,
    /// The `blob:` URLs handed out with the last drop. Revoked on the next one and
    /// when the target goes away.
    object_urls: RefCell<Vec<String>>,
}

impl Drop for DropTarget {
    fn drop(&mut self) {
        revoke_all(&mut self.object_urls.borrow_mut());
    }
}

/// Revokes all the `blob:` URLs handed out.
fn revoke_all(urls: &mut Vec<String>) {
    for url in urls.drain(..) {
        let _ = web_sys::Url::revoke_object_url(&url);
    }
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
        revoke_all(&mut urls);
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

/// The `on_file_drop` closure of a widget or window and its subscriptions. The
/// subscriptions are attached the first time a closure is set.
#[derive(Default)]
pub(crate) struct FileDrop {
    target: Rc<DropTarget>,
    listeners: RefCell<Vec<Listener>>,
}

impl FileDrop {
    /// Lets `element` take file drops. Setting it again replaces the previous
    /// closure; the URLs handed out so far are still revoked on the next drop.
    pub(crate) fn set(&self, element: &EventTarget, f: impl FnMut(&[FileEntry]) + 'static) {
        *self.target.handler.borrow_mut() = Some(Box::new(f));
        if !self.listeners.borrow().is_empty() {
            return;
        }
        guard_document();
        let mut listeners = Vec::new();
        for event in ACCEPT_EVENTS {
            let listener = Listener::attach_event(element, event, |event| {
                let Some(event) = taken_here(&event) else {
                    return;
                };
                // 内側が受け取ったら、外側 (窓や文書) へは回さない。
                event.prevent_default();
                event.stop_propagation();
                if let Some(data) = event.data_transfer() {
                    data.set_drop_effect("copy");
                }
            });
            listeners.extend(listener.ok());
        }
        let target = self.target.clone();
        let listener = Listener::attach_event(element, "drop", move |event| {
            let Some(event) = taken_here(&event) else {
                return;
            };
            event.prevent_default();
            event.stop_propagation();
            let Some(data) = event.data_transfer() else {
                return;
            };
            let files = target.read(&data);
            if !files.is_empty() {
                target.emit(&files);
            }
        });
        listeners.extend(listener.ok());
        *self.listeners.borrow_mut() = listeners;
    }
}

/// The drag event, when a target should take it: it carries files and is not
/// aimed at a text field.
fn taken_here(event: &Event) -> Option<&DragEvent> {
    let event = event.dyn_ref::<DragEvent>()?;
    // 入力欄の上なら受け口は素通しにして、文書のほうで断る。
    (carries_files(event) && !on_text_field(event)).then_some(event)
}

/// Whether the drag carries files (and not, say, selected text).
fn carries_files(event: &DragEvent) -> bool {
    event
        .data_transfer()
        .is_some_and(|data| data.types().includes(&JsValue::from_str("Files"), 0))
}

/// Whether the drag is over a text field.
fn on_text_field(event: &DragEvent) -> bool {
    let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return false;
    };
    target
        .dyn_ref::<HtmlElement>()
        .is_some_and(HtmlElement::is_content_editable)
        || target.closest(TEXT_FIELDS).ok().flatten().is_some()
}

thread_local! {
    static DOCUMENT_GUARDED: Cell<bool> = const { Cell::new(false) };
}

/// Makes the document refuse file drops no target takes, once per page.
///
/// Targets stop the events they take, so only the rest reach here, including drops
/// onto text fields. Without this, a file dropped beside a target would replace
/// the page with the file.
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
