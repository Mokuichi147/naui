//! Value types for general-purpose (modal) dialogs.
//!
//! The shape of a dialog follows the narrowest of the four platforms, WinUI 3's
//! `ContentDialog`: **a title + body text + an optional widget + up to three
//! buttons with roles**, and the reason it closed is returned as a role.
//!
//! Buttons have roles because their placement and default behavior differ by
//! platform. Differences such as "the rightmost button is the default" (macOS) and
//! "the leftmost button is the default" (WinUI) are decided by each backend from
//! the roles, not from the order.

/// Why a dialog closed.
///
/// Corresponds to the roles of the buttons added with [`DialogButtons`]. Closing
/// with the Esc key or by clicking outside the dialog gives
/// [`DialogResponse::Cancel`] (even without a cancel button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogResponse {
    /// The primary action ("Save", "OK" and so on).
    Primary,
    /// The secondary action ("Don't Save" and so on).
    Secondary,
    /// Cancel. Closing with Esc gives this too.
    Cancel,
}

/// The buttons shown in a dialog. Zero or one per role.
///
/// ```
/// # use naui_core::{DialogButtons, DialogResponse};
/// let buttons = DialogButtons::new().primary("Save").cancel("Cancel");
/// assert_eq!(buttons.label(DialogResponse::Primary), Some("Save"));
/// assert_eq!(buttons.label(DialogResponse::Secondary), None);
/// ```
///
/// With none specified, only an "OK" button for closing is shown
/// ([`DialogButtons::resolved`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogButtons {
    primary: Option<String>,
    secondary: Option<String>,
    cancel: Option<String>,
}

impl DialogButtons {
    /// Starts with no buttons.
    pub const fn new() -> Self {
        Self {
            primary: None,
            secondary: None,
            cancel: None,
        }
    }

    /// A set with only an "OK" cancel button.
    pub fn ok() -> Self {
        Self::new().cancel("OK")
    }

    /// The button for the primary action. An empty string means no button.
    pub fn primary(mut self, label: impl Into<String>) -> Self {
        self.primary = non_empty(label);
        self
    }

    /// The button for the secondary action. An empty string means no button.
    pub fn secondary(mut self, label: impl Into<String>) -> Self {
        self.secondary = non_empty(label);
        self
    }

    /// The cancel button. An empty string means no button.
    pub fn cancel(mut self, label: impl Into<String>) -> Self {
        self.cancel = non_empty(label);
        self
    }

    /// The label of the button for the role. `None` if there is no such button.
    pub fn label(&self, response: DialogResponse) -> Option<&str> {
        let slot = match response {
            DialogResponse::Primary => &self.primary,
            DialogResponse::Secondary => &self.secondary,
            DialogResponse::Cancel => &self.cancel,
        };
        slot.as_deref()
    }

    /// Whether there are no buttons at all.
    pub fn is_empty(&self) -> bool {
        self.primary.is_none() && self.secondary.is_none() && self.cancel.is_none()
    }

    /// The buttons actually shown.
    ///
    /// If none were specified, this becomes just an "OK" cancel button. A dialog
    /// without buttons would leave Esc as the only way to close it on every platform.
    pub fn resolved(&self) -> DialogButtons {
        if self.is_empty() {
            DialogButtons::ok()
        } else {
            self.clone()
        }
    }

    /// Returns roles and labels in the order of the roles passed in `order`.
    ///
    /// Button order differs by platform (macOS puts the default button on the right,
    /// WinUI 3 on the left), so the backend decides the order and passes it in.
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
