//! Button roles.

/// The role of a button. The default is [`ButtonRole::Normal`].
///
/// | Role | macOS | Windows | Linux | Web |
/// | --- | --- | --- | --- | --- |
/// | `Normal` | (default) | (default) | (default) | (default) |
/// | `Primary` | `tintProminence = .primary` (macOS 26 and later) | `AccentButtonStyle` | `.suggested-action` | none (`data-naui-role="primary"`) |
/// | `Destructive` | `hasDestructiveAction` | none | `.destructive-action` | none (`data-naui-role="destructive"`) |
///
/// The role changes only the look. **Return does not activate a `Primary` button**
/// (on macOS it is not given the Return key equivalent that makes the window's
/// default button), and `Destructive` asks for no confirmation.
///
/// ```
/// # use naui_core::ButtonRole;
/// assert_eq!(ButtonRole::default(), ButtonRole::Normal);
/// assert_eq!(ButtonRole::Primary.style_class(), Some("suggested-action"));
/// assert_eq!(ButtonRole::Destructive.xaml_style_key(), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ButtonRole {
    /// An ordinary button (default).
    #[default]
    Normal,
    /// The action most likely to be chosen, such as "Save" or "OK". Drawn in the
    /// accent color. Use it for at most one button in a group.
    Primary,
    /// An action that destroys data, such as "Delete" or "Discard".
    Destructive,
}

impl ButtonRole {
    /// Every role. Used to check that the mapping tables are exhaustive.
    pub const ALL: [ButtonRole; 3] = [
        ButtonRole::Normal,
        ButtonRole::Primary,
        ButtonRole::Destructive,
    ];

    /// The style class used on Linux (GTK4 / libadwaita).
    ///
    /// [`ButtonRole::Normal`] gets no class.
    pub fn style_class(self) -> Option<&'static str> {
        match self {
            ButtonRole::Normal => None,
            ButtonRole::Primary => Some("suggested-action"),
            ButtonRole::Destructive => Some("destructive-action"),
        }
    }

    /// The `x:Key` of the `Style` in Windows (WinUI 3)'s resources.
    ///
    /// WinUI has no style for destructive actions, so only
    /// [`ButtonRole::Primary`] has one. For the others the `Style` is cleared and
    /// the default button style applies.
    pub fn xaml_style_key(self) -> Option<&'static str> {
        match self {
            ButtonRole::Primary => Some("AccentButtonStyle"),
            ButtonRole::Normal | ButtonRole::Destructive => None,
        }
    }

    /// The value of the `data-naui-role` attribute written on the Web. `None` for
    /// [`ButtonRole::Normal`], which writes no attribute.
    ///
    /// Browsers have no look for these roles, so naui only marks the button. An app
    /// that wants a look can select it in its own CSS
    /// (`button[data-naui-role="primary"]`).
    pub fn web_attribute(self) -> Option<&'static str> {
        match self {
            ButtonRole::Normal => None,
            ButtonRole::Primary => Some("primary"),
            ButtonRole::Destructive => Some("destructive"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn roles_map_to_distinct_targets() {
        assert_eq!(
            ButtonRole::ALL.len(),
            ButtonRole::ALL.iter().collect::<HashSet<_>>().len()
        );
        let classes: HashSet<_> = ButtonRole::ALL.iter().map(|r| r.style_class()).collect();
        assert_eq!(classes.len(), ButtonRole::ALL.len());
        let attributes: HashSet<_> = ButtonRole::ALL.iter().map(|r| r.web_attribute()).collect();
        assert_eq!(attributes.len(), ButtonRole::ALL.len());
    }

    /// Only `Normal` is "unset". If this breaks, buttons with nothing specified would
    /// get classes or attributes too.
    #[test]
    fn only_normal_is_unset() {
        for role in ButtonRole::ALL {
            let normal = role == ButtonRole::Normal;
            assert_eq!(
                role.style_class().is_none(),
                normal,
                "{role:?} のスタイルクラス"
            );
            assert_eq!(role.web_attribute().is_none(), normal, "{role:?} の属性");
        }
        assert_eq!(ButtonRole::Normal.xaml_style_key(), None);
    }
}
