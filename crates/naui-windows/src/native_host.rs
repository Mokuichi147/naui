//! A place for a XAML element made outside naui (a one-cell `Grid`).
//!
//! The widget's element is a `Grid` of naui's, and the hosted element is its only
//! child. A `Grid` measures as its child does and stretches it over its whole
//! area, so an element that knows its size (a control) makes the host that size,
//! and one that does not (a `SwapChainPanel` a GPU draws into) gets the size naui's
//! layout gives.
//!
//! The element's type is naui's projection's `UIElement` (`naui-winui3`). An element
//! from another projection of WinUI 3 is the same COM object, so it is passed after
//! `cast::<UIElement>()`. A `SwapChainPanel` needs nothing from naui beyond being
//! placed: connecting a swap chain to it (`ISwapChainPanelNative`) is the drawing
//! crate's part.
//!
//! The size is reported from `SizeChanged`, the scale from
//! `XamlRoot.RasterizationScale` (watching `XamlRoot.Changed`, as `Canvas` does),
//! and joining or leaving a window from `Loaded` / `Unloaded`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use naui_core::{HostEvents, HostSize, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::Grid as XamlGrid;
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, RoutedEventHandler, SizeChangedEventHandler, UIElement, XamlRoot,
    XamlRootChangedEventArgs,
};
use windows::Foundation::TypedEventHandler;
use windows_core::Interface;

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

struct NativeHostInner {
    native: XamlGrid,
    events: HostEvents,
    /// The hosted element.
    hosted: RefCell<Option<UIElement>>,
    /// The `XamlRoot` whose `Changed` is watched, with the subscription's token.
    root: RefCell<Option<(XamlRoot, i64)>>,
}

impl NativeHostInner {
    fn element(&self) -> Option<FrameworkElement> {
        self.native.cast::<FrameworkElement>().ok()
    }

    /// The display scale of the window it is in (1.0 before it is in one).
    fn scale(&self) -> f64 {
        self.native
            .XamlRoot()
            .and_then(|root| root.RasterizationScale())
            .ok()
            .filter(|scale| scale.is_finite() && *scale > 0.0)
            .unwrap_or(1.0)
    }

    fn size(&self) -> (f64, f64) {
        self.element().map_or((0.0, 0.0), |element| {
            (
                element.ActualWidth().unwrap_or(0.0),
                element.ActualHeight().unwrap_or(0.0),
            )
        })
    }

    fn report_size(&self) {
        let (width, height) = self.size();
        self.events
            .report_size(HostSize::new(width, height, self.scale()));
    }

    /// Whether the hosted element is still the `Grid`'s child.
    fn holds(&self, element: &UIElement) -> bool {
        let Ok(children) = self.native.Children() else {
            return false;
        };
        let mut index = 0;
        children.IndexOf(element, &mut index).unwrap_or(false)
    }

    /// Watches `XamlRoot.Changed` of the window it is in, to report a change of
    /// scale alone. Moves to the new `XamlRoot` when it moves to another window.
    fn watch_root(self: &Rc<Self>) {
        let Ok(root) = self.native.XamlRoot() else {
            return;
        };
        let mut watched = self.root.borrow_mut();
        if watched
            .as_ref()
            .is_some_and(|(current, _)| *current == root)
        {
            return;
        }
        if let Some((previous, token)) = watched.take() {
            let _ = previous.RemoveChanged(token);
        }
        let target = UiThreadCell::new(Rc::downgrade(self));
        let changed = TypedEventHandler::<XamlRoot, XamlRootChangedEventArgs>::new(move |_, _| {
            let _ = target.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    inner.report_size();
                }
            });
            Ok(())
        });
        if let Ok(token) = root.Changed(&changed) {
            *watched = Some((root, token));
        }
    }
}

impl Drop for NativeHostInner {
    /// Lets go of the hosted element, taking it out of the `Grid` if it is still
    /// there, and stops watching the window's `XamlRoot` (which outlives the host).
    fn drop(&mut self) {
        if let Some(element) = self.hosted.get_mut().take() {
            if self.holds(&element) {
                if let Ok(children) = self.native.Children() {
                    let _ = children.Clear();
                }
            }
        }
        if let Some((root, token)) = self.root.get_mut().take() {
            let _ = root.RemoveChanged(token);
        }
    }
}

/// A place for a XAML element made outside naui.
///
/// Put the element in with [`set_hosted_element`](Self::set_hosted_element). naui
/// places, sizes and shows the host around it, and reports its size and scale
/// ([`on_resize`](Self::on_resize)) and when it joins or leaves a window
/// ([`on_attach`](Self::on_attach)); what the element draws is up to whoever made
/// it.
#[derive(Clone)]
pub struct NativeHost(Rc<NativeHostInner>);
impl_widget!(NativeHost, native);

impl NativeHost {
    pub(crate) fn new() -> Result<Self> {
        let native = XamlGrid::new().map_err(|e| to_error("create the NativeHost", e))?;
        let this = Self(Rc::new(NativeHostInner {
            native,
            events: HostEvents::default(),
            hosted: RefCell::new(None),
            root: RefCell::new(None),
        }));
        this.track()?;
        Ok(this)
    }

    /// Subscribes to `SizeChanged`, `Loaded` and `Unloaded`.
    fn track(&self) -> Result<()> {
        let element = self
            .0
            .element()
            .ok_or_else(|| naui_core::Error::new("create the NativeHost", "not an element"))?;
        let handler = |f: fn(&Rc<NativeHostInner>)| {
            let target = UiThreadCell::new(Rc::downgrade(&self.0));
            move || {
                let _ = target.try_with_mut(|weak: &mut Weak<NativeHostInner>| {
                    if let Some(inner) = weak.upgrade() {
                        f(&inner);
                    }
                });
            }
        };

        let resized = handler(|inner| inner.report_size());
        element
            .SizeChanged(&SizeChangedEventHandler::new(move |_, _| {
                resized();
                Ok(())
            }))
            .map_err(|e| to_error("subscribe to NativeHost size changes", e))?;

        let loaded = handler(|inner| {
            inner.watch_root();
            inner.events.report_attached(true);
            inner.report_size();
        });
        element
            .Loaded(&RoutedEventHandler::new(move |_, _| {
                loaded();
                Ok(())
            }))
            .map_err(|e| to_error("subscribe to NativeHost loading", e))?;

        // 別の親へすぐ移したときは、新しい側の `Loaded` の後に古い側の
        // `Unloaded` が届くことがある。そのときはまだ載っているので伝えない。
        let unloaded = handler(|inner| {
            let still_loaded = inner
                .element()
                .and_then(|element| element.IsLoaded().ok())
                .unwrap_or(false);
            if !still_loaded {
                inner.events.report_attached(false);
            }
        });
        element
            .Unloaded(&RoutedEventHandler::new(move |_, _| {
                unloaded();
                Ok(())
            }))
            .map_err(|e| to_error("subscribe to NativeHost unloading", e))?;
        Ok(())
    }

    /// Puts `element` in the host, replacing the element there before (which is taken
    /// out of the host and let go). `None` empties the host.
    ///
    /// Returns `false`, changing nothing, when XAML refuses the element: an element
    /// that already has a parent cannot be added to another one. naui keeps a
    /// reference to `element` until it is replaced or the host is dropped; when
    /// either happens, the element is taken out of the host.
    pub fn set_hosted_element(&self, element: Option<&UIElement>) -> bool {
        let current = self.0.hosted.borrow().clone();
        if let (Some(current), Some(element)) = (&current, element) {
            if current == element && self.0.holds(current) {
                return true;
            }
        }
        let Ok(children) = self.0.native.Children() else {
            return false;
        };
        // 新しいものを先に足し、断られたら何も変えない (表示中の中身を一度も
        // 下ろさない)。足せてから前のものを外す。
        if let Some(element) = element {
            if children.Append(element).is_err() {
                return false;
            }
        }
        if let Some(current) = &current {
            let mut index = 0;
            if children.IndexOf(current, &mut index).unwrap_or(false) {
                let _ = children.RemoveAt(index);
            }
        }
        *self.0.hosted.borrow_mut() = element.cloned();
        true
    }

    /// The element put in with [`set_hosted_element`](Self::set_hosted_element), if
    /// any.
    pub fn hosted_element(&self) -> Option<UIElement> {
        self.0.hosted.borrow().clone()
    }

    /// The current size (width, height) in logical pixels (`ActualWidth` /
    /// `ActualHeight`). 0 if not laid out yet.
    pub fn size(&self) -> (f64, f64) {
        self.0.size()
    }

    /// The window's `XamlRoot.RasterizationScale`. 1.0 before the host is in a
    /// window.
    pub fn scale_factor(&self) -> f64 {
        self.0.scale()
    }

    /// Called with the new size and scale when either changes (from `SizeChanged`,
    /// `XamlRoot.Changed` and `Loaded`). It runs after XAML's layout pass.
    pub fn on_resize(&self, f: impl FnMut(HostSize) + 'static) {
        self.0.events.on_resize(f);
    }

    /// Called with `true` when XAML loads the host into a window (`Loaded`, once the
    /// window is shown) and `false` when it unloads it (`Unloaded`).
    pub fn on_attach(&self, f: impl FnMut(bool) + 'static) {
        self.0.events.on_attach(f);
    }

    /// Whether the host is loaded into a window now, by the last `Loaded` /
    /// `Unloaded`.
    pub fn is_attached(&self) -> bool {
        self.0.events.is_attached()
    }
}
