//! リストの値型。
//!
//! リストは「行の並び + いま選ばれている行」という構造を持つ。
//! ナビゲーション ([`crate::NavItem`]) と形は似ているが、
//!
//! - 行数が多く、自前でスクロールする
//! - 複数選択がある (選択が 0 件にもなる)
//!
//! という違いがあるため、別の型として持つ。バックエンドはこれを
//! `NSTableView` / `ListBox` / `<select>` の行へ写す。

/// リストの 1 行。
///
/// 行の識別はアプリ側が持つ順序 (インデックス) で行う。
/// 選択の通知もインデックスで返るので、`&[ListItem]` を作った側が
/// そのまま自分のデータへ引き当てられる。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListItem {
    /// 画面に出る文字列。
    pub label: String,
    /// 補助の文字列。指定すると 2 行目に小さく出る。
    ///
    /// **Web だけは 1 行に収まる。** `<option>` は 1 行のテキストしか
    /// 持てないため、`ラベル — 補助` の形で同じ行に続けて出る。
    pub detail: Option<String>,
    /// 選べるかどうか。
    pub enabled: bool,
}

impl ListItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            detail: None,
            enabled: true,
        }
    }

    /// 2 行目に出す補助の文字列を指定する。
    ///
    /// ```
    /// # use naui_core::ListItem;
    /// let item = ListItem::new("東京").detail("13,960,000 人");
    /// assert_eq!(item.detail.as_deref(), Some("13,960,000 人"));
    /// ```
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// 選べるかどうかを指定する (既定は選べる)。
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// 文字列の並びから行の列を作る。
    ///
    /// ```
    /// # use naui_core::ListItem;
    /// let rows = ListItem::list(["東京", "大阪", "札幌"]);
    /// assert_eq!(rows.len(), 3);
    /// assert!(rows[0].enabled);
    /// ```
    pub fn list<I, S>(labels: I) -> Vec<ListItem>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        labels.into_iter().map(ListItem::new).collect()
    }
}

impl From<&str> for ListItem {
    fn from(label: &str) -> Self {
        ListItem::new(label)
    }
}

impl From<String> for ListItem {
    fn from(label: String) -> Self {
        ListItem::new(label)
    }
}

/// リストの選び方。
///
/// | 値 | macOS | Windows | Web |
/// | --- | --- | --- | --- |
/// | [`SelectionMode::Single`] | `allowsMultipleSelection = false` | `SelectionMode::Single` | `<select>` |
/// | [`SelectionMode::Multiple`] | `allowsMultipleSelection = true` | `SelectionMode::Extended` | `<select multiple>` |
///
/// `Multiple` はどの環境でも「⌘ / Ctrl や Shift を押しながら選ぶ」形になる。
/// WinUI の `SelectionMode::Multiple` (クリックのたびに反転する) ではなく
/// `Extended` に写しているのはこのため。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// 1 行だけ選べる (既定)。
    #[default]
    Single,
    /// 複数行を選べる。選択が 0 件になることもある。
    Multiple,
}

impl SelectionMode {
    pub fn is_multiple(self) -> bool {
        matches!(self, SelectionMode::Multiple)
    }

    /// 指定された選択を、そのまま渡せる形にそろえる。
    ///
    /// - 範囲外のインデックスを捨てる
    /// - 選べない行 ([`ListItem::enabled`] が `false`) を捨てる
    /// - 昇順に並べ、重複を取り除く
    /// - [`SelectionMode::Single`] のときは先頭の 1 件だけにする
    ///
    /// バックエンドはどれもこれを通してからネイティブへ渡すので、
    /// 3 環境で `set_selection` の意味がそろう。
    ///
    /// ```
    /// # use naui_core::{ListItem, SelectionMode};
    /// let items = ListItem::list(["a", "b", "c"]);
    /// let picked = SelectionMode::Multiple.normalize(&items, &[2, 0, 2, 9]);
    /// assert_eq!(picked, vec![0, 2]);
    /// // 単一選択では先頭の 1 件だけが残る。
    /// assert_eq!(SelectionMode::Single.normalize(&items, &[2, 0]), vec![0]);
    /// ```
    pub fn normalize(self, items: &[ListItem], indices: &[usize]) -> Vec<usize> {
        self.normalize_by(indices, |i| items.get(i).is_some_and(|item| item.enabled))
    }

    /// 行が [`ListItem`] でない一覧 (テーブルなど) 向けの [`normalize`]。
    ///
    /// `enabled` は「その行が存在し、選べるか」を返す。範囲外の行は
    /// `false` を返せばよい。そろえ方は [`normalize`] と同じ。
    ///
    /// [`normalize`]: SelectionMode::normalize
    ///
    /// ```
    /// # use naui_core::SelectionMode;
    /// let enabled = |i: usize| matches!(i, 0 | 2);
    /// assert_eq!(SelectionMode::Multiple.normalize_by(&[2, 1, 0], enabled), vec![0, 2]);
    /// assert_eq!(SelectionMode::Single.normalize_by(&[2, 0], enabled), vec![0]);
    /// ```
    pub fn normalize_by(
        self,
        indices: &[usize],
        mut enabled: impl FnMut(usize) -> bool,
    ) -> Vec<usize> {
        let mut picked: Vec<usize> = indices.iter().copied().filter(|&i| enabled(i)).collect();
        picked.sort_unstable();
        picked.dedup();
        if !self.is_multiple() {
            picked.truncate(1);
        }
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_item_defaults_to_enabled() {
        let item = ListItem::new("東京");
        assert_eq!(item.label, "東京");
        assert_eq!(item.detail, None);
        assert!(item.enabled);
        assert!(!ListItem::new("大阪").enabled(false).enabled);
        assert_eq!(ListItem::from("札幌"), ListItem::new("札幌"));
        assert_eq!(ListItem::from(String::from("那覇")).label, "那覇");
    }

    #[test]
    fn detail_is_optional_and_keeps_the_other_fields() {
        let item = ListItem::new("東京").detail("首都").enabled(false);
        assert_eq!(item.label, "東京");
        assert_eq!(item.detail.as_deref(), Some("首都"));
        assert!(!item.enabled);
        assert!(ListItem::list(["東京"])[0].detail.is_none());
    }

    #[test]
    fn selection_mode_defaults_to_single() {
        assert_eq!(SelectionMode::default(), SelectionMode::Single);
        assert!(SelectionMode::Multiple.is_multiple());
        assert!(!SelectionMode::Single.is_multiple());
    }

    #[test]
    fn normalize_sorts_and_drops_out_of_range() {
        let items = ListItem::list(["a", "b", "c"]);
        assert_eq!(
            SelectionMode::Multiple.normalize(&items, &[2, 0, 2, 3, 100]),
            vec![0, 2]
        );
        assert_eq!(SelectionMode::Multiple.normalize(&items, &[]), Vec::new());
    }

    #[test]
    fn normalize_drops_disabled_rows() {
        let items = vec![
            ListItem::new("a"),
            ListItem::new("b").enabled(false),
            ListItem::new("c"),
        ];
        assert_eq!(
            SelectionMode::Multiple.normalize(&items, &[0, 1, 2]),
            vec![0, 2]
        );
        // 選べない行だけを指定したときは、何も選ばれない。
        assert!(SelectionMode::Single.normalize(&items, &[1]).is_empty());
    }

    #[test]
    fn single_keeps_only_the_first_row() {
        let items = ListItem::list(["a", "b", "c"]);
        assert_eq!(SelectionMode::Single.normalize(&items, &[2, 1]), vec![1]);
    }
}
