//! A place for a DOM element made outside naui (a frame `<div>`).
//!
//! The widget's element is a `<div>` of naui's, a grid of one cell that the hosted
//! element stretches over, the same frame an `Image` uses. Without containment the
//! frame is as large as the element would be (a `<canvas>` reports its pixel size,
//! 300 × 150 by default); a `Fill` axis gets size containment, so the element fills
//! what the parent gives without pushing the parent wider
//! ([`apply_sizing`](crate::layout::apply_sizing)).
//!
//! The size is reported from a `ResizeObserver` on the frame, and the scale from
//! `devicePixelRatio`, whose changes alone (moving to another monitor, zooming) are
//! caught with the `change` of a `matchMedia("(resolution: …dppx)")`, as `Canvas`
//! does. The DOM has no event for an element joining the page, so one
//! `MutationObserver` on the document (shared by every host, and disconnected while
//! there are none) checks `isConnected` after each change to the tree.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use naui_core::{HostEvents, HostSize, Result};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, HtmlElement, MutationObserver, ResizeObserver};

use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, Widget};

/// The properties naui writes on the hosted element so that it fills the frame.
/// Removed again when the element leaves the host.
const STRETCH: [(&str, &str); 3] = [
    ("place-self", "stretch"),
    ("min-width", "0"),
    ("min-height", "0"),
];

struct NativeHostInner {
    /// The frame placed into the parent.
    frame: HtmlElement,
    events: HostEvents,
    /// The hosted element.
    hosted: RefCell<Option<Element>>,
    observer: RefCell<Option<ResizeObserver>>,
    _observer_callback: RefCell<Option<Closure<dyn FnMut(JsValue)>>>,
    /// The `matchMedia` subscription for the ratio last reported.
    ratio_watch: RefCell<Option<(f64, Listener)>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

impl NativeHostInner {
    fn size(&self) -> (f64, f64) {
        let rect = self.frame.get_bounding_client_rect();
        (rect.width(), rect.height())
    }

    /// The current display ratio (`devicePixelRatio`). 1 if it cannot be read.
    fn ratio(&self) -> f64 {
        web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .filter(|r| r.is_finite() && *r > 0.0)
            .unwrap_or(1.0)
    }

    fn report_size(self: &Rc<Self>) {
        let (width, height) = self.size();
        let ratio = self.ratio();
        self.watch_ratio(ratio);
        self.events.report_size(HostSize::new(width, height, ratio));
    }

    fn report_attached(&self) {
        self.events.report_attached(self.frame.is_connected());
    }

    /// Re-attaches the subscription so that a ratio departing from `ratio` is
    /// reported. Does nothing if it is already attached for the same ratio.
    fn watch_ratio(self: &Rc<Self>, ratio: f64) {
        if matches!(&*self.ratio_watch.borrow(), Some((watched, _)) if *watched == ratio) {
            return;
        }
        let Some(list) = web_sys::window()
            .and_then(|w| w.match_media(&format!("(resolution: {ratio}dppx)")).ok())
            .flatten()
        else {
            return;
        };
        let weak = Rc::downgrade(self);
        let listener = Listener::attach(list.as_ref(), "change", move || {
            if let Some(inner) = weak.upgrade() {
                inner.report_size();
            }
        });
        // 前の購読は、ここで置き換わるときに落ちて外れる。
        *self.ratio_watch.borrow_mut() = listener.ok().map(|listener| (ratio, listener));
    }

    /// Takes `element` out of the frame if it is still there, removing what naui
    /// wrote on it.
    fn release(&self, element: &Element) {
        let frame: &Element = self.frame.as_ref();
        if element.parent_element().as_ref() != Some(frame) {
            return;
        }
        let _ = frame.remove_child(element);
        if let Some(element) = element.dyn_ref::<HtmlElement>() {
            let style = element.style();
            for (property, _) in STRETCH {
                let _ = style.remove_property(property);
            }
        }
    }
}

impl Drop for NativeHostInner {
    /// Lets go of the hosted element, taking it out of the frame if it is still there.
    fn drop(&mut self) {
        if let Some(observer) = self.observer.get_mut().take() {
            observer.disconnect();
        }
        if let Some(element) = self.hosted.get_mut().take() {
            self.release(&element);
        }
    }
}

thread_local! {
    /// The hosts whose joining and leaving the page is watched.
    static HOSTS: RefCell<Vec<Weak<NativeHostInner>>> = const { RefCell::new(Vec::new()) };
    /// The observer of the document's tree, while there are hosts to watch.
    static TREE: RefCell<Option<(MutationObserver, Closure<dyn FnMut(JsValue)>)>> =
        const { RefCell::new(None) };
}

/// Adds `host` to the hosts checked after each change to the document's tree,
/// starting the observer if it is not running.
fn watch_tree(host: &Rc<NativeHostInner>) {
    HOSTS.with(|hosts| hosts.borrow_mut().push(Rc::downgrade(host)));
    TREE.with(|tree| {
        if tree.borrow().is_some() {
            return;
        }
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let callback = Closure::<dyn FnMut(JsValue)>::new(|_records: JsValue| check_tree());
        let Ok(observer) = MutationObserver::new(callback.as_ref().unchecked_ref()) else {
            return;
        };
        let options = web_sys::MutationObserverInit::new();
        options.set_child_list(true);
        options.set_subtree(true);
        if observer
            .observe_with_options(document.as_ref(), &options)
            .is_ok()
        {
            *tree.borrow_mut() = Some((observer, callback));
        }
    });
}

/// Reports each watched host's connection, dropping the hosts that are gone and
/// stopping the observer when none is left.
fn check_tree() {
    let hosts: Vec<Rc<NativeHostInner>> = HOSTS.with(|hosts| {
        let mut hosts = hosts.borrow_mut();
        hosts.retain(|host| host.strong_count() != 0);
        hosts.iter().filter_map(Weak::upgrade).collect()
    });
    if hosts.is_empty() {
        TREE.with(|tree| {
            if let Some((observer, _callback)) = tree.borrow_mut().take() {
                observer.disconnect();
            }
        });
        return;
    }
    for host in hosts {
        host.report_attached();
    }
}

/// A place for a DOM element made outside naui.
///
/// Put the element in with [`set_hosted_element`](Self::set_hosted_element). naui
/// places, sizes and shows the frame around it, and reports its size and scale
/// ([`on_resize`](Self::on_resize)) and when it joins or leaves the page
/// ([`on_attach`](Self::on_attach)); what the element draws is up to whoever made
/// it.
#[derive(Clone)]
pub struct NativeHost(Rc<NativeHostInner>);
impl_widget!(NativeHost, frame);

impl NativeHost {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let frame: HtmlElement = create(document, "div")?.unchecked_into();
        let _ = frame.set_attribute(crate::layout::HOST_FRAME_ATTR, "");
        let style = frame.style();
        let _ = style.set_property("display", "inline-grid");
        let _ = style.set_property("grid-template", "minmax(0, 1fr) / minmax(0, 1fr)");

        let inner = Rc::new(NativeHostInner {
            frame,
            events: HostEvents::default(),
            hosted: RefCell::new(None),
            observer: RefCell::new(None),
            _observer_callback: RefCell::new(None),
            ratio_watch: RefCell::new(None),
            file_drop: Default::default(),
        });

        // 大きさが変わったら伝える。ページへ載った直後と外れた直後にも呼ばれる。
        let observer_callback = Closure::<dyn FnMut(JsValue)>::new({
            let weak = Rc::downgrade(&inner);
            move |_entries: JsValue| {
                if let Some(inner) = weak.upgrade() {
                    inner.report_attached();
                    inner.report_size();
                }
            }
        });
        let observer = ResizeObserver::new(observer_callback.as_ref().unchecked_ref())
            .map_err(|e| to_error("create the ResizeObserver", e))?;
        observer.observe(inner.frame.as_ref());
        *inner.observer.borrow_mut() = Some(observer);
        *inner._observer_callback.borrow_mut() = Some(observer_callback);
        watch_tree(&inner);
        Ok(Self(inner))
    }

    /// Puts `element` in the host, replacing the element there before (which is taken
    /// out of the host and let go). `None` empties the host. Returns `true`.
    ///
    /// naui writes `place-self: stretch` and zero minimum sizes in the element's
    /// `style` so that it fills the frame, and removes them when the element leaves.
    /// An element that is in another element is moved out of it, as `appendChild`
    /// does. naui keeps a reference to `element` until it is replaced or the host is
    /// dropped; when either happens, the element is taken out of the host if it is
    /// still there.
    pub fn set_hosted_element(&self, element: Option<&Element>) -> bool {
        let previous = self.0.hosted.borrow_mut().take();
        if let Some(previous) = previous {
            let frame: &Element = self.0.frame.as_ref();
            if element.is_some_and(|element| *element == previous)
                && previous.parent_element().as_ref() == Some(frame)
            {
                *self.0.hosted.borrow_mut() = Some(previous);
                return true;
            }
            self.0.release(&previous);
        }
        let Some(element) = element else {
            return true;
        };
        if let Some(styled) = element.dyn_ref::<HtmlElement>() {
            let style = styled.style();
            for (property, value) in STRETCH {
                let _ = style.set_property(property, value);
            }
        }
        if self.0.frame.append_child(element).is_err() {
            return false;
        }
        *self.0.hosted.borrow_mut() = Some(element.clone());
        true
    }

    /// The element put in with [`set_hosted_element`](Self::set_hosted_element), if
    /// any.
    pub fn hosted_element(&self) -> Option<Element> {
        self.0.hosted.borrow().clone()
    }

    /// The current size (width, height) in CSS pixels. 0 if not laid out yet.
    pub fn size(&self) -> (f64, f64) {
        self.0.size()
    }

    /// `devicePixelRatio`, which includes the browser's zoom.
    pub fn scale_factor(&self) -> f64 {
        self.0.ratio()
    }

    /// Called with the new size and scale when either changes (from the
    /// `ResizeObserver` and the `matchMedia` subscription). It runs after the
    /// browser's layout, before the next paint.
    pub fn on_resize(&self, f: impl FnMut(HostSize) + 'static) {
        self.0.events.on_resize(f);
    }

    /// Called with `true` when the host joins the page (`isConnected`) and `false`
    /// when it leaves it. It is reported from a `MutationObserver`, so it arrives
    /// in a microtask after the change, not on the spot.
    pub fn on_attach(&self, f: impl FnMut(bool) + 'static) {
        self.0.events.on_attach(f);
    }

    /// Whether the host is in the page now (`isConnected`).
    pub fn is_attached(&self) -> bool {
        self.0.frame.is_connected()
    }
}
