//! naui-charts の例。棒と折れ線を重ねたグラフ、散布図、10 万点の折れ線を
//! 1 つの窓に並べる。
//!
//! グラフは naui の `Canvas` の上に各環境の 2D API で描かれる。軸や文字は
//! 環境の文字色で描くので、「暗い外観」を切り替えると描き直されて読める
//! ままになる。

use std::cell::Cell;
use std::rc::Rc;

use naui::{GridCell, Orientation, Padding, Result, Settings, TextStyle, Theme, Track, Ui};
use naui_charts::{Axis, Chart, ChartView, Series};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const LAST_YEAR: [f64; 12] = [
    82.0, 90.0, 104.0, 118.0, 126.0, 140.0, 152.0, 149.0, 131.0, 117.0, 98.0, 110.0,
];
const THIS_YEAR: [f64; 6] = [95.0, 101.0, 121.0, 133.0, 150.0, 161.0];
/// 10 万点の折れ線の点の数。
const SIGNAL_POINTS: usize = 100_000;

fn build(ui: &Ui) -> Result<()> {
    let window = ui.window("naui charts", 960.0, 680.0)?;

    // 上の行: 明暗の切り替えと、今年の月を 1 つ足すボタン。
    let controls = ui.stack(Orientation::Horizontal)?;
    controls.set_spacing(12.0);
    let dark = ui.toggle("Dark appearance")?;
    let next_month = ui.button("Add a month")?;
    controls.append(&dark);
    controls.append(&next_month);

    // 2 列の格子。操作の行の下に、棒と折れ線を重ねたグラフと散布図、その下に
    // 10 万点の折れ線 (画素の列ごとに間引いて描く) を、それぞれ見出しの下に置く。
    // グラフの行が余った高さを 3 : 2 で分ける。
    let grid = ui.grid()?;
    grid.set_spacing(16.0, 6.0);
    grid.set_padding(Padding::all(16.0));
    grid.set_column_track(0, Track::FILL);
    grid.set_column_track(1, Track::FILL);
    grid.set_row_track(2, Track::Fill(3.0));
    grid.set_row_track(4, Track::Fill(2.0));
    let monthly = ChartView::new(ui, monthly_chart(THIS_YEAR.len()))?;
    let scatter = ChartView::new(ui, scatter_chart())?;
    let signal = ChartView::new(ui, signal_chart())?;
    grid.attach(&controls, GridCell::new(0, 0).span(2, 1));
    grid.attach(&heading(ui, "Visits by month")?, GridCell::new(0, 1));
    grid.attach(&monthly, GridCell::new(0, 2));
    grid.attach(&heading(ui, "Height and weight")?, GridCell::new(1, 1));
    grid.attach(&scatter, GridCell::new(1, 2));
    grid.attach(
        &heading(ui, "100,000 points")?,
        GridCell::new(0, 3).span(2, 1),
    );
    grid.attach(&signal, GridCell::new(0, 4).span(2, 1));

    dark.on_toggle({
        let ui = ui.clone();
        move |on| {
            // グラフは外観が変わると自分で描き直すので、ここで何もしなくてよい。
            let _ = ui.set_theme(if on { Theme::Dark } else { Theme::System });
        }
    });

    let months = Rc::new(Cell::new(THIS_YEAR.len()));
    next_month.on_click({
        let monthly = monthly.clone();
        let next_month = next_month.clone();
        move || {
            let shown = (months.get() + 1).min(MONTHS.len());
            months.set(shown);
            // 系列を差し替えると、ChartView が描き直す。
            monthly.update(|chart| chart.series_mut()[1] = this_year(shown));
            next_month.set_enabled(shown < MONTHS.len());
        }
    });

    window.set_child(&grid);
    window.show();
    Ok(())
}

/// グラフの上の見出し。
fn heading(ui: &Ui, title: &str) -> Result<naui::Label> {
    let label = ui.label(title)?;
    label.set_style(TextStyle::Subtitle);
    Ok(label)
}

fn monthly_chart(months: usize) -> Chart {
    Chart::new()
        .categories(MONTHS)
        .y_axis(Axis::new().title("Visits (thousands)"))
        .series(Series::bar("Last year", LAST_YEAR))
        .series(this_year(months))
}

/// 今年の値を `months` 月分。まだ無い月は、去年の値から伸ばした見込み。
fn this_year(months: usize) -> Series {
    let values = (0..months).map(|i| {
        THIS_YEAR
            .get(i)
            .copied()
            .unwrap_or_else(|| LAST_YEAR[i] * 1.12)
    });
    Series::line("This year", values)
}

fn scatter_chart() -> Chart {
    let mut random = Random(7);
    let mut group = |height: f64, weight: f64| -> Vec<(f64, f64)> {
        (0..120)
            .map(|_| {
                let h = height + random.normal() * 6.0;
                (h, weight + (h - height) * 0.9 + random.normal() * 5.0)
            })
            .collect()
    };
    let adults = group(171.0, 66.0);
    let teens = group(156.0, 48.0);
    Chart::new()
        .x_axis(Axis::new().title("Height (cm)"))
        .y_axis(Axis::new().title("Weight (kg)"))
        .series(Series::scatter("Adults", adults))
        .series(Series::scatter("Teens", teens))
}

fn signal_chart() -> Chart {
    let mut random = Random(42);
    let points = (0..SIGNAL_POINTS).map(|i| {
        let t = i as f64 / 1000.0;
        let value = (t * 0.9).sin() * 40.0 + (t * 7.3).sin() * 8.0 + random.normal() * 3.0;
        (t, value)
    });
    Chart::new()
        .x_axis(Axis::new().title("Time (s)").grid(true))
        .series(Series::line_xy("Signal", points.collect::<Vec<_>>()))
}

/// 例のための小さな乱数 (xorshift)。毎回同じ並びになる。
struct Random(u64);

impl Random {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    /// 平均 0、標準偏差 1 の正規分布 (12 個の和で近似)。
    fn normal(&mut self) -> f64 {
        (0..12).map(|_| self.next()).sum::<f64>() - 6.0
    }
}

naui::entry!(Settings::new("naui charts"), build);
