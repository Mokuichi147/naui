//! `Flow`: a container that wraps its children onto new lines, laid out by naui.
//!
//! GTK4 has `GtkFlowBox`, but it lines its children up in columns (the widest
//! child of a column decides its width, and at most seven children go on a line by
//! default), and it is a list of selectable items: each child is put in a focusable
//! `GtkFlowBoxChild`, which Tab stops on and the theme pads. libadwaita's
//! `AdwWrapBox` wraps the way CSS `flex-wrap` does, but needs libadwaita 1.7, newer
//! than naui requires. So the widget here (`NauiFlow`) measures the children and
//! places them with naui-core's [`FlowLayout`], in GTK4's own measuring and
//! allocation: its height is reported for the width it is offered
//! (`HeightForWidth`), so the lines follow every resize in the same layout pass.
//!
//! Inside the rectangle a child gets, it is placed by GTK4 (`halign` / `valign` on
//! its `SizeBin`), so a child fitting its content sits at the vertical center of
//! its line and one with `Fill` on the height is stretched to it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::{flow_line_tops, Align, FlowItem, FlowLayout, Padding};

use crate::bin::{apply_padding, Placement, SizeBin};
use crate::widgets::{impl_widget, Widget};

// glib のサブクラス化マクロが `unsafe impl` を作るため、この中だけ許可する。
#[allow(unsafe_code)]
mod imp {
    use std::cell::{Cell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use naui_core::Align;

    #[derive(Default)]
    pub struct FlowWidget {
        /// The children in order.
        pub(super) children: RefCell<Vec<gtk::Widget>>,
        pub(super) column_spacing: Cell<i32>,
        pub(super) row_spacing: Cell<i32>,
        pub(super) align: Cell<Align>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FlowWidget {
        const NAME: &'static str = "NauiFlow";
        type Type = super::FlowWidget;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            // テーマが `GtkBox` に当てる書式をそのまま受けられるように。
            klass.set_css_name("box");
        }
    }

    impl ObjectImpl for FlowWidget {
        fn constructed(&self) {
            self.parent_constructed();
            self.align.set(Align::Start);
        }

        fn dispose(&self) {
            // GTK4 では、親が壊れる前に子を外しておく必要がある。
            self.children.borrow_mut().clear();
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for FlowWidget {
        /// The height follows the width: the narrower, the more lines.
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let (minimum, natural) = if orientation == gtk::Orientation::Horizontal {
                self.obj().measure_width()
            } else {
                self.obj().measure_height(for_size)
            };
            (minimum, natural.max(minimum), -1, -1)
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            self.obj().allocate_children(width);
        }
    }
}

glib::wrapper! {
    /// A container that wraps its children onto new lines.
    pub struct FlowWidget(ObjectSubclass<imp::FlowWidget>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// The shown children broken into lines for one width.
struct Lines {
    children: Vec<gtk::Widget>,
    layout: FlowLayout,
}

impl FlowWidget {
    fn new() -> Self {
        glib::Object::new()
    }

    fn imp(&self) -> &imp::FlowWidget {
        gtk::subclass::prelude::ObjectSubclassIsExt::imp(self)
    }

    fn insert(&self, index: usize, child: &impl IsA<gtk::Widget>) {
        let child = child.as_ref();
        let mut children = self.imp().children.borrow_mut();
        let index = index.min(children.len());
        // 描く順 (とフォーカスの順) も並びに合わせる。
        let next = children.get(index).cloned();
        child.insert_before(self, next.as_ref());
        children.insert(index, child.clone());
        drop(children);
        self.queue_resize();
    }

    fn remove(&self, child: &impl IsA<gtk::Widget>) {
        let child = child.as_ref();
        self.imp()
            .children
            .borrow_mut()
            .retain(|current| current != child);
        if child.parent().as_ref() == Some(self.upcast_ref()) {
            child.unparent();
        }
        self.queue_resize();
    }

    fn set_spacing(&self, column: i32, row: i32) {
        self.imp().column_spacing.set(column);
        self.imp().row_spacing.set(row);
        self.queue_resize();
    }

    fn set_align(&self, align: Align) {
        if self.imp().align.replace(align) != align {
            self.queue_allocate();
        }
    }

    /// The shown children and what each asks for along the line.
    fn items(&self) -> (Vec<gtk::Widget>, Vec<FlowItem>) {
        let children: Vec<gtk::Widget> = self
            .imp()
            .children
            .borrow()
            .iter()
            .filter(|child| child.should_layout())
            .cloned()
            .collect();
        let items = children
            .iter()
            .map(|child| {
                let (minimum, natural, ..) = child.measure(gtk::Orientation::Horizontal, -1);
                // `Fill` の子は親を押し広げないよう最小 0 を返すが、行の幅まで
                // 縮めるときは中身が切れない幅で止める (Grid と同じ)。
                let bin = child.downcast_ref::<SizeBin>();
                let fill = bin.map_or_else(
                    || child.compute_expand(gtk::Orientation::Horizontal),
                    |bin| bin.sizing().width.is_fill(),
                );
                let minimum = match bin {
                    Some(bin) if fill => {
                        minimum.max(bin.content_minimum(gtk::Orientation::Horizontal, -1))
                    }
                    _ => minimum,
                };
                FlowItem {
                    minimum: minimum as f64,
                    natural: natural as f64,
                    grow: fill,
                }
            })
            .collect();
        (children, items)
    }

    fn lines(&self, width: Option<i32>) -> Lines {
        let (children, items) = self.items();
        let spacing = self.imp().column_spacing.get() as f64;
        let width = match width {
            Some(width) if width >= 0 => width as f64,
            // 幅が決まっていなければ 1 行に並べたときの幅。
            _ => naui_core::flow_natural_width(&items, spacing),
        };
        let layout = FlowLayout::new(&items, width, spacing, self.imp().align.get());
        Lines { children, layout }
    }

    /// The minimum width is the widest child's (each on a line of its own), the
    /// natural width all children on one line.
    fn measure_width(&self) -> (i32, i32) {
        let (_, items) = self.items();
        let spacing = self.imp().column_spacing.get() as f64;
        (
            naui_core::flow_minimum_width(&items).ceil() as i32,
            naui_core::flow_natural_width(&items, spacing).ceil() as i32,
        )
    }

    /// The height of the lines at `for_width`.
    fn measure_height(&self, for_width: i32) -> (i32, i32) {
        let lines = self.lines(Some(for_width));
        let (minimums, naturals) = heights(&lines);
        let spacing = self.imp().row_spacing.get() as f64;
        let minimum = flow_line_tops(&lines.layout.line_heights(&minimums), spacing).1;
        let natural = flow_line_tops(&lines.layout.line_heights(&naturals), spacing).1;
        (minimum.ceil() as i32, natural.ceil() as i32)
    }

    fn allocate_children(&self, width: i32) {
        let lines = self.lines(Some(width));
        let (_, naturals) = heights(&lines);
        let line_heights = lines.layout.line_heights(&naturals);
        let (tops, _) = flow_line_tops(&line_heights, self.imp().row_spacing.get() as f64);
        let mirrored = self.direction() == gtk::TextDirection::Rtl;
        for (child, slot) in lines.children.iter().zip(&lines.layout.slots) {
            let start = slot.x.round() as i32;
            let w = (slot.x + slot.width).round() as i32 - start;
            let x = if mirrored { width - start - w } else { start };
            let y = tops[slot.line].round() as i32;
            let h = line_heights[slot.line].round() as i32;
            child.size_allocate(&gtk::Allocation::new(x, y, w, h), -1);
        }
    }
}

/// Each child's minimum and natural height at the width its slot gives it.
fn heights(lines: &Lines) -> (Vec<f64>, Vec<f64>) {
    lines
        .children
        .iter()
        .zip(&lines.layout.slots)
        .map(|(child, slot)| {
            let width = (slot.width.round() as i32).max(0);
            let (minimum, natural, ..) = child.measure(gtk::Orientation::Vertical, width);
            (minimum as f64, natural as f64)
        })
        .unzip()
}

struct FlowInner {
    native: FlowWidget,
    bin: SizeBin,
    /// Holds the children's handles, keeping them alive along with their closures.
    children: RefCell<Vec<Box<dyn Widget>>>,
    align: Cell<Align>,
}

/// A container that wraps its children onto new lines (a widget named `NauiFlow`
/// that places them itself; see the module documentation).
#[derive(Clone)]
pub struct Flow(Rc<FlowInner>);
impl_widget!(Flow);

impl Flow {
    pub(crate) fn new() -> Self {
        let native = FlowWidget::new();
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(FlowInner {
            native,
            bin,
            children: RefCell::new(Vec::new()),
            align: Cell::new(Align::Start),
        }))
    }

    pub fn set_spacing(&self, column: f64, row: f64) {
        self.0.native.set_spacing(to_px(column), to_px(row));
    }

    pub fn set_padding(&self, padding: Padding) {
        apply_padding(&self.0.native, padding);
    }

    pub fn set_align(&self, align: Align) {
        self.0.align.set(align);
        self.0.native.set_align(align);
    }

    /// Appends a child at the end.
    ///
    /// A widget still in another container (or already in this one) is refused, as
    /// GTK refuses it: nothing changes.
    pub fn append(&self, child: &dyn Widget) {
        self.insert(usize::MAX, child);
    }

    /// Inserts a child at the given position. If `index` is at least the current
    /// count, it is appended at the end.
    ///
    /// A widget still in another container is refused, the same as with
    /// [`append`](Self::append).
    pub fn insert(&self, index: usize, child: &dyn Widget) {
        let bin = child.size_bin();
        if crate::handover::refuses(&bin, false, "Flow::insert") {
            return;
        }
        let mut children = self.0.children.borrow_mut();
        let index = index.min(children.len());
        bin.set_placement(Placement::Loose);
        self.0.native.insert(index, &bin);
        children.insert(index, child.boxed_clone());
    }

    /// Removes the child at the given position. Does nothing if out of range.
    pub fn remove(&self, index: usize) {
        let child = {
            let mut children = self.0.children.borrow_mut();
            if index >= children.len() {
                return;
            }
            children.remove(index)
        };
        self.release(&*child);
    }

    /// Removes every child.
    pub fn clear(&self) {
        let children = std::mem::take(&mut *self.0.children.borrow_mut());
        for child in children {
            self.release(&*child);
        }
    }

    fn release(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        crate::interaction::clear_focus(&bin);
        self.0.native.remove(&bin);
        bin.set_placement(Placement::Loose);
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn to_px(value: f64) -> i32 {
    value.round().clamp(0.0, i32::MAX as f64) as i32
}
