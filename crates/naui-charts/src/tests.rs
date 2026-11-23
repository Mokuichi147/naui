//! 描画の命令を見るテスト。`Painter::new` は役割を明るい固定の色で解決し、
//! `with_role_colors` で暗い外観を真似られる。

use std::time::Instant;

use naui::{Color, DrawCommand, Paint, Painter, PathSegment, Point, Rect, TextColor};

use crate::{series_color, Axis, Chart, Legend, Series, SeriesKind};

/// 役割ごとに見分けのつく色を返す、暗い外観の描き手。
fn dark_painter(width: f64, height: f64) -> Painter {
    Painter::new(width, height).with_role_colors(true, |role| match role {
        TextColor::Default => (Color::rgb(0xf0, 0xf0, 0xf0), 1.0),
        TextColor::Secondary => (Color::rgb(0xa0, 0xa0, 0xa0), 0.8),
        _ => (Color::rgb(0x40, 0x80, 0xff), 1.0),
    })
}

fn sales() -> Chart {
    Chart::new()
        .categories(["Apr", "May", "Jun", "Jul"])
        .y_axis(Axis::new().title("Units"))
        .x_axis(Axis::new().title("Month"))
        .series(Series::bar("2025", [120.0, 90.0, 140.0, 160.0]))
        .series(Series::line("2026", [130.0, 110.0, 150.0, 175.0]))
        .series(Series::scatter("Targets", [(0.0, 125.0), (3.0, 180.0)]))
}

fn texts(painter: &Painter) -> Vec<(&str, Point, Color)> {
    painter
        .commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Text {
                text, at, color, ..
            } => Some((text.as_str(), *at, *color)),
            _ => None,
        })
        .collect()
}

fn fills(painter: &Painter) -> Vec<(&naui::Path, Color)> {
    painter
        .commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Fill {
                path,
                paint: Paint::Color(color),
                ..
            } => Some((path, *color)),
            _ => None,
        })
        .collect()
}

fn strokes(painter: &Painter) -> Vec<(&naui::Path, Color, f64)> {
    painter
        .commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Stroke {
                path, color, width, ..
            } => Some((path, *color, *width)),
            _ => None,
        })
        .collect()
}

fn points_of(path: &naui::Path) -> Vec<Point> {
    path.segments()
        .iter()
        .flat_map(|segment| match segment {
            PathSegment::MoveTo(p) | PathSegment::LineTo(p) => vec![*p],
            PathSegment::CubicTo {
                control1,
                control2,
                to,
            } => vec![*control1, *control2, *to],
            PathSegment::Close => vec![],
        })
        .collect()
}

#[test]
fn text_takes_the_role_colors_and_series_the_palette() {
    let mut painter = dark_painter(480.0, 320.0);
    sales().draw(&mut painter);
    let default = Color::rgb(0xf0, 0xf0, 0xf0);
    let secondary = Color::rgb(0xa0, 0xa0, 0xa0);
    // 文字は役割の色だけで描き、系列の色では描かない。
    let texts = texts(&painter);
    assert!(texts
        .iter()
        .all(|(_, _, c)| *c == default || *c == secondary));
    for (label, role) in [
        ("Apr", secondary),
        ("Units", default),
        ("Month", default),
        ("2025", default),
    ] {
        assert!(
            texts.iter().any(|(t, _, c)| *t == label && *c == role),
            "{label}"
        );
    }
    // 軸と格子の線は副次の文字色。
    let lines: Vec<_> = strokes(&painter)
        .into_iter()
        .filter(|(_, _, width)| *width == 1.0)
        .collect();
    assert!(!lines.is_empty());
    assert!(lines.iter().all(|(_, c, _)| *c == secondary));
    // 系列は暗い外観のパレット。
    let colors: Vec<_> = fills(&painter).into_iter().map(|(_, c)| c).collect();
    assert!(colors.contains(&series_color(0, true)), "棒");
    assert!(colors.contains(&series_color(2, true)), "点");
    assert!(strokes(&painter)
        .iter()
        .any(|(_, c, w)| *c == series_color(1, true) && *w == 2.0));
    assert!(!colors.contains(&series_color(0, false)));
}

#[test]
fn light_and_dark_differ_only_in_color() {
    let mut light = Painter::new(480.0, 320.0);
    let mut dark = dark_painter(480.0, 320.0);
    let light_layout = sales().draw(&mut light);
    let dark_layout = sales().draw(&mut dark);
    assert_eq!(light_layout, dark_layout);
    assert_eq!(light.commands().len(), dark.commands().len());
    assert!(fills(&light)
        .iter()
        .any(|(_, c)| *c == series_color(0, false)));
}

#[test]
fn a_custom_color_may_be_a_role() {
    let chart = Chart::new().series(Series::line("Load", [1.0, 2.0]).color(TextColor::Accent));
    let mut painter = dark_painter(300.0, 200.0);
    chart.draw(&mut painter);
    assert!(strokes(&painter)
        .iter()
        .any(|(_, c, w)| *c == Color::rgb(0x40, 0x80, 0xff) && *w == 2.0));
}

#[test]
fn series_are_clipped_to_the_plot_and_state_is_restored() {
    let mut painter = Painter::new(400.0, 300.0);
    painter.set_opacity(0.7);
    let layout = sales().draw(&mut painter);
    assert_eq!(painter.opacity(), 0.7);
    let commands = painter.commands();
    let clip = commands
        .iter()
        .position(|c| matches!(c, DrawCommand::Clip(_)))
        .expect("切り抜く");
    let first_series = commands
        .iter()
        .position(|c| matches!(c, DrawCommand::Fill { .. }))
        .unwrap();
    assert!(clip < first_series);
    let DrawCommand::Clip(path) = &commands[clip] else {
        unreachable!()
    };
    let bounds = path.bounds().unwrap();
    let plot = layout.plot();
    assert!(bounds.x <= plot.x && bounds.right() >= plot.right());
    assert!(
        bounds.x > plot.x - 10.0,
        "印の分だけ広げる: {bounds:?} {plot:?}"
    );
    let saves = commands.iter().filter(|c| **c == DrawCommand::Save).count();
    let restores = commands
        .iter()
        .filter(|c| **c == DrawCommand::Restore)
        .count();
    assert_eq!(saves, restores);
}

#[test]
fn the_plot_leaves_room_for_labels() {
    let mut painter = Painter::new(480.0, 320.0);
    let layout = sales().draw(&mut painter);
    let plot = layout.plot();
    let bounds = painter.bounds();
    assert!(plot.x > 20.0 && plot.right() < bounds.width);
    assert!(plot.y > 20.0, "凡例の下: {plot:?}");
    assert!(
        plot.bottom() < bounds.height - 30.0,
        "x のラベルと見出しの上: {plot:?}"
    );
    // y のラベルは右端を描画域の左に揃え、x のラベルは描画域の下に置く。
    for (text, at, _) in texts(&painter) {
        // 凡例の "2025" は目盛りではない。
        let tick = text.parse::<f64>().ok().filter(|_| at.y > plot.y - 10.0);
        if let Some(value) = tick {
            assert!(at.x < plot.x, "{text}");
            let y = layout.to_point(0.0, value).y;
            assert!((at.y - y).abs() < 11.0, "{text} は目盛りの高さ");
        }
        if ["Apr", "May", "Jun", "Jul"].contains(&text) {
            assert!(at.y > plot.bottom());
        }
    }
    // y の範囲は 0 から、データを覆う刻みまで。
    let (low, high) = layout.y_range();
    assert_eq!(low, 0.0);
    assert!(high >= 180.0);
}

#[test]
fn layout_maps_values_and_back() {
    let mut painter = Painter::new(400.0, 300.0);
    let layout = sales().draw(&mut painter);
    let plot = layout.plot();
    // カテゴリの帯は -0.5 から始まる。
    assert_eq!(layout.x_range(), (-0.5, 3.5));
    assert_eq!(
        layout.to_point(-0.5, layout.y_range().0),
        Point::new(plot.x, plot.bottom())
    );
    let point = layout.to_point(2.0, 100.0);
    let (x, y) = layout.to_value(point);
    assert!((x - 2.0).abs() < 1e-9 && (y - 100.0).abs() < 1e-9);
}

#[test]
fn bars_stand_on_zero_and_side_by_side() {
    let chart = Chart::new()
        .categories(["a", "b"])
        .series(Series::bar("up", [10.0, 20.0]))
        .series(Series::bar("down", [-5.0, 15.0]));
    let mut painter = Painter::new(400.0, 300.0);
    let layout = chart.draw(&mut painter);
    let zero = layout.to_point(0.0, 0.0).y;
    let (low, high) = layout.y_range();
    assert!(low <= -5.0 && high >= 20.0);
    let bars = fills(&painter);
    let up = bars
        .iter()
        .find(|(_, c)| *c == series_color(0, false))
        .unwrap();
    let down = bars
        .iter()
        .find(|(_, c)| *c == series_color(1, false))
        .unwrap();
    let up = up.0.bounds().unwrap();
    let down_points = points_of(down.0);
    // 上へ伸びる棒は基線より上、負の値の棒は基線より下へ伸びる。
    assert!((up.bottom() - zero).abs() < 1e-9);
    assert!(down_points.iter().any(|p| p.y > zero + 1.0));
    // 同じカテゴリの 2 本は重ならない。
    let first_down = down.0.bounds().unwrap();
    assert!(up.x < first_down.x);
    // 0 の線を引く。
    assert!(strokes(&painter).iter().any(|(path, _, _)| {
        let points = points_of(path);
        points.len() == 2
            && points
                .iter()
                .all(|p| (p.y - (zero.floor() + 0.5)).abs() < 1e-9)
    }));
}

#[test]
fn numeric_bars_are_as_wide_as_the_closest_pair() {
    let chart = Chart::new().series(Series::bar_xy("h", [(0.0, 1.0), (1.0, 2.0), (3.0, 1.0)]));
    let mut painter = Painter::new(400.0, 300.0);
    let layout = chart.draw(&mut painter);
    // 両端に帯の半分ずつ空ける。
    assert_eq!(layout.x_range(), (-0.5, 3.5));
}

#[test]
fn the_legend_shows_for_two_series_or_more() {
    let one = Chart::new().series(Series::line("Only", [1.0, 2.0]));
    let mut painter = Painter::new(400.0, 300.0);
    one.draw(&mut painter);
    assert!(!texts(&painter).iter().any(|(t, _, _)| *t == "Only"));

    let mut painter = Painter::new(400.0, 300.0);
    one.clone().legend(Legend::Shown).draw(&mut painter);
    assert!(texts(&painter).iter().any(|(t, _, _)| *t == "Only"));

    let mut painter = Painter::new(400.0, 300.0);
    sales().legend(Legend::Hidden).draw(&mut painter);
    assert!(!texts(&painter).iter().any(|(t, _, _)| *t == "2025"));
}

#[test]
fn a_narrow_legend_wraps() {
    let chart = Chart::new()
        .series(Series::line("Revenue this year", [1.0]))
        .series(Series::line("Revenue last year", [2.0]))
        .series(Series::line("Forecast", [3.0]));
    let mut painter = Painter::new(220.0, 300.0);
    chart.draw(&mut painter);
    let tops: Vec<f64> = texts(&painter)
        .iter()
        .filter(|(t, _, _)| t.starts_with("Revenue") || *t == "Forecast")
        .map(|(_, at, _)| at.y)
        .collect();
    assert_eq!(tops.len(), 3);
    assert!(tops.iter().any(|&y| y > tops[0]), "{tops:?}");
}

#[test]
fn crowded_categories_skip_labels() {
    let labels: Vec<String> = (0..60).map(|i| format!("Item {i}")).collect();
    let chart = Chart::new()
        .categories(labels)
        .series(Series::bar("v", (0..60).map(f64::from)));
    let mut painter = Painter::new(400.0, 300.0);
    chart.draw(&mut painter);
    let shown = texts(&painter)
        .iter()
        .filter(|(t, _, _)| t.starts_with("Item"))
        .count();
    assert!(shown > 1 && shown < 60, "{shown}");
}

#[test]
fn numeric_x_labels_do_not_overlap() {
    let chart = Chart::new()
        .x_axis(Axis::new().format(|v| format!("{v:.0} seconds")))
        .series(Series::line_xy("s", [(0.0, 0.0), (1000.0, 1.0)]));
    let mut painter = Painter::new(300.0, 200.0);
    let layout = chart.draw(&mut painter);
    let mut labels: Vec<(f64, f64)> = texts(&painter)
        .iter()
        .filter(|(t, _, _)| t.ends_with("seconds"))
        .map(|(t, at, _)| (at.x, painter.text_width(t, 11.0)))
        .collect();
    assert!(labels.len() >= 2);
    labels.sort_by(|a, b| a.0.total_cmp(&b.0));
    for pair in labels.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        assert!(
            left.0 + left.1 / 2.0 <= right.0 - right.1 / 2.0,
            "{labels:?}"
        );
    }
    assert!(layout.x_range().1 >= 1000.0);
}

#[test]
fn gaps_and_broken_values_never_reach_the_painter() {
    let chart = Chart::new()
        .series(Series::line(
            "gaps",
            [1.0, f64::NAN, 2.0, 3.0, f64::INFINITY, 4.0],
        ))
        .series(Series::scatter(
            "dots",
            [(f64::NAN, 1.0), (1.0, f64::NEG_INFINITY), (2.0, 2.0)],
        ))
        .series(Series::bar("bars", [f64::NAN, 1.0]));
    let mut painter = Painter::new(400.0, 300.0);
    chart.draw(&mut painter);
    for command in painter.commands() {
        let path = match command {
            DrawCommand::Fill { path, .. } | DrawCommand::Stroke { path, .. } => path,
            DrawCommand::Clip(path) => path,
            _ => continue,
        };
        assert!(points_of(path)
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite()));
    }
    // 線は欠けた所で切れる: 1 点 (0) と、2 点 (2, 3) と、1 点 (5)。
    let line = strokes(&painter)
        .into_iter()
        .find(|(_, c, _)| *c == series_color(0, false))
        .unwrap();
    let moves = line
        .0
        .segments()
        .iter()
        .filter(|s| matches!(s, PathSegment::MoveTo(_)))
        .count();
    assert_eq!(moves, 1, "線になるのは (2, 3) だけ");
    // 前後が欠けた 1 点は点で描く。
    let dots = fills(&painter)
        .into_iter()
        .filter(|(_, c)| *c == series_color(0, false))
        .count();
    assert_eq!(dots, 1);
}

#[test]
fn empty_and_tiny_charts_draw_without_trouble() {
    for (width, height) in [(0.0, 0.0), (10.0, 10.0), (40.0, 30.0), (400.0, 300.0)] {
        for chart in [
            Chart::new(),
            Chart::new().categories(Vec::<String>::new()),
            Chart::new().series(Series::line("empty", [])),
            Chart::new().series(Series::bar("nan", [f64::NAN])),
            Chart::new().series(Series::scatter("one", [(5.0, 5.0)])),
            Chart::new().y_axis(Axis::new().range(10.0, 10.0)),
            sales(),
        ] {
            let mut painter = Painter::new(width, height);
            let layout = chart.draw(&mut painter);
            let plot = layout.plot();
            assert!(plot.width >= 0.0 && plot.height >= 0.0);
            let (low, high) = layout.y_range();
            assert!(low.is_finite() && high.is_finite() && low < high);
        }
    }
}

#[test]
fn a_set_range_cuts_and_keeps_its_ticks_inside() {
    let chart = Chart::new()
        .y_axis(Axis::new().range(0.0, 50.0))
        .series(Series::line("s", [10.0, 80.0, 20.0]));
    let mut painter = Painter::new(400.0, 300.0);
    let layout = chart.draw(&mut painter);
    assert_eq!(layout.y_range(), (0.0, 50.0));
    let values: Vec<f64> = texts(&painter)
        .iter()
        .filter_map(|(t, _, _)| t.parse().ok())
        .collect();
    assert!(
        values.iter().all(|v| (0.0..=50.0).contains(v)),
        "{values:?}"
    );
}

#[test]
fn many_points_stay_few_commands() {
    let count = 100_000;
    let wave = |i: usize| ((i as f64) / 500.0).sin() * 100.0 + (i % 17) as f64;
    let chart = Chart::new()
        .series(Series::line_xy(
            "line",
            (0..count).map(|i| (i as f64, wave(i))),
        ))
        .series(Series::scatter(
            "dots",
            (0..count).map(|i| (i as f64, wave(i * 7 % count) + 50.0)),
        ));
    let mut painter = Painter::new(800.0, 400.0);
    let started = Instant::now();
    let layout = chart.draw(&mut painter);
    let elapsed = started.elapsed();
    eprintln!("10 万点の折れ線と散布図を {elapsed:?} で記録した");

    let plot = layout.plot();
    let line = strokes(&painter)
        .into_iter()
        .find(|(_, c, _)| *c == series_color(0, false))
        .unwrap();
    let columns = plot.width.ceil() + 2.0;
    assert!(
        (line.0.segments().len() as f64) <= columns * 4.0,
        "{} 点",
        line.0.segments().len()
    );
    let dots = fills(&painter)
        .into_iter()
        .find(|(_, c)| *c == series_color(1, false))
        .unwrap();
    // 円 1 つは MoveTo + 4 つの曲線 + Close。
    let circles = dots.0.segments().len() / 6;
    let cells = (plot.width / 4.0 + 3.0) * (plot.height / 4.0 + 3.0);
    assert!((circles as f64) <= cells, "{circles} 個");
    // 命令の数は系列ごとに 1 つずつで、点の数によらない。
    assert!(
        painter.commands().len() < 100,
        "{}",
        painter.commands().len()
    );
}

#[test]
fn kinds_draw_bars_first_then_lines_then_dots() {
    let chart = Chart::new()
        .series(Series::scatter("dots", [(0.0, 1.0)]))
        .series(Series::line("line", [1.0, 2.0]))
        .series(Series::bar("bars", [1.0, 2.0]));
    let mut painter = Painter::new(400.0, 300.0);
    chart.draw(&mut painter);
    let order: Vec<SeriesKind> = painter
        .commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Fill {
                paint: Paint::Color(c),
                ..
            } if *c == series_color(2, false) => Some(SeriesKind::Bar),
            DrawCommand::Stroke { color, .. } if *color == series_color(1, false) => {
                Some(SeriesKind::Line)
            }
            DrawCommand::Fill {
                paint: Paint::Color(c),
                ..
            } if *c == series_color(0, false) => Some(SeriesKind::Scatter),
            _ => None,
        })
        .take(3)
        .collect();
    assert_eq!(
        order,
        [SeriesKind::Bar, SeriesKind::Line, SeriesKind::Scatter]
    );
}

#[test]
fn a_chart_drawn_in_part_of_a_surface_keeps_its_transform() {
    let mut painter = Painter::new(800.0, 600.0);
    painter.translate(400.0, 0.0);
    painter.clip_rect(Rect::new(0.0, 0.0, 400.0, 300.0));
    sales().draw(&mut painter);
    let transform = painter.transform();
    assert_eq!(
        transform.apply(Point::new(0.0, 0.0)),
        Point::new(400.0, 0.0)
    );
}
