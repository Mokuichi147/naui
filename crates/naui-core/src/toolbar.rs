//! Toolbar items and icons.

/// The kinds of actions that can be placed on a toolbar.
///
/// Toolbars show icons on every platform, but icons are named differently per
/// platform (SF Symbols names on macOS, icon theme names on Linux, Segoe Fluent
/// Icons code points on Windows). naui takes **only the kind of action** and maps
/// it to the platform's standard icon.
///
/// ```
/// # use naui_core::ToolbarIcon;
/// assert_eq!(ToolbarIcon::Save.sf_symbol(), "square.and.arrow.down");
/// assert_eq!(ToolbarIcon::Save.icon_name(), "document-save-symbolic");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolbarIcon {
    /// New.
    New,
    /// Open.
    Open,
    /// Save.
    Save,
    /// Add.
    Add,
    /// Remove.
    Remove,
    /// Delete (trash).
    Delete,
    /// Edit.
    Edit,
    /// Copy.
    Copy,
    /// Cut.
    Cut,
    /// Paste.
    Paste,
    /// Undo.
    Undo,
    /// Redo.
    Redo,
    /// Search.
    Search,
    /// Reload.
    Refresh,
    /// Share.
    Share,
    /// Settings.
    Settings,
    /// Back.
    Back,
    /// Forward.
    Forward,
    /// Print.
    Print,
    /// Info.
    Info,
}

impl ToolbarIcon {
    /// Every kind. Used to check that the mapping tables are exhaustive.
    pub const ALL: [ToolbarIcon; 20] = [
        ToolbarIcon::New,
        ToolbarIcon::Open,
        ToolbarIcon::Save,
        ToolbarIcon::Add,
        ToolbarIcon::Remove,
        ToolbarIcon::Delete,
        ToolbarIcon::Edit,
        ToolbarIcon::Copy,
        ToolbarIcon::Cut,
        ToolbarIcon::Paste,
        ToolbarIcon::Undo,
        ToolbarIcon::Redo,
        ToolbarIcon::Search,
        ToolbarIcon::Refresh,
        ToolbarIcon::Share,
        ToolbarIcon::Settings,
        ToolbarIcon::Back,
        ToolbarIcon::Forward,
        ToolbarIcon::Print,
        ToolbarIcon::Info,
    ];

    /// The SF Symbols name used on macOS (AppKit).
    pub fn sf_symbol(self) -> &'static str {
        match self {
            ToolbarIcon::New => "doc.badge.plus",
            ToolbarIcon::Open => "folder",
            ToolbarIcon::Save => "square.and.arrow.down",
            ToolbarIcon::Add => "plus",
            ToolbarIcon::Remove => "minus",
            ToolbarIcon::Delete => "trash",
            ToolbarIcon::Edit => "pencil",
            ToolbarIcon::Copy => "doc.on.doc",
            ToolbarIcon::Cut => "scissors",
            ToolbarIcon::Paste => "doc.on.clipboard",
            ToolbarIcon::Undo => "arrow.uturn.backward",
            ToolbarIcon::Redo => "arrow.uturn.forward",
            ToolbarIcon::Search => "magnifyingglass",
            ToolbarIcon::Refresh => "arrow.clockwise",
            ToolbarIcon::Share => "square.and.arrow.up",
            ToolbarIcon::Settings => "gearshape",
            ToolbarIcon::Back => "chevron.backward",
            ToolbarIcon::Forward => "chevron.forward",
            ToolbarIcon::Print => "printer",
            ToolbarIcon::Info => "info.circle",
        }
    }

    /// The icon theme name used on Linux (GTK4) (a freedesktop standard name).
    pub fn icon_name(self) -> &'static str {
        match self {
            ToolbarIcon::New => "document-new-symbolic",
            ToolbarIcon::Open => "document-open-symbolic",
            ToolbarIcon::Save => "document-save-symbolic",
            ToolbarIcon::Add => "list-add-symbolic",
            ToolbarIcon::Remove => "list-remove-symbolic",
            ToolbarIcon::Delete => "user-trash-symbolic",
            ToolbarIcon::Edit => "document-edit-symbolic",
            ToolbarIcon::Copy => "edit-copy-symbolic",
            ToolbarIcon::Cut => "edit-cut-symbolic",
            ToolbarIcon::Paste => "edit-paste-symbolic",
            ToolbarIcon::Undo => "edit-undo-symbolic",
            ToolbarIcon::Redo => "edit-redo-symbolic",
            ToolbarIcon::Search => "system-search-symbolic",
            ToolbarIcon::Refresh => "view-refresh-symbolic",
            ToolbarIcon::Share => "emblem-shared-symbolic",
            ToolbarIcon::Settings => "preferences-system-symbolic",
            ToolbarIcon::Back => "go-previous-symbolic",
            ToolbarIcon::Forward => "go-next-symbolic",
            ToolbarIcon::Print => "document-print-symbolic",
            ToolbarIcon::Info => "dialog-information-symbolic",
        }
    }

    /// The Segoe Fluent Icons character used on Windows (WinUI 3).
    pub fn fluent_glyph(self) -> char {
        match self {
            ToolbarIcon::New => '\u{E7C3}',
            ToolbarIcon::Open => '\u{E8E5}',
            ToolbarIcon::Save => '\u{E74E}',
            ToolbarIcon::Add => '\u{E710}',
            ToolbarIcon::Remove => '\u{E738}',
            ToolbarIcon::Delete => '\u{E74D}',
            ToolbarIcon::Edit => '\u{E70F}',
            ToolbarIcon::Copy => '\u{E8C8}',
            ToolbarIcon::Cut => '\u{E8C6}',
            ToolbarIcon::Paste => '\u{E77F}',
            ToolbarIcon::Undo => '\u{E7A7}',
            ToolbarIcon::Redo => '\u{E7A6}',
            ToolbarIcon::Search => '\u{E721}',
            ToolbarIcon::Refresh => '\u{E72C}',
            ToolbarIcon::Share => '\u{E72D}',
            ToolbarIcon::Settings => '\u{E713}',
            ToolbarIcon::Back => '\u{E72B}',
            ToolbarIcon::Forward => '\u{E72A}',
            ToolbarIcon::Print => '\u{E749}',
            ToolbarIcon::Info => '\u{E946}',
        }
    }

    /// The SVG contents used on the Web (24x24 coordinate space, line drawing).
    ///
    /// Browsers have no OS icon theme, so this is the one place where naui owns the
    /// shapes. The color follows `currentColor`, so it blends in with the browser's
    /// color scheme.
    pub fn svg_path(self) -> &'static str {
        match self {
            ToolbarIcon::New => {
                "M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h6M14 3l5 5M14 3v5h5M18 14v6M15 17h6"
            }
            ToolbarIcon::Open => "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
            ToolbarIcon::Save => "M12 3v11M8 10l4 4 4-4M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2",
            ToolbarIcon::Add => "M12 5v14M5 12h14",
            ToolbarIcon::Remove => "M5 12h14",
            ToolbarIcon::Delete => "M4 7h16M10 7V5h4v2M6 7l1 13h10l1-13M10 11v6M14 11v6",
            ToolbarIcon::Edit => "M4 20h4L20 8l-4-4L4 16zM14 6l4 4",
            ToolbarIcon::Copy => "M9 9h10v12H9zM5 15V3h10v2",
            ToolbarIcon::Cut => "M6 4l12 14M18 4L6 18M7 21a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5M17 21a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5",
            ToolbarIcon::Paste => "M8 5H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2M9 5V3h6v2z",
            ToolbarIcon::Undo => "M9 8H5V4M5 8a8 8 0 1 1-1.6 6",
            ToolbarIcon::Redo => "M15 8h4V4M19 8a8 8 0 1 0 1.6 6",
            ToolbarIcon::Search => "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14M20 20l-4-4",
            ToolbarIcon::Refresh => "M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5",
            ToolbarIcon::Share => "M12 16V4M8 8l4-4 4 4M4 15v4a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4",
            ToolbarIcon::Settings => {
                "M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.2 2.2M16.9 16.9l2.2 2.2M19.1 4.9l-2.2 2.2M7.1 16.9l-2.2 2.2"
            }
            ToolbarIcon::Back => "M15 5l-7 7 7 7",
            ToolbarIcon::Forward => "M9 5l7 7-7 7",
            ToolbarIcon::Print => "M7 9V3h10v6M7 19H5a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2M7 15h10v6H7z",
            ToolbarIcon::Info => "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18M12 11v6M12 7.5v.5",
        }
    }
}

/// One toolbar item.
///
/// A toolbar is a row of "frequently used actions" placed at the top of the
/// screen. Unlike navigation, it **has no selected item**. Every click runs the
/// command right away.
///
/// It is shown as an icon; `label` is used for accessibility, tooltips and the
/// menu shown when items do not fit.
///
/// Items are identified by the order (index) the app owns, and notifications come
/// back as **the index within the sequence, separators included**. Separators
/// cannot be clicked, so their indices never come back.
///
/// ```
/// # use naui_core::{ToolbarIcon, ToolbarItem};
/// let items = [
///     ToolbarItem::new(ToolbarIcon::New, "New"),
///     ToolbarItem::separator(),
///     ToolbarItem::new(ToolbarIcon::Save, "Save").enabled(false),
/// ];
/// assert!(items[1].is_separator());
/// assert!(!items[2].enabled);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarItem {
    /// The icon shown. Not used for separators.
    pub icon: ToolbarIcon,
    /// The name of the action. Used for screen readers, tooltips and the overflow menu.
    pub label: String,
    /// Whether it can be clicked.
    pub enabled: bool,
    /// Whether this is a separator.
    pub separator: bool,
}

impl ToolbarItem {
    /// Creates a clickable item.
    pub fn new(icon: ToolbarIcon, label: impl Into<String>) -> Self {
        Self {
            icon,
            label: label.into(),
            enabled: true,
            separator: false,
        }
    }

    /// Creates a separator that divides groups of items. It cannot be clicked.
    pub fn separator() -> Self {
        Self {
            icon: ToolbarIcon::Add,
            label: String::new(),
            enabled: false,
            separator: true,
        }
    }

    /// Sets whether it can be clicked (clickable by default).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Whether this is a separator.
    pub fn is_separator(&self) -> bool {
        self.separator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_item_defaults_to_enabled() {
        let item = ToolbarItem::new(ToolbarIcon::New, "新規");
        assert_eq!(item.label, "新規");
        assert_eq!(item.icon, ToolbarIcon::New);
        assert!(item.enabled);
        assert!(!item.is_separator());
        assert!(
            !ToolbarItem::new(ToolbarIcon::Save, "保存")
                .enabled(false)
                .enabled
        );
    }

    #[test]
    fn separator_is_never_pressable() {
        let item = ToolbarItem::separator();
        assert!(item.is_separator());
        assert!(!item.enabled);
        assert!(item.label.is_empty());
    }

    /// A gap or mix-up in the mapping table would make the icon disappear on that
    /// platform only.
    #[test]
    fn every_icon_maps_to_all_backends() {
        for icon in ToolbarIcon::ALL {
            assert!(!icon.sf_symbol().is_empty(), "{icon:?} の SF Symbols");
            assert!(!icon.icon_name().is_empty(), "{icon:?} のアイコン名");
            assert!(
                icon.icon_name().ends_with("-symbolic"),
                "{icon:?} は symbolic アイコンを使う"
            );
            // Segoe Fluent Icons は私用領域に並ぶ。
            let glyph = icon.fluent_glyph() as u32;
            assert!(
                (0xE000..=0xF8FF).contains(&glyph),
                "{icon:?} の字面が私用領域にない"
            );
            assert!(icon.svg_path().starts_with('M'), "{icon:?} の SVG");
        }
    }

    /// Checks that no icon is assigned to two actions.
    #[test]
    fn icons_do_not_collide() {
        for (i, a) in ToolbarIcon::ALL.iter().enumerate() {
            for b in &ToolbarIcon::ALL[i + 1..] {
                assert_ne!(a.sf_symbol(), b.sf_symbol(), "{a:?} と {b:?}");
                assert_ne!(a.icon_name(), b.icon_name(), "{a:?} と {b:?}");
                assert_ne!(a.fluent_glyph(), b.fluent_glyph(), "{a:?} と {b:?}");
                assert_ne!(a.svg_path(), b.svg_path(), "{a:?} と {b:?}");
            }
        }
    }
}
