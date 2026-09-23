//! サイドバーの項目とまとまり。

use crate::ToolbarIcon;

/// サイドバーの既定の幅 (論理ピクセル)。
///
/// 環境ごとの既定 (macOS はおよそ 200、WinUI 3 の `OpenPaneLength` は 320、
/// libadwaita は幅の 25% で 180〜280) に任せると、同じアプリが環境ごとに
/// 違う幅で出てしまうので、naui が 4 環境でそろえる。
pub const DEFAULT_SIDEBAR_WIDTH: f64 = 220.0;

/// サイドバーの 1 項目。
///
/// 見た目は「アイコン + 文字」の 1 行で、アイコンは省ける。アイコンの
/// 種類は [`ToolbarIcon`] と同じもので、その環境の標準アイコンへ写す。
///
/// ```
/// # use naui_core::{SidebarItem, ToolbarIcon};
/// let item = SidebarItem::new("一般").icon(ToolbarIcon::Settings);
/// assert_eq!(item.label, "一般");
/// assert_eq!(item.icon, Some(ToolbarIcon::Settings));
/// assert!(item.enabled);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarItem {
    /// 画面に出る文字列。
    pub label: String,
    /// 文字の前に出すアイコン。`None` なら文字だけ。
    pub icon: Option<ToolbarIcon>,
    /// 選べるかどうか。
    pub enabled: bool,
}

impl SidebarItem {
    /// 選べる項目を作る。アイコンは無し。
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            enabled: true,
        }
    }

    /// 文字の前に出すアイコンを指定する (既定は無し)。
    pub fn icon(mut self, icon: ToolbarIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// 選べるかどうかを指定する (既定は選べる)。
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// 文字列の並びから項目列を作る。
    ///
    /// ```
    /// # use naui_core::SidebarItem;
    /// let items = SidebarItem::list(["ホーム", "検索", "設定"]);
    /// assert_eq!(items.len(), 3);
    /// ```
    pub fn list<I, S>(labels: I) -> Vec<SidebarItem>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        labels.into_iter().map(SidebarItem::new).collect()
    }
}

impl From<&str> for SidebarItem {
    fn from(label: &str) -> Self {
        SidebarItem::new(label)
    }
}

impl From<String> for SidebarItem {
    fn from(label: String) -> Self {
        SidebarItem::new(label)
    }
}

/// サイドバーの項目のまとまり。
///
/// まとまりの間には隙間が空き、見出しがあればその上に小さく出る
/// (macOS の「システム設定」や Finder のサイドバーと同じ形)。
/// 見出しは選べない。
///
/// ```
/// # use naui_core::{SidebarItem, SidebarSection};
/// let places = SidebarSection::new("場所", ["書類", "ダウンロード"]);
/// assert_eq!(places.title.as_deref(), Some("場所"));
/// assert_eq!(places.len(), 2);
///
/// // 見出しの無いまとまりは、隙間だけで区切られる。
/// let plain = SidebarSection::untitled(["一般", "外観"]);
/// assert_eq!(plain.title, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SidebarSection {
    /// まとまりの見出し。`None` なら見出しを出さない。
    pub title: Option<String>,
    /// まとまりに属する項目。
    pub items: Vec<SidebarItem>,
}

impl SidebarSection {
    /// 見出し付きのまとまりを作る。項目は文字列のままでも渡せる。
    pub fn new<I, T>(title: impl Into<String>, items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<SidebarItem>,
    {
        Self {
            title: Some(title.into()),
            items: items.into_iter().map(Into::into).collect(),
        }
    }

    /// 見出しの無いまとまりを作る。
    pub fn untitled<I, T>(items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<SidebarItem>,
    {
        Self {
            title: None,
            items: items.into_iter().map(Into::into).collect(),
        }
    }

    /// 項目数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// サイドバーに並ぶ 1 行。バックエンドが一覧を組み立てるときに使う。
///
/// まとまりを平らに並べ直したもので、項目には**まとまりをまたいで数えた
/// 通し番号**が付く。naui の通知と選択はこの番号でやり取りする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarRow<'a> {
    /// まとまりの間の隙間。2 つ目以降のまとまりの前にだけ入る。
    Gap,
    /// まとまりの見出し。
    Header(&'a str),
    /// 項目と、その通し番号。
    Item(usize, &'a SidebarItem),
}

impl SidebarRow<'_> {
    /// 項目の行なら通し番号。
    pub fn item_index(&self) -> Option<usize> {
        match self {
            SidebarRow::Item(index, _) => Some(*index),
            _ => None,
        }
    }
}

/// まとまりを、画面に並ぶ順の行へ平らにする。
///
/// 2 つ目以降のまとまりの前には [`SidebarRow::Gap`] が入る。見出しがあれば
/// 隙間の後ろに [`SidebarRow::Header`] が続く。項目の無いまとまりも見出しは
/// 出す (中身を後から足す画面のため)。
///
/// ```
/// # use naui_core::{sidebar_rows, SidebarRow, SidebarSection};
/// let sections = [
///     SidebarSection::untitled(["一般"]),
///     SidebarSection::new("場所", ["書類"]),
/// ];
/// let rows = sidebar_rows(&sections);
/// assert!(matches!(rows[0], SidebarRow::Item(0, _)));
/// assert_eq!(rows[1], SidebarRow::Gap);
/// assert_eq!(rows[2], SidebarRow::Header("場所"));
/// assert!(matches!(rows[3], SidebarRow::Item(1, _)));
/// ```
pub fn sidebar_rows(sections: &[SidebarSection]) -> Vec<SidebarRow<'_>> {
    let mut rows = Vec::new();
    let mut next = 0;
    for (i, section) in sections.iter().enumerate() {
        if i > 0 {
            rows.push(SidebarRow::Gap);
        }
        if let Some(title) = &section.title {
            rows.push(SidebarRow::Header(title));
        }
        for item in &section.items {
            rows.push(SidebarRow::Item(next, item));
            next += 1;
        }
    }
    rows
}

/// まとまりをまたいだ項目の総数。
pub fn sidebar_len(sections: &[SidebarSection]) -> usize {
    sections.iter().map(SidebarSection::len).sum()
}

/// 通し番号の項目。範囲外なら `None`。
pub fn sidebar_item(sections: &[SidebarSection], index: usize) -> Option<&SidebarItem> {
    sections.iter().flat_map(|s| s.items.iter()).nth(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_defaults_to_enabled_without_icon() {
        let item = SidebarItem::new("一般");
        assert_eq!(item.label, "一般");
        assert_eq!(item.icon, None);
        assert!(item.enabled);
        assert!(!SidebarItem::new("x").enabled(false).enabled);
    }

    #[test]
    fn sections_take_labels_or_items() {
        let section =
            SidebarSection::new("場所", [SidebarItem::new("書類").icon(ToolbarIcon::Open)]);
        assert_eq!(section.items[0].icon, Some(ToolbarIcon::Open));
        let plain = SidebarSection::untitled(vec![String::from("一般")]);
        assert_eq!(plain.items[0].label, "一般");
        assert!(SidebarSection::untitled(Vec::<SidebarItem>::new()).is_empty());
    }

    #[test]
    fn rows_number_items_across_sections() {
        let sections = [
            SidebarSection::new("A", ["a0", "a1"]),
            SidebarSection::untitled(["b0"]),
            SidebarSection::new("C", Vec::<SidebarItem>::new()),
            SidebarSection::new("D", ["d0"]),
        ];
        let rows = sidebar_rows(&sections);
        let shape: Vec<String> = rows
            .iter()
            .map(|row| match row {
                SidebarRow::Gap => "-".to_string(),
                SidebarRow::Header(title) => format!("#{title}"),
                SidebarRow::Item(index, item) => format!("{index}:{}", item.label),
            })
            .collect();
        assert_eq!(
            shape,
            ["#A", "0:a0", "1:a1", "-", "2:b0", "-", "#C", "-", "#D", "3:d0"]
        );
        assert_eq!(sidebar_len(&sections), 4);
        assert_eq!(
            sidebar_item(&sections, 3).map(|i| i.label.as_str()),
            Some("d0")
        );
        assert_eq!(sidebar_item(&sections, 4), None);
        assert_eq!(rows[1].item_index(), Some(0));
        assert_eq!(rows[0].item_index(), None);
    }

    #[test]
    fn empty_sections_make_no_rows() {
        assert!(sidebar_rows(&[]).is_empty());
        assert_eq!(sidebar_len(&[]), 0);
    }
}
