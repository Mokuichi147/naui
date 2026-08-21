//! `GtkCheckButton` の「印」を、ラベルの字面の中心へそろえる。
//!
//! GTK4 は印 (`check` / `radio` ノード) をラベルの**行の箱**
//! (ascent + descent) の中心に置く。欧文のフォントはこの箱と字面の中心が
//! ほぼ重なるので気にならないが、日本語を含む行は ascent が大きく取られる
//! ため、字面は箱の中心より下に来る。その結果、印だけが上に浮いて見える。
//!
//! そこで印へ上マージンを足し、**ベースラインから見た字面の中心**へ寄せ直す。
//! 字面の中心は「ベースラインから大文字の高さの半分だけ上」とみなす。欧文の
//! 大文字と日本語の字面は、ベースラインから見てほぼ同じ高さに収まるので、
//! この 1 つの目安で両方そろう。
//!
//! マージンは行の箱に収まる範囲でしか足さない。はみ出す量は切り捨てるので、
//! **チェックボックスの大きさは変わらない**。

use gtk::pango;
use gtk::prelude::*;

/// 印の位置をそろえ、画面に出るたびにそろえ直す。
///
/// 答えはフォントによって変わる。`map` で測り直すのは、親に付く前の
/// ウィジェットには画面のフォント設定がまだ届いていないことがあるため。
/// アプリが動いている最中にデスクトップのフォントが変わった場合は、次に
/// 画面へ出るまで前の位置のままになる (ずれても数ピクセル)。
pub(crate) fn watch(button: &gtk::CheckButton) {
    align(button);
    button.connect_map(align);
}

/// 印をラベルの字面の中心へそろえ直す。ラベルの無いものは何もしない。
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

/// 印へ足す上マージン (論理ピクセル)。
///
/// 上マージンは印の箱だけを縦に伸ばす。箱は行の中で中央に置かれるため、
/// **印そのものはマージンの半分だけ下がる**。動かしたい量の 2 倍を返す。
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

/// ラベルのフォントの大文字の高さ (論理ピクセル)。
///
/// Pango の計量に大文字の高さは無いので、`H` を組んで字面の高さを測る。
fn cap_height(label: &gtk::Label) -> f64 {
    label
        .create_pango_layout(Some("H"))
        .pixel_extents()
        .0
        .height() as f64
}
