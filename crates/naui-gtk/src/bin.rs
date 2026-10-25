//! The foundation for mapping size specifications ([`Sizing`]) onto GTK4 layout.
//!
//! GTK4 widgets have a "minimum size" (`set_size_request`), "whether to take
//! leftover space" (`set_hexpand`) and "alignment within the allotted space"
//! (`set_halign`), but **no maximum saying "grow no further than this"**.
//!
//! So every naui widget is put into a thin container called [`SizeBin`] before it
//! is handed to a container. `SizeBin` is a widget that replaces only GTK4's
//! measuring (`measure` and `request_mode`): it caps the "natural size" the content
//! reports at the maximum, and passes the content's own way of measuring up to the
//! parent. The actual placement is left to `GtkBinLayout`, so GTK4 does the layout
//! computation.

use gtk::glib;
use gtk::prelude::*;
use naui_core::{Align, Length, Sizing, Track};

// glib のサブクラス化マクロが `unsafe impl` を作るため、この中だけ許可する。
#[allow(unsafe_code)]
mod imp {
    use std::cell::Cell;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use naui_core::Sizing;

    #[derive(Default)]
    pub struct SizeBin {
        /// The size the app specified. Read when alignment is recomputed.
        pub sizing: Cell<Sizing>,
        /// The maximum width (logical pixels). No maximum if negative.
        pub max_width: Cell<i32>,
        /// The maximum height (logical pixels). No maximum if negative.
        pub max_height: Cell<i32>,
        /// Whether the content cannot shrink its width (`GtkSpinButton` and the like).
        pub rigid_width: Cell<bool>,
        /// Whether the content's natural size is left out on `Fill` axes (images).
        pub fill_ignores_natural: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SizeBin {
        const NAME: &'static str = "NauiSizeBin";
        type Type = super::SizeBin;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for SizeBin {
        fn constructed(&self) {
            self.parent_constructed();
            self.max_width.set(-1);
            self.max_height.set(-1);
        }

        fn dispose(&self) {
            // GTK4 では、親が壊れる前に子を外しておく必要がある。
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl SizeBin {
        /// The cap on "the size of the other axis" passed to `measure`. `None` if there is
        /// none.
        fn cross_cap(&self, orientation: gtk::Orientation) -> Option<i32> {
            let cap = if orientation == gtk::Orientation::Horizontal {
                self.max_height.get()
            } else {
                self.max_width.get()
            };
            (cap >= 0).then_some(cap)
        }
    }

    impl WidgetImpl for SizeBin {
        /// Passes through what the content reports, while recomputing the "natural size"
        /// on axes that have a maximum.
        ///
        /// The parent decides sizes as "hand out up to the natural size, less if there is
        /// not enough room", so the maximum is passed as the natural size.
        ///
        /// | The axis's setting | Natural size |
        /// | --- | --- |
        /// | [`Length::Fill`] + maximum | **The maximum itself** (grows up to it when there is room) |
        /// | [`Length::Auto`] / [`Length::Fixed`] + maximum | The smaller of the content and the maximum |
        /// | [`Length::Fill`] without a maximum, for content marked with [`fill_ignores_natural`](Self::fill_ignores_natural) (images) | 0 (fits into the leftover space) |
        ///
        /// The minimum (`minimum`) is lowered for two reasons.
        ///
        /// 1. If it were not lowered below the maximum, the content would overflow when the
        ///    room is narrower than the maximum.
        /// 2. [`Length::Fill`] means "the parent decides the size", so **the content must
        ///    not push the parent wider for its own sake**. Otherwise content with `Fill`
        ///    would decide the minimum size the window can shrink to (the same reason the
        ///    Web backend writes `min-width: 0`). When a minimum is needed, specify it with
        ///    [`Sizing::min_width`] and friends, which GTK4 applies again as a minimum via
        ///    `size_request`.
        ///
        /// **Content that cannot shrink** ([`rigid_width`](Self::rigid_width)) is the
        /// exception: its minimum is passed through as is. GTK4 does not shrink content
        /// when given less than its minimum; it overflows and draws anyway.
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.obj().first_child() else {
                return (0, 0, -1, -1);
            };
            // もう一方の軸に上限があるなら、中身にはそこまでしか配られない。
            // 折り返す中身は渡された大きさで高さを決めるので、上限を超えた
            // 大きさで測らせると高さが足りなくなる。
            let for_size = match (for_size, self.cross_cap(orientation)) {
                (for_size, Some(cap)) if for_size >= 0 => for_size.min(cap),
                (for_size, _) => for_size,
            };
            let (child_minimum, child_natural, min_baseline, nat_baseline) =
                child.measure(orientation, for_size);
            let (mut minimum, mut natural) = (child_minimum, child_natural);
            let horizontal = orientation == gtk::Orientation::Horizontal;
            let sizing = self.sizing.get();
            let (cap, length, rigid) = if horizontal {
                (self.max_width.get(), sizing.width, self.rigid_width.get())
            } else {
                (self.max_height.get(), sizing.height, false)
            };
            if length.is_fill() {
                minimum = 0;
                if self.fill_ignores_natural.get() {
                    natural = 0;
                }
            }
            if cap >= 0 {
                natural = if length.is_fill() {
                    // 上限は「通常時に確保したい大きさ」も兼ねる。
                    cap
                } else {
                    natural.min(cap)
                };
                minimum = minimum.min(cap);
            }
            if rigid {
                minimum = child_minimum;
                natural = natural.max(child_minimum);
            }
            (minimum, natural, min_baseline, nat_baseline)
        }

        /// Passes the content's measuring mode (`GtkSizeRequestMode`) up to the parent as
        /// is.
        ///
        /// `GtkWidget` defaults to `ConstantSize` (width and height may be measured
        /// separately), so wrapping content like a wrapping `GtkLabel`, whose "height is
        /// decided once the width is decided", would not be conveyed to the parent. The
        /// parent would then ask for the height without passing a width and allot only
        /// **the height of one unwrapped line**, so the content would overflow.
        fn request_mode(&self) -> gtk::SizeRequestMode {
            self.obj()
                .first_child()
                .map_or(gtk::SizeRequestMode::ConstantSize, |child| {
                    child.request_mode()
                })
        }

        /// Places the content filling the same space as itself, but never narrower than
        /// the content's minimum.
        ///
        /// We would like to leave this to `GtkBinLayout`, but **attaching a layout manager
        /// makes GTK4 route `measure` to it** and the maximum stops working. Placement
        /// itself takes one line, so both are handled here.
        ///
        /// [`measure`](Self::measure) reports a minimum lowered to 0, so the content can be
        /// allotted less space than it can shrink to. Passing that on as is produces
        /// negative sizes inside GTK4 (`GtkEntry` subtracts its border from the allotted
        /// width before passing it to `GtkText`, so a narrow allotment gives values like
        /// `-18` and a warning). The content is placed at its minimum, and whatever
        /// overflows is hidden with `Overflow::Hidden` (the same pairing as CSS's
        /// `min-width: 0` and `overflow: hidden`).
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let Some(child) = self.obj().first_child() else {
                return;
            };
            let (horizontal, vertical) = (gtk::Orientation::Horizontal, gtk::Orientation::Vertical);
            // 「もう一方が決まってから測る」中身があるので、測る順を守る。
            let (width, height) = if child.request_mode() == gtk::SizeRequestMode::WidthForHeight {
                let height = height.max(child.measure(vertical, -1).0);
                (width.max(child.measure(horizontal, height).0), height)
            } else {
                let width = width.max(child.measure(horizontal, -1).0);
                (width, height.max(child.measure(vertical, width).0))
            };
            child.allocate(width, height, baseline, None);
        }
    }
}

glib::wrapper! {
    /// A container that wraps one naui widget and adds a size maximum.
    pub struct SizeBin(ObjectSubclass<imp::SizeBin>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SizeBin {
    /// Creates a container holding one child.
    ///
    /// The initial alignment is `Center`. GTK4 defaults to `Fill` (stretch to fill the
    /// allotted space), but naui's default is "fit the content" ([`Length::Auto`]), so
    /// leaving it would change the meaning.
    pub(crate) fn wrap(child: &impl IsA<gtk::Widget>) -> Self {
        let bin: SizeBin = glib::Object::new();
        bin.set_halign(gtk::Align::Center);
        bin.set_valign(gtk::Align::Center);
        child.as_ref().set_parent(&bin);
        bin
    }

    fn imp(&self) -> &imp::SizeBin {
        gtk::subclass::prelude::ObjectSubclassIsExt::imp(self)
    }

    /// Reports that the content cannot shrink its width.
    ///
    /// Content like `GtkSpinButton`, which has a "minimum width for the field and
    /// buttons side by side", does not shrink when allotted less and is drawn
    /// overflowing. A container with this set favors the content's minimum over the
    /// maximum.
    pub(crate) fn mark_rigid_width(&self) {
        self.imp().rigid_width.set(true);
        self.queue_resize();
    }

    /// Reports that the content's natural size is not asked for on `Fill` axes.
    ///
    /// For an image, the natural size is its pixel size. Asking for it on a `Fill`
    /// axis, a large image pushes a parent that can grow (a vertical `Scroll`, which
    /// places its content at the natural size) out to the image's height, instead of
    /// fitting into the leftover space. A maximum still counts as the natural size.
    pub(crate) fn mark_fill_ignores_natural(&self) {
        self.imp().fill_ignores_natural.set(true);
        self.queue_resize();
    }

    /// The size the app specified.
    pub(crate) fn sizing(&self) -> Sizing {
        self.imp().sizing.get()
    }

    /// Applies the size specification. Each call discards the previous one.
    pub(crate) fn apply_sizing(&self, sizing: Sizing) {
        let imp = self.imp();
        imp.sizing.set(sizing);

        let width = axis(sizing.width, sizing.min_width, sizing.max_width);
        let height = axis(sizing.height, sizing.min_height, sizing.max_height);

        imp.max_width.set(width.cap);
        imp.max_height.set(height.cap);
        // `Fill` の軸は最小を 0 として申告する (大きさは親が決める) 以上、
        // 配られた場所からはみ出して描いてはいけない。CSS の
        // `min-width: 0` と `overflow: hidden` を組みで使うのと同じ。
        self.set_overflow(if sizing.width.is_fill() || sizing.height.is_fill() {
            gtk::Overflow::Hidden
        } else {
            gtk::Overflow::Visible
        });
        self.set_size_request(width.request, height.request);
        self.apply_expand(true, width.expand);
        self.apply_expand(false, height.expand);
        self.set_halign(width.align);
        self.set_valign(height.align);
        // Grid へ置いた後に大きさを変えても、マスでの寄せ方は保つ。
        if self.parent().is_some_and(|parent| parent.is::<gtk::Grid>()) {
            self.place_in_grid();
        }
        self.queue_resize();
    }

    /// Sets whether to take leftover space.
    ///
    /// For [`Length::Auto`] it does not decide by itself; it clears `hexpand-set` and
    /// **leaves it as propagated from the children**. This lets a `Stack` containing a
    /// `Fill` child take leftover space itself.
    fn apply_expand(&self, horizontal: bool, expand: Option<bool>) {
        let (value, is_set) = if horizontal {
            ("hexpand", "hexpand-set")
        } else {
            ("vexpand", "vexpand-set")
        };
        match expand {
            Some(expand) => self.set_property(value, expand),
            None => self.set_property(is_set, false),
        }
    }

    /// Puts it somewhere it should fill its parent, such as directly under a window.
    ///
    /// Only axes with no size of their own become `Fill`.
    pub(crate) fn fill_parent(&self) {
        let sizing = self.sizing();
        if matches!(sizing.width, Length::Auto) && sizing.max_width.is_none() {
            self.set_halign(gtk::Align::Fill);
        }
        if matches!(sizing.height, Length::Auto) && sizing.max_height.is_none() {
            self.set_valign(gtk::Align::Fill);
        }
    }

    /// Keeps the column width from being decided by the content's natural size.
    ///
    /// In a table whose rows are limited, column widths are decided **only by the rows
    /// that are built**. If the content's natural width became the column width, the
    /// columns would move every time scrolling shows other rows. The reported width is
    /// set to 0 so that column widths are decided only by the headings and the sharing
    /// of leftover space (they still fill the allotted width).
    ///
    /// The same treatment as setting `max-width-chars` to 1 on text cells. **Content
    /// with its own size specified is left alone** (the app's setting wins).
    pub(crate) fn limit_natural_width(&self) {
        let sizing = self.sizing();
        if !matches!(sizing.width, Length::Auto) || sizing.max_width.is_some() {
            return;
        }
        self.imp().max_width.set(0);
        // 配られた幅より中身が大きくても、隣の列へはみ出して描かない。
        self.set_overflow(gtk::Overflow::Hidden);
        self.set_halign(gtk::Align::Fill);
    }

    /// Places it in a grid cell the way the other backends do.
    ///
    /// Axes that do not stretch to the cell go to the **start** of the cell
    /// horizontally and to the **center** vertically (`NSGridView`'s `Leading` /
    /// `Center`, WinUI's `Left` / `Center`, CSS Grid's `justify-items: start` /
    /// `align-items: center`). Without this they stay at `Center` (from `wrap`, or
    /// from [`Length::Fixed`] and capped [`Length::Fill`]), and in a column of
    /// names of different lengths each name would sit in the middle of the column.
    /// Only [`Length::Fill`] without a maximum stretches and is left alone.
    pub(crate) fn place_in_grid(&self) {
        let sizing = self.sizing();
        if !(sizing.width.is_fill() && sizing.max_width.is_none()) {
            self.set_halign(gtk::Align::Start);
        }
        if !(sizing.height.is_fill() && sizing.max_height.is_none()) {
            self.set_valign(gtk::Align::Center);
        }
    }

    /// Receives how a grid column / row is sized.
    ///
    /// `GtkGrid` cannot give columns or rows a width of their own. Instead the setting
    /// is applied **to the children in that column**. A column's width follows its
    /// largest child, so this decides the column width.
    ///
    /// Axes with their own size specified are left alone (the child's setting wins).
    pub(crate) fn apply_track(&self, horizontal: bool, track: Track) {
        let sizing = self.sizing();
        let own = if horizontal {
            sizing.width
        } else {
            sizing.height
        };
        if !matches!(own, Length::Auto) {
            return;
        }
        match track {
            Track::Auto => {}
            Track::Fixed(value) => {
                let value = to_px(value);
                let imp = self.imp();
                if horizontal {
                    imp.max_width.set(value);
                    self.set_size_request(value, self.height_request());
                    self.set_halign(gtk::Align::Fill);
                } else {
                    imp.max_height.set(value);
                    self.set_size_request(self.width_request(), value);
                    self.set_valign(gtk::Align::Fill);
                }
            }
            Track::Fill(_) => {
                // 重みは `GtkGrid` に無い。余りは広がる列で等分される。
                // 広がるのは列だけで、中身に合わせる子そのものは伸ばさない
                // (`place_in_grid` と同じ寄せ方のまま)。伸ばしたい子は自分で
                // `Fill` を指定する。
                if horizontal {
                    self.set_hexpand(true);
                } else {
                    self.set_vexpand(true);
                }
            }
        }
        self.queue_resize();
    }

    /// Receives the container's alignment.
    ///
    /// Its own setting wins only when that axis is [`Length::Fill`] without a maximum.
    /// [`Length::Fixed`] and `Fill` with a maximum "must not stretch", so a `Fill` from
    /// the container is read as `Center`.
    pub(crate) fn set_cross_align(&self, align: Align, vertical_container: bool) {
        let sizing = self.sizing();
        let (length, max) = if vertical_container {
            (sizing.width, sizing.max_width)
        } else {
            (sizing.height, sizing.max_height)
        };
        if length.is_fill() && max.is_none() {
            return;
        }
        let mut align = to_gtk_align(align);
        if align == gtk::Align::Fill && !matches!(length, Length::Auto) {
            align = gtk::Align::Center;
        }
        if vertical_container {
            self.set_halign(align);
        } else {
            self.set_valign(align);
        }
    }
}

/// The set of values passed to GTK4 for one axis.
struct AxisPlan {
    /// The minimum size passed to `set_size_request`. Not set if -1.
    request: i32,
    /// The maximum `measure` caps at. No maximum if -1.
    cap: i32,
    /// Whether to take leftover space. `None` leaves it as propagated from the
    /// children.
    expand: Option<bool>,
    /// How it behaves within the allotted space.
    align: gtk::Align,
}

/// Translates one axis's specification into GTK4 values.
///
/// | naui | GTK4 |
/// | --- | --- |
/// | [`Length::Auto`] | Nothing (the content's natural size) |
/// | [`Length::Fixed`] | `size_request` and the maximum set to the same value, alignment `Center` |
/// | [`Length::Fill`] (no maximum) | `expand` set, alignment `Fill` |
/// | [`Length::Fill`] (with maximum) | `expand` set, capped at the maximum (alignment `Center`) |
///
/// With `Fill` alignment, GTK4 stretches it to fill the allotted space and the
/// maximum stops working. That is why only axes with a maximum use `Center`.
fn axis(length: Length, min: Option<f64>, max: Option<f64>) -> AxisPlan {
    let min = min.map(to_px);
    let max = max.map(to_px);
    match length {
        Length::Fixed(value) => {
            let value = to_px(value);
            AxisPlan {
                request: min.map_or(value, |m| m.max(value)),
                cap: max.map_or(value, |m| m.min(value)),
                expand: Some(false),
                align: gtk::Align::Center,
            }
        }
        Length::Fill => AxisPlan {
            request: min.unwrap_or(-1),
            cap: max.unwrap_or(-1),
            expand: Some(true),
            align: if max.is_some() {
                gtk::Align::Center
            } else {
                gtk::Align::Fill
            },
        },
        Length::Auto => AxisPlan {
            request: min.unwrap_or(-1),
            cap: max.unwrap_or(-1),
            expand: None,
            align: gtk::Align::Center,
        },
    }
}

fn to_px(value: f64) -> i32 {
    value.round().clamp(0.0, i32::MAX as f64) as i32
}

/// Maps naui's alignment to GTK4's `align`.
pub(crate) fn to_gtk_align(align: Align) -> gtk::Align {
    match align {
        Align::Start => gtk::Align::Start,
        Align::Center => gtk::Align::Center,
        Align::End => gtk::Align::End,
        Align::Fill => gtk::Align::Fill,
    }
}

/// Applies padding as margins around the outside of the widget.
///
/// GTK4 containers have no "inner padding", but every widget has margins on all
/// four sides. Putting margins on the container itself shrinks the area its
/// contents can occupy by that much, which means the same as naui's
/// [`Padding`](naui_core::Padding).
pub(crate) fn apply_padding(widget: &impl IsA<gtk::Widget>, padding: naui_core::Padding) {
    let widget = widget.as_ref();
    widget.set_margin_top(to_px(padding.top));
    widget.set_margin_end(to_px(padding.right));
    widget.set_margin_bottom(to_px(padding.bottom));
    widget.set_margin_start(to_px(padding.left));
}
