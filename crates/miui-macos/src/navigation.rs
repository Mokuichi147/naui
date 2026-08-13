//! ナビゲーション系のハンドル群 (AppKit)。
//!
//! タブ・ナビバー・ドック・メニュー・パンくず・ページネーション・リンクは、
//! どれも「項目の並び + いま選ばれているもの」という同じ構造を持つ。
//! そのため公開 API の形をそろえてあり、AppKit 側では
//!
//! | miui | AppKit |
//! | --- | --- |
//! | `Tabs` | `NSTabView` + `NSTabViewItem` |
//! | `Navbar` | `NSTextField` (見出し) + `NSSegmentedControl` |
//! | `Dock` | `NSSegmentedControl` (等幅) |
//! | `Menu` | `NSButton` (AccessoryBar) の縦並び |
//! | `Breadcrumbs` | `NSPathControl` + `NSPathControlItem` |
//! | `Pagination` | `NSButton` + `NSSegmentedControl` |
//! | `Link` | `NSButton` (枠なし・リンク色) + `NSWorkspace` |
//!
//! に対応させている。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use miui_core::NavItem;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{
    NSBezelStyle, NSButton, NSButtonType, NSColor, NSControlStateValueOff, NSControlStateValueOn,
    NSLayoutAttribute, NSPathControl, NSPathControlItem, NSPathStyle, NSSegmentDistribution,
    NSSegmentStyle, NSSegmentSwitchTracking, NSSegmentedControl, NSStackView,
    NSStackViewDistribution, NSTabView, NSTabViewItem, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSWorkspace,
};
use objc2_foundation::{NSArray, NSString, NSURL};

use crate::trampoline::{ActionTarget, SelectHandler, TabObserver};
use crate::widgets::{impl_widget, Widget};

/// 横並びのスタックを作る。ナビゲーション系の合成に使う。
fn row(mtm: MainThreadMarker, spacing: f64) -> Retained<NSStackView> {
    let stack = NSStackView::new(mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    stack.setDistribution(NSStackViewDistribution::Fill);
    stack.setAlignment(NSLayoutAttribute::CenterY);
    stack.setSpacing(spacing);
    stack
}

// --------------------------------------------------------------- Segments

/// `NSSegmentedControl` を包む内部ハンドル。
///
/// ナビバー・ドック・ページネーションが共有する。`Rc` なので
/// ボタンのクロージャへ clone して渡せる。
#[derive(Clone)]
struct Segments(Rc<SegmentsInner>);

struct SegmentsInner {
    native: Retained<NSSegmentedControl>,
    handler: SelectHandler,
    count: Cell<usize>,
    /// クリックを受けるトランポリン。AppKit の target は weak なので保持する。
    _target: Retained<ActionTarget>,
}

impl Segments {
    fn new(mtm: MainThreadMarker, distribution: NSSegmentDistribution) -> Self {
        let native = NSSegmentedControl::new(mtm);
        native.setSegmentStyle(NSSegmentStyle::Automatic);
        native.setTrackingMode(NSSegmentSwitchTracking::SelectOne);
        native.setSegmentDistribution(distribution);
        native.setSegmentCount(0);

        let handler = SelectHandler::default();
        let target = ActionTarget::new(mtm, {
            let native = native.clone();
            let handler = handler.clone();
            move || {
                let index = native.selectedSegment();
                if index >= 0 {
                    handler.emit(index as usize);
                }
            }
        });
        unsafe {
            native.setTarget(Some(&target));
            native.setAction(Some(sel!(invoke:)));
        }

        Self(Rc::new(SegmentsInner {
            native,
            handler,
            count: Cell::new(0),
            _target: target,
        }))
    }

    fn view(&self) -> Retained<NSView> {
        let view: &NSView = self.0.native.as_ref();
        view.retain()
    }

    fn set_items(&self, items: &[NavItem]) {
        let native = &self.0.native;
        native.setSegmentCount(items.len() as isize);
        for (i, item) in items.iter().enumerate() {
            native.setLabel_forSegment(&NSString::from_str(&item.label), i as isize);
            native.setEnabled_forSegment(item.enabled, i as isize);
        }
        self.0.count.set(items.len());
        // 項目が変わればインデックスの意味も変わるので、選択は必ず外す。
        native.setSelectedSegment(-1);
    }

    fn len(&self) -> usize {
        self.0.count.get()
    }

    fn selected(&self) -> Option<usize> {
        let index = self.0.native.selectedSegment();
        (index >= 0).then_some(index as usize)
    }

    /// 通知せずに選択を変える。
    fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.0.native.setSelectedSegment(index as isize);
        }
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    fn select(&self, index: usize) {
        if index < self.len() {
            self.0.native.setSelectedSegment(index as isize);
            self.0.handler.emit(index);
        }
    }

    fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

// ------------------------------------------------------------------- Tabs

struct TabsInner {
    native: Retained<NSTabView>,
    /// 子のハンドルを保持し、トランポリンごと生かしておく。
    children: RefCell<Vec<Box<dyn Widget>>>,
    handler: SelectHandler,
    /// `set_selected` の間だけ通知を止める。AppKit は
    /// プログラムからの選択でもデリゲートを呼ぶため。
    silent: Rc<Cell<bool>>,
    /// デリゲートは weak 参照なので保持する。
    _observer: Retained<TabObserver>,
}

/// タブ (NSTabView)。中身のウィジェットごと持つ。
#[derive(Clone)]
pub struct Tabs(Rc<TabsInner>);
impl_widget!(Tabs);

impl Tabs {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSTabView::new(mtm);
        let handler = SelectHandler::default();
        let silent = Rc::new(Cell::new(false));

        // デリゲートから来た選択を、通知を止めている間は捨てる。
        let bridge = SelectHandler::default();
        bridge.set({
            let handler = handler.clone();
            let silent = silent.clone();
            move |index| {
                if !silent.get() {
                    handler.emit(index);
                }
            }
        });
        let observer = TabObserver::new(mtm, bridge);
        native.setDelegate(Some(ProtocolObject::from_ref(&*observer)));

        Self(Rc::new(TabsInner {
            native,
            children: RefCell::new(Vec::new()),
            handler,
            silent,
            _observer: observer,
        }))
    }

    /// タブを 1 枚追加する。`child` がそのタブの中身になる。
    pub fn add_tab(&self, label: &str, child: &dyn Widget) {
        let item = NSTabViewItem::new();
        item.setLabel(&NSString::from_str(label));
        item.setView(Some(&child.native_view()));
        self.0.native.addTabViewItem(&item);
        self.0.children.borrow_mut().push(child.boxed_clone());
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        let item = self.0.native.selectedTabViewItem()?;
        let index = self.0.native.indexOfTabViewItem(&item);
        (index >= 0).then_some(index as usize)
    }

    /// 通知せずに選択を変える。
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.0.silent.set(true);
            self.0.native.selectTabViewItemAtIndex(index as isize);
            self.0.silent.set(false);
        }
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    pub fn select(&self, index: usize) {
        if index < self.len() {
            // 同じタブを選び直したときは AppKit がデリゲートを呼ばないため、
            // デリゲート経由の通知は止めて、ここで 1 回だけ通知する。
            self.0.silent.set(true);
            self.0.native.selectTabViewItemAtIndex(index as isize);
            self.0.silent.set(false);
            self.0.handler.emit(index);
        }
    }

    /// タブが切り替わったときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

// ----------------------------------------------------------------- Navbar

struct NavbarInner {
    native: Retained<NSStackView>,
    title: Retained<NSTextField>,
    segments: Segments,
}

/// 画面上部に置く横並びのナビゲーション。
///
/// 見出し (`NSTextField`) と項目 (`NSSegmentedControl`) を横スタックに並べる。
#[derive(Clone)]
pub struct Navbar(Rc<NavbarInner>);
impl_widget!(Navbar);

impl Navbar {
    pub(crate) fn new(mtm: MainThreadMarker, title: &str) -> Self {
        let native = row(mtm, 12.0);
        let title_field = NSTextField::labelWithString(&NSString::from_str(title), mtm);
        let segments = Segments::new(mtm, NSSegmentDistribution::Fit);
        native.addArrangedSubview(&title_field);
        native.addArrangedSubview(&segments.view());
        Self(Rc::new(NavbarInner {
            native,
            title: title_field,
            segments,
        }))
    }

    /// 左端の見出し。
    pub fn set_title(&self, title: &str) {
        self.0.title.setStringValue(&NSString::from_str(title));
    }

    pub fn title(&self) -> String {
        self.0.title.stringValue().to_string()
    }

    /// 項目を作り直す。インデックスの意味が変わるため、選択は外れる。
    pub fn set_items(&self, items: &[NavItem]) {
        self.0.segments.set_items(items);
    }

    pub fn len(&self) -> usize {
        self.0.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        self.0.segments.selected()
    }

    /// 通知せずに選択を変える。
    pub fn set_selected(&self, index: usize) {
        self.0.segments.set_selected(index);
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    pub fn select(&self, index: usize) {
        self.0.segments.select(index);
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.segments.on_select(f);
    }
}

// ------------------------------------------------------------------- Dock

struct DockInner {
    segments: Segments,
}

/// 画面下部に置く横並びのナビゲーション (等幅)。
///
/// **配置はアプリの責務**で、miui のレイアウトは縦横のスタックしか持たないため、
/// ウィンドウ下端への固定はできない。縦スタックの最後に置くと下端寄りになる。
#[derive(Clone)]
pub struct Dock(Rc<DockInner>);

impl Widget for Dock {
    fn native_view(&self) -> Retained<NSView> {
        self.0.segments.view()
    }
    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }
}

impl Dock {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let segments = Segments::new(mtm, NSSegmentDistribution::FillEqually);
        Self(Rc::new(DockInner { segments }))
    }

    /// 項目を作り直す。インデックスの意味が変わるため、選択は外れる。
    pub fn set_items(&self, items: &[NavItem]) {
        self.0.segments.set_items(items);
    }

    pub fn len(&self) -> usize {
        self.0.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        self.0.segments.selected()
    }

    /// 通知せずに選択を変える。
    pub fn set_selected(&self, index: usize) {
        self.0.segments.set_selected(index);
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    pub fn select(&self, index: usize) {
        self.0.segments.select(index);
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.segments.on_select(f);
    }
}

// ------------------------------------------------------------------- Menu

struct MenuInner {
    native: Retained<NSStackView>,
    buttons: RefCell<Vec<Retained<NSButton>>>,
    targets: RefCell<Vec<Retained<ActionTarget>>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
}

/// 縦に並ぶナビゲーション一覧。
///
/// macOS のサイドバーと同じく、選択状態を持つ枠なしボタン
/// (`NSButton` の AccessoryBar) を `NSStackView` に縦に並べる。
/// ポップアップの `NSMenu` ではない点に注意。
#[derive(Clone)]
pub struct Menu(Rc<MenuInner>);
impl_widget!(Menu);

impl Menu {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSStackView::new(mtm);
        native.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        native.setDistribution(NSStackViewDistribution::Fill);
        native.setAlignment(NSLayoutAttribute::Leading);
        native.setSpacing(2.0);
        Self(Rc::new(MenuInner {
            native,
            buttons: RefCell::new(Vec::new()),
            targets: RefCell::new(Vec::new()),
            handler: SelectHandler::default(),
            selected: Cell::new(None),
        }))
    }

    /// 項目を作り直す。以前の項目は取り除かれ、選択も外れる。
    pub fn set_items(&self, items: &[NavItem]) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        for button in self.0.buttons.borrow_mut().drain(..) {
            let view: &NSView = button.as_ref();
            self.0.native.removeArrangedSubview(view);
            view.removeFromSuperview();
        }
        self.0.targets.borrow_mut().clear();

        let mut buttons = Vec::with_capacity(items.len());
        let mut targets = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let button = unsafe {
                NSButton::buttonWithTitle_target_action(
                    &NSString::from_str(&item.label),
                    None,
                    None,
                    mtm,
                )
            };
            button.setBezelStyle(NSBezelStyle::AccessoryBar);
            button.setButtonType(NSButtonType::PushOnPushOff);
            button.setEnabled(item.enabled);
            // ハンドルを強く持つとトランポリンとの間で循環するため、弱参照にする。
            let target = ActionTarget::new(mtm, {
                let weak = Rc::downgrade(&self.0);
                move || {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let menu = Menu(inner);
                    menu.mark_selected(Some(index));
                    menu.0.handler.emit(index);
                }
            });
            unsafe {
                button.setTarget(Some(&target));
                button.setAction(Some(sel!(invoke:)));
            }
            self.0.native.addArrangedSubview(&button);
            buttons.push(button);
            targets.push(target);
        }
        *self.0.buttons.borrow_mut() = buttons;
        *self.0.targets.borrow_mut() = targets;

        // 項目を作り直したので、選択は一度外す。
        self.0.selected.set(None);
        self.mark_selected(None);
    }

    /// ボタンの押し込み状態を選択に合わせる。
    fn mark_selected(&self, index: Option<usize>) {
        for (i, button) in self.0.buttons.borrow().iter().enumerate() {
            let state = if Some(i) == index {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            };
            button.setState(state);
        }
        self.0.selected.set(index);
    }

    pub fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// 通知せずに選択を変える。
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.mark_selected(Some(index));
        }
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.mark_selected(Some(index));
            self.0.handler.emit(index);
        }
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

// ------------------------------------------------------------ Breadcrumbs

struct BreadcrumbsInner {
    native: Retained<NSPathControl>,
    /// クリックされた項目の位置を求めるため、トランポリンと共有する。
    items: Rc<RefCell<Vec<Retained<NSPathControlItem>>>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
    _target: Retained<ActionTarget>,
}

/// パンくず (NSPathControl)。
///
/// 階層のいまいる場所を左から順に並べる。項目をクリックすると、
/// そのインデックスで [`Breadcrumbs::on_select`] が呼ばれる。
#[derive(Clone)]
pub struct Breadcrumbs(Rc<BreadcrumbsInner>);
impl_widget!(Breadcrumbs);

impl Breadcrumbs {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSPathControl::new(mtm);
        native.setPathStyle(NSPathStyle::Standard);
        native.setEditable(false);

        let handler = SelectHandler::default();
        let items: Rc<RefCell<Vec<Retained<NSPathControlItem>>>> =
            Rc::new(RefCell::new(Vec::new()));
        let target = ActionTarget::new(mtm, {
            let native = native.clone();
            let handler = handler.clone();
            let items = items.clone();
            move || {
                let Some(clicked) = native.clickedPathItem() else {
                    return;
                };
                let index = items
                    .borrow()
                    .iter()
                    .position(|item| std::ptr::eq(&**item, &*clicked));
                if let Some(index) = index {
                    handler.emit(index);
                }
            }
        });
        unsafe {
            native.setTarget(Some(&target));
            native.setAction(Some(sel!(invoke:)));
        }

        Self(Rc::new(BreadcrumbsInner {
            native,
            items,
            handler,
            selected: Cell::new(None),
            _target: target,
        }))
    }

    /// 階層を作り直す。末尾がいまいる場所になる。
    pub fn set_items(&self, items: &[NavItem]) {
        let built: Vec<Retained<NSPathControlItem>> = items
            .iter()
            .map(|item| {
                let path_item = NSPathControlItem::new();
                path_item.setTitle(&NSString::from_str(&item.label));
                path_item
            })
            .collect();
        self.0
            .native
            .setPathItems(&NSArray::from_retained_slice(&built));
        *self.0.items.borrow_mut() = built;
        // 末尾がいまいる場所。
        self.0.selected.set(items.len().checked_sub(1));
    }

    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// いまいる場所。既定では末尾の項目。
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// 通知せずにいまいる場所を変える。
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.0.selected.set(Some(index));
        }
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.0.selected.set(Some(index));
            self.0.handler.emit(index);
        }
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

// ------------------------------------------------------------- Pagination

struct PaginationInner {
    native: Retained<NSStackView>,
    segments: Segments,
    /// 前へ / 次へのトランポリン。
    _targets: Vec<Retained<ActionTarget>>,
}

/// ページ送り。
///
/// ネイティブに相当するコントロールが無いため、前へ / 次への `NSButton` と
/// ページ番号の `NSSegmentedControl` を横に並べて構成している。
#[derive(Clone)]
pub struct Pagination(Rc<PaginationInner>);
impl_widget!(Pagination);

impl Pagination {
    pub(crate) fn new(mtm: MainThreadMarker, page_count: usize) -> Self {
        let native = row(mtm, 6.0);
        let segments = Segments::new(mtm, NSSegmentDistribution::Fit);

        let make_step = |title: &str, forward: bool| {
            let button = unsafe {
                NSButton::buttonWithTitle_target_action(&NSString::from_str(title), None, None, mtm)
            };
            let target = ActionTarget::new(mtm, {
                let segments = segments.clone();
                move || {
                    let current = segments.selected().unwrap_or(0);
                    let next = if forward {
                        current.saturating_add(1)
                    } else {
                        current.saturating_sub(1)
                    };
                    if next < segments.len() {
                        segments.select(next);
                    }
                }
            });
            unsafe {
                button.setTarget(Some(&target));
                button.setAction(Some(sel!(invoke:)));
            }
            (button, target)
        };

        let (prev, prev_target) = make_step("‹", false);
        let (next, next_target) = make_step("›", true);

        native.addArrangedSubview(&prev);
        native.addArrangedSubview(&segments.view());
        native.addArrangedSubview(&next);

        let this = Self(Rc::new(PaginationInner {
            native,
            segments,
            _targets: vec![prev_target, next_target],
        }));
        this.set_page_count(page_count);
        this
    }

    /// ページ数を変える。表示は 1 始まり、API は 0 始まり。
    pub fn set_page_count(&self, count: usize) {
        let items: Vec<NavItem> = (1..=count).map(|n| NavItem::new(n.to_string())).collect();
        self.0.segments.set_items(&items);
        if count > 0 {
            self.0.segments.set_selected(0);
        }
    }

    pub fn page_count(&self) -> usize {
        self.0.segments.len()
    }

    /// いまのページ (0 始まり)。
    pub fn page(&self) -> usize {
        self.0.segments.selected().unwrap_or(0)
    }

    /// 通知せずにページを変える。
    pub fn set_page(&self, page: usize) {
        self.0.segments.set_selected(page);
    }

    /// ユーザーが選んだのと同じ経路でページを変える (通知あり)。
    pub fn select(&self, page: usize) {
        self.0.segments.select(page);
    }

    /// 前のページへ。先頭にいるときは何もしない。
    pub fn go_previous(&self) {
        let page = self.page();
        if page > 0 {
            self.select(page - 1);
        }
    }

    /// 次のページへ。末尾にいるときは何もしない。
    pub fn go_next(&self) {
        let page = self.page();
        if page + 1 < self.page_count() {
            self.select(page + 1);
        }
    }

    /// ページが変わったときに、その番号 (0 始まり) で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(usize) + 'static) {
        self.0.segments.on_select(f);
    }
}

// ------------------------------------------------------------------- Link

struct LinkInner {
    native: Retained<NSButton>,
    /// トランポリンと共有する。`set_href` の結果がクリック時に効くようにするため。
    href: Rc<RefCell<String>>,
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// リンク。
///
/// AppKit にリンク専用のコントロールは無いため、枠なしの `NSButton` を
/// リンク色 (`NSColor::linkColor`) にしている。押すと `href` を
/// `NSWorkspace` で開き、そのあと [`Link::on_click`] が呼ばれる。
#[derive(Clone)]
pub struct Link(Rc<LinkInner>);
impl_widget!(Link);

impl Link {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str, href: &str) -> Self {
        let native = unsafe {
            NSButton::buttonWithTitle_target_action(&NSString::from_str(text), None, None, mtm)
        };
        native.setBordered(false);
        native.setContentTintColor(Some(&NSColor::linkColor()));

        let this = Self(Rc::new(LinkInner {
            native,
            href: Rc::new(RefCell::new(href.to_string())),
            target: RefCell::new(None),
        }));
        // href を開くだけのハンドラを既定で入れておく。
        this.on_click(|| {});
        this
    }

    pub fn text(&self) -> String {
        self.0.native.title().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.setTitle(&NSString::from_str(text));
    }

    pub fn href(&self) -> String {
        self.0.href.borrow().clone()
    }

    pub fn set_href(&self, href: &str) {
        *self.0.href.borrow_mut() = href.to_string();
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// 押されたときに呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// `href` が空でなければ、コールバックの前にブラウザで開く。
    pub fn on_click(&self, mut f: impl FnMut() + 'static) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let href = self.0.href.clone();
        let target = ActionTarget::new(mtm, move || {
            open_href(&href.borrow());
            f();
        });
        unsafe {
            self.0.native.setTarget(Some(&target));
            self.0.native.setAction(Some(sel!(invoke:)));
        }
        *self.0.target.borrow_mut() = Some(target);
    }

    /// クリックを発生させる (テストや自動操作用)。
    pub fn click(&self) {
        unsafe { self.0.native.performClick(None) };
    }
}

fn open_href(href: &str) {
    if href.is_empty() {
        return;
    }
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(href)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}
