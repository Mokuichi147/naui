//! The menu bar (a row of `Button` + `MenuFlyout`).
//!
//! | naui | WinUI 3 |
//! | --- | --- |
//! | `MenuBar` | A `StackPanel` of `Button`s with the background removed |
//! | Menu | `MenuFlyout` (the heading's `Button.Flyout`) |
//! | Item | `MenuFlyoutItem` |
//! | Item with a check mark | `ToggleMenuFlyoutItem` |
//! | Submenu | `MenuFlyoutSubItem` |
//! | Separator | `MenuFlyoutSeparator` |
//!
//! Showing and closing the menu, its shadow, rounded corners, light dismiss,
//! keyboard navigation (arrows and Esc) and wrapping at screen edges are all owned
//! by `MenuFlyout`. Opening when a heading is clicked is also simply left to
//! `Button.Flyout`.
//!
//! ## Why `MenuBar` is not used
//!
//! WinUI 3 has `MenuBar` / `MenuBarItem`, but they are not included in the
//! [`naui_winui3`] projection. The projection is a committed generated artifact, and
//! adding types means regenerating all of it, so naui builds the same thing from
//! **a combination of standard controls already in the projection** (the same
//! policy as navigation and `Tabs`).
//!
//! ## Shortcuts
//!
//! `KeyboardAccelerator` is not in the projection either, so pressed keys are
//! picked up by [`crate::Window`] in the root's `KeyDown` and passed to the menu
//! bar, including function keys and items inside submenus. Modifier keys are read on the spot from `GetKeyState` (since
//! `KeyRoutedEventArgs` does not carry them). The right edge of items is shown with
//! `MenuFlyoutItem.KeyboardAcceleratorTextOverride`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use naui_core::{MenuHandler, MenuItem, MenuSpec, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, MenuFlyout, MenuFlyoutItem, MenuFlyoutItemBase, MenuFlyoutSeparator,
    MenuFlyoutSubItem, Orientation as XamlOrientation, StackPanel, ToggleMenuFlyoutItem,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::RoutedEventHandler;
use windows_collections::IVector;
use windows_core::{Interface, HSTRING};

use crate::navigation::{panel, text_block};
use crate::to_error;
use crate::ui_thread::UiThreadCell;

/// The heading button. As befits a menu bar, it shows only text, with no background
/// or border (the light fill when pressed comes from `Button`'s standard template).
const TITLE_BUTTON_XAML: &str = r##"<Button
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent" BorderThickness="0" Padding="10,4"
    VerticalAlignment="Center"/>"##;

/// The control for one item. The kind follows the item: an ordinary item, one with
/// a check mark, or a submenu.
#[derive(Clone)]
pub enum MenuEntry {
    /// An ordinary item.
    Item(MenuFlyoutItem),
    /// An item with a check mark.
    Toggle(ToggleMenuFlyoutItem),
    /// A submenu heading.
    Submenu(MenuFlyoutSubItem),
}

impl MenuEntry {
    pub(crate) fn set_enabled(&self, enabled: bool) {
        let _ = match self {
            Self::Item(item) => item.SetIsEnabled(enabled),
            Self::Toggle(item) => item.SetIsEnabled(enabled),
            Self::Submenu(item) => item.SetIsEnabled(enabled),
        };
    }

    pub(crate) fn is_enabled(&self) -> bool {
        match self {
            Self::Item(item) => item.IsEnabled(),
            Self::Toggle(item) => item.IsEnabled(),
            Self::Submenu(item) => item.IsEnabled(),
        }
        .unwrap_or(false)
    }

    pub(crate) fn set_checked(&self, checked: bool) {
        if let Self::Toggle(item) = self {
            let _ = item.SetIsChecked(checked);
        }
    }
}

/// Builds the controls for `items` into `entries`, recursing into submenus.
///
/// `enabled` is whether everything leading to it is enabled, and `on_click` is
/// called with the path of the clicked item. The built controls go into `built` by
/// path (separators are not in it).
pub(crate) fn build_entries(
    entries: &IVector<MenuFlyoutItemBase>,
    items: &[MenuItem],
    path: &mut Vec<usize>,
    enabled: bool,
    on_click: &dyn Fn(Vec<usize>) -> RoutedEventHandler,
    built: &mut HashMap<Vec<usize>, MenuEntry>,
) {
    for (index, entry) in items.iter().enumerate() {
        if entry.is_separator() {
            if let Ok(separator) = MenuFlyoutSeparator::new() {
                let _ = entries.Append(&separator);
            }
            continue;
        }
        path.push(index);
        let here = entry.enabled && enabled;
        if let Ok(native) = build_entry(entry, path, here, on_click, built) {
            let _ = match &native {
                MenuEntry::Item(item) => entries.Append(item),
                MenuEntry::Toggle(item) => entries.Append(item),
                MenuEntry::Submenu(item) => entries.Append(item),
            };
            native.set_enabled(here);
            built.insert(path.clone(), native);
        }
        path.pop();
    }
}

fn build_entry(
    entry: &MenuItem,
    path: &mut Vec<usize>,
    enabled: bool,
    on_click: &dyn Fn(Vec<usize>) -> RoutedEventHandler,
    built: &mut HashMap<Vec<usize>, MenuEntry>,
) -> Result<MenuEntry> {
    let text = HSTRING::from(entry.label.as_str());
    if let Some(children) = &entry.submenu {
        let native = MenuFlyoutSubItem::new().map_err(|e| to_error("サブメニューの生成", e))?;
        native
            .SetText(&text)
            .map_err(|e| to_error("サブメニューの文字設定", e))?;
        let items = native
            .Items()
            .map_err(|e| to_error("サブメニューの項目", e))?;
        build_entries(&items, children, path, enabled, on_click, built);
        return Ok(MenuEntry::Submenu(native));
    }
    // 右端の表示だけ。押されたキーは `handle_key` が拾う。
    let shortcut = entry
        .shortcut
        .filter(|s| s.is_valid())
        .map(|s| HSTRING::from(s.label()));
    // 閉じるのは `MenuFlyout` が自分で行う。
    let handler = on_click(path.clone());
    if let Some(checked) = entry.checked {
        // 押されると `ToggleMenuFlyoutItem` が自分で印を反転してから Click が来る。
        let native = ToggleMenuFlyoutItem::new().map_err(|e| to_error("メニュー項目の生成", e))?;
        native
            .SetText(&text)
            .map_err(|e| to_error("メニュー項目の文字設定", e))?;
        let _ = native.SetIsChecked(checked);
        if let Some(shortcut) = &shortcut {
            let _ = native.SetKeyboardAcceleratorTextOverride(shortcut);
        }
        native
            .Click(&handler)
            .map_err(|e| to_error("メニュー項目の購読", e))?;
        Ok(MenuEntry::Toggle(native))
    } else {
        let native = MenuFlyoutItem::new().map_err(|e| to_error("メニュー項目の生成", e))?;
        native
            .SetText(&text)
            .map_err(|e| to_error("メニュー項目の文字設定", e))?;
        if let Some(shortcut) = &shortcut {
            let _ = native.SetKeyboardAcceleratorTextOverride(shortcut);
        }
        native
            .Click(&handler)
            .map_err(|e| to_error("メニュー項目の購読", e))?;
        Ok(MenuEntry::Item(native))
    }
}

/// The controls for one heading.
struct MenuParts {
    /// The heading's button.
    title: XamlButton,
    /// The menu shown below the heading.
    flyout: MenuFlyout,
}

struct MenuBarInner {
    native: StackPanel,
    menus: RefCell<Vec<MenuSpec>>,
    parts: RefCell<Vec<MenuParts>>,
    /// The controls looked up by path (heading index first). Separators are not in it.
    items: RefCell<HashMap<Vec<usize>, MenuEntry>>,
    handler: MenuHandler,
    /// Whether the whole menu bar is enabled. ANDed with the per-item settings.
    enabled: Cell<bool>,
}

/// The OS's application menu, attached to the top of the window.
///
/// It is not a widget laid out on screen, so it is not a
/// [`Widget`](crate::Widget). Attach it with
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar). Items are identified by
/// **a path** (the heading index, then the item index at each level of submenus,
/// counting separators). Every time an item is clicked,
/// [`on_activate`](Self::on_activate) is called with its path. Separators and
/// submenu headings are never returned.
#[derive(Clone)]
pub struct MenuBar(Rc<MenuBarInner>);

impl MenuBar {
    pub(crate) fn new() -> Result<Self> {
        // タイトルバーの下に敷く帯なので、見出しは詰めて並べる。
        let native = panel(XamlOrientation::Horizontal, 0.0)?;
        Ok(Self(Rc::new(MenuBarInner {
            native,
            menus: RefCell::new(Vec::new()),
            parts: RefCell::new(Vec::new()),
            items: RefCell::new(HashMap::new()),
            handler: MenuHandler::default(),
            enabled: Cell::new(true),
        })))
    }

    /// Rebuilds the menus. The previous menus are removed.
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        let Ok(children) = self.0.native.Children() else {
            return;
        };
        let _ = children.Clear();
        self.0.parts.borrow_mut().clear();

        let whole = self.0.enabled.get();
        let mut parts = Vec::with_capacity(menus.len());
        let mut built = HashMap::new();
        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        let weak = weak_cell(&self.0);
        let on_click = |path: Vec<usize>| {
            let weak = weak.clone();
            RoutedEventHandler::new(move |_sender, _args| {
                if let Some(inner) = weak.try_with_mut(|weak| weak.upgrade()).flatten() {
                    inner.choose(&path);
                }
                Ok(())
            })
        };
        for (menu_index, spec) in menus.iter().enumerate() {
            let Ok(title) = title_button(&spec.title) else {
                continue;
            };
            let _ = title.SetIsEnabled(whole);
            let Ok(flyout) = MenuFlyout::new() else {
                continue;
            };
            if let Ok(entries) = flyout.Items() {
                build_entries(
                    &entries,
                    &spec.items,
                    &mut vec![menu_index],
                    whole,
                    &on_click,
                    &mut built,
                );
            }

            // 見出しを押したら開くところは `Button` が引き受ける。
            let _ = title.SetFlyout(&flyout);
            let _ = children.Append(&title);
            parts.push(MenuParts { title, flyout });
        }

        *self.0.parts.borrow_mut() = parts;
        *self.0.items.borrow_mut() = built;
        let mut stored = self.0.menus.borrow_mut();
        stored.clear();
        stored.extend_from_slice(menus);
    }

    /// The number of headings.
    pub fn len(&self) -> usize {
        self.0.menus.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The number of items under one heading, separators included. `0` if out of
    /// range.
    pub fn menu_len(&self, menu: usize) -> usize {
        self.0
            .menus
            .borrow()
            .get(menu)
            .map_or(0, |spec| spec.items.len())
    }

    /// Enables or disables the item at `path`. Does nothing for separators or out of
    /// range.
    pub fn set_item_enabled(&self, path: &[usize], enabled: bool) {
        let mut menus = self.0.menus.borrow_mut();
        let Some(entry) = MenuSpec::item_at_mut(&mut menus, path) else {
            return;
        };
        if entry.is_separator() {
            return;
        }
        entry.enabled = enabled;
        drop(menus);
        self.apply_enabled();
    }

    /// Whether the item at `path` can be clicked now. `false` for separators, submenu
    /// headings and out of range.
    pub fn is_item_enabled(&self, path: &[usize]) -> bool {
        self.0.enabled.get() && MenuSpec::can_choose(&self.0.menus.borrow(), path)
    }

    /// Turns the check mark of the item at `path` on or off, without notifying. Does
    /// nothing for items without a check mark.
    pub fn set_item_checked(&self, path: &[usize], checked: bool) {
        let mut menus = self.0.menus.borrow_mut();
        let Some(entry) = MenuSpec::item_at_mut(&mut menus, path) else {
            return;
        };
        if !entry.is_checkable() {
            return;
        }
        entry.checked = Some(checked);
        drop(menus);
        if let Some(native) = self.0.items.borrow().get(path) {
            native.set_checked(checked);
        }
    }

    /// Whether the check mark of the item at `path` is on.
    pub fn is_item_checked(&self, path: &[usize]) -> bool {
        MenuSpec::item_at(&self.0.menus.borrow(), path)
            .is_some_and(|entry| entry.is_checkable() && entry.checked == Some(true))
    }

    /// Enables or disables the whole menu bar. Per-item settings are kept.
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        if !enabled {
            self.close();
        }
        self.apply_enabled();
    }

    /// Applies the per-item settings and the overall setting to the native side.
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        for parts in self.0.parts.borrow().iter() {
            let _ = parts.title.SetIsEnabled(whole);
        }
        let menus = self.0.menus.borrow();
        let items = self.0.items.borrow();
        MenuSpec::walk(&menus, |path, _| {
            if let Some(native) = items.get(path) {
                native.set_enabled(whole && MenuSpec::enabled_at(&menus, path));
            }
        });
    }

    /// Closes the menu being shown. Does nothing if none is shown.
    ///
    /// The other three platforms have no way to "close a menu from code", so this is not
    /// public API.
    pub(crate) fn close(&self) {
        for parts in self.0.parts.borrow().iter() {
            let _ = parts.flyout.Hide();
        }
    }

    /// Runs the item at `path` as if the user had chosen it.
    ///
    /// Does nothing for separators, submenu headings, disabled items or out of range.
    pub fn activate(&self, path: &[usize]) {
        if self.is_item_enabled(path) {
            self.0.choose(path);
        }
    }

    /// Called with the item's path when an item is clicked. Setting it again removes
    /// the previous callback.
    pub fn on_activate(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// The `StackPanel` laying out the headings. Public as a backend-specific escape
    /// hatch.
    pub fn native_panel(&self) -> StackPanel {
        self.0.native.clone()
    }

    /// The `MenuFlyout` shown below the heading. `None` if out of range. Public as a
    /// backend-specific escape hatch.
    pub fn native_flyout(&self, menu: usize) -> Option<MenuFlyout> {
        Some(self.0.parts.borrow().get(menu)?.flyout.clone())
    }

    /// The control for the item at `path` (submenu headings included). `None` for
    /// separators and out of range. Public as a backend-specific escape hatch.
    pub fn native_item(&self, path: &[usize]) -> Option<MenuEntry> {
        self.0.items.borrow().get(path).cloned()
    }

    /// The element inserted into the window. Used only by [`crate::Window`].
    pub(crate) fn mount(&self) -> StackPanel {
        self.0.native.clone()
    }

    /// If the pressed key is a shortcut, runs it and returns `true`.
    ///
    /// Called by [`crate::Window`] from the root's `KeyDown`. `key` is the virtual-key
    /// code.
    pub(crate) fn handle_key(&self, key: i32) -> bool {
        if !self.0.enabled.get() {
            return false;
        }
        let (primary, shift, alt) = modifiers();
        // 通知の中でメニューを組み替えられても二重借用にならないよう、
        // 借用を返してから呼ぶ。
        let hit = MenuSpec::find_shortcut(&self.0.menus.borrow(), |s| {
            s.virtual_key() == key && s.primary == primary && s.shift == shift && s.alt == alt
        });
        let Some(path) = hit else {
            return false;
        };
        self.close();
        self.0.choose(&path);
        true
    }
}

impl MenuBarInner {
    /// Flips a check item, then notifies. The same path for the user's choice and
    /// `activate`.
    ///
    /// When the user clicks, `ToggleMenuFlyoutItem` has already flipped itself, so
    /// setting the same value again changes nothing.
    fn choose(&self, path: &[usize]) {
        let flipped = {
            let mut menus = self.menus.borrow_mut();
            MenuSpec::item_at_mut(&mut menus, path)
                .filter(|item| item.is_checkable())
                .map(|item| {
                    let on = !item.checked.unwrap_or(false);
                    item.checked = Some(on);
                    on
                })
        };
        if let Some(on) = flipped {
            if let Some(native) = self.items.borrow().get(path) {
                native.set_checked(on);
            }
        }
        self.handler.emit(path);
    }
}

/// The modifier keys held now (primary modifier, ⇧, Alt).
///
/// `KeyRoutedEventArgs` does not carry modifier keys, so the key state is read on
/// the spot. `GetKeyState` returns the state as far as the thread has processed, so
/// it is the state at the same moment as the key operation being dispatched (the
/// same as `Table`'s selection).
fn modifiers() -> (bool, bool, bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_SHIFT,
    };

    // 最上位のビットが立っていれば、そのキーは押されている。
    let down = |key: VIRTUAL_KEY| -> bool {
        let state = unsafe { GetKeyState(i32::from(key.0)) };
        state < 0
    };
    (down(VK_CONTROL), down(VK_SHIFT), down(VK_MENU))
}

/// The heading button. Falls back to a plain `Button` if the XAML cannot be read.
fn title_button(title: &str) -> Result<XamlButton> {
    let button = match XamlReader::Load(&HSTRING::from(TITLE_BUTTON_XAML))
        .and_then(|element| element.cast::<XamlButton>())
    {
        Ok(button) => button,
        Err(_) => XamlButton::new().map_err(|e| to_error("メニュー見出しの生成", e))?,
    };
    button
        .SetContent(&text_block(title)?)
        .map_err(|e| to_error("メニュー見出しの文字設定", e))?;
    Ok(button)
}

/// WinRT delegates require `Send`, so it is wrapped in a UI-thread-only cell.
fn weak_cell(inner: &Rc<MenuBarInner>) -> Arc<UiThreadCell<Weak<MenuBarInner>>> {
    Arc::new(UiThreadCell::new(Rc::downgrade(inner)))
}
