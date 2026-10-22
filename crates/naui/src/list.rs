use crate::sys;
use crate::Widget;
use naui_core::{ListItem, Point, SelectionMode};

/// 選択できる行の一覧。自分でスクロールする。
///
/// 高さを指定しないときは全行の高さに追従し、固定高さや `Fill` では
/// はみ出した分をスクロールする。
#[derive(Clone)]
pub struct List(pub(crate) sys::List);

impl_widget!(List);

impl List {
    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    #[inline]
    pub fn set_items(&self, items: &[ListItem]) {
        self.0.set_items(items)
    }

    /// 行を作り直す。通常行も任意内容の行もこの経路で描画する。
    ///
    /// 行内のコントロールは通常どおりそれぞれのコールバックを持てる。
    #[inline]
    pub fn set_rows(&self, rows: &[ListRow]) {
        self.0
            .set_rows(&rows.iter().map(|row| row.0.clone()).collect::<Vec<_>>())
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.set_selection_mode(mode)
    }

    #[inline]
    pub fn selection_mode(&self) -> SelectionMode {
        self.0.selection_mode()
    }

    /// 選ばれている先頭の行。
    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// 選ばれている行を昇順で返す。
    #[inline]
    pub fn selection(&self) -> Vec<usize> {
        self.0.selection()
    }

    /// `index` 行目が見えるところまでスクロールする。選択は変わらない。
    ///
    /// すでに見えていれば動かさず、見えていなければ近いほうの端に合わせる
    /// (上にあれば上端、下にあれば下端)。範囲外なら何もしない。
    #[inline]
    pub fn scroll_to_row(&self, index: usize) {
        self.0.scroll_to_row(index)
    }

    /// 通知せずに 1 行だけ選ぶ。
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// 通知せずに選択を置き換える。
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

    /// 行が起動されたとき (ダブルクリック、または行にフォーカスがある状態での
    /// Enter) に、その行のインデックスで呼ばれる。
    ///
    /// 複数選択のすべてを開きたいときは、中で [`List::selection`] を読む。
    /// 選べない `ListItem` (`enabled(false)`) の行では呼ばれない。
    #[inline]
    pub fn on_row_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_row_activate(f)
    }

    /// 行が右クリックされたときに、その行のインデックスと、押された位置で呼ばれる。
    ///
    /// 位置はこのリストの座標 (論理ピクセル、y は下向き) で、
    /// [`crate::PopupMenu::open_at`] へそのまま渡せる。選択は変えない。
    /// 行の無い余白では呼ばれない。
    #[inline]
    pub fn on_context_menu(&self, f: impl FnMut(usize, Point) + 'static) {
        self.0.on_context_menu(f)
    }
}

/// リストへ載せる 1 行。
///
/// [`ListItem`] は文字列だけで済む一覧向けの簡便 API であり、設定画面のように
/// アイコン、複数のラベル、チェックボックス、末尾のボタンなどを組み合わせる
/// 行は `Grid` / `Stack` で作って `ListRow` に包む。
///
/// ```no_run
/// # use naui::{ListRow, Widget};
/// # fn row(content: &dyn Widget) {
/// let row = ListRow::new(content).selectable(false);
/// row.on_activate(|| println!("行がクリックされました"));
/// # let _ = row;
/// # }
/// ```
#[derive(Clone)]
pub struct ListRow(pub(crate) sys::ListRow);

impl_backend!(ListRow);

impl ListRow {
    /// ウィジェットを 1 行の内容として使う。既定では行全体も選択できる。
    #[inline]
    pub fn new(content: &dyn Widget) -> Self {
        Self(sys::ListRow::new(content.as_backend()))
    }

    /// 行内のコントロールだけを操作する行では `false` にする。
    #[inline]
    pub fn selectable(self, selectable: bool) -> Self {
        Self(self.0.selectable(selectable))
    }

    #[inline]
    pub fn is_selectable(&self) -> bool {
        self.0.is_selectable()
    }

    /// 行内のラベル・アイコン・余白がクリックされたときに呼ぶ処理。
    ///
    /// チェックボックス、ボタン、入力欄などのコントロールを直接押した場合は
    /// (それぞれが自分でクリックを受け取るため) 呼ばれず、同じ操作が二重に
    /// 発火しない。
    ///
    /// `GtkListBoxRow` を押せるようにするかどうかは行を組み立てるときに
    /// 決まるので、[`List::set_rows`] へ渡す前に設定する。
    #[inline]
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.0.on_activate(f)
    }
}
