//! レイアウト規則の検証。テキスト計測はダミー実装で置き換える。

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use miui_core::geometry::{Insets, Size};
use miui_core::layout::{BoxConstraints, CrossAxis, MainAxis};
use miui_core::painter::{LineMetrics, TextMeasurer, TextStyle};
use miui_core::theme::Theme;
use miui_core::widget::{Element, Id, LayoutCx, StateStore, Widget};
use miui_widgets::{column, row, Container, SizedBox};

/// 1 文字 = 8px、行高 = 16px として計測するダミー。
struct FakeText;

impl TextMeasurer for FakeText {
    fn measure_line(&mut self, text: &str, _style: &TextStyle) -> f32 {
        text.chars().count() as f32 * 8.0
    }
    fn line_metrics(&mut self, _style: &TextStyle) -> LineMetrics {
        LineMetrics {
            ascent: 12.0,
            descent: 4.0,
            line_height: 16.0,
        }
    }
    fn wrap_lines(&mut self, text: &str, _style: &TextStyle, _max: f32) -> Vec<(usize, usize)> {
        vec![(0, text.len())]
    }
}

/// レイアウト規則はスタイルに依存しないので、ビルド対象のテーマで検証する。
fn theme() -> Theme {
    miui_theme::for_target(miui_core::theme::ColorMode::Light)
}

/// ツリーをレイアウトし、(ルートサイズ, 出現した Id 列) を返す。
fn layout<M>(root: &mut Element<M>, size: Size) -> (Size, Vec<Id>) {
    let theme = theme();
    let mut text = FakeText;
    let mut store = StateStore::new();
    let mut alive = HashSet::new();
    let mut focusables = Vec::new();
    let mut cx = LayoutCx::new(
        &mut text,
        &theme,
        &mut store,
        &mut alive,
        &mut focusables,
        1.0,
    );
    let s = root.layout(&mut cx, BoxConstraints::tight(size));
    let mut ids: Vec<Id> = alive.into_iter().collect();
    ids.sort();
    (s, ids)
}

/// 自分が受け取ったサイズを記録するだけのテスト用ウィジェット。
struct Probe {
    log: Rc<RefCell<Vec<Size>>>,
}

impl<M> Widget<M> for Probe {
    fn type_name(&self) -> &'static str {
        "Probe"
    }
    fn layout(&mut self, _cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        let size = Size::new(
            if bc.max.width.is_finite() {
                bc.max.width
            } else {
                0.0
            },
            if bc.max.height.is_finite() {
                bc.max.height
            } else {
                0.0
            },
        );
        self.log.borrow_mut().push(size);
        size
    }
    fn paint(&self, _cx: &mut miui_core::widget::PaintCx, _node: miui_core::widget::Node) {}
}

#[test]
fn row_distributes_free_space_by_flex() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut root: Element<()> = Element::new(
        row()
            .spacing(10.0)
            .child(SizedBox::new(100.0, 20.0))
            .child_flex(Probe { log: log.clone() }, 1.0)
            .child_flex(Probe { log: log.clone() }, 3.0),
    );
    layout(&mut root, Size::new(410.0, 50.0));

    // 全幅 410 - 固定 100 - 間隔 20 = 290 を 1:3 で分配する。
    let sizes = log.borrow();
    assert_eq!(sizes.len(), 2);
    assert!((sizes[0].width - 72.5).abs() < 0.01, "{:?}", sizes[0]);
    assert!((sizes[1].width - 217.5).abs() < 0.01, "{:?}", sizes[1]);
    assert_eq!(root.size().width, 410.0);
}

#[test]
fn stretch_gives_children_the_full_cross_axis() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut root: Element<()> = Element::new(
        column()
            .align(CrossAxis::Stretch)
            .padding(Insets::all(10.0))
            .child(Probe { log: log.clone() }),
    );
    layout(&mut root, Size::new(300.0, 200.0));
    assert_eq!(log.borrow()[0].width, 280.0);
}

#[test]
fn column_hugs_content_without_flex_children() {
    let mut root: Element<()> = Element::new(
        column()
            .spacing(4.0)
            .child(SizedBox::new(50.0, 20.0))
            .child(SizedBox::new(50.0, 30.0)),
    );
    let inner = &mut root;
    let theme = theme();
    let mut text = FakeText;
    let mut store = StateStore::new();
    let mut alive = HashSet::new();
    let mut focusables = Vec::new();
    let mut cx = LayoutCx::new(
        &mut text,
        &theme,
        &mut store,
        &mut alive,
        &mut focusables,
        1.0,
    );
    // 高さは緩い制約にする。
    let size = inner.layout(
        &mut cx,
        BoxConstraints::loose(Size::new(300.0, f32::INFINITY)),
    );
    assert_eq!(size.height, 20.0 + 4.0 + 30.0);
}

#[test]
fn padding_is_added_to_content_size() {
    let mut root: Element<()> =
        Element::new(column().padding(Insets::all(12.0)).child(SizedBox::new(40.0, 40.0)));
    let theme = theme();
    let mut text = FakeText;
    let mut store = StateStore::new();
    let mut alive = HashSet::new();
    let mut focusables = Vec::new();
    let mut cx = LayoutCx::new(
        &mut text,
        &theme,
        &mut store,
        &mut alive,
        &mut focusables,
        1.0,
    );
    let size = root.layout(&mut cx, BoxConstraints::UNBOUNDED);
    assert_eq!(size.width, 40.0 + 24.0);
    assert_eq!(size.height, 40.0 + 24.0);
}

#[test]
fn container_can_stretch_and_center_its_child() {
    let mut root: Element<()> = Element::new(
        Container::new()
            .width(200.0)
            .height(80.0)
            .align(miui_core::layout::Alignment::Center)
            .child(SizedBox::new(40.0, 20.0)),
    );
    let theme = theme();
    let mut text = FakeText;
    let mut store = StateStore::new();
    let mut alive = HashSet::new();
    let mut focusables = Vec::new();
    let mut cx = LayoutCx::new(
        &mut text,
        &theme,
        &mut store,
        &mut alive,
        &mut focusables,
        1.0,
    );
    let size = root.layout(&mut cx, BoxConstraints::loose(Size::new(400.0, 400.0)));
    assert_eq!(size, Size::new(200.0, 80.0));
}

#[test]
fn ids_are_stable_across_rebuilds() {
    let build = || -> Element<()> {
        Element::new(
            column()
                .child(SizedBox::new(10.0, 10.0))
                .child(row().child(SizedBox::new(10.0, 10.0))),
        )
    };
    let mut a = build();
    let mut b = build();
    let (_, ids_a) = layout(&mut a, Size::new(100.0, 100.0));
    let (_, ids_b) = layout(&mut b, Size::new(100.0, 100.0));
    assert_eq!(ids_a, ids_b, "同じ構造なら Id 列も一致すること");
    assert!(ids_a.len() >= 4);
}

#[test]
fn justify_and_align_do_not_overflow() {
    let mut root: Element<()> = Element::new(
        row()
            .justify(MainAxis::SpaceBetween)
            .align(CrossAxis::Stretch)
            .child(SizedBox::new(30.0, 10.0))
            .child(SizedBox::new(30.0, 10.0)),
    );
    let (size, _) = layout(&mut root, Size::new(200.0, 40.0));
    assert_eq!(size, Size::new(200.0, 40.0));
}
