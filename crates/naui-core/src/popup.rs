//! ポップアップ (コンテキスト) メニューの項目。

/// ポップアップメニューの 1 項目。
///
/// 右クリック (副ボタン) で出るメニューの中身を表す。項目の識別は
/// アプリ側が持つ順序 (インデックス) で行い、選択の通知も
/// **区切り線を含めた並びの中でのインデックス**で返る。
/// 区切り線は選べないので、そのインデックスが返ることはない。
///
/// ```
/// # use naui_core::PopupItem;
/// let items = [
///     PopupItem::new("コピー"),
///     PopupItem::separator(),
///     PopupItem::new("削除").enabled(false),
/// ];
/// assert!(items[1].is_separator());
/// assert!(!items[2].enabled);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupItem {
    /// 画面に出る文字列。区切り線では使われない。
    pub label: String,
    /// 選べるかどうか。
    pub enabled: bool,
    /// 区切り線かどうか。
    pub separator: bool,
}

impl PopupItem {
    /// 押せる項目を作る。
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            separator: false,
        }
    }

    /// 区切り線を作る。選ぶことはできない。
    pub fn separator() -> Self {
        Self {
            label: String::new(),
            enabled: false,
            separator: true,
        }
    }

    /// 選べるかどうかを指定する (既定は選べる)。
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// 区切り線かどうか。
    pub fn is_separator(&self) -> bool {
        self.separator
    }

    /// 文字列の並びから項目列を作る。
    ///
    /// ```
    /// # use naui_core::PopupItem;
    /// let items = PopupItem::list(["コピー", "貼り付け"]);
    /// assert_eq!(items.len(), 2);
    /// ```
    pub fn list<I, S>(labels: I) -> Vec<PopupItem>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        labels.into_iter().map(PopupItem::new).collect()
    }
}

impl Default for PopupItem {
    fn default() -> Self {
        PopupItem::new("")
    }
}

impl From<&str> for PopupItem {
    fn from(label: &str) -> Self {
        PopupItem::new(label)
    }
}

impl From<String> for PopupItem {
    fn from(label: String) -> Self {
        PopupItem::new(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_item_defaults_to_enabled() {
        let item = PopupItem::new("コピー");
        assert_eq!(item.label, "コピー");
        assert!(item.enabled);
        assert!(!item.is_separator());
        assert!(!PopupItem::new("削除").enabled(false).enabled);
    }

    #[test]
    fn separator_is_never_selectable() {
        let item = PopupItem::separator();
        assert!(item.is_separator());
        assert!(!item.enabled);
        assert!(item.label.is_empty());
    }

    #[test]
    fn popup_item_list_keeps_order() {
        let items = PopupItem::list(["切り取り", "コピー"]);
        assert_eq!(items[0], PopupItem::from("切り取り"));
        assert_eq!(items[1].label, "コピー");
    }
}
