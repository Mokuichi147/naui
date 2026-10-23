//! The toolbar (AppKit's `NSToolbar`).
//!
//! `NSToolbar` is not an `NSView` but **something attached to an `NSWindow`**, so
//! it cannot be placed in a layout like other widgets. So [`Toolbar`] is not a
//! [`Widget`](crate::Widget); it is attached with
//! [`Window::set_toolbar`](crate::Window::set_toolbar). Its look, merging with the
//! title bar, and overflow when items do not fit are all done by AppKit.
//!
//! Items are `NSToolbarItem`s with identifiers `naui.item.<index>`. Icons are
//! [`ToolbarIcon`](naui_core::ToolbarIcon) mapped to SF Symbols
//! (`NSImage::imageWithSystemSymbolName`), and `label` is used for screen readers
//! and the overflow menu when items do not fit. Following macOS conventions,
//! `ToolbarItem::separator()` maps to `NSToolbarSpaceItemIdentifier` (a fixed-width
//! space).
//!
//! While a [`Sidebar`](crate::Sidebar) is attached to the window, AppKit's standard
//! sidebar button (`NSToolbarToggleSidebarItemIdentifier`) and a separator that
//! moves the following items above the content pane
//! (`NSToolbarSidebarTrackingSeparatorItemIdentifier`) are inserted at the start.
//! Both are items AppKit creates, so they are not counted in naui's item indices.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::ToolbarItem;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSImage, NSToolbar, NSToolbarDelegate, NSToolbarDisplayMode, NSToolbarItem,
    NSToolbarItemIdentifier, NSToolbarSidebarTrackingSeparatorItemIdentifier,
    NSToolbarSpaceItemIdentifier, NSToolbarToggleSidebarItemIdentifier,
};
use objc2_foundation::{NSArray, NSString};

use crate::trampoline::{ActionTarget, SelectHandler};

/// The running number appended to toolbar identifiers.
static NEXT_TOOLBAR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The prefix of item identifiers. The index is appended.
const ITEM_PREFIX: &str = "naui.item.";

struct ToolbarInner {
    native: Retained<NSToolbar>,
    items: RefCell<Vec<ToolbarItem>>,
    /// The created `NSToolbarItem`s. `None` at separators.
    natives: RefCell<Vec<Option<Retained<NSToolbarItem>>>>,
    /// The trampolines for clicks. `NSToolbarItem` targets are weak, so they are held.
    targets: RefCell<Vec<Retained<ActionTarget>>>,
    handler: SelectHandler,
    /// Whether the whole toolbar is enabled. ANDed with the per-item settings.
    enabled: Cell<bool>,
    /// `NSToolbar`'s delegate is weak, so it is held here.
    delegate: RefCell<Option<Retained<ToolbarDelegate>>>,
    /// Whether to insert the sidebar button and separator at the start.
    sidebar_controls: Cell<bool>,
}

impl ToolbarInner {
    /// The identifiers of the sequence including separators. Exactly the order passed
    /// to `NSToolbar`.
    fn identifiers(&self) -> Retained<NSArray<NSToolbarItemIdentifier>> {
        let mut ids: Vec<Retained<NSString>> = Vec::new();
        if self.sidebar_controls.get() {
            unsafe {
                ids.push(NSToolbarToggleSidebarItemIdentifier.retain());
                ids.push(NSToolbarSidebarTrackingSeparatorItemIdentifier.retain());
            }
        }
        ids.extend(self.items.borrow().iter().enumerate().map(|(index, item)| {
            if item.is_separator() {
                unsafe { NSToolbarSpaceItemIdentifier }.retain()
            } else {
                NSString::from_str(&format!("{ITEM_PREFIX}{index}"))
            }
        }));
        NSArray::from_retained_slice(&ids)
    }

    /// Maps an identifier back to an item index. `None` for separators or anything
    /// else.
    fn index_of(identifier: &NSToolbarItemIdentifier) -> Option<usize> {
        identifier
            .to_string()
            .strip_prefix(ITEM_PREFIX)
            .and_then(|rest| rest.parse().ok())
    }

    /// Creates the `NSToolbarItem` for the index.
    ///
    /// `NSToolbar` requires "a new item to be returned on every call", so nothing is
    /// remembered here; the real items are picked up again with
    /// [`Toolbar::collect_items`] after insertion.
    fn make_item(self: &Rc<Self>, index: usize) -> Option<Retained<NSToolbarItem>> {
        let mtm = MainThreadMarker::from(&*self.native);
        let item = self.items.borrow().get(index)?.clone();
        if item.is_separator() {
            return None;
        }

        let identifier = NSString::from_str(&format!("{ITEM_PREFIX}{index}"));
        let native = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &identifier);
        let label = NSString::from_str(&item.label);
        // ラベルは読み上げと、送り出しメニュー・カスタマイズ画面の文字に使う。
        // `title` は入れない (アイコンと二重に文字が出るため)。
        native.setLabel(&label);
        native.setPaletteLabel(&label);
        native.setToolTip(Some(&label));
        // 見た目は SF Symbols のアイコン。無い記号名なら AppKit が nil を返す。
        let symbol = NSString::from_str(item.icon.sf_symbol());
        let image =
            NSImage::imageWithSystemSymbolName_accessibilityDescription(&symbol, Some(&label));
        native.setImage(image.as_deref());
        // 既定では AppKit が `validateToolbarItem:` で有効・無効を決めてしまい、
        // `setEnabled` が上書きされる。naui はアプリの指定をそのまま使う。
        native.setAutovalidates(false);
        native.setEnabled(item.enabled && self.enabled.get());

        // ハンドルを強く持つとトランポリンとの間で循環するため、弱参照にする。
        let target = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(self);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.handler.emit(index);
            }
        });
        unsafe {
            native.setTarget(Some(&target));
            native.setAction(Some(sel!(invoke:)));
        }
        self.targets.borrow_mut().push(target);
        Some(native)
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiToolbarDelegate"]
    #[ivars = Weak<ToolbarInner>]
    struct ToolbarDelegate;

    unsafe impl NSObjectProtocol for ToolbarDelegate {}

    unsafe impl NSToolbarDelegate for ToolbarDelegate {
        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn item_for_identifier(
            &self,
            _toolbar: &NSToolbar,
            identifier: &NSToolbarItemIdentifier,
            _inserted: bool,
        ) -> Option<Retained<NSToolbarItem>> {
            // マクロが body を包むため、早期 return ではなく式で書く。
            self.ivars()
                .upgrade()
                .zip(ToolbarInner::index_of(identifier))
                .and_then(|(inner, index)| inner.make_item(index))
        }

        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn default_identifiers(
            &self,
            _toolbar: &NSToolbar,
        ) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            match self.ivars().upgrade() {
                Some(inner) => inner.identifiers(),
                None => NSArray::new(),
            }
        }

        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn allowed_identifiers(
            &self,
            _toolbar: &NSToolbar,
        ) -> Retained<NSArray<NSToolbarItemIdentifier>> {
            match self.ivars().upgrade() {
                Some(inner) => inner.identifiers(),
                None => NSArray::new(),
            }
        }
    }
);

impl ToolbarDelegate {
    fn new(mtm: MainThreadMarker, inner: &Rc<ToolbarInner>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Rc::downgrade(inner));
        unsafe { msg_send![super(this), init] }
    }
}

/// A row of frequently used actions attached to the top of the window
/// (`NSToolbar`).
///
/// Not a [`Widget`](crate::Widget). Attach it with
/// [`Window::set_toolbar`](crate::Window::set_toolbar). Unlike navigation, it
/// **has no selected item**; every click calls [`on_activate`](Self::on_activate)
/// with the index. The index is the position in the sequence including separators;
/// separators are never returned.
#[derive(Clone)]
pub struct Toolbar(Rc<ToolbarInner>);

impl Toolbar {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        // 識別子はツールバーごとに変える。AppKit は同じ識別子の `NSToolbar` を
        // 「ファミリー」として項目の出し入れを同期するので、共有すると別の
        // ツールバー (別のウィンドウのものや、サイドバーボタンだけのもの) の
        // 組み替えが伝わって、項目の位置が食い違う。
        let serial = NEXT_TOOLBAR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let native = NSToolbar::initWithIdentifier(
            NSToolbar::alloc(mtm),
            &NSString::from_str(&format!("naui.toolbar.{serial}")),
        );
        // naui は項目をインデックスで識別するため、利用者による並べ替えや
        // 出し入れは受け付けない (順序が変わると通知の意味が変わる)。
        native.setAllowsUserCustomization(false);
        native.setAutosavesConfiguration(false);
        // macOS のツールバーはアイコンだけを並べるのが既定の姿。
        // ラベルはツールチップと送り出しメニューに出る。
        native.setDisplayMode(NSToolbarDisplayMode::IconOnly);

        let inner = Rc::new(ToolbarInner {
            native,
            items: RefCell::new(Vec::new()),
            natives: RefCell::new(Vec::new()),
            targets: RefCell::new(Vec::new()),
            handler: SelectHandler::default(),
            enabled: Cell::new(true),
            delegate: RefCell::new(None),
            sidebar_controls: Cell::new(false),
        });

        let delegate = ToolbarDelegate::new(mtm, &inner);
        inner
            .native
            .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        *inner.delegate.borrow_mut() = Some(delegate);
        Self(inner)
    }

    /// Rebuilds the items. The previous items are removed.
    ///
    /// The index is the position in the sequence including separators.
    pub fn set_items(&self, items: &[ToolbarItem]) {
        self.rebuild(items);
    }

    /// Switches whether to insert the sidebar button and separator at the start.
    ///
    /// Called by [`Window`](crate::Window) as a sidebar is attached or detached.
    ///
    /// AppKit does not insert the sidebar separator
    /// (`NSToolbarSidebarTrackingSeparatorItemIdentifier`) into a toolbar not attached
    /// to a window. So if the sequence does not match, it is rearranged on every call
    /// (calling it after attaching to the window inserts it).
    pub(crate) fn set_sidebar_controls(&self, on: bool) {
        self.0.sidebar_controls.set(on);
        let expected: Vec<String> = self
            .0
            .identifiers()
            .iter()
            .map(|id| id.to_string())
            .collect();
        let current: Vec<String> = self
            .0
            .native
            .items()
            .iter()
            .map(|item| item.itemIdentifier().to_string())
            .collect();
        if expected != current {
            let items = self.0.items.borrow().clone();
            self.rebuild(&items);
        }
    }

    /// Whether it is a handle to the same toolbar (clones count as the same).
    pub(crate) fn is_same(&self, other: &Toolbar) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    fn rebuild(&self, items: &[ToolbarItem]) {
        self.0.items.borrow_mut().clear();
        self.0.items.borrow_mut().extend_from_slice(items);
        *self.0.natives.borrow_mut() = vec![None; items.len()];
        self.0.targets.borrow_mut().clear();

        // 並べ替えると識別子とインデックスの対応が変わるので、いったん空にする。
        while !self.0.native.items().is_empty() {
            self.0.native.removeItemAtIndex(0);
        }
        let identifiers = self.0.identifiers();
        for index in 0..identifiers.len() {
            // 挿入のたびに delegate が呼ばれ、`NSToolbarItem` が作られる。
            // 入れる位置は「いまの末尾」。AppKit が受け付けない項目 (ウィンドウに
            // 付いていないときのサイドバー用の区切り) があっても、後ろがずれない。
            let end = self.0.native.items().len();
            self.0.native.insertItemWithItemIdentifier_atIndex(
                &identifiers.objectAtIndex(index),
                end as isize,
            );
        }
        self.collect_items();
        // 実物が揃ってから、項目ごとの有効・無効を反映する。
        self.apply_enabled();
    }

    /// Picks up the items the `NSToolbar` currently holds again, per index.
    ///
    /// What the delegate returned and what the toolbar actually uses are not
    /// necessarily the same (AppKit sometimes makes copies). So that enabled/disabled
    /// is not applied to the wrong object, the real items are remembered after
    /// insertion finishes.
    fn collect_items(&self) {
        let items = self.0.native.items();
        let mut natives = self.0.natives.borrow_mut();
        for position in 0..items.len() {
            let item = items.objectAtIndex(position);
            let Some(index) = ToolbarInner::index_of(&item.itemIdentifier()) else {
                // 区切り (space 項目) は覚えない。
                continue;
            };
            if let Some(slot) = natives.get_mut(index) {
                *slot = Some(item);
            }
        }
    }

    /// The number of items, separators included.
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Enables or disables one item. Does nothing for separators or out of range.
    pub fn set_item_enabled(&self, index: usize, enabled: bool) {
        let mut items = self.0.items.borrow_mut();
        let Some(item) = items.get_mut(index) else {
            return;
        };
        if item.is_separator() {
            return;
        }
        item.enabled = enabled;
        drop(items);
        self.apply_enabled();
    }

    /// Whether the item can be clicked now. `false` for separators and out of range.
    pub fn is_item_enabled(&self, index: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .items
                .borrow()
                .get(index)
                .is_some_and(|item| !item.is_separator() && item.enabled)
    }

    /// Enables or disables the whole toolbar. Per-item settings are kept.
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// Applies the per-item settings and the overall setting to the native side.
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        let items = self.0.items.borrow();
        for (native, item) in self.0.natives.borrow().iter().zip(items.iter()) {
            if let Some(native) = native {
                native.setEnabled(item.enabled && whole);
            }
        }
    }

    /// Runs the item as if the user had clicked it.
    ///
    /// Does nothing for separators, disabled items or out of range.
    pub fn activate(&self, index: usize) {
        if self.is_item_enabled(index) {
            self.0.handler.emit(index);
        }
    }

    /// Called with the index when an item is clicked. Setting it again removes the
    /// previous callback.
    pub fn on_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// The real AppKit toolbar. Public as a backend-specific escape hatch.
    pub fn native_toolbar(&self) -> Retained<NSToolbar> {
        self.0.native.clone()
    }

    /// The `NSToolbarItem` for the item. `None` for separators and out of range. Public
    /// as a backend-specific escape hatch.
    pub fn native_item(&self, index: usize) -> Option<Retained<NSToolbarItem>> {
        self.0.natives.borrow().get(index)?.clone()
    }
}
