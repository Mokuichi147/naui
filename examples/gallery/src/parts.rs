//! 画面の組み方をそろえるための、見出しと文字づかい。
//!
//! 10 枚のタブはどれも「見出し・説明・試せるウィジェット」の繰り返しで
//! できている。同じ形をタブごとに手で組むと、見出しの大きさも余白も
//! ばらつくので、組み方をここへ 1 か所にまとめる。
//!
//! 操作の結果 (押された・選ばれた・保存した) は画面に Label を置かず、
//! [`Notice`] のトーストで知らせる。結果を出すためだけの行が並ばないぶん、
//! タブにはウィジェットそのものが残る。
//!
//! 文字の見た目は [`naui::TextStyle`] と [`naui::TextColor`] へ渡す段階と
//! 役割だけで決める。級数も色の値もここには書かない (決めるのは OS)。
//!
//! 節はコンテナへまとめず、タブの `Stack` へ 1 段で積む。並びが 1 本だと
//! 余った高さの行き先が「いちばん外側の末尾」に決まり、どの環境でも同じ
//! 見え方になる。まとまりは文字の段階と色で見せる。

use std::cell::RefCell;
use std::rc::Rc;

use naui::{
    Align, Label, Orientation, Padding, RadioGroup, Result, Sizing, Stack, TextColor, TextStyle,
    Toast, Toggle, Ui,
};

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

/// 見出しを付けて横に並べた `RadioGroup`。表示のしかたなどを 1 つ選ばせる所に使う。
///
/// `Navbar` も見た目は似ているが、あちらは画面を移るためのもので、読み上げでも
/// ナビゲーションとして扱われる。その場の設定を選ぶだけならこちらを使う。
/// 最初は先頭が選ばれている。
pub(crate) fn choice(ui: &Ui, title: &str, items: &[&str]) -> Result<(Stack, RadioGroup)> {
    let row = ui.stack(Orientation::Horizontal)?;
    row.set_spacing(12.0);
    row.append(&ui.label(title)?);
    let radio = ui.radio_group()?;
    radio.set_items(items);
    radio.set_orientation(Orientation::Horizontal);
    radio.set_selected(0);
    row.append(&radio);
    Ok((row, radio))
}

/// 入力に合わせて変わる表示 (文字数・合計・進捗など)。
///
/// 操作の結果を知らせるのは [`Notice`] の役目で、こちらは実際のアプリでも
/// 画面に置いておく値だけに使う。アクセントカラーにして、説明の文と
/// 見分けられるようにする。
pub(crate) fn readout(ui: &Ui, text: &str) -> Result<Label> {
    let readout = ui.label(text)?;
    readout.set_color(TextColor::Accent);
    Ok(readout)
}

/// 操作の結果を知らせるトースト。ギャラリー全体で 1 つを使い回す。
///
/// トーストは同時に 1 つしか出ないので、知らせごとに作る必要は無い。
#[derive(Clone)]
pub(crate) struct Notice(Toast);

impl Notice {
    pub(crate) fn new(ui: &Ui) -> Result<Self> {
        Ok(Self(ui.toast("")?))
    }

    /// `text` を知らせる。
    ///
    /// 出ている間は文字だけを書き換える。スライダーを動かしている間や
    /// 文字を打っている間のように通知が続けて届くと、出し直すたびに
    /// 消えては現れるのを繰り返してしまうため。
    pub(crate) fn show(&self, text: &str) {
        self.0.set_message(text);
        if !self.0.is_visible() {
            self.0.show();
        }
    }
}

/// タブの中の操作できるウィジェットを、スイッチ 1 つでまとめて無効にする。
///
/// 無効の見た目は、別に作った無効なウィジェットを並べるより、いま触っている
/// ものをその場で切り替えたほうが違いがわかる。入れた値が残ることや、
/// 有効へ戻せることも同時に確かめられる。
pub(crate) struct Disabler {
    toggle: Toggle,
    targets: Targets,
}

/// 有効・無効を切り替える先。引数は「有効にするか」。
type Targets = Rc<RefCell<Vec<Box<dyn Fn(bool)>>>>;

impl Disabler {
    /// 節の見出しと説明、スイッチを `pane` へ置く。
    pub(crate) fn new(ui: &Ui, pane: &Stack, notes: &[&str]) -> Result<Self> {
        section(ui, pane, "無効状態", notes)?;
        let toggle = ui.toggle("この画面の操作を無効にする")?;
        let targets: Targets = Rc::default();
        toggle.on_toggle({
            let targets = targets.clone();
            move |disabled| {
                for set_enabled in targets.borrow().iter() {
                    set_enabled(!disabled);
                }
            }
        });
        pane.append(&toggle);
        Ok(Self { toggle, targets })
    }

    /// スイッチで切り替える対象に加える。`set_enabled` には
    /// `naui::TextInput::set_enabled` のように型のメソッドを渡す。
    ///
    /// 後から加えたものも、いまのスイッチの状態に合わせる。
    pub(crate) fn add<W: Clone + 'static>(&self, widget: &W, set_enabled: fn(&W, bool)) {
        let widget = widget.clone();
        if self.toggle.is_on() {
            set_enabled(&widget, false);
        }
        self.targets
            .borrow_mut()
            .push(Box::new(move |enabled| set_enabled(&widget, enabled)));
    }
}
