//! 「画面に出ている行」の求め方。
//!
//! 行数が数万を超えると、行を 1 つずつウィジェットにしていては開くだけで
//! 固まってしまう。`NSTableView` は自分で必要な行だけを組み立てるが、
//! `GtkListBox` / `ListView` / `<table>` は渡した行をすべて作る。そこで
//! **スクロール位置から見えている範囲を求め、その分だけを組み立てる**。
//!
//! 範囲の求め方はどのバックエンドでも同じなので、計算だけをここへ置く。
//! ここには描画も測定も無く、入るのは数値だけである (行の高さを測るのは
//! バックエンドの仕事)。
//!
//! ```
//! # use naui_core::{row_window, RowWindow};
//! // 20 px の行が 100,000 行。上から 1,000 px の位置に、高さ 200 px の枠。
//! let window = row_window(100_000, 1_000.0, 200.0, 20.0, 5);
//! assert_eq!(window.start, 45, "見えている 50 行目の少し手前から");
//! assert!(window.end >= 60 && window.end < 100, "画面の少し先まで");
//! // 見えていない行の分は、上下の詰め物の高さになる。
//! assert_eq!(window.leading(20.0), 900.0);
//! ```

use std::ops::Range;

/// 行を「見えている分だけ」に絞り始める行数。
///
/// これ以下なら全行を組み立てる。少ない行でわざわざ絞ると、
/// キーボード操作やスクロールバーの挙動がネイティブのままでなくなるため、
/// **ふつうの大きさの表は今までどおり**にしておく。
pub const ROW_WINDOW_THRESHOLD: usize = 200;

/// 画面の外にも余分に組み立てておく行数。
///
/// スクロールの通知が届く前に空白が見えてしまわないよう、上下に少し多めに
/// 作っておく。大きくすると空白は出にくくなるが、組み立ての量は増える。
pub const ROW_WINDOW_OVERSCAN: usize = 12;

/// いま組み立てておく行の範囲。
///
/// `start..end` の半開区間で、`end` は含まない。`count` は表の全行数で、
/// 上下の詰め物 ([`RowWindow::leading`] / [`RowWindow::trailing`]) の
/// 高さを求めるのに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowWindow {
    /// 組み立てる先頭の行。
    pub start: usize,
    /// 組み立てる最後の行の次 (この行は含まない)。
    pub end: usize,
    /// 表の全行数。
    pub count: usize,
}

impl RowWindow {
    /// 全行を組み立てる範囲。
    pub fn all(count: usize) -> Self {
        Self {
            start: 0,
            end: count,
            count,
        }
    }

    /// 組み立てる行数。
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// 全行を組み立てているか。絞られていなければ `true`。
    pub fn is_complete(self) -> bool {
        self.start == 0 && self.end >= self.count
    }

    /// その行を組み立てているか。
    pub fn contains(self, index: usize) -> bool {
        self.start <= index && index < self.end
    }

    /// 組み立てる行の並び。
    pub fn indices(self) -> Range<usize> {
        self.start..self.end
    }

    /// `needed` の行がすべてこの窓の中にあるか。
    ///
    /// スクロールのたびに作り直さずに済ませるために使う。作るときは
    /// [`ROW_WINDOW_OVERSCAN`] 行を余分に持たせ、確かめるときはその半分で
    /// 見ると、余りの半分ぶんスクロールするまで作り直さなくてよくなる。
    ///
    /// ```
    /// # use naui_core::{row_window, RowWindow, ROW_WINDOW_OVERSCAN};
    /// let built = row_window(10_000, 1_000.0, 200.0, 20.0, ROW_WINDOW_OVERSCAN);
    /// // 1 行ぶんスクロールしただけなら、まだ作り直さなくてよい。
    /// let needed = row_window(10_000, 1_020.0, 200.0, 20.0, ROW_WINDOW_OVERSCAN / 2);
    /// assert!(built.covers(needed));
    /// ```
    pub fn covers(self, needed: RowWindow) -> bool {
        self.count == needed.count && self.start <= needed.start && self.end >= needed.end
    }

    /// 範囲より前にある行が占める高さ (上の詰め物)。
    pub fn leading(self, row_height: f64) -> f64 {
        self.start as f64 * row_height.max(0.0)
    }

    /// 範囲より後にある行が占める高さ (下の詰め物)。
    pub fn trailing(self, row_height: f64) -> f64 {
        self.count.saturating_sub(self.end) as f64 * row_height.max(0.0)
    }
}

/// スクロール位置から、組み立てておく行の範囲を求める。
///
/// - `count`: 表の全行数
/// - `scroll_top`: いちばん上の行から、枠の上端までの距離 (論理ピクセル)
/// - `viewport`: 行が見えている高さ (論理ピクセル)
/// - `row_height`: 1 行の高さ (論理ピクセル)。どの行も同じ高さとみなす
/// - `overscan`: 画面の外にも作っておく行数 ([`ROW_WINDOW_OVERSCAN`])
///
/// 行数が [`ROW_WINDOW_THRESHOLD`] 以下なら、絞らずに全行を返す。
/// 行の高さや枠の高さがまだ分からない (0 以下や `NaN`) ときは、
/// 測るための足がかりとして先頭から [`ROW_WINDOW_THRESHOLD`] 行を返す。
///
/// ```
/// # use naui_core::row_window;
/// // 行数が少なければ、絞らない。
/// assert!(row_window(50, 0.0, 100.0, 20.0, 5).is_complete());
/// // 高さがまだ分からないうちは、先頭だけを作って測れるようにする。
/// assert_eq!(row_window(10_000, 0.0, 0.0, 0.0, 5).start, 0);
/// ```
pub fn row_window(
    count: usize,
    scroll_top: f64,
    viewport: f64,
    row_height: f64,
    overscan: usize,
) -> RowWindow {
    if count <= ROW_WINDOW_THRESHOLD {
        return RowWindow::all(count);
    }
    let unknown = !row_height.is_finite() || row_height <= 0.0 || !viewport.is_finite();
    if unknown || viewport <= 0.0 {
        // まだ大きさが決まっていない。先頭だけを作れば、そこから
        // 行の高さを測って次の回で絞り直せる。
        return RowWindow {
            start: 0,
            end: count.min(ROW_WINDOW_THRESHOLD),
            count,
        };
    }

    let top = scroll_top.max(0.0);
    // 行数が減った直後など、いまの位置が表の外を指していることがある。
    // 範囲が空にならないよう、最後の行までに抑える。
    let first = ((top / row_height).floor() as usize).min(count - 1);
    // 端が半分だけ見えている分を足して、画面の下端を含む行まで数える。
    let visible = (viewport / row_height).ceil() as usize + 1;
    let start = first.saturating_sub(overscan);
    let end = first
        .saturating_add(visible)
        .saturating_add(overscan)
        .min(count);
    RowWindow {
        start: start.min(end),
        end,
        count,
    }
}

/// いま組み立ててある窓のままでよいか。
///
/// スクロールのたびに全部作り直すのは重いので、**余分に作ってある分の半分
/// までは、そのまま使う**。`needed` は [`ROW_WINDOW_OVERSCAN`] の半分で求めた
/// 「最低限これだけは要る」範囲、`next` は作り直すとしたときの窓。
///
/// 作り直すのは次のどちらか。
///
/// - いまの窓が `needed` を覆えていない (スクロールで外れた)
/// - いまの窓が `next` より大きい (大きさが分かる前に多めに作った分を捨てる)
///
/// ```
/// # use naui_core::{keeps_row_window, row_window, ROW_WINDOW_OVERSCAN};
/// let current = row_window(10_000, 1_000.0, 200.0, 20.0, ROW_WINDOW_OVERSCAN);
/// let needed = row_window(10_000, 1_040.0, 200.0, 20.0, ROW_WINDOW_OVERSCAN / 2);
/// let next = row_window(10_000, 1_040.0, 200.0, 20.0, ROW_WINDOW_OVERSCAN);
/// // 2 行ぶんのスクロールなら作り直さない。
/// assert!(keeps_row_window(current, needed, next));
/// ```
pub fn keeps_row_window(current: RowWindow, needed: RowWindow, next: RowWindow) -> bool {
    current.covers(needed) && current.len() <= next.len()
}

/// 窓の外にある選択を残すか。
///
/// 行を絞っている一覧では、ネイティブのコントロールが知っているのは
/// **組み立ててある行の選択だけ**である。ユーザーが行を押したときに
/// 返ってくるのもその範囲なので、窓の外の選択を残してよいのか、
/// 押し直しとして落とすのかを、変わり方から見分ける。
///
/// - 窓の中の選択が増えただけ (⌘ / Ctrl を押しながら足した) → 残す
/// - 窓の中の選択が減っただけ (⌘ / Ctrl を押しながら外した) → 残す
/// - 入れ替わった (ふつうのクリックや Shift の範囲選び) → 落とす
///
/// `previous` は前の選択のうち窓の中にあったもの、`current` はネイティブが
/// 返してきた選択。どちらも昇順で重複が無いこと。
///
/// ```
/// # use naui_core::keeps_hidden_selection;
/// // 足しただけなら、画面の外で選ばれている行はそのまま。
/// assert!(keeps_hidden_selection(&[1], &[1, 3]));
/// // 外しただけでも、そのまま。
/// assert!(keeps_hidden_selection(&[1, 3], &[1]));
/// // 別の行を押したときは、選び直しなので落とす。
/// assert!(!keeps_hidden_selection(&[1, 3], &[5]));
/// ```
pub fn keeps_hidden_selection(previous: &[usize], current: &[usize]) -> bool {
    let added = current.iter().all(|index| previous.contains(index));
    let removed = previous.iter().all(|index| current.contains(index));
    // どちらか一方だけの変化なら、押し足し / 押し外し。
    added || removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_tables_are_not_windowed() {
        let window = row_window(ROW_WINDOW_THRESHOLD, 0.0, 100.0, 20.0, 5);
        assert!(window.is_complete());
        assert_eq!(window.len(), ROW_WINDOW_THRESHOLD);
        assert_eq!(window.leading(20.0), 0.0);
        assert_eq!(window.trailing(20.0), 0.0);
    }

    #[test]
    fn window_follows_the_scroll_position() {
        let window = row_window(10_000, 1_000.0, 200.0, 20.0, 5);
        // 50 行目が上端。その 5 行手前から。
        assert_eq!(window.start, 45);
        // 画面には 10 行入る。端の分と余分を足した先まで。
        assert_eq!(window.end, 50 + 11 + 5);
        assert_eq!(window.count, 10_000);
        assert!(!window.is_complete());
        assert!(window.contains(50));
        assert!(!window.contains(44));
    }

    #[test]
    fn window_stays_inside_the_table() {
        // いちばん上。上へはみ出さない。
        let top = row_window(10_000, 0.0, 200.0, 20.0, 5);
        assert_eq!(top.start, 0);
        assert_eq!(top.leading(20.0), 0.0);
        // いちばん下。下へはみ出さない。
        let bottom = row_window(10_000, 10_000.0 * 20.0, 200.0, 20.0, 5);
        assert_eq!(bottom.end, 10_000);
        assert_eq!(bottom.trailing(20.0), 0.0);
        assert!(bottom.start < bottom.end, "行が 1 つも無い範囲にはしない");
    }

    #[test]
    fn spacers_cover_the_rows_outside_the_window() {
        let window = row_window(10_000, 1_000.0, 200.0, 20.0, 5);
        let total = window.leading(20.0) + window.len() as f64 * 20.0 + window.trailing(20.0);
        assert_eq!(total, 10_000.0 * 20.0, "詰め物と合わせて全行分の高さになる");
    }

    #[test]
    fn unknown_sizes_fall_back_to_the_first_rows() {
        for (viewport, height) in [(0.0, 20.0), (200.0, 0.0), (200.0, f64::NAN), (-1.0, 20.0)] {
            let window = row_window(10_000, 0.0, viewport, height, 5);
            assert_eq!(window.start, 0);
            assert_eq!(window.end, ROW_WINDOW_THRESHOLD);
        }
    }

    /// スクロール位置から「最低限これだけ要る」窓と「作り直すなら」の窓。
    fn windows(count: usize, top: f64) -> (RowWindow, RowWindow) {
        (
            row_window(count, top, 200.0, 20.0, ROW_WINDOW_OVERSCAN / 2),
            row_window(count, top, 200.0, 20.0, ROW_WINDOW_OVERSCAN),
        )
    }

    #[test]
    fn a_window_survives_small_scrolls() {
        let current = row_window(10_000, 1_000.0, 200.0, 20.0, ROW_WINDOW_OVERSCAN);
        // 余りの半分までスクロールしても、作り直さずに済む。
        for rows in 0..=(ROW_WINDOW_OVERSCAN / 2) {
            let (needed, next) = windows(10_000, 1_000.0 + rows as f64 * 20.0);
            assert!(
                keeps_row_window(current, needed, next),
                "{rows} 行ぶんのスクロール"
            );
        }
        // それより動いたら作り直す。
        let (needed, next) = windows(10_000, 1_000.0 + 20.0 * 20.0);
        assert!(!keeps_row_window(current, needed, next));
        // 行数が変わったら、範囲が同じでも作り直す。
        let (needed, next) = windows(20_000, 1_000.0);
        assert!(!keeps_row_window(current, needed, next));
    }

    #[test]
    fn an_oversized_window_is_rebuilt() {
        // 大きさが分かる前に作った窓 (先頭から threshold 行) は、
        // 高さが分かった時点で作り直す。
        let current = row_window(10_000, 0.0, 0.0, 0.0, ROW_WINDOW_OVERSCAN);
        assert_eq!(current.len(), ROW_WINDOW_THRESHOLD);
        let (needed, next) = windows(10_000, 0.0);
        assert!(current.covers(needed), "覆えてはいる");
        assert!(
            !keeps_row_window(current, needed, next),
            "覆えていても、多く作りすぎていれば作り直す"
        );
    }

    #[test]
    fn window_is_never_empty_past_the_end() {
        // 行数が減って、位置が表の外を指しているとき。
        let window = row_window(10_000, 1_000_000.0, 200.0, 20.0, 5);
        assert!(!window.is_empty(), "行が 1 つも無い範囲にはしない");
        assert_eq!(window.end, 10_000);
        assert!(window.contains(9_999));
    }

    #[test]
    fn hidden_selection_survives_adding_and_removing() {
        // 足す / 外すだけなら、画面の外の選択は残す。
        assert!(keeps_hidden_selection(&[1], &[1, 3]));
        assert!(keeps_hidden_selection(&[1, 3], &[1]));
        assert!(keeps_hidden_selection(&[], &[2]), "0 件から足したとき");
        assert!(keeps_hidden_selection(&[2], &[]), "全部外したとき");
        // 入れ替わったら選び直しとみなす。
        assert!(!keeps_hidden_selection(&[1, 3], &[5]));
        assert!(!keeps_hidden_selection(&[1, 3], &[3, 5]));
    }

    #[test]
    fn empty_window_is_reported() {
        let window = RowWindow::all(0);
        assert!(window.is_empty());
        assert!(window.is_complete());
        assert_eq!(window.indices(), 0..0);
    }
}
