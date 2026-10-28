//! Receiving files dropped from Explorer (`AllowDrop` with `DragOver` / `Drop`).
//!
//! As with `on_key_down` (see `keys`), one pair of handlers on the window's root
//! does the work. XAML's drag events bubble up from the element under the pointer,
//! so walking up the parents from `OriginalSource` finds the innermost registered
//! widget, and failing that the window. Subscribing on each element instead would
//! leave the event tokens to be managed per element.
//!
//! `TextBox` handles drags itself and stops them, so drops onto text fields do not
//! arrive (the same as the other platforms, where the field takes them as text).
//!
//! The files come from `DataPackageView::GetStorageItemsAsync`, which completes
//! later, so the drop is held with a deferral until they are read, and the closure
//! is called on the UI thread from the app's tasks.
//!
//! The registry holds elements by weak reference, as `keys` does, and the handlers
//! are released on the shutdown path with [`release_handlers`].

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{FileEntry, Tasks};
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{
    DependencyObject, DragEventArgs, DragEventHandler, UIElement,
};
use windows::ApplicationModel::DataTransfer::{
    DataPackageOperation, DataPackageView, StandardDataFormats,
};
use windows_core::{IInspectable, Interface, Weak};

use crate::keys::same;
use crate::ui_thread::{Registry, UiThreadCell};
use crate::Slot;

type DropCallback = dyn FnMut(&[FileEntry]);

/// The closure notified with dropped files.
///
/// So that setting it again from inside the notification does not double-borrow,
/// the closure is taken out only for the duration of the call.
#[derive(Default)]
pub(crate) struct FileDropHandler(RefCell<Slot<DropCallback>>);

impl FileDropHandler {
    pub(crate) fn set(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn is_set(&self) -> bool {
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

type Entries = Registry<(Weak<UIElement>, Rc<FileDropHandler>)>;

thread_local! {
    static WIDGETS: Entries = const { Registry::new() };
}

/// Lets the widget whose element is `element` take file drops. Each call replaces
/// the previous closure.
pub(crate) fn set_widget_handler(element: &UIElement, f: impl FnMut(&[FileEntry]) + 'static) {
    WIDGETS.with(|entries| {
        let mut entries = entries.borrow_mut();
        entries.retain(|(weak, _)| weak.upgrade().is_some());
        if let Some((_, handler)) = entries
            .iter()
            .find(|(weak, _)| weak.upgrade().is_some_and(|other| same(&other, element)))
        {
            handler.set(f);
            return;
        }
        let Ok(weak) = element.downgrade() else {
            return;
        };
        let handler = Rc::new(FileDropHandler::default());
        handler.set(f);
        entries.push((weak, handler));
    });
}

/// Releases every handler. Called on the shutdown path, before XAML's cleanup.
pub(crate) fn release_handlers() {
    // 借用の外で捨てる。
    drop(WIDGETS.with(Registry::take));
}

fn lookup(object: &DependencyObject) -> Option<Rc<FileDropHandler>> {
    WIDGETS.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(weak, _)| weak.upgrade().is_some_and(|other| same(&other, object)))
            .map(|(_, handler)| handler.clone())
    })
}

/// Who takes a drop that started at `source`: the nearest registered widget among
/// its ancestors, or the window.
fn target(source: &IInspectable, window: &Rc<FileDropHandler>) -> Option<Rc<FileDropHandler>> {
    let mut current = source.cast::<DependencyObject>().ok();
    while let Some(object) = current {
        if let Some(handler) = lookup(&object) {
            return Some(handler);
        }
        current = VisualTreeHelper::GetParent(&object).ok();
    }
    window.is_set().then(|| window.clone())
}

/// Whether the drag carries files (and not, say, text).
fn carries_files(args: &DragEventArgs) -> bool {
    let Ok(format) = StandardDataFormats::StorageItems() else {
        return false;
    };
    args.DataView()
        .and_then(|view| view.Contains(&format))
        .unwrap_or(false)
}

/// Reads the dropped items as entries carrying absolute paths. Items without a
/// path (virtual files such as the inside of a zip shown by Explorer) are left out.
async fn read_files(view: DataPackageView) -> Vec<FileEntry> {
    let Ok(operation) = view.GetStorageItemsAsync() else {
        return Vec::new();
    };
    let Ok(items) = operation.await else {
        return Vec::new();
    };
    let count = items.Size().unwrap_or(0);
    (0..count)
        .filter_map(|i| items.GetAt(i).ok())
        .filter_map(|item| item.Path().ok())
        .filter(|path| !path.is_empty())
        .map(|path| FileEntry::from_path(path.to_string()))
        .collect()
}

/// Attaches the drop receiver to the window's root. `window` is the window's
/// handler, and `tasks` runs the reading of the files on the UI thread.
pub(crate) fn install(
    root: &UIElement,
    window: Rc<FileDropHandler>,
    tasks: Tasks,
) -> windows_core::Result<()> {
    root.SetAllowDrop(true)?;
    let window = Arc::new(UiThreadCell::new(window));

    let over = DragEventHandler::new({
        let window = window.clone();
        move |_, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            if !carries_files(args) {
                return Ok(());
            }
            let Some(window) = window.try_with_mut(|window| window.clone()) else {
                return Ok(());
            };
            // 受け取り手がいなければ断る (ポインターに禁止の印が出る)。
            let operation = if target(&args.OriginalSource()?, &window).is_some() {
                DataPackageOperation::Copy
            } else {
                DataPackageOperation::None
            };
            args.SetAcceptedOperation(operation)?;
            args.SetHandled(true)?;
            Ok(())
        }
    });
    root.DragOver(&over)?;

    let tasks = UiThreadCell::new(tasks);
    let drop = DragEventHandler::new(move |_, args| {
        let Some(args) = args.as_ref() else {
            return Ok(());
        };
        if !carries_files(args) {
            return Ok(());
        }
        let Some(window) = window.try_with_mut(|window| window.clone()) else {
            return Ok(());
        };
        let Some(handler) = target(&args.OriginalSource()?, &window) else {
            return Ok(());
        };
        let Some(tasks) = tasks.try_with_mut(|tasks| tasks.clone()) else {
            return Ok(());
        };
        args.SetHandled(true)?;
        let view = args.DataView()?;
        // 読み終えるまで、落とした操作を終わらせずに待たせる。
        let deferral = args.GetDeferral()?;
        tasks.spawn(async move {
            let files = read_files(view).await;
            let _ = deferral.Complete();
            if !files.is_empty() {
                handler.emit(&files);
            }
        });
        Ok(())
    });
    root.Drop(&drop)?;
    Ok(())
}
