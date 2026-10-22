use crate::sys;
use crate::Widget;
use naui_core::{Result, SelectionMode, SortOrder, TableColumn, TableRow};

/// 列見出しを持つ表。自分でスクロールする。
///
/// 高さは中身から決まらないので、[`Table::set_sizing`] で指定しておく。
#[derive(Clone)]
pub struct Table(pub(crate) sys::Table);

impl_widget!(Table);

impl Table {
    /// 列を作り直す。行と選択はそのまま残り、セルの並べ直しだけが起きる。
    ///
    /// 並べ替えの指定も、その列がまだ並べ替えられるなら残る。
    #[inline]
    pub fn set_columns(&self, columns: &[TableColumn]) {
        self.0.set_columns(columns)
    }

    /// 列数。
    #[inline]
    pub fn column_count(&self) -> usize {
        self.0.column_count()
    }

    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    ///
    /// 行数が多いときは、`GtkListBoxRow` を作るのも画面に出ている分だけに
    /// なる (残りは上下の詰め物が高さを持つので、スクロールバーの長さは
    /// 全行分のまま)。
    #[inline]
    pub fn set_rows(&self, rows: &[TableRow]) {
        self.0.set_rows(rows)
    }

    /// セルにウィジェットを置ける行を、**見えたときに組み立てる**形で渡す。
    ///
    /// `count` は行数で、`build` は 0 から `count - 1` のインデックスを受けて
    /// その行の中身 ([`TableCells`]) を返す。呼ばれるのは画面に出ている行
    /// (と、その少し前後) だけなので、行数が数十万になっても開くのは速い。
    ///
    /// ```no_run
    /// # use naui::{Table, TableCells};
    /// # fn fill(table: &Table, cities: Vec<String>) {
    /// table.set_row_builder(cities.len(), move |index| {
    ///     Ok(TableCells::new().text(&cities[index]))
    /// });
    /// # }
    /// ```
    #[inline]
    pub fn set_row_builder(
        &self,
        count: usize,
        mut build: impl FnMut(usize) -> Result<TableCells> + 'static,
    ) {
        self.0
            .set_row_builder(count, move |index| build(index).map(|cells| cells.0))
    }

    /// 行が選べるかどうかを、**行を組み立てずに**答える関数を渡す。
    ///
    /// [`Table::set_row_builder`] で組み立てる行では、まだ作っていない行が
    /// 選べるかどうかを naui は知らない。これを渡しておくと、画面の外の行に
    /// ついても [`Table::set_selection`] が「選べない行を取り除く」を守れる。
    /// 渡さないときは、作ってある行は [`TableCells::selectable`] に従い、
    /// まだ作っていない行は選べるものとして扱う。
    ///
    /// 返す答えは `build` が返す [`TableCells::selectable`] と同じにすること。
    ///
    /// ```no_run
    /// # use naui::{Table, TableCells};
    /// # fn fill(table: &Table, done: std::rc::Rc<Vec<bool>>) {
    /// let rows = done.clone();
    /// table.set_row_selectable(move |index| !rows[index]);
    /// # }
    /// ```
    #[inline]
    pub fn set_row_selectable(&self, selectable: impl Fn(usize) -> bool + 'static) {
        self.0.set_row_selectable(selectable)
    }

    /// 見えている行を組み立て直す。行数と選択はそのまま。
    #[inline]
    pub fn refresh(&self) {
        self.0.refresh()
    }

    /// 行の高さを論理ピクセルで決める。0 以下を渡すと、組み立てた行から測る。
    ///
    /// どの行も同じ高さになる。画面の外にある行の分は「行数 × この高さ」で
    /// 詰めるので、行の高さがそろっていないと、スクロールバーの長さが
    /// 実際と少しずれる。
    #[inline]
    pub fn set_row_height(&self, height: f64) {
        self.0.set_row_height(height)
    }

    /// 行の高さ (論理ピクセル)。まだ 1 行も組み立てていなければ 0。
    #[inline]
    pub fn row_height(&self) -> f64 {
        self.0.row_height()
    }

    /// 行数。
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 選び方を変える。選択の意味が変わるため、選択は外れる。
    #[inline]
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.set_selection_mode(mode)
    }

    #[inline]
    pub fn selection_mode(&self) -> SelectionMode {
        self.0.selection_mode()
    }

    /// 選ばれている行のうち、いちばん上のもの。
    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// 選ばれている行 (昇順)。単一選択なら 0 件か 1 件。
    ///
    /// 窓の外にある行も入る。`GtkListBox` が知っているのは組み立ててある行
    /// だけなので、覚えているほうを返す。
    #[inline]
    pub fn selection(&self) -> Vec<usize> {
        self.0.selection()
    }

    /// `index` 行目が見えるところまでスクロールする。選択は変わらない。
    ///
    /// すでに見えていれば動かさず、見えていなければ近いほうの端に合わせる
    /// (上にあれば上端、下にあれば下端)。範囲外なら何もしない。
    ///
    /// 行を絞って組み立てているときも、行の高さから位置を求めて送るので、
    /// まだ組み立てていない行へも送れる (送った先の行はその場で組み立てる)。
    #[inline]
    pub fn scroll_to_row(&self, index: usize) {
        self.0.scroll_to_row(index)
    }

    /// 通知せずに 1 行だけを選ぶ。
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// 通知せずに選択を置き換える。
    ///
    /// 範囲外・選べない行・重複は取り除かれ、単一選択なら先頭の 1 件だけが残る
    /// ([`SelectionMode::normalize_by`])。
    #[inline]
    pub fn set_selection(&self, indices: &[usize]) {
        self.0.set_selection(indices)
    }

    /// 通知せずに選択をすべて外す。
    #[inline]
    pub fn clear_selection(&self) {
        self.0.clear_selection()
    }

    /// ユーザーが選んだのと同じ経路で 1 行を選ぶ (通知あり)。
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// ユーザーが選んだのと同じ経路で選択を置き換える (通知あり)。
    #[inline]
    pub fn select_many(&self, indices: &[usize]) {
        self.0.select_many(indices)
    }

    /// 選択が変わるたびに、選ばれている行のインデックスで呼ばれる。
    #[inline]
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_select(f)
    }

    /// 見出しが押されて並べ替えの指定が変わったときに、
    /// 押された列と向きで呼ばれる。
    ///
    /// [`TableColumn::sortable`] を立てた列だけが押せる。**行を並べ替えるのは
    /// アプリの仕事**で、通知を受けたら並べ替えた行を [`Table::set_rows`] で
    /// 渡し直す (`set_rows` は選択を外すので、必要なら選び直す)。
    #[inline]
    pub fn on_sort(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        self.0.on_sort(f)
    }

    /// いまの並べ替えの指定 (列と向き)。押されたことが無ければ `None`。
    #[inline]
    pub fn sort(&self) -> Option<(usize, SortOrder)> {
        self.0.sort()
    }

    /// 通知せずに並べ替えの指定を置き換える。見出しの指標だけが変わる。
    ///
    /// 並べ替えられない列や範囲外の列を指すと、指定は外れる。
    #[inline]
    pub fn set_sort(&self, sort: Option<(usize, SortOrder)>) {
        self.0.set_sort(sort)
    }
}

/// 表へ載せる 1 行の中身。
///
/// [`TableRow`] は文字列だけで済む表向けの簡便 API であり、セルにボタンや
/// チェックボックス、アイコンを置きたいときは `Grid` / `Stack` で中身を
/// 作ってこの型へ並べる。行は [`Table::set_row_builder`] から返す。
///
/// ```no_run
/// # use naui::{Table, TableCells};
/// # fn fill(table: &Table, cities: Vec<String>) {
/// table.set_row_builder(cities.len(), move |index| {
///     Ok(TableCells::new().text(&cities[index]).text("13,960,000"))
/// });
/// # }
/// ```
#[derive(Clone, Default)]
pub struct TableCells(pub(crate) sys::TableCells);

impl_backend!(TableCells);

impl TableCells {
    /// セルが 1 つも無い行を作る。
    #[inline]
    pub fn new() -> Self {
        Self(sys::TableCells::new())
    }

    /// 文字のセルを 1 つ足す。揃えは列の指定に従う。
    #[inline]
    pub fn text(self, text: impl Into<String>) -> Self {
        Self(self.0.text(text))
    }

    /// ウィジェットのセルを 1 つ足す。
    #[inline]
    pub fn cell(self, content: &dyn Widget) -> Self {
        Self(self.0.cell(content.as_backend()))
    }

    /// 行全体を選択できるようにするかどうか (既定はできる)。
    #[inline]
    pub fn selectable(self, selectable: bool) -> Self {
        Self(self.0.selectable(selectable))
    }

    #[inline]
    pub fn is_selectable(&self) -> bool {
        self.0.is_selectable()
    }

    /// 列数。
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 行のセルや余白がクリックされたときに呼ぶ処理。
    ///
    /// ボタンや入力欄を直接押した場合は、そのコントロールがクリックを
    /// 受け取るので呼ばれない。
    #[inline]
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.0.on_activate(f)
    }
}
