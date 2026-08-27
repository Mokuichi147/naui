//! 分割ビュー (`GtkPaned`)。
//!
//! GTK4 に同じ役目のコントロールがあるので、そのまま使う。仕切りのつまみも、
//! ドラッグでの追従も、カーソルの形も `GtkPaned` が行う。
//!
//! naui 側で決めているのは次の 3 つ。
//!
//! - 並べる向き。naui の `Orientation::Horizontal` は「区画が横に並ぶ」なので
//!   `GtkPaned` も `Horizontal` (GTK4 の向きの意味と同じ)。
//! - 余りの配り先。`resize-start-child` を切り `resize-end-child` を立てて、
//!   start 側は指定した大きさを保ち、余りは end 側が受け取るようにする。
//! - 両側の最小の大きさ。`GtkPaned` の子そのものに最小を持たせると、アプリが
//!   [`Sizing`](naui_core::Sizing) で指定した `size_request` とぶつかるため、
//!   区画ごとに入れ物 ([`SizeBin`]) をもう 1 枚かぶせ、そちらへ持たせている。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::{clamp_split_position, Orientation, DEFAULT_SPLIT_POSITION};

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

struct SplitViewInner {
    native: gtk::Paned,
    bin: SizeBin,
    orientation: Orientation,
    /// 区画ごとの入れ物。最小の大きさはここが持つ。
    start_pane: SizeBin,
    end_pane: SizeBin,
    /// 区画のハンドルを保持し、コールバックごと生かしておく。
    start: RefCell<Option<Box<dyn Widget>>>,
    end: RefCell<Option<Box<dyn Widget>>>,
    position: Cell<f64>,
    min_start: Cell<f64>,
    min_end: Cell<f64>,
    on_resize: Notifier<f64>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 2 つの区画を、動かせる仕切りで分けるコンテナ (`GtkPaned`)。
#[derive(Clone)]
pub struct SplitView(Rc<SplitViewInner>);
impl_widget!(SplitView);

impl SplitView {
    pub(crate) fn new(orientation: Orientation) -> Self {
        let native = gtk::Paned::new(if orientation.is_vertical() {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        });
        // 余りは end 側が受け取る (start は指定した大きさを保つ)。
        native.set_resize_start_child(false);
        native.set_resize_end_child(true);
        // 最小の大きさを効かせるため、どちらも最小より縮ませない。
        native.set_shrink_start_child(false);
        native.set_shrink_end_child(false);

        let start_pane = pane_container();
        let end_pane = pane_container();
        native.set_start_child(Some(&start_pane));
        native.set_end_child(Some(&end_pane));

        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(SplitViewInner {
            native,
            bin,
            orientation,
            start_pane,
            end_pane,
            start: RefCell::new(None),
            end: RefCell::new(None),
            position: Cell::new(DEFAULT_SPLIT_POSITION),
            min_start: Cell::new(0.0),
            min_end: Cell::new(0.0),
            on_resize: Notifier::default(),
            handler: RefCell::new(None),
        });
        inner.native.set_position(to_px(DEFAULT_SPLIT_POSITION));

        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_position_notify(move |native| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let position = f64::from(native.position());
                if (position - inner.position.get()).abs() < 1.0 {
                    return;
                }
                inner.position.set(position);
                inner.on_resize.emit(position);
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// 並べる向き。`Horizontal` なら区画が横に並ぶ (仕切りは縦)。
    pub fn orientation(&self) -> Orientation {
        self.0.orientation
    }

    /// 左 (または上) の区画。呼ぶたびに置き換わる。
    pub fn set_start(&self, child: &dyn Widget) {
        self.0.set_pane(true, child);
    }

    /// 右 (または下) の区画。呼ぶたびに置き換わる。
    pub fn set_end(&self, child: &dyn Widget) {
        self.0.set_pane(false, child);
    }

    /// いまの仕切りの位置 (start 側の大きさ、論理ピクセル)。
    pub fn position(&self) -> f64 {
        self.0.position.get()
    }

    /// 仕切りを動かす。`on_resize` は呼ばれない。
    pub fn set_position(&self, position: f64) {
        let clamped = self.0.clamp(position);
        self.0.position.set(clamped);
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_position(to_px(clamped));
        });
    }

    /// 利用者がドラッグしたのと同じく仕切りを動かす。`on_resize` を呼ぶ。
    pub fn drag_to(&self, position: f64) {
        let clamped = self.0.clamp(position);
        self.set_position(clamped);
        self.0.on_resize.emit(clamped);
    }

    /// 両側の区画の最小の大きさ。既定はどちらも 0。
    pub fn set_min_sizes(&self, start: f64, end: f64) {
        self.0.min_start.set(start.max(0.0));
        self.0.min_end.set(end.max(0.0));
        self.0.apply_min_sizes();
        // いまの位置が範囲の外なら押し戻す (通知はしない)。
        self.set_position(self.0.position.get());
    }

    /// 利用者が仕切りを動かすたび、動いた後の位置で呼ばれる。
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_resize.set(f);
    }

    /// 対応する `GtkPaned`。バックエンド固有の脱出口として公開している。
    pub fn native_paned(&self) -> gtk::Paned {
        self.0.native.clone()
    }
}

impl SplitViewInner {
    fn set_pane(&self, is_start: bool, child: &dyn Widget) {
        let container = if is_start {
            &self.start_pane
        } else {
            &self.end_pane
        };
        while let Some(previous) = container.first_child() {
            previous.unparent();
        }
        let bin = child.size_bin();
        // 区画の中身は、配られた場所いっぱいに置く (`Scroll` と同じ扱い)。
        bin.fill_parent();
        bin.set_parent(container);

        let slot = if is_start { &self.start } else { &self.end };
        *slot.borrow_mut() = Some(child.boxed_clone());
    }

    /// 2 つの区画が分け合える大きさ (仕切りの分を除く)。まだ 0 なら 0。
    ///
    /// `GtkPaned` の仕切りの太さは CSS が決めるので引き算では求めず、
    /// 区画に実際に配られた大きさの和を使う。
    fn total(&self) -> f64 {
        let total = if self.orientation.is_vertical() {
            self.start_pane.height() + self.end_pane.height()
        } else {
            self.start_pane.width() + self.end_pane.width()
        };
        f64::from(total.max(0))
    }

    fn clamp(&self, position: f64) -> f64 {
        clamp_split_position(
            position,
            self.total(),
            self.min_start.get(),
            self.min_end.get(),
        )
    }

    /// 最小の大きさを、区画の入れ物の `size_request` として渡す。
    fn apply_min_sizes(&self) {
        let (start, end) = (to_px(self.min_start.get()), to_px(self.min_end.get()));
        if self.orientation.is_vertical() {
            self.start_pane.set_size_request(-1, start);
            self.end_pane.set_size_request(-1, end);
        } else {
            self.start_pane.set_size_request(start, -1);
            self.end_pane.set_size_request(end, -1);
        }
    }
}

/// 区画 1 つぶんの入れ物。最小の大きさを持たせるために 1 枚かぶせる。
fn pane_container() -> SizeBin {
    let container: SizeBin = glib::Object::new();
    container.set_halign(gtk::Align::Fill);
    container.set_valign(gtk::Align::Fill);
    container.set_hexpand(true);
    container.set_vexpand(true);
    container
}

fn to_px(value: f64) -> i32 {
    value.round().clamp(0.0, f64::from(i32::MAX)) as i32
}
