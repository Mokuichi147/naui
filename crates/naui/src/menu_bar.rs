use crate::sys;
use naui_core::MenuSpec;

/// The OS's application menu, attached to the top of the window.
///
/// Not a [`Widget`](crate::Widget). Attach it with
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar). Items are identified by
/// **a path**: the heading index followed by the item index at each level of
/// submenus, counting separators (`[1, 0]` is the first item under the second
/// heading, `[1, 2, 0]` the first item in the submenu that is its third item).
/// Every time an item is clicked, [`on_activate`](Self::on_activate) is called with
/// its path. Separators and submenu headings are never returned.
///
/// ```no_run
/// # use naui::{MenuBar, MenuItem, MenuShortcut, MenuSpec};
/// # fn build(menu_bar: &MenuBar) {
/// menu_bar.set_menus(&[MenuSpec::new(
///     "View",
///     [
///         MenuItem::new("Show Sidebar").checked(true),
///         MenuItem::submenu("Zoom", ["Zoom In", "Zoom Out"]),
///         MenuItem::separator(),
///         MenuItem::new("Full Screen").shortcut(MenuShortcut::function(11)),
///     ],
/// )]);
/// menu_bar.on_activate(|path| match path {
///     [0, 0] => println!("sidebar: {}", "toggled"),
///     [0, 1, zoom] => println!("zoom {zoom}"),
///     [0, 3] => println!("full screen"),
///     _ => {}
/// });
/// # }
/// ```
#[derive(Clone)]
pub struct MenuBar(pub(crate) sys::MenuBar);

impl_backend!(MenuBar);

impl MenuBar {
    /// Rebuilds the menus. The previous menus (and their check states) are removed.
    #[inline]
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        self.0.set_menus(menus)
    }

    /// The number of headings.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The number of items under one heading, separators included. `0` if out of
    /// range.
    #[inline]
    pub fn menu_len(&self, menu: usize) -> usize {
        self.0.menu_len(menu)
    }

    /// Enables or disables the item at `path`. Disabling a submenu keeps it from
    /// opening. Does nothing for separators or out of range.
    #[inline]
    pub fn set_item_enabled(&self, path: &[usize], enabled: bool) {
        self.0.set_item_enabled(path, enabled)
    }

    /// Whether the item at `path` can be clicked now (it and the submenus leading to it
    /// are enabled). `false` for separators, submenu headings and out of range.
    #[inline]
    pub fn is_item_enabled(&self, path: &[usize]) -> bool {
        self.0.is_item_enabled(path)
    }

    /// Turns the check mark of the item at `path` on or off, without notifying.
    ///
    /// Does nothing for items not made with [`MenuItem::checked`](crate::MenuItem::checked).
    #[inline]
    pub fn set_item_checked(&self, path: &[usize], checked: bool) {
        self.0.set_item_checked(path, checked)
    }

    /// Whether the check mark of the item at `path` is on. `false` for items without a
    /// check mark and out of range.
    ///
    /// Choosing a check item flips it before [`on_activate`](Self::on_activate) is
    /// called, so read the new state here inside the notification.
    #[inline]
    pub fn is_item_checked(&self, path: &[usize]) -> bool {
        self.0.is_item_checked(path)
    }

    /// Enables or disables the whole menu bar. Per-item settings are kept.
    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Runs the item at `path` as if the user had chosen it (a check item flips).
    ///
    /// Does nothing for separators, submenu headings, disabled items or out of range.
    #[inline]
    pub fn activate(&self, path: &[usize]) {
        self.0.activate(path)
    }

    /// Called with the item's path when an item is clicked. Setting it again removes
    /// the previous callback.
    #[inline]
    pub fn on_activate(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_activate(f)
    }
}
