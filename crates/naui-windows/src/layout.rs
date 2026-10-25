//! Size specifications and layout containers (Grid / Scroll / Spacer).
//!
//! The computing is done by WinUI 3's layout pass; naui only sets properties such as
//! `Width` / `MinWidth` / `RowDefinition`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use naui_core::{
    Align, GridCell, Length, Padding, Result, ScrollMetrics, ScrollNotifier, ScrollPolicy,
    ScrollTarget, Sizing, Track,
};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    ColumnDefinition, Grid as XamlGrid, RowDefinition, ScrollBarVisibility, ScrollMode,
    ScrollViewer, ScrollViewerViewChangedEventArgs,
};
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{
    DependencyObject, FrameworkElement, GridLength, GridUnitType, HorizontalAlignment,
    SizeChangedEventHandler, Thickness, UIElement, VerticalAlignment, Visibility,
    Window as XamlWindow,
};
use windows::Foundation::{EventHandler, IReference, PropertyValue};
use windows_core::Interface;

use crate::ui_thread::UiThreadCell;

use crate::to_error;
use crate::widgets::{impl_widget, Widget};

/// The `Sizing` and the parent container's information. Held in a state table on
/// the UI thread rather than in the user-facing `FrameworkElement.Tag`.
#[derive(Clone, Copy)]
enum ParentLayout {
    Stack {
        align: Align,
        vertical: bool,
    },
    Scroll {
        horizontal_scroll_enabled: bool,
        vertical_scroll_enabled: bool,
    },
}

impl ParentLayout {
    fn owns_horizontal(self) -> bool {
        match self {
            Self::Stack { vertical, .. } => vertical,
            Self::Scroll {
                horizontal_scroll_enabled,
                ..
            } => !horizontal_scroll_enabled,
        }
    }

    fn owns_vertical(self) -> bool {
        match self {
            Self::Stack { vertical, .. } => !vertical,
            Self::Scroll {
                vertical_scroll_enabled,
                ..
            } => !vertical_scroll_enabled,
        }
    }

    fn same_kind(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Stack { .. }, Self::Stack { .. }) | (Self::Scroll { .. }, Self::Scroll { .. })
        )
    }
}

#[derive(Clone, Copy, Default)]
struct LayoutState {
    fill_width: bool,
    fill_height: bool,
    parent: Option<ParentLayout>,
    restore_horizontal: Option<HorizontalAlignment>,
    restore_vertical: Option<VerticalAlignment>,
}

fn element_key(element: &UIElement) -> usize {
    let unknown = element
        .cast::<windows_core::IUnknown>()
        .expect("UIElement は IUnknown へ変換できる");
    Interface::as_raw(&unknown) as usize
}

fn state_key(element: &FrameworkElement) -> Option<usize> {
    element
        .cast::<UIElement>()
        .ok()
        .map(|element| element_key(&element))
}

fn update_layout_state(element: &UIElement, update: impl FnOnce(&mut LayoutState)) {
    let key = element_key(element);
    LAYOUT_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let empty = {
            let state = states.entry(key).or_default();
            update(state);
            !state.fill_width
                && !state.fill_height
                && state.parent.is_none()
                && state.restore_horizontal.is_none()
                && state.restore_vertical.is_none()
        };
        if empty {
            states.remove(&key);
        }
    });
}

fn set_sizing_state(element: &UIElement, sizing: Sizing) {
    let horizontal_alignment = match sizing.width {
        Length::Fill => HorizontalAlignment::Stretch,
        _ => HorizontalAlignment::Left,
    };
    let vertical_alignment = match sizing.height {
        Length::Fill => VerticalAlignment::Stretch,
        _ => VerticalAlignment::Top,
    };
    update_layout_state(element, |state| {
        state.fill_width = sizing.width.is_fill();
        state.fill_height = sizing.height.is_fill();
        if let Some(parent) = state.parent {
            if parent.owns_horizontal() {
                state.restore_horizontal = Some(horizontal_alignment);
            }
            if parent.owns_vertical() {
                state.restore_vertical = Some(vertical_alignment);
            }
        }
    });
}

/// Whether this element was given `Fill` in that direction.
pub(crate) fn wants_fill(element: &FrameworkElement, horizontal: bool) -> bool {
    let Some(key) = state_key(element) else {
        return false;
    };
    LAYOUT_STATES.with(|states| {
        let states = states.borrow();
        let Some(state) = states.get(&key) else {
            return false;
        };
        if horizontal {
            state.fill_width
        } else {
            state.fill_height
        }
    })
}

fn set_parent_state(element: &UIElement, framework: &FrameworkElement, parent: ParentLayout) {
    if let Some(previous) = parent_state(framework) {
        if !previous.same_kind(parent) {
            clear_parent_layout(element);
        }
    }

    let previous = parent_state(framework);
    let owns_horizontal = parent.owns_horizontal();
    let owns_vertical = parent.owns_vertical();
    let captured_horizontal = owns_horizontal.then(|| framework.HorizontalAlignment().ok());
    let captured_vertical = owns_vertical.then(|| framework.VerticalAlignment().ok());
    let mut restore_horizontal = None;
    let mut restore_vertical = None;

    update_layout_state(element, |state| {
        if let Some(previous) = previous {
            if previous.owns_horizontal() && !owns_horizontal {
                restore_horizontal = state.restore_horizontal;
                state.restore_horizontal = None;
            } else if !previous.owns_horizontal() && owns_horizontal {
                state.restore_horizontal = captured_horizontal.flatten();
            }
            if previous.owns_vertical() && !owns_vertical {
                restore_vertical = state.restore_vertical;
                state.restore_vertical = None;
            } else if !previous.owns_vertical() && owns_vertical {
                state.restore_vertical = captured_vertical.flatten();
            }
        } else {
            if owns_horizontal {
                state.restore_horizontal = captured_horizontal.flatten();
            }
            if owns_vertical {
                state.restore_vertical = captured_vertical.flatten();
            }
        }
        state.parent = Some(parent);
    });

    if let Some(value) = restore_horizontal {
        let _ = framework.SetHorizontalAlignment(value);
    }
    if let Some(value) = restore_vertical {
        let _ = framework.SetVerticalAlignment(value);
    }
}

fn parent_state(element: &FrameworkElement) -> Option<ParentLayout> {
    let key = state_key(element)?;
    LAYOUT_STATES.with(|states| states.borrow().get(&key).and_then(|state| state.parent))
}

/// Records the Stack parent's information on the child and applies the current
/// alignment.
pub(crate) fn set_stack_parent(element: &UIElement, align: Align, vertical_stack: bool) {
    let Ok(framework) = element.cast::<FrameworkElement>() else {
        return;
    };
    set_parent_state(
        element,
        &framework,
        ParentLayout::Stack {
            align,
            vertical: vertical_stack,
        },
    );
    apply_stack_alignment(&framework, align, vertical_stack);
}

/// Records the Scroll parent's information on the child and applies Stretch on the
/// non-scrolling axis.
pub(crate) fn set_scroll_parent(
    element: &UIElement,
    horizontal_scroll_enabled: bool,
    vertical_scroll_enabled: bool,
) {
    let Ok(framework) = element.cast::<FrameworkElement>() else {
        return;
    };
    set_parent_state(
        element,
        &framework,
        ParentLayout::Scroll {
            horizontal_scroll_enabled,
            vertical_scroll_enabled,
        },
    );
    stretch_non_scrolling_child(
        &framework,
        horizontal_scroll_enabled,
        vertical_scroll_enabled,
    );
}

/// Applies the parent container's layout again after Sizing overwrote it.
pub(crate) fn reapply_parent_layout(element: &FrameworkElement) {
    let Some(parent) = parent_state(element) else {
        return;
    };
    match parent {
        ParentLayout::Stack { align, vertical } => {
            apply_stack_alignment(element, align, vertical);
        }
        ParentLayout::Scroll {
            horizontal_scroll_enabled,
            vertical_scroll_enabled,
        } => {
            stretch_non_scrolling_child(
                element,
                horizontal_scroll_enabled,
                vertical_scroll_enabled,
            );
        }
    }
}

/// Removes the parent container's record and restores the placement the parent had
/// overridden.
pub(crate) fn clear_parent_layout(element: &UIElement) {
    let key = element_key(element);
    let Some(mut state) = LAYOUT_STATES.with(|states| states.borrow_mut().remove(&key)) else {
        return;
    };

    if state.parent.is_some() {
        if let Ok(framework) = element.cast::<FrameworkElement>() {
            if let Some(value) = state.restore_horizontal {
                let _ = framework.SetHorizontalAlignment(value);
            }
            if let Some(value) = state.restore_vertical {
                let _ = framework.SetVerticalAlignment(value);
            }
        }
    }
    state.parent = None;
    state.restore_horizontal = None;
    state.restore_vertical = None;
    if state.fill_width || state.fill_height {
        LAYOUT_STATES.with(|states| {
            states.borrow_mut().insert(key, state);
        });
    }
}

/// Removes the element's state from the table when the widget is destroyed.
pub(crate) fn clear_layout_state(element: &UIElement) {
    let key = element_key(element);
    LAYOUT_STATES.with(|states| {
        states.borrow_mut().remove(&key);
    });
}

/// Applies the size specification to the element. Each call replaces the previous
/// one.
pub(crate) fn apply_sizing(element: &UIElement, sizing: Sizing) {
    let Ok(framework) = element.cast::<FrameworkElement>() else {
        return;
    };
    set_sizing_state(element, sizing);
    // WinUI では NaN が「中身に合わせる」を表す。
    let _ = framework.SetWidth(sizing.width.fixed_value().unwrap_or(f64::NAN));
    let _ = framework.SetHeight(sizing.height.fixed_value().unwrap_or(f64::NAN));
    let _ = framework.SetMinWidth(sizing.min_width.unwrap_or(0.0));
    let _ = framework.SetMinHeight(sizing.min_height.unwrap_or(0.0));
    let _ = framework.SetMaxWidth(sizing.max_width.unwrap_or(f64::INFINITY));
    let _ = framework.SetMaxHeight(sizing.max_height.unwrap_or(f64::INFINITY));

    let _ = framework.SetHorizontalAlignment(match sizing.width {
        Length::Fill => HorizontalAlignment::Stretch,
        _ => HorizontalAlignment::Left,
    });
    let _ = framework.SetVerticalAlignment(match sizing.height {
        Length::Fill => VerticalAlignment::Stretch,
        _ => VerticalAlignment::Top,
    });
    reapply_parent_layout(&framework);
    refit_in_grid(element);
}

/// Maps `Stack::set_align` onto the StackPanel's children.
///
/// The StackPanel's own alignment only changes where the panel sits within its
/// parent, so cross-axis alignment is set on the children. A child's own `Fill`, if
/// any, takes precedence.
fn apply_stack_alignment(element: &FrameworkElement, align: Align, vertical_stack: bool) {
    if wants_fill(element, vertical_stack) {
        return;
    }
    if vertical_stack {
        let value = match align {
            Align::Start => HorizontalAlignment::Left,
            Align::Center => HorizontalAlignment::Center,
            Align::End => HorizontalAlignment::Right,
            Align::Fill => HorizontalAlignment::Stretch,
        };
        let _ = element.SetHorizontalAlignment(value);
    } else {
        let value = match align {
            Align::Start => VerticalAlignment::Top,
            Align::Center => VerticalAlignment::Center,
            Align::End => VerticalAlignment::Bottom,
            Align::Fill => VerticalAlignment::Stretch,
        };
        let _ = element.SetVerticalAlignment(value);
    }
}

fn grid_length(track: Track) -> GridLength {
    match track {
        Track::Auto => GridLength {
            Value: 1.0,
            GridUnitType: GridUnitType::Auto,
        },
        Track::Fixed(value) => GridLength {
            Value: value,
            GridUnitType: GridUnitType::Pixel,
        },
        Track::Fill(_) => GridLength {
            Value: track.weight(),
            GridUnitType: GridUnitType::Star,
        },
    }
}

// ----------------------------------------------------------------- Spacer

struct SpacerInner {
    native: XamlGrid,
}

/// A widget that is just empty space (an empty Grid).
///
/// WinUI's `StackPanel` does not hand out leftover space to children, so inside a
/// `Stack` it takes no space. To split leftover space, use [`Track::Fill`] of a
/// [`Grid`].
#[derive(Clone)]
pub struct Spacer(Rc<SpacerInner>);
impl_widget!(Spacer, native);

impl Spacer {
    pub(crate) fn new() -> Result<Self> {
        let native = XamlGrid::new().map_err(|e| to_error("Spacer の生成", e))?;
        let this = Self(Rc::new(SpacerInner { native }));
        this.set_sizing(Sizing::fill());
        Ok(this)
    }
}

// ------------------------------------------------------------------- Grid

/// A child placed in a [`Grid`].
struct PlacedChild {
    cell: GridCell,
    /// Kept so that the widget lives while it is placed.
    widget: Box<dyn Widget>,
    /// The one-column Grid that holds the child, when its `Fill` width has a maximum
    /// (see [`wrap_at_start`]).
    wrapper: Option<XamlGrid>,
}

struct GridInner {
    native: XamlGrid,
    /// The placed children and their positions. Kept to remove them by cell.
    children: RefCell<Vec<PlacedChild>>,
    columns: Cell<usize>,
    rows: Cell<usize>,
}

impl Drop for GridInner {
    fn drop(&mut self) {
        let this: *const GridInner = self;
        let Ok(children) = self.children.try_borrow() else {
            return;
        };
        let _ = GRID_PARENTS.try_with(|parents| {
            let mut parents = parents.borrow_mut();
            for placed in children.iter() {
                let key = element_key(&placed.widget.native_element());
                if parents
                    .get(&key)
                    .is_some_and(|grid| std::ptr::eq(grid.as_ptr(), this))
                {
                    parents.remove(&key);
                }
            }
        });
    }
}

/// A container that positions children by row and column (WinUI 3's Grid).
#[derive(Clone)]
pub struct Grid(Rc<GridInner>);
impl_widget!(Grid, native);

impl Grid {
    pub(crate) fn new() -> Result<Self> {
        let native = XamlGrid::new().map_err(|e| to_error("Grid の生成", e))?;
        Ok(Self(Rc::new(GridInner {
            native,
            children: RefCell::new(Vec::new()),
            columns: Cell::new(0),
            rows: Cell::new(0),
        })))
    }

    /// The spacing between columns and between rows.
    pub fn set_spacing(&self, column: f64, row: f64) {
        let _ = self.0.native.SetColumnSpacing(column);
        let _ = self.0.native.SetRowSpacing(row);
    }

    /// The outer padding.
    pub fn set_padding(&self, padding: Padding) {
        let _ = self.0.native.SetPadding(Thickness {
            Left: padding.left,
            Top: padding.top,
            Right: padding.right,
            Bottom: padding.bottom,
        });
    }

    /// Places a child at the given position. Missing rows and columns are added
    /// automatically.
    pub fn attach(&self, child: &dyn Widget, cell: GridCell) {
        self.ensure_size(cell.columns_needed(), cell.rows_needed());
        let element = child.native_element();
        let Ok(child_framework) = element.cast::<FrameworkElement>() else {
            return;
        };
        let wrapper = match capped_fill_width(&child_framework) {
            Some(max_width) => match wrap_at_start(&element, max_width) {
                Some(wrapper) => Some(wrapper),
                // 包めないのは、すでに別の親に属しているとき。そのまま置いても
                // 同じ理由で失敗する。
                None => return,
            },
            None => None,
        };
        let placed: UIElement = match &wrapper {
            Some(wrapper) => match wrapper.cast() {
                Ok(placed) => placed,
                Err(_) => {
                    release(wrapper);
                    return;
                }
            },
            None => element.clone(),
        };
        let appended = self
            .0
            .native
            .Children()
            .and_then(|children| children.Append(&placed));
        if appended.is_ok() {
            // 置き場所は添付プロパティなので、FrameworkElement として設定する。
            // Append に成功してから設定することで、すでに別の親に属している
            // 要素の追加に失敗しても、既存の配置を変更しない。
            if let Ok(framework) = placed.cast::<FrameworkElement>() {
                place_in_cell(&framework, &child_framework, cell);
            }
            self.0.children.borrow_mut().push(PlacedChild {
                cell,
                widget: child.boxed_clone(),
                wrapper,
            });
            GRID_PARENTS.with(|parents| {
                parents
                    .borrow_mut()
                    .insert(element_key(&element), Rc::downgrade(&self.0));
            });
        } else if let Some(wrapper) = &wrapper {
            release(wrapper);
        }
    }

    /// Replaces what is in the cell. Whatever was in the same cell is removed.
    ///
    /// Used to safely replace controls, such as `MediaPlayerElement`, for which WinUI
    /// internally runs template application when the TabView switches content.
    pub fn replace(&self, child: &dyn Widget, cell: GridCell) {
        self.remove(cell);
        self.attach(child, cell);
    }

    /// Removes whatever is in the given cell. Does nothing if it is empty.
    ///
    /// Only the column and row of `cell` are considered, not the span.
    pub fn remove(&self, cell: GridCell) {
        let mut children = self.0.children.borrow_mut();
        let mut index = 0;
        while index < children.len() {
            let placed = &children[index].cell;
            if placed.column != cell.column || placed.row != cell.row {
                index += 1;
                continue;
            }
            // `attach` は Children へ順に足すので、位置はここの並びと同じ。
            let removed = self
                .0
                .native
                .Children()
                .and_then(|c| c.RemoveAt(index as u32));
            if removed.is_err() {
                index += 1;
                continue;
            }
            self.forget(children.remove(index));
        }
    }

    /// Removes every child. The row and column settings stay.
    pub fn clear(&self) {
        if let Ok(children) = self.0.native.Children() {
            let _ = children.Clear();
        }
        let removed: Vec<PlacedChild> = self.0.children.borrow_mut().drain(..).collect();
        for placed in removed {
            self.forget(placed);
        }
    }

    /// Lets go of a child already taken out of `native`: takes it out of its wrapper
    /// and forgets that it was placed here.
    fn forget(&self, placed: PlacedChild) {
        if let Some(wrapper) = &placed.wrapper {
            release(wrapper);
        }
        let key = element_key(&placed.widget.native_element());
        GRID_PARENTS.with(|parents| {
            let mut parents = parents.borrow_mut();
            if parents
                .get(&key)
                .is_some_and(|grid| std::ptr::eq(grid.as_ptr(), Rc::as_ptr(&self.0)))
            {
                parents.remove(&key);
            }
        });
    }

    /// Places a child again after its `Sizing` changed (see [`refit_in_grid`]).
    ///
    /// A child that now has a `Fill` width with a maximum goes into the wrapper of
    /// [`wrap_at_start`], one that no longer has it comes out, and a wrapped child's
    /// new maximum is copied to the wrapper's column. The cell's vertical alignment
    /// is set again too, since `apply_sizing` overwrote it.
    fn refit(&self, element: &UIElement) {
        let Ok(child_framework) = element.cast::<FrameworkElement>() else {
            return;
        };
        let Ok(native_children) = self.0.native.Children() else {
            return;
        };
        let key = element_key(element);
        let mut children = self.0.children.borrow_mut();
        let Some(position) = children
            .iter()
            .position(|placed| element_key(&placed.widget.native_element()) == key)
        else {
            return;
        };
        // `attach` は Children へ順に足すので、位置はここの並びと同じ。
        let index = position as u32;
        let placed = &mut children[position];
        match (placed.wrapper.clone(), capped_fill_width(&child_framework)) {
            (Some(wrapper), Some(max_width)) => {
                if let Ok(column) = wrapper
                    .ColumnDefinitions()
                    .and_then(|definitions| definitions.GetAt(0))
                {
                    let _ = column.SetMaxWidth(max_width);
                }
            }
            (Some(wrapper), None) => {
                release(&wrapper);
                if native_children.SetAt(index, element).is_ok() {
                    placed.wrapper = None;
                } else {
                    let _ = wrapper.Children().and_then(|c| c.Append(element));
                }
            }
            (None, Some(max_width)) => {
                // 包みを先にマスへ置いて子を外してから、子を包みへ入れる。
                if let Some((wrapper, placed_wrapper)) = empty_wrapper(max_width)
                    .and_then(|wrapper| Some((wrapper.clone(), wrapper.cast::<UIElement>().ok()?)))
                {
                    if native_children.SetAt(index, &placed_wrapper).is_ok() {
                        if wrapper.Children().and_then(|c| c.Append(element)).is_ok() {
                            placed.wrapper = Some(wrapper);
                        } else {
                            let _ = native_children.SetAt(index, element);
                        }
                    }
                }
            }
            (None, None) => {}
        }
        let framework = match &placed.wrapper {
            Some(wrapper) => wrapper.cast::<FrameworkElement>().ok(),
            None => Some(child_framework.clone()),
        };
        if let Some(framework) = framework {
            place_in_cell(&framework, &child_framework, placed.cell);
        }
    }

    /// How a column's width is determined.
    pub fn set_column_track(&self, index: usize, track: Track) {
        self.ensure_size(index + 1, 0);
        if let Ok(definition) = self
            .0
            .native
            .ColumnDefinitions()
            .and_then(|definitions| definitions.GetAt(index as u32))
        {
            let _ = definition.SetWidth(grid_length(track));
        }
    }

    /// How a row's height is determined.
    pub fn set_row_track(&self, index: usize, track: Track) {
        self.ensure_size(0, index + 1);
        if let Ok(definition) = self
            .0
            .native
            .RowDefinitions()
            .and_then(|definitions| definitions.GetAt(index as u32))
        {
            let _ = definition.SetHeight(grid_length(track));
        }
    }

    /// The current number of columns.
    pub fn columns(&self) -> usize {
        self.0.columns.get()
    }

    /// The current number of rows.
    pub fn rows(&self) -> usize {
        self.0.rows.get()
    }

    /// The number of children placed.
    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn ensure_size(&self, columns: usize, rows: usize) {
        while self.0.columns.get() < columns {
            let Ok(definition) = ColumnDefinition::new() else {
                break;
            };
            // 既定 (Star) ではなく「中身に合わせる」から始める。
            let _ = definition.SetWidth(grid_length(Track::Auto));
            let appended = self
                .0
                .native
                .ColumnDefinitions()
                .and_then(|definitions| definitions.Append(&definition));
            if appended.is_err() {
                break;
            }
            self.0.columns.set(self.0.columns.get() + 1);
        }
        while self.0.rows.get() < rows {
            let Ok(definition) = RowDefinition::new() else {
                break;
            };
            let _ = definition.SetHeight(grid_length(Track::Auto));
            let appended = self
                .0
                .native
                .RowDefinitions()
                .and_then(|definitions| definitions.Append(&definition));
            if appended.is_err() {
                break;
            }
            self.0.rows.set(self.0.rows.get() + 1);
        }
    }
}

/// Sets the cell on what is put into the Grid (the child, or its wrapper) and its
/// vertical alignment, from the child's `Sizing`.
fn place_in_cell(placed: &FrameworkElement, child: &FrameworkElement, cell: GridCell) {
    let _ = XamlGrid::SetColumn(placed, cell.column as i32);
    let _ = XamlGrid::SetRow(placed, cell.row as i32);
    let _ = XamlGrid::SetColumnSpan(placed, cell.column_span as i32);
    let _ = XamlGrid::SetRowSpan(placed, cell.row_span as i32);
    // 縦は中央ぞろえ。既定の Stretch のままだと、同じ行に置いた
    // ラベルと入力欄のように高さの違うものが上端で揃ってしまう。
    let _ = placed.SetVerticalAlignment(if wants_fill(child, false) {
        VerticalAlignment::Stretch
    } else {
        VerticalAlignment::Center
    });
}

/// Lets the Grid the element is placed in, if any, place it again after
/// `apply_sizing` (see [`Grid::refit`]).
fn refit_in_grid(element: &UIElement) {
    let grid = GRID_PARENTS.with(|parents| {
        parents
            .borrow()
            .get(&element_key(element))
            .and_then(Weak::upgrade)
    });
    if let Some(grid) = grid {
        Grid(grid).refit(element);
    }
}

/// The maximum width of a child whose width is `Fill` with a maximum.
fn capped_fill_width(element: &FrameworkElement) -> Option<f64> {
    let max_width = element.MaxWidth().ok()?;
    (wants_fill(element, true) && max_width.is_finite()).then_some(max_width)
}

/// Puts a child whose `Fill` width has a maximum into a one-column Grid, so that it
/// sits at the start of a wider cell.
///
/// WinUI centers a `Stretch` element that its `MaxWidth` stops short of the slot.
/// The other backends put such a child where an `Auto` child would go, at the start
/// of the cell (GTK's `Start`, `NSGridView`'s `Leading`, CSS Grid's `start`). `Left`
/// alignment alone would not do: the child would shrink to its content instead of
/// growing up to the maximum. A `Star` column with the same `MaxWidth` takes
/// `min(cell, maximum)` and leaves the rest of the cell empty after it, and the
/// child stretches to that column.
fn wrap_at_start(element: &UIElement, max_width: f64) -> Option<XamlGrid> {
    let wrapper = empty_wrapper(max_width)?;
    wrapper
        .Children()
        .and_then(|children| children.Append(element))
        .ok()?;
    Some(wrapper)
}

/// The one-column Grid of [`wrap_at_start`], with nothing in it yet.
fn empty_wrapper(max_width: f64) -> Option<XamlGrid> {
    let wrapper = XamlGrid::new().ok()?;
    let column = ColumnDefinition::new().ok()?;
    column.SetWidth(grid_length(Track::FILL)).ok()?;
    column.SetMaxWidth(max_width).ok()?;
    wrapper
        .ColumnDefinitions()
        .and_then(|definitions| definitions.Append(&column))
        .ok()?;
    Some(wrapper)
}

/// Takes the child out of the Grid made by [`wrap_at_start`], so that it can be put
/// into another container.
fn release(wrapper: &XamlGrid) {
    if let Ok(children) = wrapper.Children() {
        let _ = children.Clear();
    }
}

// ----------------------------------------------------------------- Scroll

struct ScrollInner {
    native: ScrollViewer,
    child: RefCell<Option<Box<dyn Widget>>>,
    horizontal_scroll_enabled: Cell<bool>,
    vertical_scroll_enabled: Cell<bool>,
    /// State for choosing only the ScrollViewer directly under the pointer on wheel
    /// input.
    hovered: std::sync::Arc<crate::ui_thread::UiThreadCell<usize>>,
    /// `ViewChanged` also arrives for intermediate progress, so it notifies only when
    /// the position changed.
    on_scroll: ScrollNotifier,
    /// The destination requested before the size was decided (before it is on screen,
    /// for example). Applied once the size is decided (`SizeChanged`).
    pending: Cell<Option<ScrollTarget>>,
}

impl Drop for ScrollInner {
    fn drop(&mut self) {
        if let Ok(child) = self.child.try_borrow() {
            if let Some(child) = child.as_ref() {
                clear_parent_layout(&child.native_element());
            }
        }
    }
}

thread_local! {
    static LAYOUT_STATES: RefCell<HashMap<usize, LayoutState>> =
        RefCell::new(HashMap::new());
    /// The Grid each placed child is in, so that `apply_sizing` can have it placed
    /// again.
    static GRID_PARENTS: RefCell<HashMap<usize, Weak<GridInner>>> =
        RefCell::new(HashMap::new());
    static SCROLLS: RefCell<Vec<Weak<ScrollInner>>> = const { RefCell::new(Vec::new()) };
    static LIST_SCROLLS: RefCell<Vec<Weak<ListScrollTarget>>> = const { RefCell::new(Vec::new()) };
    static WHEEL_TARGETS: RefCell<Vec<windows::Win32::Foundation::HWND>> =
        const { RefCell::new(Vec::new()) };
    static WHEEL_HOOK: RefCell<Option<windows::Win32::UI::WindowsAndMessaging::HHOOK>> =
        const { RefCell::new(None) };
}

const WHEEL_SUBCLASS_ID: usize = 0x4D49_5549;

unsafe extern "system" fn low_level_mouse_hook(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetAncestor, GetForegroundWindow, GA_ROOT, MSLLHOOKSTRUCT, WM_MOUSEHWHEEL,
        WM_MOUSEWHEEL,
    };

    if code >= 0 {
        let input = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let foreground_root = GetAncestor(GetForegroundWindow(), GA_ROOT);
        let over_target_window = WHEEL_TARGETS.with(|targets| {
            targets
                .borrow()
                .iter()
                .any(|target| GetAncestor(*target, GA_ROOT) == foreground_root)
        });

        let message = wparam.0 as u32;
        if over_target_window && (message == WM_MOUSEWHEEL || message == WM_MOUSEHWHEEL) {
            let delta = ((input.mouseData >> 16) as i16) as f64;
            let horizontal = message == WM_MOUSEHWHEEL;
            // ポインターの下の Canvas の `on_wheel` が先。`Handled` なら外側の
            // Scroll へは回さない。
            let handled = delta != 0.0
                && (crate::canvas::dispatch_wheel(delta, horizontal) == Some(true)
                    || (!horizontal && apply_wheel_delta(delta)));
            if handled {
                return windows::Win32::Foundation::LRESULT(1);
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

fn install_low_level_wheel_hook(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WH_MOUSE_LL};

    WHEEL_TARGETS.with(|targets| {
        let mut targets = targets.borrow_mut();
        if !targets.contains(&hwnd) {
            targets.push(hwnd);
        }
    });
    WHEEL_HOOK.with(|hook| {
        let mut hook = hook.borrow_mut();
        if hook.is_some() {
            return true;
        }
        let Ok(handle) =
            (unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_hook), None, 0) })
        else {
            return false;
        };
        *hook = Some(handle);
        true
    })
}

fn apply_wheel_delta(delta: f64) -> bool {
    if apply_list_wheel_delta(delta) {
        return true;
    }
    apply_scroll_wheel_delta(delta)
}

/// Scrolls only the deepest Scroll in the visual tree among those under the pointer.
///
/// Even when a scrollable inner Scroll has reached its end, it consumes the input,
/// preventing unintended scroll chaining to the outer one. Only when the inner one
/// has no overflow is it passed outward.
fn apply_scroll_wheel_delta(delta: f64) -> bool {
    SCROLLS.with(|scrolls| {
        let mut scrolls = scrolls.borrow_mut();
        scrolls.retain(|scroll| scroll.strong_count() != 0);
        let target = scrolls
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|inner| inner.vertical_scroll_enabled.get())
            .filter(|inner| inner.hovered.with_mut(|depth| *depth != 0))
            .filter(|inner| inner.native.ScrollableHeight().unwrap_or(0.0) > 0.0)
            .filter_map(|inner| visual_depth(&inner.native).map(|depth| (depth, inner)))
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, inner)| inner);

        let Some(inner) = target else {
            return false;
        };
        let current = inner.native.VerticalOffset().unwrap_or(0.0);
        let maximum = inner.native.ScrollableHeight().unwrap_or(0.0);
        let next = (current - delta).clamp(0.0, maximum);
        if next != current {
            let _ = inner.native.ScrollToVerticalOffset(next);
        }
        true
    })
}

fn visual_depth(native: &ScrollViewer) -> Option<usize> {
    let mut current = Some(native.cast::<FrameworkElement>().ok()?);
    let mut depth = 0;
    while let Some(element) = current {
        let ui_element = element.cast::<UIElement>().ok()?;
        if ui_element.Visibility().ok()? != Visibility::Visible {
            return None;
        }
        current = element
            .Parent()
            .ok()
            .and_then(|parent| parent.cast::<FrameworkElement>().ok());
        depth += 1;
    }
    Some(depth)
}

fn apply_list_wheel_delta(delta: f64) -> bool {
    LIST_SCROLLS.with(|targets| {
        let mut targets = targets.borrow_mut();
        targets.retain(|target| target.strong_count() != 0);
        for weak in targets.iter().rev() {
            let Some(target) = weak.upgrade() else {
                continue;
            };
            let hovered = target.hovered.with_mut(|depth| *depth != 0);
            if !hovered {
                continue;
            }
            let current = target.native.VerticalOffset().unwrap_or(0.0);
            let maximum = target.native.ScrollableHeight().unwrap_or(0.0);
            if maximum <= 0.0 {
                continue;
            }
            let next = (current - delta).clamp(0.0, maximum);
            if next != current {
                let _ = target.native.ScrollToVerticalOffset(next);
            }
            // List が端に達していても外側の Scroll へ連鎖させない。
            return true;
        }
        false
    })
}

unsafe extern "system" fn wheel_subclass(
    hwnd: windows::Win32::Foundation::HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    subclass_id: usize,
    _ref_data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{WM_MOUSEWHEEL, WM_POINTERWHEEL};

    if message == WM_MOUSEWHEEL || message == WM_POINTERWHEEL {
        let delta = ((wparam.0 >> 16) as i16) as f64;
        if delta != 0.0 && apply_wheel_delta(delta) {
            return windows::Win32::Foundation::LRESULT(0);
        }
    }

    if message == windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY {
        let result = DefSubclassProc(hwnd, message, wparam, lparam);
        let _ = RemoveWindowSubclass(hwnd, Some(wheel_subclass), subclass_id);
        return result;
    }

    DefSubclassProc(hwnd, message, wparam, lparam)
}

pub(crate) fn register_scroll(scroll: &Scroll) {
    SCROLLS.with(|scrolls| {
        let mut scrolls = scrolls.borrow_mut();
        scrolls.retain(|scroll| scroll.strong_count() != 0);
        scrolls.push(Rc::downgrade(&scroll.0));
    });
}

fn track_scroll_pointer(scroll: &Scroll) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::Input::PointerEventHandler;

    let entered_state = scroll.0.hovered.clone();
    let entered = PointerEventHandler::new(move |_, _| {
        entered_state.with_mut(|depth| *depth = depth.saturating_add(1));
        Ok(())
    });
    scroll
        .0
        .native
        .PointerEntered(&entered)
        .map_err(|e| to_error("Scroll のポインター購読", e))?;

    let exited_state = scroll.0.hovered.clone();
    let exited = PointerEventHandler::new(move |_, _| {
        exited_state.with_mut(|depth| *depth = depth.saturating_sub(1));
        Ok(())
    });
    scroll
        .0
        .native
        .PointerExited(&exited)
        .map_err(|e| to_error("Scroll のポインター購読", e))?;

    // タブ切り替えなどで PointerEntered が発生しない場合にも、次のホイール
    // 入力までに対象を確定できるよう PointerMoved で補正する。
    let moved_state = scroll.0.hovered.clone();
    let moved = PointerEventHandler::new(move |_, _| {
        moved_state.with_mut(|depth| {
            if *depth == 0 {
                *depth = 1;
            }
        });
        Ok(())
    });
    scroll
        .0
        .native
        .PointerMoved(&moved)
        .map_err(|e| to_error("Scroll のポインター購読", e))?;

    Ok(())
}

pub(crate) struct ListScrollTarget {
    native: ScrollViewer,
    hovered: std::sync::Arc<crate::ui_thread::UiThreadCell<usize>>,
}

/// Looks for the first `ScrollViewer` inside the element, depth-first.
///
/// `ListView` and `TreeView` have their own `ScrollViewer` inside their templates.
/// How many levels deep depends on the template, so it is found by type, not name.
/// The contents are not assembled until `Loaded`, so call it after waiting for that.
pub(crate) fn scroll_viewer_within<T: Interface>(root: &T) -> Option<ScrollViewer> {
    descendant_of_type(root)
}

/// The first element among `root` itself and its descendants that can be treated as
/// `D` (depth-first).
///
/// Finds parts inside templates by type. As with [`scroll_viewer_within`], the
/// contents are not assembled until `Loaded`.
pub(crate) fn descendant_of_type<T: Interface, D: Interface>(root: &T) -> Option<D> {
    let root = root.cast::<DependencyObject>().ok()?;
    let mut queue = vec![root];
    while let Some(element) = queue.pop() {
        if let Ok(found) = element.cast::<D>() {
            return Some(found);
        }
        let count = VisualTreeHelper::GetChildrenCount(&element).unwrap_or(0);
        for index in (0..count).rev() {
            if let Ok(child) = VisualTreeHelper::GetChild(&element, index) {
                queue.push(child);
            }
        }
    }
    None
}

pub(crate) fn register_list_scroll(
    native: ScrollViewer,
    hovered: std::sync::Arc<crate::ui_thread::UiThreadCell<usize>>,
) -> Rc<ListScrollTarget> {
    let target = Rc::new(ListScrollTarget { native, hovered });
    LIST_SCROLLS.with(|targets| {
        let mut targets = targets.borrow_mut();
        targets.retain(|target| target.strong_count() != 0);
        targets.push(Rc::downgrade(&target));
    });
    target
}

pub(crate) fn install_wheel_subclass(window: &XamlWindow) -> bool {
    let Ok(native) = window.cast::<naui_winui3::IWindowNative>() else {
        return false;
    };
    let Ok(hwnd) = (unsafe { native.WindowHandle() }) else {
        return false;
    };
    let low_level_hook_installed = install_low_level_wheel_hook(hwnd);
    let has_scroll = SCROLLS.with(|scrolls| {
        let mut scrolls = scrolls.borrow_mut();
        scrolls.retain(|scroll| scroll.strong_count() != 0);
        !scrolls.is_empty()
    });
    if !has_scroll {
        return low_level_hook_installed;
    }
    let installed = unsafe {
        windows::Win32::UI::Shell::SetWindowSubclass(
            hwnd,
            Some(wheel_subclass),
            WHEEL_SUBCLASS_ID,
            1,
        )
        .as_bool()
    };
    installed || low_level_hook_installed
}

/// A container that scrolls when its contents overflow (ScrollViewer).
#[derive(Clone)]
pub struct Scroll(Rc<ScrollInner>);
impl_widget!(Scroll, native);

/// Whether scrolling is allowed horizontally and vertically.
///
/// `ScrollBarVisibility` is how the scroll bar is shown; what stops scrolling itself
/// is `ScrollMode`. Unless both match, they disagree: the bar disappears yet the
/// wheel still scrolls.
fn set_scroll_mode(native: &ScrollViewer, horizontal: ScrollPolicy, vertical: ScrollPolicy) {
    fn mode(policy: ScrollPolicy) -> ScrollMode {
        match policy {
            ScrollPolicy::Never => ScrollMode::Disabled,
            _ => ScrollMode::Enabled,
        }
    }
    let _ = native.SetHorizontalScrollMode(mode(horizontal));
    let _ = native.SetVerticalScrollMode(mode(vertical));
}

impl Scroll {
    pub(crate) fn new() -> Result<Self> {
        let native = ScrollViewer::new().map_err(|e| to_error("ScrollViewer の生成", e))?;
        let this = Self(Rc::new(ScrollInner {
            native,
            child: RefCell::new(None),
            horizontal_scroll_enabled: Cell::new(false),
            vertical_scroll_enabled: Cell::new(true),
            hovered: std::sync::Arc::new(crate::ui_thread::UiThreadCell::new(0)),
            on_scroll: ScrollNotifier::default(),
            pending: Cell::new(None),
        }));
        register_scroll(&this);
        this.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
        track_scroll_pointer(&this)?;
        this.track_view()?;
        Ok(this)
    }

    fn track_view(&self) -> Result<()> {
        let target = UiThreadCell::new(Rc::downgrade(&self.0));
        let changed = EventHandler::<ScrollViewerViewChangedEventArgs>::new(move |_, _| {
            // 借用を抱えたまま通知すると、通知の中の `scroll_to` で入れ子に
            // なったときに借りられないので、取り出してから呼ぶ。
            if let Some(inner) = target.try_with_mut(|weak| weak.upgrade()).flatten() {
                Scroll(inner).notify_if_moved();
            }
            Ok(())
        });
        self.0
            .native
            .ViewChanged(&changed)
            .map_err(|e| to_error("Scroll の位置の購読", e))?;

        let target = UiThreadCell::new(Rc::downgrade(&self.0));
        let resized = SizeChangedEventHandler::new(move |_, _| {
            if let Some(inner) = target.try_with_mut(|weak| weak.upgrade()).flatten() {
                // レイアウトは済んでいるので、ここで UpdateLayout を呼び直さない。
                let scroll = Scroll(inner);
                scroll.apply_pending(&scroll.read_metrics());
            }
            Ok(())
        });
        self.0
            .native
            .SizeChanged(&resized)
            .map_err(|e| to_error("Scroll の大きさの購読", e))?;
        Ok(())
    }

    /// The current scroll position and sizes.
    ///
    /// Even if the contents just changed, it measures after finishing layout.
    pub fn metrics(&self) -> ScrollMetrics {
        let _ = self.0.native.UpdateLayout();
        self.read_metrics()
    }

    /// Scrolls to the given position, clamped to the scrollable range.
    ///
    /// If the size is not decided yet (before it is on screen, for example), it scrolls
    /// once it is.
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
        self.0.on_scroll.set(&self.read_metrics(), f);
    }

    fn read_metrics(&self) -> ScrollMetrics {
        let native = &self.0.native;
        let viewport_width = native.ViewportWidth().unwrap_or(0.0);
        let viewport_height = native.ViewportHeight().unwrap_or(0.0);
        ScrollMetrics {
            x: native.HorizontalOffset().unwrap_or(0.0),
            y: native.VerticalOffset().unwrap_or(0.0),
            viewport_width,
            viewport_height,
            content_width: native.ExtentWidth().unwrap_or(0.0).max(viewport_width),
            content_height: native.ExtentHeight().unwrap_or(0.0).max(viewport_height),
        }
    }

    fn request(&self, target: ScrollTarget) {
        self.0.pending.set(Some(target));
        self.apply_pending(&self.metrics());
    }

    /// Scrolls to the remembered destination. If the size is not decided, keeps
    /// remembering it.
    fn apply_pending(&self, metrics: &ScrollMetrics) {
        let Some(target) = self.0.pending.get() else {
            return;
        };
        if metrics.viewport_width <= 0.0 || metrics.viewport_height <= 0.0 {
            return;
        }
        self.0.pending.set(None);
        let (x, y) = target.resolve(metrics);
        if (x, y) == (metrics.x, metrics.y) {
            return;
        }
        let (Some(x), Some(y)) = (offset_value(x), offset_value(y)) else {
            return;
        };
        // アニメーションさせずに、その場で送る。
        let _ = self
            .0
            .native
            .ChangeViewWithOptionalAnimation(&x, &y, None, true);
    }

    fn notify_if_moved(&self) {
        self.0.on_scroll.notify_if_moved(|| self.read_metrics());
    }

    /// Whether scrolling is allowed horizontally and vertically. The default is
    /// horizontal `Never`, vertical `Auto`.
    pub fn set_policy(&self, horizontal: ScrollPolicy, vertical: ScrollPolicy) {
        self.0
            .horizontal_scroll_enabled
            .set(horizontal.is_enabled());
        self.0
            .vertical_scroll_enabled
            .set(!matches!(vertical, ScrollPolicy::Never));
        set_scroll_mode(&self.0.native, horizontal, vertical);
        // `Control` の内容配置の既定は左上寄せなので、スクロールしない軸
        // だけはビューポートいっぱいへ広げる。スクロールする軸は内容の
        // 自然な大きさを残し、必要ならそのまま送れるようにする。
        let _ = self
            .0
            .native
            .SetHorizontalContentAlignment(if horizontal.is_enabled() {
                HorizontalAlignment::Left
            } else {
                HorizontalAlignment::Stretch
            });
        let _ = self
            .0
            .native
            .SetVerticalContentAlignment(if vertical.is_enabled() {
                VerticalAlignment::Top
            } else {
                VerticalAlignment::Stretch
            });
        if let Some(child) = self.0.child.borrow().as_ref() {
            set_scroll_parent(
                &child.native_element(),
                horizontal.is_enabled(),
                vertical.is_enabled(),
            );
        }
        let _ = self
            .0
            .native
            .SetHorizontalScrollBarVisibility(visibility(horizontal));
        let _ = self
            .0
            .native
            .SetVerticalScrollBarVisibility(visibility(vertical));
    }

    /// The content to scroll. Each call replaces it.
    pub fn set_child(&self, child: &dyn Widget) {
        let element = child.native_element();
        let previous = self
            .0
            .child
            .borrow()
            .as_ref()
            .map(|child| child.native_element());
        if self.0.native.SetContent(&element).is_ok() {
            set_scroll_parent(
                &element,
                self.0.horizontal_scroll_enabled.get(),
                self.0.vertical_scroll_enabled.get(),
            );
            if let Some(previous) = previous {
                if element_key(&previous) != element_key(&element) {
                    clear_parent_layout(&previous);
                }
            }
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }
}

/// Creates the position `ChangeView` takes (`IReference<f64>`).
fn offset_value(value: f64) -> Option<IReference<f64>> {
    PropertyValue::CreateDouble(value)
        .and_then(|value| value.cast::<IReference<f64>>())
        .ok()
}

/// Stretches the content on the non-scrolling axis to the viewport's size.
///
/// Children with an explicit size or maximum keep the app's setting. For `Fill`,
/// `HorizontalAlignment` / `VerticalAlignment` are already `Stretch`, so writing the
/// same value here changes nothing.
fn stretch_non_scrolling_child(
    element: &FrameworkElement,
    horizontal_scroll_enabled: bool,
    vertical_scroll_enabled: bool,
) {
    if !horizontal_scroll_enabled {
        let width = element.Width().unwrap_or(f64::NAN);
        let max_width = element.MaxWidth().unwrap_or(f64::INFINITY);
        if width.is_nan() && max_width.is_infinite() {
            let _ = element.SetHorizontalAlignment(HorizontalAlignment::Stretch);
        }
    }
    if !vertical_scroll_enabled {
        let height = element.Height().unwrap_or(f64::NAN);
        let max_height = element.MaxHeight().unwrap_or(f64::INFINITY);
        if height.is_nan() && max_height.is_infinite() {
            let _ = element.SetVerticalAlignment(VerticalAlignment::Stretch);
        }
    }
}

fn visibility(policy: ScrollPolicy) -> ScrollBarVisibility {
    match policy {
        ScrollPolicy::Auto => ScrollBarVisibility::Auto,
        ScrollPolicy::Always => ScrollBarVisibility::Visible,
        ScrollPolicy::Never => ScrollBarVisibility::Disabled,
    }
}
