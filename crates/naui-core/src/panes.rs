//! Showing a list and its detail side by side or one at a time.
//!
//! Only the values and the decision are here. The panes themselves are each
//! backend's native control (`AdwNavigationSplitView`, WinUI's `TwoPaneView`) or a
//! container naui puts together where there is none (AppKit, the DOM).

use crate::layout::reaches_breakpoint;

/// The narrowest width (logical pixels) at which `AdaptivePanes` shows both panes.
///
/// The same as WinUI `TwoPaneView`'s default `MinWideModeWidth` (641, which it
/// compares with "wider than"), rounded to a width that counts as reaching it.
pub const DEFAULT_PANES_BREAKPOINT: f64 = 640.0;

/// The width of the list pane while both panes are shown (logical pixels).
///
/// The same as `AdwNavigationSplitView`'s default largest sidebar width.
pub const DEFAULT_LIST_PANE_WIDTH: f64 = 280.0;

/// One of the two panes of `AdaptivePanes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    /// The list (or navigation) the detail is chosen from. Shown at the start (left).
    #[default]
    List,
    /// The detail of what is chosen in the list. Shown at the end (right).
    Detail,
}

/// Decides whether `AdaptivePanes` shows both panes or one, and which one. Used by
/// backends whose toolkit does not decide it.
///
/// It is told the container's width after each layout:
///
/// - At least as wide as the breakpoint, both panes are shown; narrower, one at a
///   time.
/// - Once both are shown, they stay shown until the width drops a few pixels (8)
///   below the breakpoint, the same slack as `StackArrangement`.
/// - Before the first width arrives, both are shown (desktop windows are usually
///   wide). Changing the breakpoint later decides again from the last width,
///   without slack.
///
/// Which pane the one-pane state shows is kept separately, and also changes while
/// both are shown, so a detail opened in a wide window stays open when it narrows.
///
/// ```
/// # use naui_core::{Pane, PanesArrangement};
/// let mut panes = PanesArrangement::new(600.0);
/// assert!(!panes.is_collapsed());
/// assert!(panes.resized(500.0));
/// assert!(panes.is_collapsed());
/// assert!(panes.shows(Pane::List) && !panes.shows(Pane::Detail));
/// panes.show(Pane::Detail);
/// assert!(panes.shows_back());
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PanesArrangement {
    breakpoint: f64,
    /// The last width. 0 until the first one arrives.
    width: f64,
    /// Whether the last width reached the breakpoint.
    wide: bool,
    /// The pane shown while only one is.
    visible: Pane,
}

impl Default for PanesArrangement {
    fn default() -> Self {
        Self::new(DEFAULT_PANES_BREAKPOINT)
    }
}

impl PanesArrangement {
    pub const fn new(breakpoint: f64) -> Self {
        Self {
            breakpoint,
            width: 0.0,
            wide: true,
            visible: Pane::List,
        }
    }

    /// Changes the breakpoint, deciding again from the last width.
    pub fn set_breakpoint(&mut self, breakpoint: f64) {
        self.breakpoint = breakpoint;
        if self.width > 0.0 {
            self.wide = self.width >= breakpoint;
        }
    }

    pub fn breakpoint(&self) -> f64 {
        self.breakpoint
    }

    /// Takes the container's width and returns whether
    /// [`is_collapsed`](Self::is_collapsed) changed.
    ///
    /// A width of 0 or less (not laid out yet, or hidden) is ignored.
    pub fn resized(&mut self, width: f64) -> bool {
        if !width.is_finite() || width <= 0.0 {
            return false;
        }
        let first = self.width <= 0.0;
        self.width = width;
        let before = self.wide;
        // 最初の幅には遊びを持たせない (既定の「2 区画」は幅から決めたものではないため)。
        self.wide = reaches_breakpoint(before && !first, width, self.breakpoint);
        self.wide != before
    }

    /// Whether only one pane is shown.
    pub fn is_collapsed(&self) -> bool {
        !self.wide
    }

    /// The pane shown while only one is.
    pub fn visible(&self) -> Pane {
        self.visible
    }

    /// Chooses the pane shown while only one is. Returns whether it changed.
    pub fn show(&mut self, pane: Pane) -> bool {
        std::mem::replace(&mut self.visible, pane) != pane
    }

    /// Whether `pane` is on screen now.
    pub fn shows(&self, pane: Pane) -> bool {
        self.wide || self.visible == pane
    }

    /// Whether the way back to the list is shown: only the detail is on screen.
    pub fn shows_back(&self) -> bool {
        !self.wide && self.visible == Pane::Detail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_width_with_slack() {
        let mut p = PanesArrangement::new(600.0);
        assert!(!p.is_collapsed(), "幅が来るまでは 2 区画");
        assert!(!p.resized(0.0), "並ぶ前の 0 は数えない");
        assert!(!p.resized(f64::NAN));
        assert!(!p.resized(600.0), "ちょうどは広い側");
        assert!(!p.resized(593.0), "しきい値のすぐ下では畳まない");
        assert!(p.resized(591.0));
        assert!(p.is_collapsed());
        assert!(
            !p.resized(599.0),
            "畳んだあとは、しきい値までは 1 区画のまま"
        );
        assert!(p.resized(600.0));
        assert!(!p.is_collapsed());
    }

    #[test]
    fn the_first_width_has_no_slack() {
        let mut p = PanesArrangement::new(600.0);
        assert!(p.resized(595.0), "最初の幅がしきい値より狭ければすぐ畳む");
        assert!(p.is_collapsed());
    }

    #[test]
    fn a_new_breakpoint_decides_from_the_last_width() {
        let mut p = PanesArrangement::new(600.0);
        p.set_breakpoint(900.0);
        assert!(!p.is_collapsed(), "幅が来る前は変えない");
        p.resized(800.0);
        assert!(p.is_collapsed());
        p.set_breakpoint(800.0);
        assert_eq!(p.breakpoint(), 800.0);
        assert!(!p.is_collapsed(), "ちょうどは広い側");
        p.set_breakpoint(805.0);
        assert!(p.is_collapsed(), "決め直しには遊びを持たせない");
    }

    #[test]
    fn the_visible_pane_is_kept_across_widths() {
        let mut p = PanesArrangement::new(600.0);
        p.resized(800.0);
        assert!(p.shows(Pane::List) && p.shows(Pane::Detail));
        assert!(!p.shows_back(), "2 区画のときは戻る手段を出さない");
        assert!(p.show(Pane::Detail));
        assert!(!p.show(Pane::Detail), "同じ区画は変化として返さない");
        assert!(!p.shows_back());

        p.resized(500.0);
        assert!(!p.shows(Pane::List) && p.shows(Pane::Detail));
        assert!(
            p.shows_back(),
            "広いときに開いた詳細は、狭まっても開いたまま"
        );

        p.show(Pane::List);
        assert_eq!(p.visible(), Pane::List);
        assert!(p.shows(Pane::List) && !p.shows(Pane::Detail));
        assert!(!p.shows_back());
    }
}
