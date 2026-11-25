//! Charts drawn on each platform's real `Canvas`: the platform measures the text
//! and resolves the colors of the roles, and the surface draws again when the
//! chart changes or the appearance switches.
//!
//! ```sh
//! cargo test -p naui-charts --test ui                                  # macOS, Linux, Windows
//! cargo test -p naui-charts --test ui --target wasm32-unknown-unknown  # the Web, in a browser
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui::{testing, AsWidget, Color, DrawCommand, Paint, Result, Sizing, Theme, Ui};
use naui_charts::{series_color, Axis, Chart, ChartView, Series};

naui::test_cases!(
    chart_view_draws_and_redraws,
    a_dropped_view_keeps_drawing,
    chart_follows_light_and_dark
);

fn sales() -> Chart {
    Chart::new()
        .categories(["Apr", "May", "Jun", "Jul"])
        .y_axis(Axis::new().title("Units"))
        .series(Series::bar("2025", [120.0, 90.0, 140.0, 160.0]))
        .series(Series::line("2026", [130.0, 110.0, 150.0, 175.0]))
}

/// A `ChartView` is drawn once it is shown, inside its `Canvas`, and again after
/// `update`.
async fn chart_view_draws_and_redraws(ui: Ui) -> Result<()> {
    let window = ui.window("chart", 480.0, 320.0)?;
    let view = ChartView::new(&ui, sales())?;
    window.set_child(&view);
    window.show();
    testing::wait_until(&ui, || view.layout().is_some()).await?;
    testing::wait_until(&ui, || view.canvas().size().0 > 0.0).await?;
    let (width, height) = view.canvas().size();
    let layout = view.layout().unwrap();
    let plot = layout.plot();
    assert!(
        plot.x > 0.0 && plot.right() <= width,
        "{plot:?} in {width} x {height}"
    );
    assert!(
        plot.y > 0.0 && plot.bottom() <= height,
        "{plot:?} in {width} x {height}"
    );
    assert_eq!(layout.x_range(), (-0.5, 3.5));

    view.update(|chart| {
        chart
            .series_mut()
            .push(Series::scatter("Peak", [(2.0, 1000.0)]));
    });
    testing::wait_until(&ui, || {
        view.layout()
            .is_some_and(|layout| layout.y_range().1 >= 1000.0)
    })
    .await?;

    // 大きさが変わっても描き直す。大きさは AsWidget を通して Canvas へ届く。
    view.set_sizing(Sizing::fixed(300.0, 200.0));
    testing::wait_until(&ui, || {
        view.layout()
            .is_some_and(|layout| layout.plot().right() <= 300.0)
    })
    .await?;
    testing::wait_until(&ui, || view.canvas().size() == (300.0, 200.0)).await?;
    view.set_visible(false);
    assert!(!view.canvas().is_visible());
    assert!(!view.is_visible());
    window.close();
    Ok(())
}

/// A `ChartView` let go of once its `Canvas` is in the window still draws: the
/// `Canvas` keeps what it draws.
async fn a_dropped_view_keeps_drawing(ui: Ui) -> Result<()> {
    let window = ui.window("chart dropped", 480.0, 320.0)?;
    // y の目盛りを書くたびに数える (描かれたことの印)。
    let labels = Rc::new(Cell::new(0));
    let chart = sales().y_axis(Axis::new().format({
        let labels = labels.clone();
        move |value| {
            labels.set(labels.get() + 1);
            format!("{value}")
        }
    }));
    let view = ChartView::new(&ui, chart)?;
    window.set_child(&view);
    drop(view);
    window.show();
    testing::wait_until(&ui, || labels.get() > 0).await?;
    window.close();
    Ok(())
}

/// What one draw on the real surface used.
#[derive(Debug, Clone, PartialEq)]
struct Drawn {
    dark: bool,
    /// The color of the legend's text (`TextColor::Default`).
    legend_text: Option<Color>,
    fills: Vec<Color>,
    widest_label: f64,
}

/// The chart takes the platform's text colors for the appearance it is shown in,
/// and the palette for light or dark, and draws again on a switch.
async fn chart_follows_light_and_dark(ui: Ui) -> Result<()> {
    let window = ui.window("chart theme", 480.0, 320.0)?;
    let canvas = ui.canvas()?;
    canvas.set_sizing(Sizing::fill());
    let drawn: Rc<RefCell<Option<Drawn>>> = Rc::default();
    canvas.on_draw({
        let drawn = drawn.clone();
        move |painter| {
            sales().draw(painter);
            let mut legend_text = None;
            let mut fills = Vec::new();
            for command in painter.commands() {
                match command {
                    DrawCommand::Text { text, color, .. } if text == "2025" => {
                        legend_text = Some(*color)
                    }
                    DrawCommand::Fill {
                        paint: Paint::Color(color),
                        ..
                    } => fills.push(*color),
                    _ => {}
                }
            }
            *drawn.borrow_mut() = Some(Drawn {
                dark: painter.is_dark(),
                legend_text,
                fills,
                widest_label: painter.text_width("160", 11.0),
            });
        }
    });
    window.set_child(&canvas);
    window.show();

    let drawn_in = |dark: bool| {
        let drawn = drawn.clone();
        move || drawn.borrow().as_ref().is_some_and(|d| d.dark == dark)
    };
    ui.set_theme(Theme::Light)?;
    testing::wait_until(&ui, drawn_in(false)).await?;
    let light = drawn.borrow().clone().unwrap();
    // 文字幅は環境のフォントで測っている (固定の推定は 1 文字に大きさの半分)。
    assert!(light.widest_label > 0.0);
    assert!(light.fills.contains(&series_color(0, false)), "{light:?}");

    ui.set_theme(Theme::Dark)?;
    let switched = testing::wait_until(&ui, drawn_in(true)).await;
    ui.set_theme(Theme::System)?;
    switched?;
    let dark = drawn.borrow().clone().unwrap();
    assert!(dark.fills.contains(&series_color(0, true)), "{dark:?}");
    assert!(!dark.fills.contains(&series_color(0, false)), "{dark:?}");
    // 文字色は環境が外観に合わせて解決する: 明るい外観では暗く、暗い外観では明るい。
    // 副次の色 (Web の GrayText など) は中ほどの灰色のことがあるので、本文の色で見る。
    let luminance = |c: Color| 0.2126 * c.r as f64 + 0.7152 * c.g as f64 + 0.0722 * c.b as f64;
    assert!(
        light.legend_text.is_some_and(|c| luminance(c) < 128.0),
        "{light:?}"
    );
    assert!(
        dark.legend_text.is_some_and(|c| luminance(c) > 128.0),
        "{dark:?}"
    );
    window.close();
    Ok(())
}
