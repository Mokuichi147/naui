//! 画面の組み方をそろえるための、見出しと並べ方。
//!
//! どの区分の画面も「見出しと、試せるウィジェット」の繰り返しでできている。
//! 見出しの下に説明の文は置かない。何を試す所かは、並べ方で伝える。
//!
//! - 同じ種類の小さなウィジェットは [`Form`] で「名前 | ウィジェット | 値」の
//!   3 列に並べる。左の名前が見出しの代わりになり、右端の値は、その行の
//!   操作で変わるものだとわかる。
//! - 一覧や表のような大きなウィジェットは見出しの下に置き、それを操作する
//!   ボタンは [`row`] で 1 行にまとめて、すぐ上か下へ置く。
//!
//! 操作の結果 (押された・選ばれた・保存した) は画面に Label を置かず、
//! [`Notice`] のトーストで知らせる。結果を出すためだけの行が並ばないぶん、
//! 画面にはウィジェットそのものが残る。
//!
//! 文字の見た目は [`naui::TextStyle`] と [`naui::TextColor`] へ渡す段階と
//! 役割だけで決める。級数も色の値もここには書かない (決めるのは OS)。
//!
//! 節はコンテナへまとめず、画面の `Stack` へ 1 段で積む。並びが 1 本だと
//! 余った高さの行き先が「いちばん外側の末尾」に決まり、どの環境でも同じ
//! 見え方になる。まとまりは見出しの前の空きと文字の段階で見せる。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui::{
    Align, Grid, GridCell, Label, Orientation, Padding, RadioGroup, Result, Sizing, Stack,
    TextColor, TextStyle, Toast, Toggle, Track, Ui, Widget,
};

/// 区分 1 つ分の画面の土台。
///
/// `Stack` の交差軸は既定が中央ぞろえで、幅の違うウィジェットが 1 つずつ
/// 中心に置かれてしまう。文章と同じ向きで読めるように、ギャラリーは
/// どの画面も左端をそろえる。
pub(crate) fn pane(ui: &Ui) -> Result<Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(12.0);
    pane.set_padding(Padding::all(12.0));
    pane.set_align(Align::Start);
    Ok(pane)
}

/// 節の見出しを置く。ウィジェットの種別ごとの区切りに使う。
pub(crate) fn section(ui: &Ui, pane: &Stack, title: &str) -> Result<()> {
    heading(ui, pane, TextStyle::Subtitle, title)
}

/// 節の中の小見出し。段階が 1 つ下がるだけで、置き方は節と同じ。
pub(crate) fn group(ui: &Ui, pane: &Stack, title: &str) -> Result<()> {
    heading(ui, pane, TextStyle::Heading, title)
}

/// 見出しの前の空き。節の中の間隔 (画面の `Stack` の 12) より広く空けて、
/// 前の節との切れ目を余白で見せる。
const SECTION_GAP: f64 = 16.0;

fn heading(ui: &Ui, pane: &Stack, style: TextStyle, title: &str) -> Result<()> {
    let heading = ui.label(title)?;
    heading.set_style(style);
    if pane.is_empty() {
        pane.append(&heading);
        return Ok(());
    }
    // Label は余白を持てないので、1 つだけ入れた Stack の余白で空ける。
    // 縦の Stack にする。macOS の Stack は主軸の向きにだけ中身へ縮むので、
    // 横の Stack だと縦に伸びて、見出しの下が空きすぎることがある。
    let gap = ui.stack(Orientation::Vertical)?;
    gap.set_align(Align::Start);
    gap.set_padding(Padding {
        top: SECTION_GAP,
        ..Padding::ZERO
    });
    gap.append(&heading);
    pane.append(&gap);
    Ok(())
}

/// ボタンなどを横 1 行に並べる。並べた先のウィジェットを操作するものを
/// まとめるのに使う。
pub(crate) fn row(ui: &Ui) -> Result<Stack> {
    let row = ui.stack(Orientation::Horizontal)?;
    row.set_spacing(8.0);
    Ok(row)
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
/// 画面に置いておく値だけに使う。アクセントカラーにして、名前や
/// ウィジェットの文字と見分けられるようにする。
pub(crate) fn readout(ui: &Ui, text: &str) -> Result<Label> {
    let readout = ui.label(text)?;
    readout.set_color(TextColor::Accent);
    Ok(readout)
}

/// 「名前 | ウィジェット | 値」の 3 列に並べる表。
///
/// 名前の列は長いものに合わせ、ウィジェットの列が残りの幅を受け取る。値の列は
/// 中身の幅だけを取る。値の無い行は、ウィジェットが値の列まで広がる。
///
/// 名前と値を横に並べるのは `Stack` ではなく `Grid` で組む。Windows の
/// `Stack` (StackPanel) は主軸の `Fill` を配らないので、横 `Stack` の中の
/// 入力欄は幅いっぱいに伸びない。
pub(crate) struct Form {
    grid: Grid,
    rows: Cell<usize>,
}

impl Form {
    /// 空の表を作り、`pane` へ置く。
    pub(crate) fn new(ui: &Ui, pane: &Stack) -> Result<Self> {
        let grid = ui.grid()?;
        grid.set_spacing(16.0, 10.0);
        grid.set_column_track(0, Track::Auto);
        grid.set_column_track(1, Track::FILL);
        grid.set_column_track(2, Track::Auto);
        grid.set_sizing(Sizing::fill_width());
        pane.append(&grid);
        Ok(Self {
            grid,
            rows: Cell::new(0),
        })
    }

    /// 名前とウィジェットの行を足す。
    pub(crate) fn row(&self, ui: &Ui, title: &str, widget: &dyn Widget) -> Result<()> {
        let row = self.next_row(ui, title)?;
        self.grid.attach(widget, GridCell::new(1, row).span(2, 1));
        Ok(())
    }

    /// 名前とウィジェットに、右端の値を添えた行を足す。
    pub(crate) fn row_with(
        &self,
        ui: &Ui,
        title: &str,
        widget: &dyn Widget,
        value: &dyn Widget,
    ) -> Result<()> {
        let row = self.next_row(ui, title)?;
        self.grid.attach(widget, GridCell::new(1, row));
        self.grid.attach(value, GridCell::new(2, row));
        Ok(())
    }

    fn next_row(&self, ui: &Ui, title: &str) -> Result<usize> {
        let row = self.rows.get();
        self.rows.set(row + 1);
        self.grid.attach(&ui.label(title)?, GridCell::new(0, row));
        Ok(row)
    }
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

/// 画面の中の操作できるウィジェットを、スイッチ 1 つでまとめて無効にする。
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
    /// スイッチを `parent` へ置く。画面の先頭 (画面の `Stack` か、その先頭の行)
    /// に置いて、画面全体に効くことを位置で見せる。
    pub(crate) fn new(ui: &Ui, parent: &Stack) -> Result<Self> {
        let toggle = ui.toggle("Disable controls on this page")?;
        let targets: Targets = Rc::default();
        toggle.on_toggle({
            let targets = targets.clone();
            move |disabled| {
                for set_enabled in targets.borrow().iter() {
                    set_enabled(!disabled);
                }
            }
        });
        parent.append(&toggle);
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
