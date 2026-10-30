//! A collapsible section (WinUI 3's native `Expander`).
//!
//! The control is created directly from the [`naui_winui3`] projection. Expanding,
//! the heading and the content layout are done by WinUI's standard template.
//!
//! The heading and content should stretch to the full width, so only
//! `HorizontalAlignment` and `HorizontalContentAlignment` are set to `Stretch`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::Result;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Control, Expander as XamlExpander, ExpanderCollapsedEventArgs, ExpanderExpandingEventArgs,
    TextBlock,
};
use naui_winui3::Microsoft::UI::Xaml::{HorizontalAlignment, UIElement};
use windows::Foundation::TypedEventHandler;
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// The handler notified when expansion changes.
///
/// WinRT delegates require `Send + Sync`, so it is placed in a [`UiThreadCell`].
/// The closure is taken out only for the duration of the call, so operating on the
/// same expander from inside the notification does not double-borrow.
#[derive(Clone)]
struct ToggleHandler(HandlerCell<dyn FnMut(bool)>);

impl ToggleHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(bool) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, expanded: bool) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(expanded);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct ExpanderInner {
    native: XamlExpander,
    label: TextBlock,
    /// Holds the content's handle, keeping it alive along with its callbacks.
    child: RefCell<Option<Box<dyn Widget>>>,
    handler: ToggleHandler,
    /// Changes made by `set_expanded` do not call `on_toggle`.
    silent: Cell<bool>,
}

/// A container whose contents are shown and hidden by clicking its heading.
#[derive(Clone)]
pub struct Expander(Rc<ExpanderInner>);
impl_widget!(Expander, native);

impl Expander {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = XamlExpander::new().map_err(|e| to_error("create the Expander", e))?;
        native
            .SetHorizontalAlignment(HorizontalAlignment::Stretch)
            .and_then(|()| native.SetHorizontalContentAlignment(HorizontalAlignment::Stretch))
            .map_err(|e| to_error("set the Expander alignment", e))?;
        let label = TextBlock::new().map_err(|e| to_error("create the header", e))?;
        label
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("set the header", e))?;
        native
            .SetHeader(&label)
            .map_err(|e| to_error("set the Expander header", e))?;

        let this = Self(Rc::new(ExpanderInner {
            native,
            label,
            child: RefCell::new(None),
            handler: ToggleHandler::new(),
            silent: Cell::new(false),
        }));
        this.connect()?;
        Ok(this)
    }

    /// Connects WinUI's `Expanding` / `Collapsed` to the Rust closure.
    fn connect(&self) -> Result<()> {
        let expanding_target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let expanding =
            TypedEventHandler::<XamlExpander, ExpanderExpandingEventArgs>::new(move |_, _| {
                let _ = expanding_target.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        if !inner.silent.get() {
                            inner.handler.emit(true);
                        }
                    }
                });
                Ok(())
            });
        self.0
            .native
            .Expanding(&expanding)
            .map_err(|e| to_error("subscribe to Expander expanding", e))?;

        let collapsed_target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let collapsed =
            TypedEventHandler::<XamlExpander, ExpanderCollapsedEventArgs>::new(move |_, _| {
                let _ = collapsed_target.try_with_mut(|weak| {
                    if let Some(inner) = weak.upgrade() {
                        if !inner.silent.get() {
                            inner.handler.emit(false);
                        }
                    }
                });
                Ok(())
            });
        self.0
            .native
            .Collapsed(&collapsed)
            .map_err(|e| to_error("subscribe to Expander collapsing", e))?;
        Ok(())
    }

    /// The heading text.
    pub fn text(&self) -> String {
        self.0
            .label
            .Text()
            .map(|text| text.to_string())
            .unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        let _ = self.0.label.SetText(&HSTRING::from(text));
    }

    /// Whether it is expanded.
    pub fn is_expanded(&self) -> bool {
        self.0.native.IsExpanded().unwrap_or(false)
    }

    /// Expands or collapses it from code. `on_toggle` is not called.
    pub fn set_expanded(&self, expanded: bool) {
        self.0.silent.set(true);
        let _ = self.0.native.SetIsExpanded(expanded);
        self.0.silent.set(false);
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(control) = self.0.native.cast::<Control>() {
            let _ = control.SetIsEnabled(enabled);
        }
    }

    /// The contents to collapse. Each call replaces them.
    pub fn set_child(&self, child: &dyn Widget) {
        if self.0.native.SetContent(None).is_err() {
            return;
        }
        let element = child.native_element();
        if self.0.native.SetContent(&element).is_ok() {
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// Called with the new state every time the user expands or collapses it.
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.handler.set(f);
    }
}
