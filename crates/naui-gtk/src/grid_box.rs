//! The container behind `Grid`: it places children in rows and columns, sharing the
//! space with [`naui_core::GridAxis`].
//!
//! `GtkGrid` has no way to give a column a width of its own: it splits leftover
//! space evenly among the columns whose children expand, so [`Track::Fill`] weights
//! could not work, and the same code laid out differently than on Windows and the
//! Web. This widget does only the measuring and allocating; drawing, focus and
//! accessibility stay GTK4's, and the children are naui's [`SizeBin`]s, which keep
//! aligning themselves within their cells.
//!
//! Widths decide heights (a wrapping label is taller when narrower), so the columns
//! are shared out first, and each child's height is measured for the width of its
//! cell.
//!
//! [`SizeBin`]: crate::SizeBin

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use naui_core::{GridAxis, GridCell, GridItem, Track};

// glib のサブクラス化マクロが `unsafe impl` を作るため、この中だけ許可する。
#[allow(unsafe_code)]
mod imp {
    use std::cell::{Cell, RefCell};

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use naui_core::{GridCell, Track};

    #[derive(Default)]
    pub struct GridBox {
        /// The children and the cells they occupy, in the order they were attached.
        pub children: RefCell<Vec<(gtk::Widget, GridCell)>>,
        pub column_tracks: RefCell<Vec<Track>>,
        pub row_tracks: RefCell<Vec<Track>>,
        pub column_spacing: Cell<i32>,
        pub row_spacing: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GridBox {
        const NAME: &'static str = "NauiGridBox";
        type Type = super::GridBox;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            // テーマや利用者の CSS が `grid` を当てにしていても変わらないように。
            klass.set_css_name("grid");
            klass.set_accessible_role(gtk::AccessibleRole::Generic);
        }
    }

    impl ObjectImpl for GridBox {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for GridBox {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            let obj = self.obj();
            let mut child = obj.first_child();
            while let Some(widget) = child {
                if widget.request_mode() != gtk::SizeRequestMode::ConstantSize {
                    return gtk::SizeRequestMode::HeightForWidth;
                }
                child = widget.next_sibling();
            }
            gtk::SizeRequestMode::ConstantSize
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let obj = self.obj();
            let columns = obj.column_axis();
            let (minimum, natural) = if orientation == gtk::Orientation::Horizontal {
                (columns.minimum(), columns.natural())
            } else {
                let width = if for_size >= 0 {
                    f64::from(for_size)
                } else {
                    columns.natural()
                };
                let positions = columns.positions(&columns.allocate(width));
                let rows = obj.row_axis(&positions);
                (rows.minimum(), rows.natural())
            };
            let minimum = minimum.ceil() as i32;
            (minimum, (natural.ceil() as i32).max(minimum), -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            self.obj().allocate_children(width, height);
        }
    }
}

glib::wrapper! {
    /// The container behind naui's `Grid` on Linux. Public as a backend-specific
    /// escape hatch (what [`Widget::native_widget`](crate::Widget::native_widget)
    /// returns for a `Grid`).
    ///
    /// Its CSS name is `grid`, the same as `GtkGrid`'s.
    pub struct GridBox(ObjectSubclass<imp::GridBox>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for GridBox {
    fn default() -> Self {
        Self::new()
    }
}

impl GridBox {
    pub(crate) fn new() -> Self {
        glib::Object::new()
    }

    /// Places a child in a cell. The child is appended as the last child.
    pub(crate) fn attach(&self, child: &impl IsA<gtk::Widget>, cell: GridCell) {
        let child = child.as_ref();
        if child.parent().as_ref() == Some(self.upcast_ref()) {
            self.forget(child);
        } else {
            child.set_parent(self);
        }
        self.imp().children.borrow_mut().push((child.clone(), cell));
        self.queue_resize();
    }

    /// Takes a child out.
    pub(crate) fn remove(&self, child: &impl IsA<gtk::Widget>) {
        let child = child.as_ref();
        if child.parent().as_ref() != Some(self.upcast_ref()) {
            return;
        }
        self.forget(child);
        child.unparent();
        self.queue_resize();
    }

    fn forget(&self, child: &gtk::Widget) {
        self.imp()
            .children
            .borrow_mut()
            .retain(|(widget, _)| widget != child);
    }

    /// The cell a child occupies. `None` if it is not a child.
    pub fn cell_of(&self, child: &impl IsA<gtk::Widget>) -> Option<GridCell> {
        let child = child.as_ref();
        self.imp()
            .children
            .borrow()
            .iter()
            .find(|(widget, _)| widget == child)
            .map(|(_, cell)| *cell)
    }

    pub(crate) fn set_spacing(&self, column: i32, row: i32) {
        self.imp().column_spacing.set(column.max(0));
        self.imp().row_spacing.set(row.max(0));
        self.queue_resize();
    }

    /// The space between columns (logical pixels).
    pub fn column_spacing(&self) -> i32 {
        self.imp().column_spacing.get()
    }

    /// The space between rows (logical pixels).
    pub fn row_spacing(&self) -> i32 {
        self.imp().row_spacing.get()
    }

    pub(crate) fn set_tracks(&self, columns: &[Track], rows: &[Track]) {
        *self.imp().column_tracks.borrow_mut() = columns.to_vec();
        *self.imp().row_tracks.borrow_mut() = rows.to_vec();
        self.queue_resize();
    }

    /// The children that take part in layout (hidden ones do not), with their cells.
    fn laid_out(&self) -> Vec<(gtk::Widget, GridCell)> {
        self.imp()
            .children
            .borrow()
            .iter()
            .filter(|(widget, _)| widget.should_layout())
            .cloned()
            .collect()
    }

    fn column_axis(&self) -> GridAxis {
        let children = self.laid_out();
        let count = children
            .iter()
            .map(|(_, cell)| cell.columns_needed())
            .max()
            .unwrap_or(0);
        let items: Vec<GridItem> = children
            .iter()
            .map(|(widget, cell)| {
                let (minimum, natural, _, _) = widget.measure(gtk::Orientation::Horizontal, -1);
                GridItem {
                    start: cell.column,
                    span: cell.column_span,
                    minimum: f64::from(minimum),
                    natural: f64::from(natural),
                }
            })
            .collect();
        GridAxis::new(
            count,
            &self.imp().column_tracks.borrow(),
            f64::from(self.column_spacing()),
            &items,
        )
    }

    /// The rows, with each child's height measured for the width of its cell.
    fn row_axis(&self, columns: &[(f64, f64)]) -> GridAxis {
        let children = self.laid_out();
        let count = children
            .iter()
            .map(|(_, cell)| cell.rows_needed())
            .max()
            .unwrap_or(0);
        let items: Vec<GridItem> = children
            .iter()
            .map(|(widget, cell)| {
                let (_, width) = GridAxis::extent(columns, cell.column, cell.column_span);
                let width =
                    (width.floor() as i32).max(widget.measure(gtk::Orientation::Horizontal, -1).0);
                let (minimum, natural, _, _) = widget.measure(gtk::Orientation::Vertical, width);
                GridItem {
                    start: cell.row,
                    span: cell.row_span,
                    minimum: f64::from(minimum),
                    natural: f64::from(natural),
                }
            })
            .collect();
        GridAxis::new(
            count,
            &self.imp().row_tracks.borrow(),
            f64::from(self.row_spacing()),
            &items,
        )
    }

    fn allocate_children(&self, width: i32, height: i32) {
        let columns = self.column_axis();
        let column_positions = rounded(&columns.positions(&columns.allocate(f64::from(width))));
        let rows = self.row_axis(&column_positions);
        let row_positions = rounded(&rows.positions(&rows.allocate(f64::from(height))));
        let rtl = self.direction() == gtk::TextDirection::Rtl;
        for (widget, cell) in self.laid_out() {
            let (x, cell_width) =
                GridAxis::extent(&column_positions, cell.column, cell.column_span);
            let (y, cell_height) = GridAxis::extent(&row_positions, cell.row, cell.row_span);
            // 子は自分の最小より狭くは置かない (GTK4 が警告する)。はみ出しは
            // 親の SizeBin が隠す。
            let child_width =
                (cell_width as i32).max(widget.measure(gtk::Orientation::Horizontal, -1).0);
            let child_height =
                (cell_height as i32).max(widget.measure(gtk::Orientation::Vertical, child_width).0);
            let x = if rtl {
                width - x as i32 - child_width
            } else {
                x as i32
            };
            widget.size_allocate(
                &gtk::Allocation::new(x, y as i32, child_width, child_height),
                -1,
            );
        }
    }
}

/// Rounds line positions to whole pixels without the rounding piling up: each line
/// starts where the rounded end of the previous one is.
fn rounded(positions: &[(f64, f64)]) -> Vec<(f64, f64)> {
    positions
        .iter()
        .map(|&(start, size)| {
            let begin = start.round();
            let end = (start + size).round();
            (begin, (end - begin).max(0.0))
        })
        .collect()
}
