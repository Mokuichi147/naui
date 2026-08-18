//! 汎用ダイアログ (モーダル) の値型。
//!
//! ダイアログの形は、4 環境のうちいちばん狭い WinUI 3 の `ContentDialog` に
//! そろえてある。すなわち **見出し + 本文 + 任意のウィジェット + 役割つきの
//! ボタン 3 つまで**で、閉じた理由は役割で返る。
//!
//! 役割を持たせるのは、置き場所と既定のふるまいが環境ごとに違うため。
//! 「いちばん右が既定のボタン」(macOS) と「いちばん左が既定のボタン」(WinUI)
//! のような差は、並びではなく役割から各バックエンドが決める。

/// ダイアログが閉じた理由。
///
/// [`DialogButtons`] で付けたボタンの役割に対応する。Esc キーや、
/// ダイアログの外側を押して閉じたときは [`DialogResponse::Cancel`] になる
/// (取り消しボタンを置いていなくても同じ)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogResponse {
    /// 主となる操作 (「保存」「OK」など)。
    Primary,
    /// 副となる操作 (「保存しない」など)。
    Secondary,
    /// 取り消し。Esc で閉じたときもこれになる。
    Cancel,
}

/// ダイアログに出すボタン。役割ごとに 0 個か 1 個。
///
/// ```
/// # use naui_core::{DialogButtons, DialogResponse};
/// let buttons = DialogButtons::new().primary("保存").cancel("キャンセル");
/// assert_eq!(buttons.label(DialogResponse::Primary), Some("保存"));
/// assert_eq!(buttons.label(DialogResponse::Secondary), None);
/// ```
///
/// 1 つも指定しないと、閉じるための「OK」だけが出る ([`DialogButtons::resolved`])。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogButtons {
    primary: Option<String>,
    secondary: Option<String>,
    cancel: Option<String>,
}

impl DialogButtons {
    /// ボタンを 1 つも持たない状態から始める。
    pub const fn new() -> Self {
        Self {
            primary: None,
            secondary: None,
            cancel: None,
        }
    }

    /// 「OK」の取り消しボタンだけを持つ組み合わせ。
    pub fn ok() -> Self {
        Self::new().cancel("OK")
    }

    /// 主となる操作のボタン。空文字列を渡すと置かない。
    pub fn primary(mut self, label: impl Into<String>) -> Self {
        self.primary = non_empty(label);
        self
    }

    /// 副となる操作のボタン。空文字列を渡すと置かない。
    pub fn secondary(mut self, label: impl Into<String>) -> Self {
        self.secondary = non_empty(label);
        self
    }

    /// 取り消しのボタン。空文字列を渡すと置かない。
    pub fn cancel(mut self, label: impl Into<String>) -> Self {
        self.cancel = non_empty(label);
        self
    }

    /// 役割に対応するボタンの文字列。置いていなければ `None`。
    pub fn label(&self, response: DialogResponse) -> Option<&str> {
        let slot = match response {
            DialogResponse::Primary => &self.primary,
            DialogResponse::Secondary => &self.secondary,
            DialogResponse::Cancel => &self.cancel,
        };
        slot.as_deref()
    }

    /// ボタンを 1 つも持たないか。
    pub fn is_empty(&self) -> bool {
        self.primary.is_none() && self.secondary.is_none() && self.cancel.is_none()
    }

    /// 実際に出すボタン。
    ///
    /// 1 つも指定されていなければ「OK」の取り消しボタンだけにそろえる。
    /// ボタンの無いダイアログは、どの環境でも閉じる手段が Esc だけになって
    /// しまうため。
    pub fn resolved(&self) -> DialogButtons {
        if self.is_empty() {
            DialogButtons::ok()
        } else {
            self.clone()
        }
    }

    /// 役割と文字列を、`order` で渡した役割の順に返す。
    ///
    /// ボタンの並び順は環境ごとに違う (macOS は既定のボタンが右端、
    /// WinUI 3 は左端) ため、順番はバックエンドが決めて渡す。
    pub fn in_order(&self, order: &[DialogResponse]) -> Vec<(DialogResponse, String)> {
        order
            .iter()
            .filter_map(|&role| self.label(role).map(|label| (role, label.to_string())))
            .collect()
    }
}

fn non_empty(label: impl Into<String>) -> Option<String> {
    let label = label.into();
    if label.is_empty() {
        None
    } else {
        Some(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_keep_labels_per_role() {
        let buttons = DialogButtons::new()
            .primary("保存")
            .secondary("保存しない")
            .cancel("キャンセル");
        assert_eq!(buttons.label(DialogResponse::Primary), Some("保存"));
        assert_eq!(buttons.label(DialogResponse::Secondary), Some("保存しない"));
        assert_eq!(buttons.label(DialogResponse::Cancel), Some("キャンセル"));
        assert!(!buttons.is_empty());
    }

    #[test]
    fn empty_label_removes_the_button() {
        let buttons = DialogButtons::new().primary("OK").primary("");
        assert_eq!(buttons.label(DialogResponse::Primary), None);
        assert!(buttons.is_empty());
    }

    #[test]
    fn no_button_resolves_to_a_single_ok() {
        let resolved = DialogButtons::new().resolved();
        assert_eq!(resolved.label(DialogResponse::Cancel), Some("OK"));
        assert_eq!(resolved.label(DialogResponse::Primary), None);
        assert_eq!(DialogButtons::ok(), resolved);
    }

    #[test]
    fn resolved_keeps_the_given_buttons() {
        let buttons = DialogButtons::new().primary("削除");
        assert_eq!(buttons.resolved(), buttons);
    }

    #[test]
    fn in_order_skips_missing_roles() {
        let buttons = DialogButtons::new().primary("保存").cancel("キャンセル");
        let order = [
            DialogResponse::Primary,
            DialogResponse::Cancel,
            DialogResponse::Secondary,
        ];
        assert_eq!(
            buttons.in_order(&order),
            [
                (DialogResponse::Primary, "保存".to_string()),
                (DialogResponse::Cancel, "キャンセル".to_string()),
            ]
        );
    }
}
