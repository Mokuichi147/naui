//! Line, bar and scatter charts for [naui](https://crates.io/crates/naui), drawn on
//! its [`Canvas`] with each platform's own 2D API (Core Graphics, cairo, Direct2D,
//! the 2D context of `<canvas>`).
//!
//! A [`Chart`] is a value: its [`Series`], its [`Axis`] settings and its
//! [`Legend`]. [`ChartView`] shows one in a `Canvas` and draws it again when it
//! changes, when the surface is resized and when the appearance switches between
//! light and dark. [`Chart::draw`] draws one on any [`Painter`], to put a chart in
//! part of a drawing of your own or to look at the drawing commands in a test.
//!
//! ```no_run
//! use naui::Settings;
//! use naui_charts::{Axis, Chart, ChartView, Series};
//!
//! fn main() -> naui::Result<()> {
//!     naui::run(Settings::new("sales"), |ui| {
//!         let window = ui.window("Sales", 640.0, 400.0)?;
//!         let chart = Chart::new()
//!             .categories(["Apr", "May", "Jun", "Jul"])
//!             .y_axis(Axis::new().title("Units"))
//!             .series(Series::bar("2025", [120.0, 90.0, 140.0, 160.0]))
//!             .series(Series::line("2026", [130.0, 110.0, 150.0, 175.0]));
//!         let view = ChartView::new(ui, chart)?;
//!         window.set_child(&view);
//!         window.show();
//!         Ok(())
//!     })
//! }
//! ```
//!
//! # Colors
//!
//! Axes, tick labels, grid lines and the legend's text are drawn in the platform's
//! text colors ([`TextColor::Secondary`](naui::TextColor::Secondary) and
//! [`TextColor::Default`](naui::TextColor::Default)), which `Canvas` resolves for
//! the appearance it is shown in. Series take a fixed palette
//! ([`series_color`]) with its own colors for light and for dark, chosen so that
//! neighbors stay apart for the common kinds of color blindness. A series can have
//! a color of its own, including a role such as `TextColor::Accent`
//! ([`Series::color`]).
//!
//! # Many points
//!
//! A line with more points than about four per pixel of its width keeps, in each
//! pixel column, its first, last, highest and lowest point, so it looks the same
//! with far fewer. A scatter series with more than 2,000 points leaves out a dot
//! that would land on one already drawn. The bars, the line and the dots of a
//! series are each one shape filled or stroked once, not one command per point.

mod chart;
mod palette;
mod thin;
mod ticks;

#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;

use naui::{AsWidget, Canvas, Painter, Result, Sizing, Ui, Widget};

pub use chart::{Axis, Chart, ChartLayout, Legend, Series, SeriesKind};
pub use palette::{series_color, PALETTE_LEN};
pub use ticks::Ticks;

/// A [`Chart`] shown in a [`Canvas`].
///
/// It draws the chart again when it is changed ([`set_chart`](Self::set_chart),
/// [`update`](Self::update)), when the surface changes size and when the appearance
/// switches between light and dark.
///
/// It implements [`AsWidget`], so it goes into a container as it is
/// (`stack.append(&view)`), and the common methods ([`set_sizing`](AsWidget::set_sizing),
/// [`set_visible`](AsWidget::set_visible) and the others) reach the `Canvas` inside
/// it. A `Canvas` has no size of its own; the view starts filling its parent and at
/// least 160 by 120 pixels.
///
/// ```no_run
/// # use naui::{AsWidget, Orientation, Result, Sizing, Ui};
/// # use naui_charts::{Chart, ChartView, Series};
/// # fn build(ui: &Ui) -> Result<()> {
/// let stack = ui.stack(Orientation::Vertical)?;
/// let view = ChartView::new(ui, Chart::new().series(Series::line("Load", [0.2, 0.5, 0.4])))?;
/// view.set_sizing(Sizing::fill_width().height(naui::Length::Fixed(240.0)));
/// stack.append(&view);
/// // Later, add the latest value; the chart draws itself again.
/// view.update(|chart| chart.series_mut()[0].points_mut().push((3.0, 0.7)));
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct ChartView {
    canvas: Canvas,
    state: Rc<RefCell<State>>,
}

struct State {
    /// Shared with a draw in progress, so that drawing does not hold the borrow.
    chart: Rc<Chart>,
    layout: Option<ChartLayout>,
}

impl ChartView {
    /// A `Canvas` showing `chart`.
    pub fn new(ui: &Ui, chart: Chart) -> Result<ChartView> {
        let canvas = ui.canvas()?;
        canvas.set_sizing(Sizing::fill().min_width(160.0).min_height(120.0));
        let state = Rc::new(RefCell::new(State {
            chart: Rc::new(chart),
            layout: None,
        }));
        // 描画の閉包が状態を強く持つ。ChartView を手放しても、Canvas が窓にある
        // 限り描き続けるため (状態は Canvas を持たないので循環しない)。
        canvas.on_draw({
            let state = state.clone();
            move |painter: &mut Painter| {
                // 描いている間は借用を持たない (書式の関数から update されてもよいように)。
                let chart = state.borrow().chart.clone();
                let layout = chart.draw(painter);
                state.borrow_mut().layout = Some(layout);
            }
        });
        canvas.redraw();
        Ok(ChartView { canvas, state })
    }

    /// The `Canvas` the chart is drawn on: listen to the pointer on it
    /// ([`Canvas::on_pointer`], with [`layout`](Self::layout) to turn positions into
    /// values) or read its size.
    ///
    /// To put the chart in a container or size it, use the view itself: it
    /// implements [`AsWidget`].
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// The chart shown.
    pub fn chart(&self) -> Chart {
        (*self.state.borrow().chart).clone()
    }

    /// Shows `chart` instead.
    pub fn set_chart(&self, chart: Chart) {
        self.state.borrow_mut().chart = Rc::new(chart);
        self.canvas.redraw();
    }

    /// Changes the chart shown in place and draws it again.
    pub fn update(&self, f: impl FnOnce(&mut Chart)) {
        {
            let mut state = self.state.borrow_mut();
            f(Rc::make_mut(&mut state.chart));
        }
        self.canvas.redraw();
    }

    /// Where the chart was drawn the last time, to turn a pointer position on the
    /// `Canvas` into values ([`ChartLayout::to_value`]). `None` until it has been
    /// drawn.
    pub fn layout(&self) -> Option<ChartLayout> {
        self.state.borrow().layout
    }
}

impl AsWidget for ChartView {
    /// The `Canvas` the chart is drawn on.
    fn as_widget(&self) -> &dyn Widget {
        &self.canvas
    }
}
