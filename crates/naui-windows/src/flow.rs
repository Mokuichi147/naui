//! `Flow`: a `Panel` that wraps its children onto new lines, laid out by naui.
//!
//! WinUI 3 has no panel that wraps children of different widths line by line:
//! `VariableSizedWrapGrid` gives every child the same cell, `ItemsRepeater`'s
//! layouts (`StackLayout`, `UniformGridLayout`) lay out items it creates from data,
//! and `WrapPanel` lives in the Windows Community Toolkit. So naui derives its own
//! panel, the way XAML panels are written: a `Panel` composed with a Rust object that
//! implements `IFrameworkElementOverrides` (`MeasureOverride` / `ArrangeOverride`),
//! and places the children with naui-core's [`FlowLayout`]. The lines are decided in
//! WinUI's own layout pass, so they follow every resize, and the panel's desired
//! height is the height at the width it is offered.
//!
//! WinUI mirrors the arrangement itself in a right-to-left layout (`FlowDirection`),
//! so the panel always places the lines from the left.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;

use naui_core::{flow_line_tops, Align, FlowItem, FlowLayout, Padding, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::{IPanelFactory, Panel};
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, IFrameworkElementOverrides, IFrameworkElementOverrides_Impl,
    IFrameworkElementOverrides_Vtbl, UIElement, Visibility,
};
use naui_winui3::{ChildClass, Compose, CreateInstanceFn};
use windows::Foundation::{Rect, Size};
use windows_core::{
    ComObject, ComObjectInner, ComObjectInterface, IInspectable, IInspectable_Vtbl, IUnknown,
    IUnknownImpl, Interface, InterfaceRef, GUID, HRESULT, HSTRING,
};

use crate::to_error;
use crate::widgets::{impl_widget, Widget};

/// What the panel and its handle share.
struct FlowState {
    /// The children's elements in order.
    children: RefCell<Vec<UIElement>>,
    column_spacing: Cell<f64>,
    row_spacing: Cell<f64>,
    padding: Cell<Padding>,
    align: Cell<Align>,
}

/// One shown child, as measured.
struct Measured {
    element: UIElement,
    /// The size it asks for with no limit on the width.
    natural: Size,
    fill_width: bool,
    fill_height: bool,
    max_width: f64,
}

impl FlowState {
    /// Measures the shown children with no limit on the width (their natural size)
    /// and breaks them into lines for `width`, the room inside the padding; `None`
    /// for the width of one line.
    fn lines(&self, width: Option<f64>) -> (Vec<Measured>, FlowLayout) {
        // 測っている間に表を借りたままにしない (子の測定から naui の処理へ戻る
        // ことがある)。
        let children = self.children.borrow().clone();
        let unlimited = Size {
            Width: f32::INFINITY,
            Height: f32::INFINITY,
        };
        let measured: Vec<Measured> = children
            .into_iter()
            .filter(|element| element.Visibility().ok() != Some(Visibility::Collapsed))
            .map(|element| {
                let _ = element.Measure(unlimited);
                let natural = element.DesiredSize().unwrap_or_default();
                let framework = element.cast::<FrameworkElement>().ok();
                let fill = |horizontal| {
                    framework
                        .as_ref()
                        .is_some_and(|framework| crate::layout::wants_fill(framework, horizontal))
                };
                let max_width = framework
                    .as_ref()
                    .and_then(|framework| framework.MaxWidth().ok())
                    .unwrap_or(f64::INFINITY);
                Measured {
                    fill_width: fill(true),
                    fill_height: fill(false),
                    max_width,
                    natural,
                    element,
                }
            })
            .collect();
        let items: Vec<FlowItem> = measured
            .iter()
            .map(|child| FlowItem {
                // `Fill` の子は行の幅まで縮められる。ほかは測った幅を守る。
                minimum: if child.fill_width {
                    0.0
                } else {
                    child.natural.Width as f64
                },
                natural: child.natural.Width as f64,
                grow: child.fill_width,
            })
            .collect();
        let spacing = self.column_spacing.get();
        let width = match width {
            Some(width) if width.is_finite() => width.max(0.0),
            _ => naui_core::flow_natural_width(&items, spacing),
        };
        let layout = FlowLayout::new(&items, width, spacing, self.align.get());
        (measured, layout)
    }

    /// The width each child gets: its slot, held to its maximum.
    fn widths(measured: &[Measured], layout: &FlowLayout) -> Vec<f64> {
        measured
            .iter()
            .zip(&layout.slots)
            .map(|(child, slot)| slot.width.min(child.max_width))
            .collect()
    }

    /// Each child's height at its width. A child whose width differs from its natural
    /// one (widened by `Fill`, or narrowed to the line) is measured again at it.
    fn heights(measured: &[Measured], widths: &[f64]) -> Vec<f64> {
        measured
            .iter()
            .zip(widths)
            .map(|(child, width)| {
                if (child.natural.Width as f64 - width).abs() > 0.5 {
                    let _ = child.element.Measure(Size {
                        Width: *width as f32,
                        Height: f32::INFINITY,
                    });
                    child
                        .element
                        .DesiredSize()
                        .map(|size| size.Height as f64)
                        .unwrap_or(child.natural.Height as f64)
                } else {
                    child.natural.Height as f64
                }
            })
            .collect()
    }

    fn measure(&self, available: Size) -> Size {
        let padding = self.padding.get();
        let horizontal = padding.start + padding.end;
        let vertical = padding.top + padding.bottom;
        let room = available.Width as f64 - horizontal;
        let (measured, layout) = self.lines(Some(room));
        let widths = Self::widths(&measured, &layout);
        let heights = Self::heights(&measured, &widths);
        let (_, height) = flow_line_tops(&layout.line_heights(&heights), self.row_spacing.get());
        // 幅は 1 行に並べた幅を、与えられた幅までに抑えたもの (CSS の
        // fit-content)。ただし一番広い子より狭くはならない。
        let items_width = layout
            .slots
            .iter()
            .map(|slot| slot.x + slot.width)
            .fold(0.0, f64::max);
        Size {
            Width: (items_width + horizontal) as f32,
            Height: (height + vertical) as f32,
        }
    }

    fn arrange(&self, final_size: Size) -> Size {
        let padding = self.padding.get();
        let room = final_size.Width as f64 - padding.start - padding.end;
        let (measured, layout) = self.lines(Some(room));
        let widths = Self::widths(&measured, &layout);
        let heights = Self::heights(&measured, &widths);
        let line_heights = layout.line_heights(&heights);
        let (tops, _) = flow_line_tops(&line_heights, self.row_spacing.get());
        for (((child, slot), width), height) in measured
            .iter()
            .zip(&layout.slots)
            .zip(&widths)
            .zip(&heights)
        {
            let line_height = line_heights[slot.line];
            let height = if child.fill_height {
                line_height
            } else {
                *height
            };
            let _ = child.element.Arrange(Rect {
                X: (padding.start + slot.x) as f32,
                Y: (padding.top + tops[slot.line] + (line_height - height) / 2.0) as f32,
                Width: *width as f32,
                Height: height as f32,
            });
        }
        final_size
    }
}

/// The Rust half of the composed panel.
pub(crate) struct FlowPanel {
    state: Rc<FlowState>,
}

#[repr(C)]
pub(crate) struct FlowPanelImpl {
    identity: &'static IInspectable_Vtbl,
    overrides: &'static IFrameworkElementOverrides_Vtbl,
    this: FlowPanel,
    count: windows_core::imp::WeakRefCount,
}

impl FlowPanelImpl {
    const VTABLE_IDENTITY: IInspectable_Vtbl = IInspectable_Vtbl::new::<Self, Panel, 0>();
    const VTABLE_OVERRIDES: IFrameworkElementOverrides_Vtbl =
        IFrameworkElementOverrides_Vtbl::new::<Self, -1>();

    fn into_outer(this: FlowPanel) -> Self {
        Self {
            identity: &Self::VTABLE_IDENTITY,
            overrides: &Self::VTABLE_OVERRIDES,
            this,
            count: windows_core::imp::WeakRefCount::new(),
        }
    }
}

impl IFrameworkElementOverrides_Impl for FlowPanelImpl {
    fn MeasureOverride(&self, available: &Size) -> windows_core::Result<Size> {
        Ok(self.this.state.measure(*available))
    }

    fn ArrangeOverride(&self, final_size: &Size) -> windows_core::Result<Size> {
        Ok(self.this.state.arrange(*final_size))
    }

    /// A panel has no template.
    fn OnApplyTemplate(&self) -> windows_core::Result<()> {
        Ok(())
    }

    /// A panel has no visual states (only a `Control` does).
    fn GoToElementStateCore(
        &self,
        _state_name: &HSTRING,
        _use_transitions: bool,
    ) -> windows_core::Result<bool> {
        Ok(false)
    }
}

impl IUnknownImpl for FlowPanelImpl {
    type Impl = FlowPanel;

    fn get_impl(&self) -> &Self::Impl {
        &self.this
    }

    fn get_impl_mut(&mut self) -> &mut Self::Impl {
        &mut self.this
    }

    fn into_inner(self) -> Self::Impl {
        self.this
    }

    fn AddRef(&self) -> u32 {
        self.count.add_ref()
    }

    unsafe fn Release(self_: *mut Self) -> u32 {
        let remaining = (*self_).count.release();
        if remaining == 0 {
            drop(Box::from_raw(self_));
        }
        remaining
    }

    fn is_reference_count_one(&self) -> bool {
        self.count.is_one()
    }

    fn to_object(&self) -> ComObject<Self::Impl> {
        self.count.add_ref();
        unsafe { ComObject::from_raw(NonNull::from(self)) }
    }

    unsafe fn GetTrustLevel(&self, value: *mut i32) -> HRESULT {
        if value.is_null() {
            return HRESULT(-2147467261);
        }
        *value = 0;
        HRESULT(0)
    }

    unsafe fn QueryInterface(
        &self,
        iid: *const GUID,
        interface: *mut *mut std::ffi::c_void,
    ) -> HRESULT {
        if iid.is_null() || interface.is_null() {
            return HRESULT(-2147467261);
        }

        let iid = *iid;
        let interface_ptr =
            if iid == <IInspectable as Interface>::IID || iid == <IUnknown as Interface>::IID {
                &self.identity as *const _ as *const std::ffi::c_void
            } else if IFrameworkElementOverrides_Vtbl::matches(&iid) {
                &self.overrides as *const _ as *const std::ffi::c_void
            } else {
                // ほかは基底の Panel が答える (Compose が内側へ回す)。
                std::ptr::null()
            };

        *interface = interface_ptr as *mut std::ffi::c_void;
        if interface_ptr.is_null() {
            HRESULT(-2147467262)
        } else {
            self.count.add_ref();
            HRESULT(0)
        }
    }
}

impl ComObjectInterface<IInspectable> for FlowPanelImpl {
    fn as_interface_ref(&self) -> InterfaceRef<'_, IInspectable> {
        unsafe { std::mem::transmute(&self.identity) }
    }
}

impl ComObjectInterface<IUnknown> for FlowPanelImpl {
    fn as_interface_ref(&self) -> InterfaceRef<'_, IUnknown> {
        unsafe { std::mem::transmute(&self.identity) }
    }
}

impl ComObjectInterface<IFrameworkElementOverrides> for FlowPanelImpl {
    fn as_interface_ref(&self) -> InterfaceRef<'_, IFrameworkElementOverrides> {
        unsafe { std::mem::transmute(&self.overrides) }
    }
}

impl ComObjectInner for FlowPanel {
    type Outer = FlowPanelImpl;

    fn into_object(self) -> ComObject<Self> {
        let boxed = Box::new(FlowPanelImpl::into_outer(self));
        unsafe { ComObject::from_raw(NonNull::new_unchecked(Box::into_raw(boxed))) }
    }
}

impl From<FlowPanel> for IInspectable {
    fn from(panel: FlowPanel) -> Self {
        ComObject::new(panel).into_interface()
    }
}

impl ChildClass for FlowPanel {
    type BaseType = Panel;
    type FactoryInterface = IPanelFactory;

    fn create_interface_fn(
        vtable: &<Self::FactoryInterface as Interface>::Vtable,
    ) -> CreateInstanceFn {
        vtable.CreateInstance
    }

    fn identity_vtable(outer: &mut Self::Outer) -> &mut &'static IInspectable_Vtbl {
        &mut outer.identity
    }

    fn ref_count(outer: &Self::Outer) -> &windows_core::imp::WeakRefCount {
        &outer.count
    }

    fn into_outer(self) -> Self::Outer {
        FlowPanelImpl::into_outer(self)
    }
}

struct FlowInner {
    native: Panel,
    state: Rc<FlowState>,
    /// Kept so that the widgets live while they are placed.
    children: RefCell<Vec<Box<dyn Widget>>>,
}

/// A container that wraps its children onto new lines (a `Panel` that naui lays out;
/// see the module documentation).
#[derive(Clone)]
pub struct Flow(Rc<FlowInner>);
impl_widget!(Flow, native);

impl Flow {
    pub(crate) fn new() -> Result<Self> {
        let state = Rc::new(FlowState {
            children: RefCell::new(Vec::new()),
            column_spacing: Cell::new(0.0),
            row_spacing: Cell::new(0.0),
            padding: Cell::new(Padding::ZERO),
            align: Cell::new(Align::Start),
        });
        let native = Compose::compose(FlowPanel {
            state: state.clone(),
        })
        .map_err(|e| to_error("create the Flow panel", e))?;
        Ok(Self(Rc::new(FlowInner {
            native,
            state,
            children: RefCell::new(Vec::new()),
        })))
    }

    fn relayout(&self) {
        let _ = self.0.native.InvalidateMeasure();
    }

    pub fn set_spacing(&self, column: f64, row: f64) {
        self.0.state.column_spacing.set(column.max(0.0));
        self.0.state.row_spacing.set(row.max(0.0));
        self.relayout();
    }

    pub fn set_padding(&self, padding: Padding) {
        self.0.state.padding.set(padding);
        self.relayout();
    }

    pub fn set_align(&self, align: Align) {
        if self.0.state.align.replace(align) != align {
            self.relayout();
        }
    }

    /// Appends a child at the end.
    ///
    /// A child still in another container (this one included) is refused: nothing
    /// changes, neither here nor on the child.
    pub fn append(&self, child: &dyn Widget) {
        self.insert(usize::MAX, child);
    }

    /// Inserts a child at the given position. If `index` is at least the current
    /// count, it is appended at the end.
    ///
    /// A child still in another container is refused, as with
    /// [`append`](Self::append).
    pub fn insert(&self, index: usize, child: &dyn Widget) {
        let element = child.native_element();
        if crate::layout::has_parent(&element) {
            return;
        }
        let mut children = self.0.children.borrow_mut();
        let index = index.min(children.len());
        let inserted = self
            .0
            .native
            .Children()
            .and_then(|c| c.InsertAt(index as u32, &element));
        if inserted.is_ok() {
            crate::layout::note_place(&element, crate::layout::Place::panel(&self.0.native));
            children.insert(index, child.boxed_clone());
            self.0.state.children.borrow_mut().insert(index, element);
            drop(children);
            self.relayout();
        }
    }

    /// Removes the child at the given position. Does nothing if out of range.
    pub fn remove(&self, index: usize) {
        let mut children = self.0.children.borrow_mut();
        if index >= children.len() {
            return;
        }
        let removed = self
            .0
            .native
            .Children()
            .and_then(|c| c.RemoveAt(index as u32));
        if removed.is_ok() {
            let element = children.remove(index).native_element();
            crate::layout::note_place(&element, None);
            self.0.state.children.borrow_mut().remove(index);
            drop(children);
            self.relayout();
        }
    }

    /// Removes every child.
    pub fn clear(&self) {
        let Ok(native_children) = self.0.native.Children() else {
            return;
        };
        if native_children.Clear().is_err() {
            return;
        }
        for child in std::mem::take(&mut *self.0.children.borrow_mut()) {
            crate::layout::note_place(&child.native_element(), None);
        }
        self.0.state.children.borrow_mut().clear();
        self.relayout();
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
