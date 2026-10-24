//! Layout containers (`Grid` / `Scroll` / `Spacer`).
//!
//! The computing is done by GTK4's layout (`GtkGrid` / `GtkScrolledWindow`); naui
//! only writes properties.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::{
    GridCell, Padding, ScrollMetrics, ScrollNotifier, ScrollPolicy, ScrollTarget, Track,
};

use crate::bin::{apply_padding, SizeBin};
use crate::widgets::{impl_widget, Widget};

// ------------------------------------------------------------------- Grid

struct GridInner {
    native: gtk::Grid,
    bin: SizeBin,
    /// The placed children and their positions. Kept so the column / row settings can
    /// be changed later.
    children: RefCell<Vec<(GridCell, Box<dyn Widget>)>>,
    column_tracks: RefCell<Vec<Track>>,
    row_tracks: RefCell<Vec<Track>>,
}

/// A container that positions children by row and column (`GtkGrid`).
#[derive(Clone)]
pub struct Grid(Rc<GridInner>);
impl_widget!(Grid);

impl Grid {
    pub(crate) fn new() -> Self {
        let native = gtk::Grid::new();
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(GridInner {
            native,
            bin,
            children: RefCell::new(Vec::new()),
            column_tracks: RefCell::new(Vec::new()),
            row_tracks: RefCell::new(Vec::new()),
        }))
    }

    pub fn set_spacing(&self, column: f64, row: f64) {
        self.0.native.set_column_spacing(to_px(column) as u32);
        self.0.native.set_row_spacing(to_px(row) as u32);
    }

    pub fn set_padding(&self, padding: Padding) {
        apply_padding(&self.0.native, padding);
    }

    /// Places a child in a cell. It can be stacked onto a cell that already holds
    /// something.
    pub fn attach(&self, child: &dyn Widget, cell: GridCell) {
        let bin = child.size_bin();
        self.0.native.attach(
            &bin,
            cell.column as i32,
            cell.row as i32,
            cell.column_span as i32,
            cell.row_span as i32,
        );
        self.apply_tracks_to(&bin, cell);
        self.0
            .children
            .borrow_mut()
            .push((cell, child.boxed_clone()));
    }

    /// Replaces what is in the cell. Whatever was in the same cell is removed.
    pub fn replace(&self, child: &dyn Widget, cell: GridCell) {
        self.remove(cell);
        self.attach(child, cell);
    }

    /// Removes every child. The row and column settings stay.
    pub fn clear(&self) {
        for (_, child) in std::mem::take(&mut *self.0.children.borrow_mut()) {
            self.0.native.remove(&child.size_bin());
        }
    }

    /// Removes whatever is in the given cell. Does nothing if it is empty.
    ///
    /// Only the column and row of `cell` are considered, not the span.
    pub fn remove(&self, cell: GridCell) {
        let mut children = self.0.children.borrow_mut();
        let mut index = 0;
        while index < children.len() {
            if children[index].0.column == cell.column && children[index].0.row == cell.row {
                let (_, child) = children.remove(index);
                self.0.native.remove(&child.size_bin());
            } else {
                index += 1;
            }
        }
    }

    /// How a column's width is determined.
    ///
    /// `GtkGrid` cannot give columns a width of their own, so this is applied **to the
    /// children in that column**. A column's width follows its largest content, so
    /// this ends up setting the column width. `GtkGrid` has no weights for
    /// [`Track::Fill`]; leftover space is split evenly among the growing columns (the
    /// same limitation as macOS).
    pub fn set_column_track(&self, index: usize, track: Track) {
        set_track(&self.0.column_tracks, index, track);
        self.reapply_tracks();
    }

    /// How a row's height is determined. Has the same limitations as
    /// [`Grid::set_column_track`].
    pub fn set_row_track(&self, index: usize, track: Track) {
        set_track(&self.0.row_tracks, index, track);
        self.reapply_tracks();
    }

    fn reapply_tracks(&self) {
        for (cell, child) in self.0.children.borrow().iter() {
            self.apply_tracks_to(&child.size_bin(), *cell);
        }
    }

    /// Applies the settings of the column and row a child is placed in to that child.
    ///
    /// Children spanning several cells do not take part in deciding column / row
    /// widths (it would be unclear which column's width they decide).
    fn apply_tracks_to(&self, bin: &SizeBin, cell: GridCell) {
        if cell.column_span == 1 {
            if let Some(track) = self.0.column_tracks.borrow().get(cell.column).copied() {
                bin.apply_track(true, track);
            }
        }
        if cell.row_span == 1 {
            if let Some(track) = self.0.row_tracks.borrow().get(cell.row).copied() {
                bin.apply_track(false, track);
            }
        }
    }

    /// The number of columns the placed children need.
    pub fn columns(&self) -> usize {
        self.0
            .children
            .borrow()
            .iter()
            .map(|(cell, _)| cell.columns_needed())
            .max()
            .unwrap_or(0)
    }

    /// The number of rows the placed children need.
    pub fn rows(&self) -> usize {
        self.0
            .children
            .borrow()
            .iter()
            .map(|(cell, _)| cell.rows_needed())
            .max()
            .unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Writes a column / row setting, first extending the list as far as needed.
fn set_track(tracks: &RefCell<Vec<Track>>, index: usize, track: Track) {
    let mut tracks = tracks.borrow_mut();
    if tracks.len() <= index {
        tracks.resize(index + 1, Track::Auto);
    }
    tracks[index] = track;
}

// ----------------------------------------------------------------- Scroll

/// The number of frames a `scroll_to` is remembered.
///
/// GTK4 has no way to lay out synchronously, and right after the contents change,
/// the `GtkAdjustment` still has the previous layout's sizes. The requested
/// destination is remembered until the next layout and re-applied when the sizes
/// change (`changed`). The start of a frame (tick) comes before layout, so on the
/// second tick it is assumed that "the next layout has finished" and it is
/// forgotten.
///
/// While hidden (not mapped) there is no layout, so frames are not counted. A
/// destination requested inside a hidden tab starts counting once the tab is
/// shown.
const PENDING_FRAMES: u8 = 2;

struct ScrollInner {
    native: gtk::ScrolledWindow,
    bin: SizeBin,
    child: RefCell<Option<Box<dyn Widget>>>,
    on_scroll: ScrollNotifier,
    /// The destination remembered until layout has finished.
    pending: Cell<Option<ScrollTarget>>,
    /// The number of frames left before the destination is forgotten.
    frames_left: Cell<u8>,
    tick: RefCell<Option<gtk::TickCallbackId>>,
}

impl Drop for ScrollInner {
    fn drop(&mut self) {
        if let Some(tick) = self.tick.borrow_mut().take() {
            tick.remove();
        }
    }
}

/// A container that scrolls when its contents overflow (`GtkScrolledWindow`).
#[derive(Clone)]
pub struct Scroll(Rc<ScrollInner>);
impl_widget!(Scroll);

impl Scroll {
    pub(crate) fn new() -> Self {
        let native = gtk::ScrolledWindow::new();
        native.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        let bin = SizeBin::wrap(&native);
        let this = Self(Rc::new(ScrollInner {
            native,
            bin,
            child: RefCell::new(None),
            on_scroll: ScrollNotifier::default(),
            pending: Cell::new(None),
            frames_left: Cell::new(0),
            tick: RefCell::new(None),
        }));
        for adjustment in [this.0.native.hadjustment(), this.0.native.vadjustment()] {
            let weak = Rc::downgrade(&this.0);
            adjustment.connect_value_changed(move |_| {
                if let Some(inner) = weak.upgrade() {
                    Scroll(inner).notify_if_moved();
                }
            });
            // レイアウトで中身や見える範囲の大きさが変わった。
            let weak = Rc::downgrade(&this.0);
            adjustment.connect_changed(move |_| {
                if let Some(inner) = weak.upgrade() {
                    Scroll(inner).apply_pending();
                }
            });
        }
        // 隠れている間に頼まれた行き先を、出てからのフレームで数え直す。
        let weak = Rc::downgrade(&this.0);
        this.0.native.connect_map(move |_| {
            if let Some(inner) = weak.upgrade() {
                if inner.pending.get().is_some() {
                    inner.frames_left.set(PENDING_FRAMES);
                    Scroll(inner).count_frames();
                }
            }
        });
        this
    }

    /// The current scroll position and sizes.
    ///
    /// GTK4 cannot be made to lay out synchronously, so right after the contents
    /// change, the previous sizes remain until the next frame.
    pub fn metrics(&self) -> ScrollMetrics {
        let horizontal = self.0.native.hadjustment();
        let vertical = self.0.native.vadjustment();
        let viewport_width = horizontal.page_size();
        let viewport_height = vertical.page_size();
        ScrollMetrics {
            x: horizontal.value() - horizontal.lower(),
            y: vertical.value() - vertical.lower(),
            viewport_width,
            viewport_height,
            content_width: (horizontal.upper() - horizontal.lower()).max(viewport_width),
            content_height: (vertical.upper() - vertical.lower()).max(viewport_height),
        }
    }

    /// Scrolls to the given position, clamped to the scrollable range.
    ///
    /// Even if the contents just changed, it scrolls again using the sizes after the
    /// next layout.
    pub fn scroll_to(&self, x: f64, y: f64) {
        self.request(ScrollTarget::To { x, y });
    }

    /// Scrolls to the vertical end (the very bottom). The horizontal position stays.
    pub fn scroll_to_end(&self) {
        self.request(ScrollTarget::End);
    }

    /// Notified when the scroll position changes.
    ///
    /// Besides user interaction, it also arrives for moves caused by `scroll_to` or by
    /// the contents shrinking. It does not arrive when only the sizes change.
    pub fn on_scroll(&self, f: impl FnMut(ScrollMetrics) + 'static) {
        self.0.on_scroll.set(&self.metrics(), f);
    }

    fn request(&self, target: ScrollTarget) {
        self.0.pending.set(Some(target));
        self.0.frames_left.set(PENDING_FRAMES);
        self.apply_pending();
        // 隠れているなら、出たとき (`map`) に数え始める。
        if self.0.native.is_mapped() {
            self.count_frames();
        }
    }

    /// Counts the frames until the destination is forgotten.
    fn count_frames(&self) {
        if self.0.tick.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        let id = self.0.native.add_tick_callback(move |_, _| {
            let Some(inner) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !inner.native.is_mapped() {
                // 数えている途中で隠れた。行き先は覚えたまま、出るのを待つ。
                inner.tick.borrow_mut().take();
                return glib::ControlFlow::Break;
            }
            let left = inner.frames_left.get().saturating_sub(1);
            inner.frames_left.set(left);
            if left > 0 {
                return glib::ControlFlow::Continue;
            }
            inner.pending.set(None);
            inner.tick.borrow_mut().take();
            glib::ControlFlow::Break
        });
        *self.0.tick.borrow_mut() = Some(id);
    }

    /// Scrolls to the remembered destination. It is forgotten when the frames have
    /// been counted.
    fn apply_pending(&self) {
        let Some(target) = self.0.pending.get() else {
            return;
        };
        let metrics = self.metrics();
        if metrics.viewport_width <= 0.0 || metrics.viewport_height <= 0.0 {
            return;
        }
        let (x, y) = target.resolve(&metrics);
        let horizontal = self.0.native.hadjustment();
        let vertical = self.0.native.vadjustment();
        // 縦と横は別々の `value-changed` になるので、まとめて 1 回だけ通知する。
        self.0.on_scroll.batch(
            || {
                horizontal.set_value(horizontal.lower() + x);
                vertical.set_value(vertical.lower() + y);
            },
            || self.metrics(),
        );
    }

    fn notify_if_moved(&self) {
        self.0.on_scroll.notify_if_moved(|| self.metrics());
    }

    pub fn set_policy(&self, horizontal: ScrollPolicy, vertical: ScrollPolicy) {
        self.0
            .native
            .set_policy(to_policy(horizontal), to_policy(vertical));
        self.follow_natural_size();
    }

    pub fn set_child(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        // スクロールの中では、中身は自分の大きさのまま置かれる。
        bin.fill_parent();
        self.0.native.set_child(Some(&bin));
        self.follow_natural_size();
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// In scrollable directions, places the content at its "natural size".
    ///
    /// `GtkScrolledWindow` wraps content that does not scroll by itself in a
    /// `GtkViewport`. Its default (`Minimum`) **squashes the content to its minimum
    /// size** when there is not enough room. Widgets whose minimum and natural sizes
    /// differ, such as a `List` without a height set, shrink to a single row.
    ///
    /// In scrollable directions (`Auto` / `Always`), overflow can be seen by scrolling,
    /// so the content is placed at its natural size. Non-scrollable directions
    /// (`Never`) keep the default: with no scroll bar, whatever overflows would become
    /// invisible.
    fn follow_natural_size(&self) {
        let Some(viewport) = self.0.native.child().and_downcast::<gtk::Viewport>() else {
            return;
        };
        viewport.set_hscroll_policy(scrollable_policy(self.0.native.hscrollbar_policy()));
        viewport.set_vscroll_policy(scrollable_policy(self.0.native.vscrollbar_policy()));
    }
}

/// Decides how the content is placed from the scroll bar policy.
fn scrollable_policy(policy: gtk::PolicyType) -> gtk::ScrollablePolicy {
    match policy {
        gtk::PolicyType::Never => gtk::ScrollablePolicy::Minimum,
        _ => gtk::ScrollablePolicy::Natural,
    }
}

fn to_policy(policy: ScrollPolicy) -> gtk::PolicyType {
    match policy {
        ScrollPolicy::Auto => gtk::PolicyType::Automatic,
        ScrollPolicy::Always => gtk::PolicyType::Always,
        ScrollPolicy::Never => gtk::PolicyType::Never,
    }
}

// ----------------------------------------------------------------- Spacer

struct SpacerInner {
    native: gtk::Box,
    bin: SizeBin,
}

/// A widget that is just empty space. It absorbs a stack's leftover space and
/// pushes the others away.
#[derive(Clone)]
pub struct Spacer(Rc<SpacerInner>);
impl_widget!(Spacer);

impl Spacer {
    pub(crate) fn new() -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let bin = SizeBin::wrap(&native);
        // 中身が無いので自然な大きさは 0。余りだけを受け取る。
        bin.apply_sizing(naui_core::Sizing::fill());
        Self(Rc::new(SpacerInner { native, bin }))
    }
}

fn to_px(value: f64) -> i32 {
    value.round().clamp(0.0, i32::MAX as f64) as i32
}
