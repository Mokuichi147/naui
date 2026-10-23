//! Aligns the "indicator" of a `GtkCheckButton` with the center of the label's
//! glyphs.
//!
//! GTK4 places the indicator (the `check` / `radio` node) at the center of the
//! label's **line box** (ascent + descent). For Latin fonts the box's center and the
//! glyphs' center nearly coincide, so it is not noticeable, but lines containing
//! Japanese get a large ascent, so the glyphs sit below the box's center. As a
//! result, only the indicator appears to float upward.
//!
//! So a top margin is added to the indicator to move it back to **the center of the
//! glyphs as seen from the baseline**. The glyph center is taken to be "half the
//! cap height above the baseline". Latin capitals and Japanese glyphs fit into
//! nearly the same height above the baseline, so this one measure aligns both.
//!
//! The margin is added only within the line box. Any excess is cut off, so **the
//! size of the check box does not change**.

use gtk::pango;
use gtk::prelude::*;

/// Aligns the indicator's position, and re-aligns it every time it appears on
/// screen.
///
/// The answer depends on the font. It is measured again on `map` because a widget
/// not yet attached to a parent may not have received the screen's font settings.
/// If the desktop font changes while the app is running, the old position remains
/// until it next appears on screen (off by a few pixels at most).
pub(crate) fn watch(button: &gtk::CheckButton) {
    align(button);
    button.connect_map(align);
}

/// Re-aligns the indicator with the center of the label's glyphs. Does nothing for
/// buttons without a label.
fn align(button: &gtk::CheckButton) {
    let Some(indicator) = button.first_child() else {
        return;
    };
    let Some(label) = indicator.next_sibling().and_downcast::<gtk::Label>() else {
        return;
    };
    let margin = top_margin(&label, &indicator);
    // 同じ値でも `set_margin_top` は再レイアウトを頼むので、変わるときだけ書く。
    if indicator.margin_top() != margin {
        indicator.set_margin_top(margin);
    }
}

/// The top margin added to the indicator (logical pixels).
///
/// A top margin stretches only the indicator's box vertically. The box is centered
/// within the line, so **the indicator itself moves down by half the margin**.
/// Returns twice the amount to move.
fn top_margin(label: &gtk::Label, indicator: &gtk::Widget) -> i32 {
    let layout = label.layout();
    let line_height = layout.pixel_extents().1.height();
    let baseline = layout.baseline() as f64 / pango::SCALE as f64;
    let ideal = baseline - cap_height(label) / 2.0;
    let shift = ideal - line_height as f64 / 2.0;
    if shift <= 0.0 {
        return 0;
    }
    // 印の箱が行の箱より高くなると、その分だけボタン全体が高くなる。
    // 見た目の中心よりボタンの大きさを優先し、収まる範囲で止める。
    //
    // `gtk_widget_measure` はマージンを含んだ大きさを返すので、**前に足した
    // マージンを引いてから**比べる。引かないと、2 回目に測ったときは余白が
    // 無いと見えてしまい、そろえたぶんを自分で取り消してしまう。
    let (_, height, _, _) = indicator.measure(gtk::Orientation::Vertical, -1);
    let bare = height - indicator.margin_top() - indicator.margin_bottom();
    let slack = (line_height - bare).max(0);
    (shift * 2.0).round().clamp(0.0, slack as f64) as i32
}

/// The cap height of the label's font (logical pixels).
///
/// Pango's metrics have no cap height, so an `H` is laid out and the height of its
/// glyph is measured.
fn cap_height(label: &gtk::Label) -> f64 {
    label
        .create_pango_layout(Some("H"))
        .pixel_extents()
        .0
        .height() as f64
}
