//! レイアウト用のコンテナ (`Grid` / `Scroll` / `Spacer`)。
//!
//! 計算するのは GTK4 のレイアウト (`GtkGrid` / `GtkScrolledWindow`) で、
//! naui 側はプロパティを書くだけ。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use naui_core::{GridCell, Padding, ScrollPolicy, Track};

use crate::bin::{apply_padding, SizeBin};
use crate::widgets::{impl_widget, Widget};

// ------------------------------------------------------------------- Grid

struct GridInner {
    native: gtk::Grid,
    bin: SizeBin,
    /// 置いた子と、その置き場所。列 / 行の指定を後から変えるために持つ。
    children: RefCell<Vec<(GridCell, Box<dyn Widget>)>>,
    column_tracks: RefCell<Vec<Track>>,
    row_tracks: RefCell<Vec<Track>>,
}

/// 行と列で位置を決めるコンテナ (`GtkGrid`)。
#[derive(Clone)]
pub struct Grid(Rc<GridInner>);
impl_widget!(Grid);

impl Grid {
    pub(crate) fn new() -> Self {
        let native = gtk::Grid::new();
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(GridInner {
            native,
            bin,
            children: RefCell::new(Vec::new()),
            column_tracks: RefCell::new(Vec::new()),
            row_tracks: RefCell::new(Vec::new()),
        }))
    }

    pub fn set_spacing(&self, column: f64, row: f64) {
        self.0.native.set_column_spacing(to_px(column) as u32);
        self.0.native.set_row_spacing(to_px(row) as u32);
    }

    pub fn set_padding(&self, padding: Padding) {
        apply_padding(&self.0.native, padding);
    }

    /// マスへ置く。すでに何かが置かれているマスへ重ねて置ける。
    pub fn attach(&self, child: &dyn Widget, cell: GridCell) {
        let bin = child.size_bin();
        self.0.native.attach(
            &bin,
            cell.column as i32,
            cell.row as i32,
            cell.column_span as i32,
            cell.row_span as i32,
        );
        self.apply_tracks_to(&bin, cell);
        self.0
            .children
            .borrow_mut()
            .push((cell, child.boxed_clone()));
    }

    /// マスの中身を差し替える。同じマスに置かれていたものは外れる。
    pub fn replace(&self, child: &dyn Widget, cell: GridCell) {
        self.remove_at(cell);
        self.attach(child, cell);
    }

    /// 指定のマスに置かれているものを外す。
    fn remove_at(&self, cell: GridCell) {
        let mut children = self.0.children.borrow_mut();
        let mut index = 0;
        while index < children.len() {
            if children[index].0.column == cell.column && children[index].0.row == cell.row {
                let (_, child) = children.remove(index);
                self.0.native.remove(&child.size_bin());
            } else {
                index += 1;
            }
        }
    }

    /// 列の幅の決め方。
    ///
    /// `GtkGrid` は列そのものに幅を持たせられないため、**その列に入っている
    /// 子**へ写す。列の幅は中身のいちばん大きいものに合わせて決まるので、
    /// 結果として列の幅が決まる。[`Track::Fill`] の重みは `GtkGrid` に無く、
    /// 余りは広がる列で等分される (macOS と同じ制限)。
    pub fn set_column_track(&self, index: usize, track: Track) {
        set_track(&self.0.column_tracks, index, track);
        self.reapply_tracks();
    }

    /// 行の高さの決め方。[`Grid::set_column_track`] と同じ制限がある。
    pub fn set_row_track(&self, index: usize, track: Track) {
        set_track(&self.0.row_tracks, index, track);
        self.reapply_tracks();
    }

    fn reapply_tracks(&self) {
        for (cell, child) in self.0.children.borrow().iter() {
            self.apply_tracks_to(&child.size_bin(), *cell);
        }
    }

    /// 1 つの子へ、置かれている列と行の指定を写す。
    ///
    /// 複数マスにまたがる子は、列 / 行の幅を決める役目を負わない
    /// (どの列の幅なのかが決まらないため)。
    fn apply_tracks_to(&self, bin: &SizeBin, cell: GridCell) {
        if cell.column_span == 1 {
            if let Some(track) = self.0.column_tracks.borrow().get(cell.column).copied() {
                bin.apply_track(true, track);
            }
        }
        if cell.row_span == 1 {
            if let Some(track) = self.0.row_tracks.borrow().get(cell.row).copied() {
                bin.apply_track(false, track);
            }
        }
    }

    /// 置かれているものが必要とする列数。
    pub fn columns(&self) -> usize {
        self.0
            .children
            .borrow()
            .iter()
            .map(|(cell, _)| cell.columns_needed())
            .max()
            .unwrap_or(0)
    }

    /// 置かれているものが必要とする行数。
    pub fn rows(&self) -> usize {
        self.0
            .children
            .borrow()
            .iter()
            .map(|(cell, _)| cell.rows_needed())
            .max()
            .unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 列 / 行の指定を、必要なだけ伸ばしてから書き込む。
fn set_track(tracks: &RefCell<Vec<Track>>, index: usize, track: Track) {
    let mut tracks = tracks.borrow_mut();
    if tracks.len() <= index {
        tracks.resize(index + 1, Track::Auto);
    }
    tracks[index] = track;
}

// ----------------------------------------------------------------- Scroll

struct ScrollInner {
    native: gtk::ScrolledWindow,
    bin: SizeBin,
    child: RefCell<Option<Box<dyn Widget>>>,
}

/// 中身がはみ出したらスクロールさせるコンテナ (`GtkScrolledWindow`)。
#[derive(Clone)]
pub struct Scroll(Rc<ScrollInner>);
impl_widget!(Scroll);

impl Scroll {
    pub(crate) fn new() -> Self {
        let native = gtk::ScrolledWindow::new();
        native.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(ScrollInner {
            native,
            bin,
            child: RefCell::new(None),
        }))
    }

    pub fn set_policy(&self, horizontal: ScrollPolicy, vertical: ScrollPolicy) {
        self.0
            .native
            .set_policy(to_policy(horizontal), to_policy(vertical));
        self.follow_natural_size();
    }

    pub fn set_child(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        // スクロールの中では、中身は自分の大きさのまま置かれる。
        bin.fill_parent();
        self.0.native.set_child(Some(&bin));
        self.follow_natural_size();
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// 送れる向きでは、中身を「自然な大きさ」で置く。
    ///
    /// `GtkScrolledWindow` は自分でスクロールしない中身を `GtkViewport` へ
    /// 包む。その既定 (`Minimum`) は、場所が足りないときに中身を**最小の
    /// 大きさまで潰す**。高さを指定していない `List` のように最小と自然な
    /// 大きさが違うウィジェットが、1 行分まで縮んでしまう。
    ///
    /// 送れる向き (`Auto` / `Always`) は、はみ出してもスクロールで見られる
    /// ので自然な大きさで置く。送れない向き (`Never`) は既定のままにする。
    /// スクロールバーが出ない以上、はみ出した分は見えなくなるため。
    fn follow_natural_size(&self) {
        let Some(viewport) = self.0.native.child().and_downcast::<gtk::Viewport>() else {
            return;
        };
        viewport.set_hscroll_policy(scrollable_policy(self.0.native.hscrollbar_policy()));
        viewport.set_vscroll_policy(scrollable_policy(self.0.native.vscrollbar_policy()));
    }
}

/// スクロールバーの出し方から、中身の置き方を決める。
fn scrollable_policy(policy: gtk::PolicyType) -> gtk::ScrollablePolicy {
    match policy {
        gtk::PolicyType::Never => gtk::ScrollablePolicy::Minimum,
        _ => gtk::ScrollablePolicy::Natural,
    }
}

fn to_policy(policy: ScrollPolicy) -> gtk::PolicyType {
    match policy {
        ScrollPolicy::Auto => gtk::PolicyType::Automatic,
        ScrollPolicy::Always => gtk::PolicyType::Always,
        ScrollPolicy::Never => gtk::PolicyType::Never,
    }
}

// ----------------------------------------------------------------- Spacer

struct SpacerInner {
    native: gtk::Box,
    bin: SizeBin,
}

/// 余白そのものになるウィジェット。スタックの余りを吸って他を押しやる。
#[derive(Clone)]
pub struct Spacer(Rc<SpacerInner>);
impl_widget!(Spacer);

impl Spacer {
    pub(crate) fn new() -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let bin = SizeBin::wrap(&native);
        // 中身が無いので自然な大きさは 0。余りだけを受け取る。
        bin.apply_sizing(naui_core::Sizing::fill());
        Self(Rc::new(SpacerInner { native, bin }))
    }
}

fn to_px(value: f64) -> i32 {
    value.round().clamp(0.0, i32::MAX as f64) as i32
}
