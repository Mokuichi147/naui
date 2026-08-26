//! 分割ビュー (`NSSplitView`)。
//!
//! AppKit に同じ役目のコントロールがあるので、そのまま使う。仕切りの描画も、
//! ドラッグでの追従も、カーソルの形 (⇔) も `NSSplitView` が行う。
//!
//! naui 側で決めているのは次の 3 つだけで、いずれもデリゲートとして渡す。
//!
//! - 並べる向き (`setVertical`)。naui の `Orientation::Horizontal` は
//!   「区画が横に並ぶ」= 仕切りが縦なので `setVertical(true)` になる。
//! - 大きさが変わったときの区画の配り方
//!   (`splitView:resizeSubviewsWithOldSize:`)。start 側は指定された大きさを
//!   保ち、余りは end 側が受け取る。`NSSplitView` の既定は**割合**で配るので、
//!   「サイドバーの幅は変えない」形にここで置き換えている。
//! - 仕切りを動かせる範囲 (`splitView:constrainMinCoordinate:…` と
//!   `…constrainMaxCoordinate:…`)。
//!
//! 区画の frame は naui が直接置くので、区画のビューは
//! `translatesAutoresizingMaskIntoConstraints` を**切らない** (他のコンテナと
//! 違う点)。区画の中身は、その frame の中で Auto Layout が並べる。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{clamp_split_position, Orientation, DEFAULT_SPLIT_POSITION};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSSplitView, NSSplitViewDelegate, NSSplitViewDividerStyle, NSView,
};
use objc2_foundation::{NSInteger, NSNotification, NSPoint, NSRect, NSSize};

use crate::trampoline::ValueHandler;
use crate::widgets::{impl_widget, Widget};

struct SplitViewInner {
    native: Retained<NSSplitView>,
    orientation: Orientation,
    /// 区画のハンドル。中継ごと生かしておくために持つ。
    start: RefCell<Option<Box<dyn Widget>>>,
    end: RefCell<Option<Box<dyn Widget>>>,
    /// アプリが指定した仕切りの位置 (start 側の大きさ)。
    ///
    /// せまくて入りきらないときは端へ寄せて**表示**するが、この値はそのまま
    /// 残す。広がったときに元の位置へ戻すため。
    position: Cell<f64>,
    min_start: Cell<f64>,
    min_end: Cell<f64>,
    handler: ValueHandler<f64>,
    /// デリゲートは弱参照で持たれるので、ここで生かしておく。
    delegate: RefCell<Option<Retained<SplitObserver>>>,
}

/// 2 つの区画を、動かせる仕切りで分けるコンテナ (`NSSplitView`)。
#[derive(Clone)]
pub struct SplitView(Rc<SplitViewInner>);
impl_widget!(SplitView);

impl SplitView {
    pub(crate) fn new(mtm: MainThreadMarker, orientation: Orientation) -> Self {
        let native = NSSplitView::new(mtm);
        // naui の Horizontal は「区画が横に並ぶ」= 仕切りは縦。
        native.setVertical(!orientation.is_vertical());
        // 2 区画の分割は、macOS の作法どおり細い仕切り。
        native.setDividerStyle(NSSplitViewDividerStyle::Thin);

        let this = Self(Rc::new(SplitViewInner {
            native,
            orientation,
            start: RefCell::new(None),
            end: RefCell::new(None),
            position: Cell::new(DEFAULT_SPLIT_POSITION),
            min_start: Cell::new(0.0),
            min_end: Cell::new(0.0),
            handler: ValueHandler::default(),
            delegate: RefCell::new(None),
        }));

        // 中継はハンドルと同じ寿命で持つ。通知の中から位置を変えても、
        // 実行中の中継そのものを解放しないようにするため。
        let delegate = SplitObserver::new(mtm, Rc::downgrade(&this.0));
        this.0
            .native
            .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        *this.0.delegate.borrow_mut() = Some(delegate);
        this
    }

    /// 並べる向き。`Horizontal` なら区画が横に並ぶ (仕切りは縦)。
    pub fn orientation(&self) -> Orientation {
        self.0.orientation
    }

    /// 左 (または上) の区画。呼ぶたびに置き換わる。
    pub fn set_start(&self, child: &dyn Widget) {
        self.0.set_pane(0, child);
    }

    /// 右 (または下) の区画。呼ぶたびに置き換わる。
    pub fn set_end(&self, child: &dyn Widget) {
        self.0.set_pane(1, child);
    }

    /// いまの仕切りの位置 (start 側の大きさ、論理ピクセル)。
    pub fn position(&self) -> f64 {
        self.0.position.get()
    }

    /// 仕切りを動かす。`on_resize` は呼ばれない。
    pub fn set_position(&self, position: f64) {
        self.0.position.set(self.0.clamp(position));
        self.0.relayout();
    }

    /// 利用者がドラッグしたのと同じく仕切りを動かす。`on_resize` を呼ぶ。
    pub fn drag_to(&self, position: f64) {
        let clamped = self.0.clamp(position);
        self.0.position.set(clamped);
        self.0.relayout();
        self.0.handler.emit(clamped);
    }

    /// 両側の区画の最小の大きさ。既定はどちらも 0。
    pub fn set_min_sizes(&self, start: f64, end: f64) {
        self.0.min_start.set(start.max(0.0));
        self.0.min_end.set(end.max(0.0));
        // いまの位置が範囲の外なら押し戻す (通知はしない)。
        self.0.position.set(self.0.clamp(self.0.position.get()));
        self.0.relayout();
    }

    /// 利用者が仕切りを動かすたび、動いた後の位置で呼ばれる。
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.handler.set(f);
    }

    /// 対応する `NSSplitView`。バックエンド固有の脱出口として公開している。
    pub fn native_split_view(&self) -> Retained<NSSplitView> {
        self.0.native.clone()
    }
}

impl SplitViewInner {
    /// 区画を差し替える。`index` は 0 が start、1 が end。
    fn set_pane(&self, index: usize, child: &dyn Widget) {
        let slot = if index == 0 { &self.start } else { &self.end };
        if let Some(previous) = slot.borrow_mut().take() {
            let view = previous.native_view();
            self.native.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }

        let view = child.native_view();
        // 区画の frame は naui が置くので、Auto Layout には任せない。
        view.setTranslatesAutoresizingMaskIntoConstraints(true);
        view.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
        self.native
            .insertArrangedSubview_atIndex(&view, index as NSInteger);
        *slot.borrow_mut() = Some(child.boxed_clone());
        self.relayout();
    }

    /// 2 つの区画が分け合える大きさ (仕切りの分を除く)。まだ 0 なら 0。
    fn total(&self) -> f64 {
        let size = self.native.frame().size;
        let length = if self.orientation.is_vertical() {
            size.height
        } else {
            size.width
        };
        if length <= 0.0 {
            return 0.0;
        }
        (length - self.native.dividerThickness()).max(0.0)
    }

    /// 位置を、いまの大きさと最小の指定に収める。
    fn clamp(&self, position: f64) -> f64 {
        clamp_split_position(
            position,
            self.total(),
            self.min_start.get(),
            self.min_end.get(),
        )
    }

    /// いま画面に出ている start 側の大きさ。
    fn measured_position(&self) -> Option<f64> {
        let arranged = self.native.arrangedSubviews();
        if arranged.len() < 2 {
            return None;
        }
        let size = arranged.objectAtIndex(0).frame().size;
        Some(if self.orientation.is_vertical() {
            size.height
        } else {
            size.width
        })
    }

    fn relayout(&self) {
        self.native.adjustSubviews();
    }

    /// 区画の frame を置く (`splitView:resizeSubviewsWithOldSize:` の中身)。
    fn resize_subviews(&self) {
        let arranged = self.native.arrangedSubviews();
        if arranged.len() < 2 {
            return;
        }
        let size = self.native.frame().size;
        let thickness = self.native.dividerThickness();
        let position = self.clamp(self.position.get());
        let (start, end) = if self.orientation.is_vertical() {
            let rest = (size.height - thickness - position).max(0.0);
            (
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size.width, position)),
                NSRect::new(
                    NSPoint::new(0.0, position + thickness),
                    NSSize::new(size.width, rest),
                ),
            )
        } else {
            let rest = (size.width - thickness - position).max(0.0);
            (
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(position, size.height)),
                NSRect::new(
                    NSPoint::new(position + thickness, 0.0),
                    NSSize::new(rest, size.height),
                ),
            )
        };
        arranged.objectAtIndex(0).setFrame(start);
        arranged.objectAtIndex(1).setFrame(end);
    }

    /// `NSSplitView` が区画の大きさを変え終わったときの処理。
    ///
    /// 利用者のドラッグでも、naui が置き直したときでも届く。**いま指定されて
    /// いる位置どおりに置かれていれば naui の仕業**なので、そうでないときだけ
    /// 「利用者が動かした」と見なす。
    fn did_resize(&self) {
        if self.total() <= 0.0 {
            return;
        }
        let Some(measured) = self.measured_position() else {
            return;
        };
        // 1 ピクセル未満のずれは AppKit の丸めなので拾わない。
        if (measured - self.clamp(self.position.get())).abs() < 1.0 {
            return;
        }
        self.position.set(measured);
        self.handler.emit(measured);
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiSplitObserver"]
    #[ivars = Weak<SplitViewInner>]
    /// 区画の配り方・動かせる範囲・移動の通知を受け持つデリゲート。
    struct SplitObserver;

    unsafe impl NSObjectProtocol for SplitObserver {}

    unsafe impl NSSplitViewDelegate for SplitObserver {
        #[unsafe(method(splitView:resizeSubviewsWithOldSize:))]
        fn resize_subviews(&self, _split_view: &NSSplitView, _old_size: NSSize) {
            if let Some(inner) = self.ivars().upgrade() {
                inner.resize_subviews();
            }
        }

        #[unsafe(method(splitView:constrainMinCoordinate:ofSubviewAt:))]
        fn constrain_min(
            &self,
            _split_view: &NSSplitView,
            proposed: f64,
            _divider_index: NSInteger,
        ) -> f64 {
            match self.ivars().upgrade() {
                Some(inner) => proposed.max(inner.min_start.get()),
                None => proposed,
            }
        }

        #[unsafe(method(splitView:constrainMaxCoordinate:ofSubviewAt:))]
        fn constrain_max(
            &self,
            _split_view: &NSSplitView,
            proposed: f64,
            _divider_index: NSInteger,
        ) -> f64 {
            let Some(inner) = self.ivars().upgrade() else {
                return proposed;
            };
            let total = inner.total();
            if total <= 0.0 {
                return proposed;
            }
            proposed.min((total - inner.min_end.get()).max(inner.min_start.get()))
        }

        #[unsafe(method(splitViewDidResizeSubviews:))]
        fn did_resize_subviews(&self, _notification: &NSNotification) {
            if let Some(inner) = self.ivars().upgrade() {
                inner.did_resize();
            }
        }
    }
);

impl SplitObserver {
    fn new(mtm: MainThreadMarker, inner: Weak<SplitViewInner>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(inner);
        unsafe { msg_send![super(this), init] }
    }
}
