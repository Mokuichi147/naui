use crate::sys;
use crate::Widget;
use naui_core::{GridCell, Padding, ScrollMetrics, ScrollPolicy, Track};

/// 行と列で位置を決めるコンテナ (`GtkGrid`)。
#[derive(Clone)]
pub struct Grid(pub(crate) sys::Grid);

impl_widget!(Grid);

impl Grid {
    #[inline]
    pub fn set_spacing(&self, column: f64, row: f64) {
        self.0.set_spacing(column, row)
    }

    #[inline]
    pub fn set_padding(&self, padding: Padding) {
        self.0.set_padding(padding)
    }

    /// マスへ置く。すでに何かが置かれているマスへ重ねて置ける。
    #[inline]
    pub fn attach(&self, child: &dyn Widget, cell: GridCell) {
        self.0.attach(child.as_backend(), cell)
    }

    /// そのマスの中身を差し替える。同じマスに置かれていたものは外れる。
    #[inline]
    pub fn replace(&self, child: &dyn Widget, cell: GridCell) {
        self.0.replace(child.as_backend(), cell)
    }

    /// 子をすべて外す。行と列の指定はそのまま残る。
    #[inline]
    pub fn clear(&self) {
        self.0.clear()
    }

    /// 指定したマスに置かれているものを外す。何も無ければ何もしない。
    ///
    /// 見るのは `cell` の列と行だけで、span は見ない。
    #[inline]
    pub fn remove(&self, cell: GridCell) {
        self.0.remove(cell)
    }

    /// 列の幅の決め方。
    ///
    /// `GtkGrid` は列そのものに幅を持たせられないため、**その列に入っている
    /// 子**へ写す。列の幅は中身のいちばん大きいものに合わせて決まるので、
    /// 結果として列の幅が決まる。[`Track::Fill`] の重みは `GtkGrid` に無く、
    /// 余りは広がる列で等分される (macOS と同じ制限)。
    #[inline]
    pub fn set_column_track(&self, index: usize, track: Track) {
        self.0.set_column_track(index, track)
    }

    /// 行の高さの決め方。[`Grid::set_column_track`] と同じ制限がある。
    #[inline]
    pub fn set_row_track(&self, index: usize, track: Track) {
        self.0.set_row_track(index, track)
    }

    /// 置かれているものが必要とする列数。
    #[inline]
    pub fn columns(&self) -> usize {
        self.0.columns()
    }

    /// 置かれているものが必要とする行数。
    #[inline]
    pub fn rows(&self) -> usize {
        self.0.rows()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// 中身がはみ出したらスクロールさせるコンテナ (`GtkScrolledWindow`)。
#[derive(Clone)]
pub struct Scroll(pub(crate) sys::Scroll);

impl_widget!(Scroll);

impl Scroll {
    /// いまのスクロール位置と大きさ。
    ///
    /// GTK4 は同期でレイアウトさせられないので、直前に中身を変えたときは
    /// 次のフレームまで前の大きさのまま。
    #[inline]
    pub fn metrics(&self) -> ScrollMetrics {
        self.0.metrics()
    }

    /// 指定した位置へ送る。送れる範囲に丸める。
    ///
    /// 直前に中身を変えていても、次のレイアウトの後の大きさで送り直す。
    #[inline]
    pub fn scroll_to(&self, x: f64, y: f64) {
        self.0.scroll_to(x, y)
    }

    /// 縦の末尾 (いちばん下) へ送る。横の位置はそのまま。
    #[inline]
    pub fn scroll_to_end(&self) {
        self.0.scroll_to_end()
    }

    /// スクロール位置が変わったときの通知。
    ///
    /// 利用者の操作に加え、`scroll_to` や中身が縮んだことによる移動でも
    /// 届く。大きさだけが変わったときは届かない。
    #[inline]
    pub fn on_scroll(&self, f: impl FnMut(ScrollMetrics) + 'static) {
        self.0.on_scroll(f)
    }

    #[inline]
    pub fn set_policy(&self, horizontal: ScrollPolicy, vertical: ScrollPolicy) {
        self.0.set_policy(horizontal, vertical)
    }

    #[inline]
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.set_child(child.as_backend())
    }
}

/// 余白そのものになるウィジェット。スタックの余りを吸って他を押しやる。
#[derive(Clone)]
pub struct Spacer(pub(crate) sys::Spacer);

impl_widget!(Spacer);
