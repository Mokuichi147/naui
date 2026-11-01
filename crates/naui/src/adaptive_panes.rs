use crate::sys;
use crate::Widget;
use naui_core::Pane;

/// A list and its detail: side by side when wide, one at a time when narrow
/// (`AdwNavigationSplitView`).
///
/// For the list–detail screens of mail, notes and settings apps. While it is at
/// least [`breakpoint`](Self::breakpoint) wide, the list is at the start (left) with
/// a fixed width and the detail takes the rest. Narrower than that, only one pane is
/// shown, the one chosen with [`show`](Self::show), and while it is the detail, a
/// back button above it returns to the list. The app shows the detail when an item
/// is chosen in the list:
///
/// ```no_run
/// # use naui::{ListItem, Pane};
/// # fn build(ui: &naui::Ui) -> naui::Result<()> {
/// let panes = ui.adaptive_panes()?;
/// let list = ui.list()?;
/// let detail = ui.label("")?;
/// list.set_items(&[ListItem::new("Inbox"), ListItem::new("Sent")]);
/// panes.set_list(&list);
/// panes.set_detail(&detail);
/// list.on_select({
///     let panes = panes.downgrade();
///     let detail = detail.downgrade();
///     move |selected: &[usize]| {
///         let (Some(&index), Some(panes), Some(detail)) =
///             (selected.first(), panes.upgrade(), detail.upgrade())
///         else {
///             return;
///         };
///         detail.set_text(&format!("Item {index}"));
///         // Side by side this changes nothing on screen; one at a time it
///         // replaces the list with the detail.
///         panes.show(Pane::Detail);
///     }
/// });
/// # Ok(())
/// # }
/// ```
///
/// The width that counts is the one the panes get, so give them `Fill` (or make
/// them the window's child) for them to follow the window. Going back to one pane
/// takes a few pixels less than the breakpoint, so a width right at the boundary
/// does not flip it back and forth. Both panes are shown before the first layout.
///
/// Each platform's own control is used where there is one:
///
/// - **Linux**: `AdwNavigationSplitView`. Narrow, it slides between the panes,
///   and Escape, Alt+←, the mouse's back button and a swipe also go back.
///   `AdwBreakpoint` would take away the window's minimum size, so naui decides
///   when it collapses.
/// - **Windows**: `TwoPaneView`, which decides by its own width. It has no slack
///   at the boundary.
/// - **macOS** and **Web**: there is no such control (`NSSplitViewController`
///   only collapses a sidebar, and a CSS container query cannot tell the page which
///   pane it shows), so naui decides and lays the panes out itself.
///
/// The panes are not resized by dragging, as with `TwoPaneView` and
/// `AdwNavigationSplitView`. For three panes, attach a [`Sidebar`](crate::Sidebar)
/// to the window, which collapses by itself, or put another `AdaptivePanes` in the
/// detail: each decides by its own width.
#[derive(Clone)]
pub struct AdaptivePanes(pub(crate) sys::AdaptivePanes);

impl_widget!(AdaptivePanes);

impl AdaptivePanes {
    /// The list pane. Each call replaces it.
    #[inline]
    pub fn set_list(&self, child: &dyn Widget) {
        self.0.set_list(child.as_backend())
    }

    /// The detail pane. Each call replaces it.
    #[inline]
    pub fn set_detail(&self, child: &dyn Widget) {
        self.0.set_detail(child.as_backend())
    }

    /// The narrowest width (logical pixels) at which both panes are shown. The
    /// default is [`DEFAULT_PANES_BREAKPOINT`](crate::DEFAULT_PANES_BREAKPOINT).
    ///
    /// A breakpoint narrower than the two panes need side by side is never crossed
    /// going down, because the window cannot get that narrow.
    #[inline]
    pub fn set_breakpoint(&self, width: f64) {
        self.0.set_breakpoint(width)
    }

    #[inline]
    pub fn breakpoint(&self) -> f64 {
        self.0.breakpoint()
    }

    /// The width of the list pane while both panes are shown (logical pixels). The
    /// default is [`DEFAULT_LIST_PANE_WIDTH`](crate::DEFAULT_LIST_PANE_WIDTH).
    #[inline]
    pub fn set_list_width(&self, width: f64) {
        self.0.set_list_width(width)
    }

    #[inline]
    pub fn list_width(&self) -> f64 {
        self.0.list_width()
    }

    /// Whether only one pane is shown now.
    #[inline]
    pub fn is_collapsed(&self) -> bool {
        self.0.is_collapsed()
    }

    /// The pane shown while only one is: [`Pane::List`] at first.
    #[inline]
    pub fn visible_pane(&self) -> Pane {
        self.0.visible_pane()
    }

    /// Chooses the pane shown while only one is. [`on_back`](Self::on_back) is not
    /// called.
    ///
    /// The choice is kept while both panes are shown, so a detail opened in a wide
    /// window is still the one shown after the window narrows.
    #[inline]
    pub fn show(&self, pane: Pane) {
        self.0.show(pane)
    }

    /// Goes back to the list as if the user had pressed the back button. Calls
    /// [`on_back`](Self::on_back).
    #[inline]
    pub fn go_back(&self) {
        self.0.go_back()
    }

    /// Called when the user goes back from the detail to the list, by the back
    /// button or, on Linux, by the toolkit's own ways of going back.
    ///
    /// A list that keeps the item highlighted can clear its selection here, so
    /// choosing the same item again opens it.
    #[inline]
    pub fn on_back(&self, f: impl FnMut() + 'static) {
        self.0.on_back(f)
    }
}
