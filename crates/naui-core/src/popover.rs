//! Value types for popovers (panels attached to a widget).

/// Which side of the widget a popover appears on.
///
/// This is a preference. When there is not enough room on that side, every platform
/// moves it to the opposite side (`NSPopover`, `GtkPopover`, `Flyout` and CSS's
/// `position-try-fallbacks` all do this themselves).
///
/// ```
/// # use naui_core::PopoverPosition;
/// assert_eq!(PopoverPosition::default(), PopoverPosition::Bottom);
/// assert_eq!(PopoverPosition::Top.opposite(), PopoverPosition::Bottom);
/// assert!(PopoverPosition::Left.is_horizontal());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PopoverPosition {
    /// Above the widget.
    Top,
    /// Below the widget (default). The usual place for a panel opened from a button.
    #[default]
    Bottom,
    /// To the left of the widget.
    Left,
    /// To the right of the widget.
    Right,
}

impl PopoverPosition {
    /// The side across the widget.
    pub fn opposite(self) -> Self {
        match self {
            PopoverPosition::Top => PopoverPosition::Bottom,
            PopoverPosition::Bottom => PopoverPosition::Top,
            PopoverPosition::Left => PopoverPosition::Right,
            PopoverPosition::Right => PopoverPosition::Left,
        }
    }

    /// Whether it appears beside the widget (left or right) rather than above or
    /// below it.
    pub fn is_horizontal(self) -> bool {
        matches!(self, PopoverPosition::Left | PopoverPosition::Right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_is_its_own_inverse() {
        for position in [
            PopoverPosition::Top,
            PopoverPosition::Bottom,
            PopoverPosition::Left,
            PopoverPosition::Right,
        ] {
            assert_ne!(position.opposite(), position);
            assert_eq!(position.opposite().opposite(), position);
            assert_eq!(
                position.opposite().is_horizontal(),
                position.is_horizontal()
            );
        }
    }
}
