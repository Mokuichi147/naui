use crate::sys;
use crate::Widget;
use naui_core::NavItem;

/// Breadcrumbs. Lays out the current position in a hierarchy from left to right.
///
/// GTK4 has no equivalent control, so the links (`<a href>`) supported by
/// `GtkLabel` markup are laid out with separators (`›`) between them. Only the
/// current position is plain text, not a link (as with the Web's `<a>` and macOS's
/// `NSPathControl`, it is a row of text without borders). It is not bold because
/// the text width would change on every reselection and shift the items after it.
/// Buttons are not used because libadwaita's padding (`padding: 5px 10px`) would
/// be added to every item and spread them out.
#[derive(Clone)]
pub struct Breadcrumbs(pub(crate) sys::Breadcrumbs);

impl_widget!(Breadcrumbs);

impl Breadcrumbs {
    /// Rebuilds the hierarchy. **The last item is the current position.**
    #[inline]
    pub fn set_items(&self, items: &[NavItem]) {
        self.0.set_items(items)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// Changes the selection without notifying.
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// Called with the index when an item is chosen.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }
}

/// Horizontal navigation placed at the bottom of the screen (equal widths).
///
/// **Placement is the app's job.** Put it last in a vertical stack and give the
/// element before it [`Length::Fill`](naui_core::Length::Fill) or a `Spacer` to push
/// it to the bottom.
#[derive(Clone)]
pub struct Dock(pub(crate) sys::Dock);

impl_widget!(Dock);

impl Dock {
    /// Rebuilds the items. Indices change meaning, so the selection is cleared.
    #[inline]
    pub fn set_items(&self, items: &[NavItem]) {
        self.0.set_items(items)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// Changes the selection without notifying.
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// Called with the index when an item is chosen.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }
}

/// A link (`GtkLinkButton`).
///
/// If `href` is not empty, clicking it opens it in the browser (the desktop's
/// default handler).
#[derive(Clone)]
pub struct Link(pub(crate) sys::Link);

impl_widget!(Link);

impl Link {
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn href(&self) -> String {
        self.0.href()
    }

    #[inline]
    pub fn set_href(&self, href: &str) {
        self.0.set_href(href)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called on every click. If there is an `href`, it is opened as well.
    #[inline]
    pub fn on_click(&self, f: impl FnMut() + 'static) {
        self.0.on_click(f)
    }
}

/// A vertical list of navigation items.
///
/// As in GNOME's sidebars, borderless buttons with a selected state are stacked
/// vertically. **It is not the popup shown on right-click** (that is
/// [`PopupMenu`]).
///
/// [`PopupMenu`]: crate::PopupMenu
#[derive(Clone)]
pub struct Menu(pub(crate) sys::Menu);

impl_widget!(Menu);

impl Menu {
    /// Rebuilds the items. Indices change meaning, so the selection is cleared.
    #[inline]
    pub fn set_items(&self, items: &[NavItem]) {
        self.0.set_items(items)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// Changes the selection without notifying.
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// Called with the index when an item is chosen.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }
}

/// Horizontal navigation placed at the top of the screen.
///
/// A heading (`GtkLabel`) and items (a row of `GtkToggleButton`s) are laid out in a
/// `GtkBox`.
#[derive(Clone)]
pub struct Navbar(pub(crate) sys::Navbar);

impl_widget!(Navbar);

impl Navbar {
    /// Rebuilds the items. Indices change meaning, so the selection is cleared.
    #[inline]
    pub fn set_items(&self, items: &[NavItem]) {
        self.0.set_items(items)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// Changes the selection without notifying.
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// Called with the index when an item is chosen.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }

    /// The heading at the left end.
    #[inline]
    pub fn set_title(&self, title: &str) {
        self.0.set_title(title)
    }

    #[inline]
    pub fn title(&self) -> String {
        self.0.title()
    }
}

/// Pagination.
///
/// GTK4 has no equivalent control, so it is built from previous/next buttons and
/// toggle buttons for the page numbers laid out in a row.
#[derive(Clone)]
pub struct Pagination(pub(crate) sys::Pagination);

impl_widget!(Pagination);

impl Pagination {
    /// Resets the number of pages. The first page becomes selected.
    #[inline]
    pub fn set_page_count(&self, count: usize) {
        self.0.set_page_count(count)
    }

    #[inline]
    pub fn page_count(&self) -> usize {
        self.0.page_count()
    }

    /// The page currently open (starting at 0).
    #[inline]
    pub fn page(&self) -> usize {
        self.0.page()
    }

    /// Changes the page without notifying.
    #[inline]
    pub fn set_page(&self, page: usize) {
        self.0.set_page(page)
    }

    /// Changes the page through the same path as a user's click (notifies).
    #[inline]
    pub fn select(&self, page: usize) {
        self.0.select(page)
    }

    /// Goes to the previous page (notifies). Does nothing on the first page.
    #[inline]
    pub fn go_previous(&self) {
        self.0.go_previous()
    }

    /// Goes to the next page (notifies). Does nothing on the last page.
    #[inline]
    pub fn go_next(&self) {
        self.0.go_next()
    }

    /// Called with the page when the page changes.
    #[inline]
    pub fn on_change(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_change(f)
    }
}

/// Tabs (`GtkNotebook`). Holds each tab's content widget.
#[derive(Clone)]
pub struct Tabs(pub(crate) sys::Tabs);

impl_widget!(Tabs);

impl Tabs {
    /// Adds one tab. `child` becomes that tab's content.
    #[inline]
    pub fn add_tab(&self, label: &str, child: &dyn Widget) {
        self.0.add_tab(label, child.as_backend())
    }

    /// Removes one tab. Does nothing if out of range.
    ///
    /// When the selected tab is removed, the platform selects a nearby tab instead.
    /// Like [`set_selected`](Tabs::set_selected), this move is not notified.
    #[inline]
    pub fn remove_tab(&self, index: usize) {
        self.0.remove_tab(index)
    }

    /// Removes every tab.
    #[inline]
    pub fn clear(&self) {
        self.0.clear()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// Changes the selection without notifying.
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// Called with the index when the tab changes.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }
}
