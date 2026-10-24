//! The menu bar (a row of `Button` + `MenuFlyout`).
//!
//! | naui | WinUI 3 |
//! | --- | --- |
//! | `MenuBar` | A `StackPanel` of `Button`s with the background removed |
//! | Menu | `MenuFlyout` (the heading's `Button.Flyout`) |
//! | Item | `MenuFlyoutItem` |
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
//! bar. Modifier keys are read on the spot from `GetKeyState` (since
//! `KeyRoutedEventArgs` does not carry them). The right edge of items is shown with
//! `MenuFlyoutItem.KeyboardAcceleratorTextOverride`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use naui_core::{MenuItem, MenuSpec, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, MenuFlyout, MenuFlyoutItem, MenuFlyoutSeparator,
    Orientation as XamlOrientation, StackPanel,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::RoutedEventHandler;
use windows_core::{Interface, HSTRING};

use crate::navigation::{panel, text_block};
use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};

/// The heading button. As befits a menu bar, it shows only text, with no background
/// or border (the light fill when pressed comes from `Button`'s standard template).
const TITLE_BUTTON_XAML: &str = r##"<Button
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent" BorderThickness="0" Padding="10,4"
    VerticalAlignment="Center"/>"##;

/// The handler for the clicked item.
///
/// WinRT delegates require `Send + Sync`, so it is placed in a `UiThreadCell`. The
/// closure is taken out only for the duration of the call, so rebuilding the same
/// menu bar from inside the notification does not double-borrow.
#[derive(Clone)]
struct ActivateHandler(HandlerCell<dyn FnMut(usize, usize)>);

impl ActivateHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(usize, usize) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, menu: usize, item: usize) {
        let Some(mut f) = self.0.with_mut(|slot| slot.take()) else {
            return;
        };
        f(menu, item);
        self.0.with_mut(|slot| {
            // 呼び出しの中で差し替えられていたら、新しいほうを残す。
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

/// The controls for one heading.
struct MenuParts {
    /// The heading's button.
    title: XamlButton,
    /// The menu shown below the heading.
    flyout: MenuFlyout,
    /// The `MenuFlyoutItem` per item. `None` at separator positions.
    items: Vec<Option<MenuFlyoutItem>>,
}

struct MenuBarInner {
    native: StackPanel,
    menus: RefCell<Vec<MenuSpec>>,
    parts: RefCell<Vec<MenuParts>>,
    handler: ActivateHandler,
    /// Whether the whole menu bar is enabled. ANDed with the per-item settings.
    enabled: Cell<bool>,
}

/// The OS's application menu, attached to the top of the window.
///
/// It is not a widget laid out on screen, so it is not a
/// [`Widget`](crate::Widget). Attach it with
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar). Every time an item is
/// clicked, [`on_activate`](Self::on_activate) is called with its **(heading index,
/// item index)**. The item index is the position in the sequence including
/// separators; separators are never returned.
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
            handler: ActivateHandler::new(),
            enabled: Cell::new(true),
        })))
    }

    /// Rebuilds the menus. The previous menus are removed.
    ///
    /// The heading index is the position in the sequence passed, and the item index is
    /// the position in the sequence including separators.
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        let Ok(children) = self.0.native.Children() else {
            return;
        };
        let _ = children.Clear();
        self.0.parts.borrow_mut().clear();

        let whole = self.0.enabled.get();
        let mut parts = Vec::with_capacity(menus.len());
        for (menu_index, spec) in menus.iter().enumerate() {
            let Ok(title) = title_button(&spec.title) else {
                continue;
            };
            let _ = title.SetIsEnabled(whole);
            let Ok(flyout) = MenuFlyout::new() else {
                continue;
            };

            let mut items = Vec::with_capacity(spec.items.len());
            if let Ok(entries) = flyout.Items() {
                for (item_index, entry) in spec.items.iter().enumerate() {
                    if entry.is_separator() {
                        if let Ok(separator) = MenuFlyoutSeparator::new() {
                            let _ = entries.Append(&separator);
                        }
                        items.push(None);
                        continue;
                    }
                    match self.build_item(entry, menu_index, item_index, whole) {
                        Ok(native) => {
                            let _ = entries.Append(&native);
                            items.push(Some(native));
                        }
                        Err(_) => items.push(None),
                    }
                }
            }

            // 見出しを押したら開くところは `Button` が引き受ける。
            let _ = title.SetFlyout(&flyout);
            let _ = children.Append(&title);
            parts.push(MenuParts {
                title,
                flyout,
                items,
            });
        }

        *self.0.parts.borrow_mut() = parts;
        let mut stored = self.0.menus.borrow_mut();
        stored.clear();
        stored.extend_from_slice(menus);
    }

    fn build_item(
        &self,
        entry: &MenuItem,
        menu_index: usize,
        item_index: usize,
        whole: bool,
    ) -> Result<MenuFlyoutItem> {
        let native = MenuFlyoutItem::new().map_err(|e| to_error("メニュー項目の生成", e))?;
        native
            .SetText(&HSTRING::from(entry.label.as_str()))
            .map_err(|e| to_error("メニュー項目の文字設定", e))?;
        let _ = native.SetIsEnabled(entry.enabled && whole);
        if let Some(shortcut) = entry.shortcut {
            // 右端の表示だけ。押されたキーは `handle_key` が拾う。
            let _ = native.SetKeyboardAcceleratorTextOverride(&HSTRING::from(shortcut.label()));
        }

        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        let weak = weak_cell(&self.0);
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            if let Some(inner) = weak.try_with_mut(|weak| weak.upgrade()).flatten() {
                // 閉じるのは `MenuFlyout` が自分で行う。
                MenuBar(inner).0.handler.emit(menu_index, item_index);
            }
            Ok(())
        });
        native
            .Click(&handler)
            .map_err(|e| to_error("メニュー項目の購読", e))?;
        Ok(native)
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

    /// Enables or disables one item. Does nothing for separators or out of range.
    pub fn set_item_enabled(&self, menu: usize, item: usize, enabled: bool) {
        let mut menus = self.0.menus.borrow_mut();
        let Some(entry) = menus
            .get_mut(menu)
            .and_then(|spec| spec.items.get_mut(item))
        else {
            return;
        };
        if entry.is_separator() {
            return;
        }
        entry.enabled = enabled;
        drop(menus);
        self.apply_enabled();
    }

    /// Whether the item can be clicked now. `false` for separators and out of range.
    pub fn is_item_enabled(&self, menu: usize, item: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .menus
                .borrow()
                .get(menu)
                .and_then(|spec| spec.items.get(item))
                .is_some_and(|entry| !entry.is_separator() && entry.enabled)
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
        let menus = self.0.menus.borrow();
        for (spec, parts) in menus.iter().zip(self.0.parts.borrow().iter()) {
            let _ = parts.title.SetIsEnabled(whole);
            for (entry, native) in spec.items.iter().zip(parts.items.iter()) {
                if let Some(native) = native {
                    let _ = native.SetIsEnabled(entry.enabled && whole);
                }
            }
        }
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

    /// Runs the item as if the user had chosen it.
    ///
    /// Does nothing for separators, disabled items or out of range.
    pub fn activate(&self, menu: usize, item: usize) {
        if self.is_item_enabled(menu, item) {
            self.0.handler.emit(menu, item);
        }
    }

    /// Called with the heading and item indices when an item is clicked. Setting it
    /// again removes the previous callback.
    pub fn on_activate(&self, f: impl FnMut(usize, usize) + 'static) {
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

    /// The `MenuFlyoutItem` for the item. `None` for separators and out of range.
    /// Public as a backend-specific escape hatch.
    pub fn native_item(&self, menu: usize, item: usize) -> Option<MenuFlyoutItem> {
        self.0.parts.borrow().get(menu)?.items.get(item)?.clone()
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
        if !primary {
            return false;
        }
        // 通知の中でメニューを組み替えられても二重借用にならないよう、
        // 借用を返してから呼ぶ。
        let hit = {
            let menus = self.0.menus.borrow();
            menus.iter().enumerate().find_map(|(menu, spec)| {
                spec.items
                    .iter()
                    .enumerate()
                    .find(|(_, entry)| {
                        !entry.is_separator()
                            && entry.enabled
                            && entry.shortcut.is_some_and(|s| {
                                s.virtual_key() == key && s.shift == shift && s.alt == alt
                            })
                    })
                    .map(|(item, _)| (menu, item))
            })
        };
        let Some((menu, item)) = hit else {
            return false;
        };
        self.close();
        self.0.handler.emit(menu, item);
        true
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
