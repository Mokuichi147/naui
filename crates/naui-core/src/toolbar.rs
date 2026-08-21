//! ツールバーの項目とアイコン。

/// ツールバーに置ける操作の種類。
///
/// ツールバーはどのプラットフォームでもアイコンで並べるものだが、
/// アイコンの呼び名は環境ごとに違う (macOS は SF Symbols の記号名、
/// Linux はアイコンテーマの名前、Windows は Segoe Fluent Icons の
/// 文字コード)。naui は**操作の種類**だけを受け取り、その環境の
/// 標準アイコンへ写す。
///
/// ```
/// # use naui_core::ToolbarIcon;
/// assert_eq!(ToolbarIcon::Save.sf_symbol(), "square.and.arrow.down");
/// assert_eq!(ToolbarIcon::Save.icon_name(), "document-save-symbolic");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolbarIcon {
    /// 新規作成。
    New,
    /// 開く。
    Open,
    /// 保存。
    Save,
    /// 追加。
    Add,
    /// 取り除く。
    Remove,
    /// 削除 (ごみ箱)。
    Delete,
    /// 編集。
    Edit,
    /// コピー。
    Copy,
    /// 切り取り。
    Cut,
    /// 貼り付け。
    Paste,
    /// 元に戻す。
    Undo,
    /// やり直す。
    Redo,
    /// 検索。
    Search,
    /// 再読み込み。
    Refresh,
    /// 共有。
    Share,
    /// 設定。
    Settings,
    /// 戻る。
    Back,
    /// 進む。
    Forward,
    /// 印刷。
    Print,
    /// 情報。
    Info,
}

impl ToolbarIcon {
    /// すべての種類。対応表の網羅を確かめるために使う。
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

    /// macOS (AppKit) が使う SF Symbols の記号名。
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

    /// Linux (GTK4) が使うアイコンテーマの名前 (freedesktop の標準名)。
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

    /// Windows (WinUI 3) が使う Segoe Fluent Icons の文字。
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

    /// Web が使う SVG の中身 (24x24 の座標系、線描き)。
    ///
    /// ブラウザには OS のアイコンテーマが無いため、ここだけは naui が
    /// 図形を持つ。色は `currentColor` に従うので、ブラウザの配色に馴染む。
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

/// ツールバーの 1 項目。
///
/// ツールバーは画面の上端に置く「よく使う操作」の並びで、
/// ナビゲーションと違い**選ばれている項目を持たない**。押されるたびに
/// その場でコマンドが走る。
///
/// 見た目はアイコンで、`label` はアクセシビリティ・ツールチップ・
/// 項目が入りきらないときのメニューに使われる。
///
/// 項目の識別はアプリ側が持つ順序 (インデックス) で行い、通知も
/// **区切りを含めた並びの中でのインデックス**で返る。区切りは
/// 押せないので、そのインデックスが返ることはない。
///
/// ```
/// # use naui_core::{ToolbarIcon, ToolbarItem};
/// let items = [
///     ToolbarItem::new(ToolbarIcon::New, "新規"),
///     ToolbarItem::separator(),
///     ToolbarItem::new(ToolbarIcon::Save, "保存").enabled(false),
/// ];
/// assert!(items[1].is_separator());
/// assert!(!items[2].enabled);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarItem {
    /// 表示するアイコン。区切りでは使われない。
    pub icon: ToolbarIcon,
    /// 操作の名前。読み上げ・ツールチップ・送り出しメニューに使う。
    pub label: String,
    /// 押せるかどうか。
    pub enabled: bool,
    /// 区切りかどうか。
    pub separator: bool,
}

impl ToolbarItem {
    /// 押せる項目を作る。
    pub fn new(icon: ToolbarIcon, label: impl Into<String>) -> Self {
        Self {
            icon,
            label: label.into(),
            enabled: true,
            separator: false,
        }
    }

    /// 項目のまとまりを分ける区切りを作る。押すことはできない。
    pub fn separator() -> Self {
        Self {
            icon: ToolbarIcon::Add,
            label: String::new(),
            enabled: false,
            separator: true,
        }
    }

    /// 押せるかどうかを指定する (既定は押せる)。
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// 区切りかどうか。
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

    /// 対応表に抜けや取り違えがあると、その環境だけアイコンが出なくなる。
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

    /// 同じアイコンを 2 つの操作へ割り当てていないか。
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
