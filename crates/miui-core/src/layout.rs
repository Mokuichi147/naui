//! レイアウトの値型。
//!
//! ここにあるのは「どう置きたいか」を表す値だけで、レイアウト計算は行わない。
//! 実際の配置は各バックエンドのネイティブなレイアウト機構
//! (Auto Layout / XAML のレイアウトパス / CSS) が行う。

/// 幅または高さの決め方。
///
/// | 値 | 意味 |
/// | --- | --- |
/// | [`Length::Auto`] | 中身に合わせる (既定) |
/// | [`Length::Fixed`] | 論理ピクセルで固定する |
/// | [`Length::Fill`] | 親の余りを受け取って広がる |
///
/// `Fill` の意味は軸によって変わる。親の並び方向 (主軸) では
/// **余った空間を受け取る**、それと直交する方向 (交差軸) では
/// **親いっぱいに広がる**。
///
/// バックエンドごとの対応は次のとおり。
///
/// | | 主軸の `Fill` | 交差軸の `Fill` |
/// | --- | --- | --- |
/// | macOS (AppKit) | hugging priority を下げて NSStackView から余りを受け取る | 親の幅 / 高さに合わせる制約 |
/// | Web (DOM) | `flex-grow: 1` | `align-self: stretch` |
/// | Windows (WinUI 3) | **Stack では効かない** (StackPanel は余りを配らない)。`Grid` の [`Track::Fill`] を使う | `HorizontalAlignment`/`VerticalAlignment` の `Stretch` |
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    /// 中身に合わせる (既定)。
    #[default]
    Auto,
    /// 論理ピクセルで固定する。
    Fixed(f64),
    /// 親の余りを受け取って広がる。
    Fill,
}

impl Length {
    /// 固定長ならその値。
    pub fn fixed_value(self) -> Option<f64> {
        match self {
            Length::Fixed(v) => Some(v),
            _ => None,
        }
    }

    pub fn is_fill(self) -> bool {
        matches!(self, Length::Fill)
    }
}

/// ウィジェット 1 つの大きさの指定。
///
/// 既定はすべて「中身に合わせる」で、指定した項目だけがネイティブ側に反映される。
///
/// ```
/// # use miui_core::{Length, Sizing};
/// // 幅は親いっぱい、高さは 200px、ただし幅は 120px 以上。
/// let sizing = Sizing::new()
///     .width(Length::Fill)
///     .height(Length::Fixed(200.0))
///     .min_width(120.0);
/// assert_eq!(sizing.min_width, Some(120.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sizing {
    pub width: Length,
    pub height: Length,
    pub min_width: Option<f64>,
    pub min_height: Option<f64>,
    pub max_width: Option<f64>,
    pub max_height: Option<f64>,
}

impl Sizing {
    /// すべて「中身に合わせる」。
    pub const AUTO: Sizing = Sizing::new();

    pub const fn new() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
        }
    }

    /// 幅・高さとも固定する。
    pub const fn fixed(width: f64, height: f64) -> Self {
        Self::new()
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
    }

    /// 幅・高さとも親いっぱいに広げる。
    pub const fn fill() -> Self {
        Self::new().width(Length::Fill).height(Length::Fill)
    }

    /// 幅だけ親いっぱいに広げる。
    pub const fn fill_width() -> Self {
        Self::new().width(Length::Fill)
    }

    /// 高さだけ親いっぱいに広げる。
    pub const fn fill_height() -> Self {
        Self::new().height(Length::Fill)
    }

    pub const fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    pub const fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    pub const fn min_width(mut self, value: f64) -> Self {
        self.min_width = Some(value);
        self
    }

    pub const fn min_height(mut self, value: f64) -> Self {
        self.min_height = Some(value);
        self
    }

    pub const fn max_width(mut self, value: f64) -> Self {
        self.max_width = Some(value);
        self
    }

    pub const fn max_height(mut self, value: f64) -> Self {
        self.max_height = Some(value);
        self
    }

    /// 最小の幅と高さをまとめて指定する。
    pub const fn min_size(self, width: f64, height: f64) -> Self {
        self.min_width(width).min_height(height)
    }

    /// 最大の幅と高さをまとめて指定する。
    pub const fn max_size(self, width: f64, height: f64) -> Self {
        self.max_width(width).max_height(height)
    }
}

/// グリッドの 1 列 (または 1 行) の幅の決め方。
///
/// | 値 | Web | Windows | macOS |
/// | --- | --- | --- | --- |
/// | [`Track::Auto`] | `auto` | `GridLength` の `Auto` | NSGridView の既定 (中身に合わせる) |
/// | [`Track::Fixed`] | `<n>px` | `GridLength` の `Pixel` | `NSGridColumn::setWidth` |
/// | [`Track::Fill`] | `<w>fr` | `GridLength` の `Star` | 近似 (`Fill` 配置 + hugging priority) |
///
/// `Fill` の重みは「余った空間を何対何で分けるか」を表す。macOS の NSGridView には
/// 重みの概念が無いため、重みの違いは反映されない。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Track {
    /// 中身に合わせる (既定)。
    #[default]
    Auto,
    /// 論理ピクセルで固定する。
    Fixed(f64),
    /// 余った空間を重みの比で分け合う。
    Fill(f64),
}

impl Track {
    /// 重み 1 で余りを受け取る。
    pub const FILL: Track = Track::Fill(1.0);

    /// 余りを分けるときの重み。`Fill` 以外は 0。
    ///
    /// 0 以下の重みは 1 として扱う (`0fr` は何も受け取らないため、
    /// 「広がってほしい」という指定の意図から外れる)。
    pub fn weight(self) -> f64 {
        match self {
            Track::Fill(w) if w > 0.0 => w,
            Track::Fill(_) => 1.0,
            _ => 0.0,
        }
    }

    pub fn is_fill(self) -> bool {
        matches!(self, Track::Fill(_))
    }
}

/// グリッド上の置き場所。左上が `(0, 0)`。
///
/// マスの中では縦が中央ぞろえになる ([`Length::Fill`] を指定した子だけ
/// マスいっぱいに広がる)。高さの違うものを同じ行に並べても上端でずれない。
///
/// ```
/// # use miui_core::GridCell;
/// // 2 列目・1 行目に置き、横 2 マス分を占める。
/// let cell = GridCell::new(1, 0).span(2, 1);
/// assert_eq!(cell.column_span, 2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridCell {
    /// 左から数えた列 (0 始まり)。
    pub column: usize,
    /// 上から数えた行 (0 始まり)。
    pub row: usize,
    /// 横に占めるマス数 (1 以上)。
    pub column_span: usize,
    /// 縦に占めるマス数 (1 以上)。
    pub row_span: usize,
}

impl GridCell {
    pub const fn new(column: usize, row: usize) -> Self {
        Self {
            column,
            row,
            column_span: 1,
            row_span: 1,
        }
    }

    /// 占めるマス数を指定する。0 は 1 として扱う。
    pub const fn span(mut self, column_span: usize, row_span: usize) -> Self {
        self.column_span = if column_span == 0 { 1 } else { column_span };
        self.row_span = if row_span == 0 { 1 } else { row_span };
        self
    }

    /// この配置が必要とする列数。
    pub const fn columns_needed(&self) -> usize {
        self.column + self.column_span
    }

    /// この配置が必要とする行数。
    pub const fn rows_needed(&self) -> usize {
        self.row + self.row_span
    }
}

impl Default for GridCell {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

/// スクロールバーの出し方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollPolicy {
    /// はみ出したときだけスクロールできる (既定)。
    #[default]
    Auto,
    /// 常にスクロールできる (スクロールバーを出す)。
    Always,
    /// スクロールさせない。
    Never,
}

impl ScrollPolicy {
    pub fn is_enabled(self) -> bool {
        !matches!(self, ScrollPolicy::Never)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizing_defaults_to_auto() {
        let s = Sizing::new();
        assert_eq!(s.width, Length::Auto);
        assert_eq!(s.height, Length::Auto);
        assert_eq!(s.min_width, None);
        assert_eq!(s, Sizing::default());
        assert_eq!(s, Sizing::AUTO);
    }

    #[test]
    fn sizing_builders_keep_other_fields() {
        let s = Sizing::fixed(100.0, 40.0).min_width(10.0).max_size(200.0, 80.0);
        assert_eq!(s.width, Length::Fixed(100.0));
        assert_eq!(s.height, Length::Fixed(40.0));
        assert_eq!(s.min_width, Some(10.0));
        assert_eq!(s.max_width, Some(200.0));
        assert_eq!(s.max_height, Some(80.0));
        assert_eq!(s.min_height, None);
    }

    #[test]
    fn fill_helpers_touch_one_axis_only() {
        assert!(Sizing::fill_width().width.is_fill());
        assert!(!Sizing::fill_width().height.is_fill());
        assert!(Sizing::fill().height.is_fill());
        assert_eq!(Length::Fixed(12.0).fixed_value(), Some(12.0));
        assert_eq!(Length::Fill.fixed_value(), None);
    }

    #[test]
    fn track_weight_falls_back_to_one() {
        assert_eq!(Track::FILL.weight(), 1.0);
        assert_eq!(Track::Fill(2.5).weight(), 2.5);
        // 0 以下は「広がらない」ではなく重み 1 として扱う。
        assert_eq!(Track::Fill(0.0).weight(), 1.0);
        assert_eq!(Track::Fill(-3.0).weight(), 1.0);
        assert_eq!(Track::Auto.weight(), 0.0);
        assert!(!Track::Fixed(10.0).is_fill());
    }

    #[test]
    fn grid_cell_span_is_at_least_one() {
        let cell = GridCell::new(2, 3);
        assert_eq!((cell.column_span, cell.row_span), (1, 1));
        let spanned = cell.span(0, 2);
        assert_eq!(spanned.column_span, 1);
        assert_eq!(spanned.row_span, 2);
        assert_eq!(spanned.columns_needed(), 3);
        assert_eq!(spanned.rows_needed(), 5);
    }

    #[test]
    fn scroll_policy_defaults_to_auto() {
        assert_eq!(ScrollPolicy::default(), ScrollPolicy::Auto);
        assert!(ScrollPolicy::Always.is_enabled());
        assert!(!ScrollPolicy::Never.is_enabled());
    }
}
