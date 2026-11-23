//! Receiving files dropped from Explorer (`AllowDrop` with `DragOver` / `Drop`).
//!
//! As with `on_key_down` (see `keys`), one pair of handlers on the window's root
//! does the work. Subscribing on each element instead would leave the event tokens
//! to be managed per element.
//!
//! Who is under the pointer is worked out here rather than taken from the event.
//! naui's process is not DPI aware, and on a display scaled above 100% XAML mixes
//! up logical and physical coordinates for drags: `DragEventArgs::GetPosition` is
//! off, and so is XAML's own hit-testing behind `OriginalSource`. So the pointer's
//! position is read from the cursor, the element there is found by hit-testing the
//! root, and walking up its parents finds the innermost registered widget, and
//! failing that the window.
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
//!
//! `drop_files` on widgets and windows (for tests and automation) goes through the
//! same routing ([`route`]), starting from the widget's own element instead of the
//! element under the pointer. The files are delivered as given, on the spot.

use std::cell::RefCell;
use std::rc::{Rc, Weak as RcWeak};
use std::sync::Arc;

use naui_core::{FileEntry, Tasks};
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{
    DependencyObject, DragEventArgs, DragEventHandler, FrameworkElement, UIElement,
};
use windows::ApplicationModel::DataTransfer::{
    DataPackageOperation, DataPackageView, StandardDataFormats,
};
use windows::Foundation::Point;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows_collections::IIterable;
use windows_core::{Interface, Weak};

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

    pub(crate) fn emit(&self, files: &[FileEntry]) {
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

/// A window's root and the window's handler. The window holds the handler, so it is
/// held weakly here.
type Roots = Registry<(Weak<UIElement>, RcWeak<FileDropHandler>)>;

thread_local! {
    static WIDGETS: Entries = const { Registry::new() };
    /// The roots [`install`] attached the receiver to, so that routing from a
    /// widget's element finds the window's handler as the real drop does.
    static ROOTS: Roots = const { Registry::new() };
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
    drop(ROOTS.with(Registry::take));
}

/// Remembers that `root` belongs to the window whose handler is `window`. Besides
/// the window's XAML root, the layer holding its content is registered, so that a
/// walk through naui's own record of places (which ends there) still finds the
/// window.
pub(crate) fn register_root(root: &UIElement, window: &Rc<FileDropHandler>) {
    ROOTS.with(|roots| {
        let mut roots = roots.borrow_mut();
        roots.retain(|(weak, handler)| {
            handler.strong_count() > 0 && weak.upgrade().is_some_and(|other| !same(&other, root))
        });
        if let Ok(weak) = root.downgrade() {
            roots.push((weak, Rc::downgrade(window)));
        }
    });
}

/// The handler of the window whose root is `object`, if it is one.
fn root_handler(object: &DependencyObject) -> Option<Option<Rc<FileDropHandler>>> {
    ROOTS.with(|roots| {
        roots
            .borrow()
            .iter()
            .find(|(weak, _)| weak.upgrade().is_some_and(|other| same(&other, object)))
            .map(|(_, handler)| handler.upgrade())
    })
}

/// Drops `files` onto the widget whose element is `element`, as a real drop onto it
/// would go: to its own handler, or else the nearest ancestor's, or else its
/// window's (`Widget::drop_files`). Delivered on the spot. Nothing happens if `files`
/// is empty or nothing takes them.
pub(crate) fn drop_on(element: &UIElement, files: &[FileEntry]) {
    if files.is_empty() {
        return;
    }
    // 根までたどれないとき (ダイアログの中など、窓の根の下に無いもの) は、
    // 同じ `XamlRoot` の中身を根とみなして窓へ渡す。
    let window = element
        .XamlRoot()
        .and_then(|xaml_root| xaml_root.Content())
        .ok()
        .and_then(|root| root.cast::<DependencyObject>().ok())
        .and_then(|root| root_handler(&root))
        .flatten();
    let start = element.cast::<DependencyObject>().ok();
    if let Some(handler) = route(start, window) {
        handler.emit(files);
    }
}

/// Drops `files` onto the window whose handler is `window`, where no widget takes
/// them (`Window::drop_files`). Delivered on the spot.
pub(crate) fn drop_on_window(window: &FileDropHandler, files: &[FileEntry]) {
    if !files.is_empty() {
        window.emit(files);
    }
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

/// Who takes a drag over `root` (the content of the window `hwnd`): the nearest
/// registered widget among the ancestors of the element under the pointer, or the
/// window.
fn target(
    root: &UIElement,
    hwnd: HWND,
    window: &Rc<FileDropHandler>,
) -> Option<Rc<FileDropHandler>> {
    let start = pointer_position(root, hwnd)
        .and_then(|point| element_at(point, root))
        .and_then(|element| element.cast::<DependencyObject>().ok());
    route(start, Some(window.clone()))
}

/// Who takes a drop that lands on `start`: the nearest registered widget among
/// `start` and its ancestors, or else the window. The window is the one whose root
/// the walk reaches, or `window` when it reaches none.
///
/// The real drop and `drop_files` both come here. Parents are followed in the
/// visual tree, and where that has none yet (the content of a control whose
/// template has not been applied), through `FrameworkElement::Parent`. In the live
/// tree the visual parent is always there, so the real drop walks the visual tree
/// alone.
fn route(
    start: Option<DependencyObject>,
    window: Option<Rc<FileDropHandler>>,
) -> Option<Rc<FileDropHandler>> {
    let mut window = window;
    let mut current = start;
    while let Some(object) = current {
        if let Some(handler) = lookup(&object) {
            return Some(handler);
        }
        if let Some(root) = root_handler(&object) {
            // 窓の根に着いた。その窓の受け口が最後の受け取り手。
            window = root;
            break;
        }
        current = parent(&object);
    }
    window.filter(|window| window.is_set())
}

/// `object`'s parent: the visual one, or else the logical one, or else the
/// container naui put it in. A window nothing has drawn yet (as on CI runners)
/// reports neither of the first two, so `drop_files` walks up through naui's own
/// record there.
fn parent(object: &DependencyObject) -> Option<DependencyObject> {
    VisualTreeHelper::GetParent(object)
        .ok()
        .or_else(|| {
            object
                .cast::<FrameworkElement>()
                .and_then(|element| element.Parent())
                .ok()
        })
        .or_else(|| {
            object
                .cast::<UIElement>()
                .ok()
                .and_then(|element| crate::layout::placed_in(&element))
        })
}

/// The pointer's position in `root`'s coordinates (DIPs), read from the cursor.
///
/// `root` is the window's content, so it starts at the client area's origin.
fn pointer_position(root: &UIElement, hwnd: HWND) -> Option<Point> {
    let mut point = POINT::default();
    // SAFETY: 書き込み先として手元の POINT を渡すだけ。
    unsafe { GetCursorPos(&mut point) }.ok()?;
    // SAFETY: この窓の HWND と、上で埋めた POINT を渡すだけ。
    if !unsafe { ScreenToClient(hwnd, &mut point) }.as_bool() {
        return None;
    }
    // DPI に対応していないあいだは 1.0 で、クライアント座標がそのまま DIP。
    let scale = root
        .XamlRoot()
        .and_then(|xaml_root| xaml_root.RasterizationScale())
        .ok()
        .filter(|scale| *scale > 0.0)
        .unwrap_or(1.0);
    Some(Point {
        X: (f64::from(point.x) / scale) as f32,
        Y: (f64::from(point.y) / scale) as f32,
    })
}

/// The innermost element in `root` at `point`.
///
/// Hit-testing alone misses parts of widgets that are hit-testable only where they
/// draw (a `TextBlock` away from its glyphs, a panel without a background), and the
/// drop would go to the element behind. Bounds alone would see through what is
/// stacked on top (an overlay layer). So the topmost hit-testable element decides
/// the subtree, and within it the innermost element whose bounds contain the point
/// is taken (as on the other platforms, which test against widget frames).
fn element_at(point: Point, root: &UIElement) -> Option<UIElement> {
    let top = first(&VisualTreeHelper::FindElementsInHostCoordinatesPoint(point, root).ok()?)?;
    let all = VisualTreeHelper::FindAllElementsInHostCoordinatesPoint(point, root, true).ok()?;
    // どちらも手前のものから並ぶので、`top` の中で最初に出てくるものが一番内側。
    let iterator = all.First().ok()?;
    while iterator.HasCurrent().unwrap_or(false) {
        if let Ok(element) = iterator.Current() {
            if is_within(&element, &top) {
                return Some(element);
            }
        }
        if !iterator.MoveNext().unwrap_or(false) {
            break;
        }
    }
    Some(top)
}

fn first(elements: &IIterable<UIElement>) -> Option<UIElement> {
    let iterator = elements.First().ok()?;
    if !iterator.HasCurrent().ok()? {
        return None;
    }
    iterator.Current().ok()
}

/// Whether `element` is `ancestor` or inside it.
fn is_within(element: &UIElement, ancestor: &UIElement) -> bool {
    let mut current = element.cast::<DependencyObject>().ok();
    while let Some(object) = current {
        if same(&object, ancestor) {
            return true;
        }
        current = VisualTreeHelper::GetParent(&object).ok();
    }
    false
}

/// The element the handlers are attached to (the window's root), from the event's
/// sender. Capturing the root in its own handlers would keep both alive.
fn sender_element(sender: &windows_core::Ref<windows_core::IInspectable>) -> Option<UIElement> {
    sender.as_ref().and_then(|sender| sender.cast().ok())
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

/// Attaches the drop receiver to the window's root. `hwnd` is the window's, `window`
/// is the window's handler, and `tasks` runs the reading of the files on the UI
/// thread.
pub(crate) fn install(
    root: &UIElement,
    hwnd: HWND,
    window: Rc<FileDropHandler>,
    tasks: Tasks,
) -> windows_core::Result<()> {
    root.SetAllowDrop(true)?;
    register_root(root, &window);
    let window = Arc::new(UiThreadCell::new((window, hwnd)));

    let over = DragEventHandler::new({
        let window = window.clone();
        move |sender, args| {
            let (Some(root), Some(args)) = (sender_element(&sender), args.as_ref()) else {
                return Ok(());
            };
            if !carries_files(args) {
                return Ok(());
            }
            let Some((window, hwnd)) = window.try_with_mut(|window| window.clone()) else {
                return Ok(());
            };
            // 受け取り手がいなければ断る (ポインターに禁止の印が出る)。
            let operation = if target(&root, hwnd, &window).is_some() {
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
    let drop = DragEventHandler::new(move |sender, args| {
        let (Some(root), Some(args)) = (sender_element(&sender), args.as_ref()) else {
            return Ok(());
        };
        if !carries_files(args) {
            return Ok(());
        }
        let Some((window, hwnd)) = window.try_with_mut(|window| window.clone()) else {
            return Ok(());
        };
        let Some(handler) = target(&root, hwnd, &window) else {
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
