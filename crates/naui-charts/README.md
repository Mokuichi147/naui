# naui-charts

Line, bar and scatter charts for [naui](https://crates.io/crates/naui), drawn on
its `Canvas` with each platform's own 2D API (Core Graphics on macOS, cairo on
Linux, Direct2D on Windows, the 2D context of `<canvas>` on the Web).

```rust,no_run
use naui::Settings;
use naui_charts::{Axis, Chart, ChartView, Series};

fn main() -> naui::Result<()> {
    naui::run(Settings::new("sales"), |ui| {
        let window = ui.window("Sales", 640.0, 400.0)?;
        let chart = Chart::new()
            .categories(["Apr", "May", "Jun", "Jul"])
            .y_axis(Axis::new().title("Units"))
            .series(Series::bar("2025", [120.0, 90.0, 140.0, 160.0]))
            .series(Series::line("2026", [130.0, 110.0, 150.0, 175.0]));
        let view = ChartView::new(ui, chart)?;
        window.set_child(view.root());
        window.show();
        Ok(())
    })
}
```

- **Line, bar and scatter series**, mixed in one chart. The x axis is either
  named categories or numeric. Several bar series stand side by side.
- **Axes that read well**: ticks at steps of 1, 2, 2.5 or 5 times a power of ten,
  as many as fit, with labels measured by the platform so that they do not
  overlap. Axis titles, a fixed range and your own label format are optional.
- **Light and dark**: axes, labels and grid lines take the platform's text colors,
  and series a fixed palette with its own colors for each appearance (neighbors
  stay apart for the common kinds of color blindness). The chart draws itself
  again when the appearance switches.
- **Many points**: a line keeps its first, last, highest and lowest point in each
  pixel column, and crowded scatter dots are thinned, so 100,000 points draw as
  one shape per series.

`Chart::draw` draws a chart on any `Painter`, to put it in part of a drawing of
your own or to check the drawing commands in a unit test.

naui's containers take only naui's own widgets, so a `ChartView` is put in one
through the `Canvas` inside it (`view.root()`).

Tooltips, zooming and other interaction are not part of it yet. `ChartLayout`
(from `ChartView::layout`) turns a pointer position on the `Canvas` into values,
for building them on `Canvas::on_pointer`.

See [`examples/charts`](https://github.com/mokuichi147/naui/tree/main/examples/charts)
for a window with all three kinds and a line of 100,000 points.
