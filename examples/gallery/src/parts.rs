//! 画面の組み方をそろえるための、見出しと文字づかい。
//!
//! 9 枚のタブはどれも「見出し・説明・試せるウィジェット・操作結果」の
//! 繰り返しでできている。同じ形をタブごとに手で組むと、見出しの大きさも
//! 余白もばらつくので、組み方をここへ 1 か所にまとめる。
//!
//! 文字の見た目は [`naui::TextStyle`] と [`naui::TextColor`] へ渡す段階と
//! 役割だけで決める。級数も色の値もここには書かない (決めるのは OS)。
//!
//! 節はコンテナへまとめず、タブの `Stack` へ 1 段で積む。並びが 1 本だと
//! 余った高さの行き先が「いちばん外側の末尾」に決まり、どの環境でも同じ
//! 見え方になる。まとまりは文字の段階と色で見せる。

use naui::{Align, Label, Orientation, Padding, Result, Sizing, Stack, TextColor, TextStyle, Ui};

/// タブ 1 枚分の土台。
///
/// `Stack` の交差軸は既定が中央ぞろえで、幅の違うウィジェットが 1 つずつ
/// 中心に置かれてしまう。文章と同じ向きで読めるように、ギャラリーは
/// どのタブも左端をそろえる。
pub(crate) fn pane(ui: &Ui) -> Result<Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(12.0);
    pane.set_padding(Padding::all(12.0));
    pane.set_align(Align::Start);
    Ok(pane)
}

/// 節の見出しと説明を置く。ウィジェットの種別ごとの区切りに使う。
pub(crate) fn section(ui: &Ui, pane: &Stack, title: &str, notes: &[&str]) -> Result<()> {
    heading(ui, pane, TextStyle::Subtitle, title, notes)
}

/// 節の中の小見出し。段階が 1 つ下がるだけで、置き方は節と同じ。
pub(crate) fn group(ui: &Ui, pane: &Stack, title: &str, notes: &[&str]) -> Result<()> {
    heading(ui, pane, TextStyle::Heading, title, notes)
}

fn heading(ui: &Ui, pane: &Stack, style: TextStyle, title: &str, notes: &[&str]) -> Result<()> {
    let heading = ui.label(title)?;
    heading.set_style(style);
    pane.append(&heading);
    for text in notes {
        pane.append(&note(ui, text)?);
    }
    Ok(())
}

/// 説明の文。小さく淡い文字にして、見出しと操作の邪魔をしないようにする。
pub(crate) fn note(ui: &Ui, text: &str) -> Result<Label> {
    let note = ui.label(text)?;
    note.set_style(TextStyle::Caption);
    note.set_color(TextColor::Secondary);
    // 窓を狭めても読めるように折り返す。折り返す幅は親が決めるので幅も渡す。
    note.set_wrap(true);
    note.set_sizing(Sizing::fill_width());
    Ok(note)
}

/// 操作の結果を出す 1 行。アクセントカラーにして、説明の文と見分けられるようにする。
pub(crate) fn status(ui: &Ui, text: &str) -> Result<Label> {
    let status = ui.label(text)?;
    status.set_color(TextColor::Accent);
    Ok(status)
}
