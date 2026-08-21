//! ツールバー (AppKit の `NSToolbar`)。
//!
//! `NSToolbar` は `NSView` ではなく **`NSWindow` に取り付けるもの**なので、
//! ほかのウィジェットのようにレイアウトへ載せることはできない。そのため
//! [`Toolbar`] は [`Widget`](crate::Widget) ではなく、
//! [`Window::set_toolbar`](crate::Window::set_toolbar) で取り付ける。
//! 見た目・タイトルバーとの一体化・項目が入りきらないときの送り出しは
//! すべて AppKit が行う。
//!
//! 項目は `NSToolbarItem` で、識別子は `naui.item.<インデックス>`。
//! アイコンは [`ToolbarIcon`](naui_core::ToolbarIcon) を SF Symbols
//! (`NSImage::imageWithSystemSymbolName`) へ写したもので、`label` は
//! 読み上げと、項目が入りきらないときの送り出しメニューに使う。
//! `ToolbarItem::separator()` は macOS の作法にならって
//! `NSToolbarSpaceItemIdentifier` (一定幅の空き) へ写す。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::ToolbarItem;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSImage, NSToolbar, NSToolbarDelegate, NSToolbarDisplayMode, NSToolbarItem,
    NSToolbarItemIdentifier, NSToolbarSpaceItemIdentifier,
};
use objc2_foundation::{NSArray, NSString};

use crate::trampoline::{ActionTarget, SelectHandler};

/// 項目の識別子の頭。後ろにインデックスが付く。
const ITEM_PREFIX: &str = "naui.item.";

struct ToolbarInner {
    native: Retained<NSToolbar>,
    items: RefCell<Vec<ToolbarItem>>,
    /// 生成した `NSToolbarItem`。区切りのところは `None`。
    natives: RefCell<Vec<Option<Retained<NSToolbarItem>>>>,
    /// 押されたときのトランポリン。`NSToolbarItem` の target は weak なので保持する。
    targets: RefCell<Vec<Retained<ActionTarget>>>,
    handler: SelectHandler,
    /// ツールバー全体の有効・無効。項目ごとの指定と AND を取る。
    enabled: Cell<bool>,
    /// `NSToolbar` の delegate は weak なので、ここで保持する。
    delegate: RefCell<Option<Retained<ToolbarDelegate>>>,
}

impl ToolbarInner {
    /// 区切りを含めた並びの識別子。`NSToolbar` へ渡す順序そのもの。
    fn identifiers(&self) -> Retained<NSArray<NSToolbarItemIdentifier>> {
        let ids: Vec<Retained<NSString>> = self
            .items
            .borrow()
            .iter()
            .enumerate()
            .map(|(index, item)| {
                if item.is_separator() {
                    unsafe { NSToolbarSpaceItemIdentifier }.retain()
                } else {
                    NSString::from_str(&format!("{ITEM_PREFIX}{index}"))
                }
            })
            .collect();
        NSArray::from_retained_slice(&ids)
    }

    /// 識別子から項目のインデックスへ戻す。区切りや別物なら `None`。
    fn index_of(identifier: &NSToolbarItemIdentifier) -> Option<usize> {
        identifier
            .to_string()
            .strip_prefix(ITEM_PREFIX)
            .and_then(|rest| rest.parse().ok())
    }

    /// インデックスに対応する `NSToolbarItem` を作る。
    ///
    /// `NSToolbar` は「呼ばれるたびに新しい項目を返すこと」を求めるので、
    /// ここでは覚えず、挿入後に [`Toolbar::collect_items`] で実物を拾い直す。
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

/// ウィンドウの上端に付く、よく使う操作の並び (`NSToolbar`)。
///
/// [`Widget`](crate::Widget) ではない。
/// [`Window::set_toolbar`](crate::Window::set_toolbar) で取り付ける。
/// ナビゲーションと違い**選ばれている項目を持たず**、押されるたびに
/// そのインデックスで [`on_activate`](Self::on_activate) が呼ばれる。
/// インデックスは区切りを含めた並びの位置で、区切りが返ることはない。
#[derive(Clone)]
pub struct Toolbar(Rc<ToolbarInner>);

impl Toolbar {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSToolbar::initWithIdentifier(
            NSToolbar::alloc(mtm),
            &NSString::from_str("naui.toolbar"),
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
        });

        let delegate = ToolbarDelegate::new(mtm, &inner);
        inner
            .native
            .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        *inner.delegate.borrow_mut() = Some(delegate);
        Self(inner)
    }

    /// 項目を作り直す。以前の項目は取り除かれる。
    ///
    /// インデックスは区切りを含めた並びの位置。
    pub fn set_items(&self, items: &[ToolbarItem]) {
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
            self.0.native.insertItemWithItemIdentifier_atIndex(
                &identifiers.objectAtIndex(index),
                index as isize,
            );
        }
        self.collect_items();
        // 実物が揃ってから、項目ごとの有効・無効を反映する。
        self.apply_enabled();
    }

    /// `NSToolbar` がいま持っている項目を、インデックスごとに拾い直す。
    ///
    /// delegate が返したものと、ツールバーが実際に使うものは同じとは限らない
    /// (AppKit は複製を作ることがある)。有効・無効の反映先を取り違えないよう、
    /// 挿入が終わってから実物を覚える。
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

    /// 区切りを含めた項目数。
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 項目 1 つの有効・無効を変える。区切りと範囲外は何もしない。
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

    /// いま押せる項目か。区切りと範囲外は `false`。
    pub fn is_item_enabled(&self, index: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .items
                .borrow()
                .get(index)
                .is_some_and(|item| !item.is_separator() && item.enabled)
    }

    /// ツールバー全体の有効・無効を変える。項目ごとの指定は残る。
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// 項目ごとの指定と全体の指定をネイティブへ反映する。
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        let items = self.0.items.borrow();
        for (native, item) in self.0.natives.borrow().iter().zip(items.iter()) {
            if let Some(native) = native {
                native.setEnabled(item.enabled && whole);
            }
        }
    }

    /// 利用者が押したのと同じように項目を実行する。
    ///
    /// 区切り・押せない項目・範囲外は何もしない。
    pub fn activate(&self, index: usize) {
        if self.is_item_enabled(index) {
            self.0.handler.emit(index);
        }
    }

    /// 項目が押されたときに、そのインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// AppKit の実ツールバー。バックエンド固有の脱出口として公開している。
    pub fn native_toolbar(&self) -> Retained<NSToolbar> {
        self.0.native.clone()
    }

    /// 項目に対応する `NSToolbarItem`。区切りと範囲外は `None`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_item(&self, index: usize) -> Option<Retained<NSToolbarItem>> {
        self.0.natives.borrow().get(index)?.clone()
    }
}
