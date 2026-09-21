//! 描画面 (`Canvas`)。棒グラフと図形を、`Painter` の命令だけで描く。
//!
//! ここで描いているものは naui の中に「棒グラフ」や「円」の部品があるわけでは
//! なく、`on_draw` の中で矩形や円の**命令を積んでいる**だけ。積んだ命令を
//! 画素にするのは各環境の 2D API (Core Graphics / cairo / XAML の図形 /
//! `<canvas>`) で、アンチエイリアスや高解像度の倍率もそちらが持つ。

use std::cell::{Cell, RefCell};
use std::f64::consts::{FRAC_PI_2, TAU};
use std::rc::Rc;

use naui::{
    Align, Color, Length, Orientation, Path, Point, PointerPhase, Rect, Result, Sizing, Ui,
};

use crate::parts;

/// 棒グラフに出す値。
const SALES: [(&str, f64); 6] = [
    ("4 月", 42.0),
    ("5 月", 58.0),
    ("6 月", 35.0),
    ("7 月", 71.0),
    ("8 月", 64.0),
    ("9 月", 80.0),
];

/// 図の地色と枠。テーマに関係なく同じ見え方にするため、面の中はアプリが塗る。
const PAPER: Color = Color::rgb(0xfa, 0xfa, 0xfa);
const FRAME: Color = Color::rgb(0xc8, 0xc8, 0xc8);
const INK: Color = Color::rgb(0x33, 0x33, 0x33);
const ACCENT: Color = Color::rgb(0x33, 0x66, 0xff);
const ACCENT_STRONG: Color = Color::rgb(0xff, 0x88, 0x00);

/// 棒グラフの状態。ポインターの位置から「どの棒の上か」を決め直す。
#[derive(Default)]
struct ChartState {
    hovered: Option<usize>,
    selected: Option<usize>,
    /// いま描いている棒の場所。当たり判定に使う。
    bars: Vec<Rect>,
}

pub(crate) fn build(ui: &Ui) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    parts::section(
        ui,
        &pane,
        "Canvas",
        &[
            "アプリが自分で描く面です。矩形・円・線・パス・文字の命令を Painter に積むと、その環境の 2D API が画素にします。",
            "棒の上にポインターを置くと色が変わり、押すと選ばれます。面の幅を変えると描き直されます。",
        ],
    )?;

    let chart_status = parts::status(ui, "棒を押すと値が出ます")?;
    let chart = ui.canvas()?;
    chart.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(220.0)),
    );
    let state = Rc::new(RefCell::new(ChartState::default()));

    chart.on_draw({
        let state = state.clone();
        move |painter| {
            let mut state = state.borrow_mut();
            let bounds = painter.bounds();
            painter.fill_rect(bounds, PAPER);
            painter.stroke_rect(bounds.inset(0.5), FRAME, 1.0);

            // 目盛りの線 (破線) と、その値。
            let plot = Rect::new(48.0, 16.0, bounds.width - 64.0, bounds.height - 48.0);
            let max = 100.0;
            painter.set_text_align(Align::End);
            for step in 0..=4 {
                let value = max * f64::from(step) / 4.0;
                let y = plot.bottom() - plot.height * value / max;
                painter.set_dash(&[3.0, 3.0]);
                painter.line(
                    Point::new(plot.x, y),
                    Point::new(plot.right(), y),
                    FRAME,
                    1.0,
                );
                painter.set_dash(&[]);
                painter.text(
                    &format!("{value:.0}"),
                    Point::new(plot.x - 8.0, y - 7.0),
                    11.0,
                    INK,
                );
            }

            // 棒。選ばれているものは濃く、ポインターの下にあるものは少し透ける。
            let slot = plot.width / SALES.len() as f64;
            state.bars.clear();
            painter.set_text_align(Align::Center);
            for (index, (label, value)) in SALES.iter().enumerate() {
                let height = plot.height * value / max;
                let bar = Rect::new(
                    plot.x + slot * index as f64 + slot * 0.2,
                    plot.bottom() - height,
                    slot * 0.6,
                    height,
                );
                let color = if state.selected == Some(index) {
                    ACCENT_STRONG
                } else {
                    ACCENT
                };
                painter.set_opacity(if state.hovered == Some(index) {
                    0.7
                } else {
                    1.0
                });
                painter.fill_rect(bar, color);
                painter.set_opacity(1.0);
                painter.text(
                    label,
                    Point::new(bar.center().x, plot.bottom() + 8.0),
                    11.0,
                    INK,
                );
                if state.hovered == Some(index) || state.selected == Some(index) {
                    painter.text(
                        &format!("{value:.0}"),
                        Point::new(bar.center().x, bar.y - 16.0),
                        11.0,
                        INK,
                    );
                }
                state.bars.push(bar);
            }
            painter.line(
                Point::new(plot.x, plot.bottom() + 0.5),
                Point::new(plot.right(), plot.bottom() + 0.5),
                INK,
                1.0,
            );
        }
    });

    chart.on_pointer({
        let state = state.clone();
        let chart = chart.clone();
        let status = chart_status.clone();
        move |event| {
            let hit = state
                .borrow()
                .bars
                .iter()
                .position(|bar| bar.contains(event.point));
            let mut changed = false;
            {
                let mut state = state.borrow_mut();
                if state.hovered != hit {
                    state.hovered = hit;
                    changed = true;
                }
                if event.phase == PointerPhase::Down && state.selected != hit {
                    state.selected = hit;
                    changed = true;
                    match hit {
                        Some(index) => {
                            status.set_text(&format!("{}: {:.0}", SALES[index].0, SALES[index].1))
                        }
                        None => status.set_text("棒を押すと値が出ます"),
                    }
                }
            }
            if changed {
                chart.redraw();
            }
        }
    });
    pane.append(&chart);
    pane.append(&chart_status);

    parts::group(
        ui,
        &pane,
        "図形",
        &[
            "円・楕円・扇形・ベジェ曲線・多角形・破線・透け具合。スライダーで扇形の角度を変えると描き直します。",
        ],
    )?;

    let angle = Rc::new(Cell::new(0.65));
    let shapes = ui.canvas()?;
    shapes.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(160.0)),
    );
    shapes.on_draw({
        let angle = angle.clone();
        move |painter| {
            let bounds = painter.bounds();
            painter.fill_rect(bounds, PAPER);
            painter.stroke_rect(bounds.inset(0.5), FRAME, 1.0);
            let cy = bounds.height / 2.0;

            // 扇形: 中心へ移ってから弧を描いて閉じる。
            let pie = Point::new(60.0, cy);
            painter.fill_circle(pie, 44.0, FRAME);
            let end = -FRAC_PI_2 + TAU * angle.get();
            let wedge = Path::new()
                .move_to(pie)
                .arc(pie, 44.0, -FRAC_PI_2, end)
                .close();
            painter.fill_path(&wedge, ACCENT);
            painter.set_text_align(Align::Center);
            painter.text(
                &format!("{:.0}%", angle.get() * 100.0),
                Point::new(pie.x, pie.y + 52.0),
                11.0,
                INK,
            );

            // 楕円の輪郭と、その上を通る破線。
            painter.set_dash(&[6.0, 3.0]);
            painter.stroke_path(
                &Path::new().ellipse(Point::new(170.0, cy), 48.0, 30.0),
                ACCENT_STRONG,
                2.0,
            );
            painter.set_dash(&[]);

            // ベジェ曲線 (波) と、その下を透けて塗る多角形。
            let wave = Path::new()
                .move_to(Point::new(240.0, cy))
                .quad_to(Point::new(270.0, cy - 50.0), Point::new(300.0, cy))
                .quad_to(Point::new(330.0, cy + 50.0), Point::new(360.0, cy));
            painter.stroke_path(&wave, INK, 2.0);
            painter.set_opacity(0.35);
            painter.fill_polygon(
                &[
                    Point::new(250.0, cy + 40.0),
                    Point::new(300.0, cy - 20.0),
                    Point::new(350.0, cy + 40.0),
                ],
                ACCENT,
            );
            painter.set_opacity(1.0);

            // 面の右端に寄せた文字。幅が変わっても右端に付いてくる。
            painter.set_text_align(Align::End);
            painter.text(
                &format!("{:.0} × {:.0}", bounds.width, bounds.height),
                Point::new(bounds.width - 8.0, 8.0),
                11.0,
                INK,
            );
        }
    });
    pane.append(&shapes);

    let angle_slider = ui.slider(0.0, 1.0)?;
    angle_slider.set_value(angle.get());
    angle_slider.set_sizing(Sizing::new().width(Length::Fixed(240.0)));
    angle_slider.on_change({
        let angle = angle.clone();
        let shapes = shapes.clone();
        move |value| {
            angle.set(value);
            shapes.redraw();
        }
    });
    let slider_row = ui.stack(Orientation::Horizontal)?;
    slider_row.set_spacing(8.0);
    slider_row.append(&ui.label("扇形の角度")?);
    slider_row.append(&angle_slider);
    pane.append(&slider_row);

    Ok(pane)
}
