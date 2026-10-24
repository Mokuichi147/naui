use std::cell::Cell;
use std::rc::Rc;

use naui::{
    Align, GridCell, Length, Orientation, Padding, Result, ScrollMetrics, ScrollPolicy, Sizing,
    Track, Ui,
};

use crate::parts::{self, Notice};

/// Stack、Grid、Spacer、Expander、SplitView、Scroll の配置特性と、
/// 表示・非表示、フォーカス、ツールチップ。
///
/// 並べ方の違いが目で見えるよう、マスや端にはボタンを置く。ボタンの文字が
/// そのマスの指定 (固定幅・Fill・またがり) を表す。
pub(crate) fn build(ui: &Ui, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    // Horizontal は横へ順番に並べる。隠したものは Stack の中で場所を空けない
    // ので、「中央」を隠すと右が詰まる。
    parts::section(ui, &pane, "Stack")?;
    let horizontal = parts::row(ui)?;
    let middle = ui.button("Middle")?;
    horizontal.append(&ui.button("Left")?);
    horizontal.append(&middle);
    horizontal.append(&ui.button("Right")?);
    pane.append(&horizontal);

    let hide = ui.button("Hide “Middle”")?;
    hide.set_tooltip(Some("set_visible on the Middle button"));
    hide.on_click({
        let middle = middle.clone();
        let hide = hide.downgrade();
        move || {
            let visible = !middle.is_visible();
            middle.set_visible(visible);
            let Some(hide) = hide.upgrade() else {
                return;
            };
            hide.set_text(if visible {
                "Hide “Middle”"
            } else {
                "Show “Middle”"
            });
        }
    });
    pane.append(&hide);

    build_dynamic_children(ui, &pane)?;
    build_focus(ui, &pane)?;

    parts::section(ui, &pane, "Grid")?;
    let grid = ui.grid()?;
    grid.set_spacing(10.0, 8.0);
    grid.set_column_track(0, Track::Fixed(120.0));
    grid.set_column_track(1, Track::FILL);
    grid.set_sizing(Sizing::fill_width());
    for (title, cell) in [
        ("Fixed 120", GridCell::new(0, 0)),
        ("Fill", GridCell::new(1, 0)),
        ("Span 2 columns", GridCell::new(0, 1).span(2, 1)),
    ] {
        let button = ui.button(title)?;
        button.set_sizing(Sizing::fill_width());
        grid.attach(&button, cell);
    }
    pane.append(&grid);

    // 余った幅を吸収し、後ろの要素を端へ押す。
    parts::section(ui, &pane, "Spacer")?;
    let spacer_example = ui.stack(Orientation::Horizontal)?;
    spacer_example.set_sizing(Sizing::fill_width());
    spacer_example.append(&ui.button("Start")?);
    spacer_example.append(&ui.spacer()?);
    spacer_example.append(&ui.button("End")?);
    pane.append(&spacer_example);

    // 見出しを押すと中身を出し入れする。閉じている間は場所も空けない。
    parts::section(ui, &pane, "Expander")?;
    let details_body = ui.stack(Orientation::Vertical)?;
    details_body.set_spacing(8.0);
    // 交差軸の既定は中央ぞろえなので、チェックボックスの左端をそろえる。
    details_body.set_align(Align::Start);
    details_body.append(&ui.checkbox("Back up")?);
    details_body.append(&ui.checkbox("Format on save")?);
    let details = ui.expander("Advanced")?;
    details.set_child(&details_body);
    details.set_sizing(Sizing::fill_width());
    details.on_toggle({
        let notice = notice.clone();
        move |expanded| {
            notice.show(if expanded {
                "Expander: expanded"
            } else {
                "Expander: collapsed"
            });
        }
    });
    pane.append(&details);

    // 仕切りをドラッグすると区画の大きさが変わる。余った幅は右側が受け取る。
    // 仕切りを左へ動かすと、本文は区画の幅で折り返す。
    parts::section(ui, &pane, "SplitView")?;
    let mailboxes = ui.stack(Orientation::Vertical)?;
    mailboxes.set_spacing(6.0);
    mailboxes.set_padding(Padding::all(10.0));
    mailboxes.set_align(Align::Start);
    for name in ["Inbox", "Sent", "Drafts"] {
        mailboxes.append(&ui.button(name)?);
    }
    let body = ui.stack(Orientation::Vertical)?;
    body.set_spacing(6.0);
    body.set_padding(Padding::all(10.0));
    body.set_align(Align::Start);
    let subject = ui.label("Weekly sync")?;
    subject.set_style(naui::TextStyle::Heading);
    body.append(&subject);
    // 区画は狭くなるので、本文は折り返す (Label の既定は 1 行)。
    // 折り返す幅は親が決めるため、幅も与えておく。
    let message = ui.label(
        "The weekly sync moves to Tuesday at 10:00. I will put the slides in the shared folder the day before.",
    )?;
    message.set_wrap(true);
    message.set_sizing(Sizing::fill_width());
    body.append(&message);
    let split = ui.split_view(Orientation::Horizontal)?;
    split.set_start(&mailboxes);
    split.set_end(&body);
    split.set_min_sizes(120.0, 140.0);
    split.on_resize({
        let notice = notice.clone();
        move |position| notice.show(&format!("SplitView: divider at {position:.0} px"))
    });
    // Scroll と同じく中身の高さでは決まらないので、大きさを指定する。
    split.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(160.0)),
    );
    pane.append(&split);

    build_scroll(ui, &pane)?;
    Ok(pane)
}

/// 高さを固定し、はみ出した内容だけをスクロールする。
///
/// 行を足すと、末尾の近くを見ていたときだけ末尾へ追従する。位置は操作の行の
/// 右端に出す。
fn build_scroll(ui: &Ui, pane: &naui::Stack) -> Result<()> {
    parts::section(ui, pane, "Scroll")?;
    let content = ui.stack(Orientation::Vertical)?;
    content.set_spacing(5.0);
    content.set_padding(Padding::all(8.0));
    content.set_align(Align::Start);
    for index in 1..=20 {
        content.append(&ui.label(&format!("Line {index}"))?);
    }
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(&content);
    scroll.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(220.0)),
    );

    let position = parts::readout(ui, "0 px")?;
    scroll.on_scroll({
        let position = position.clone();
        move |m: ScrollMetrics| position.set_text(&format!("{:.0} / {:.0} px", m.y, m.max_y()))
    });

    let controls = parts::row(ui)?;
    let append = ui.button("Add line")?;
    let count = Rc::new(Cell::new(20));
    append.on_click({
        let ui = ui.clone();
        let content = content.clone();
        let scroll = scroll.clone();
        move || {
            // 追従するかどうかは、足す前の位置で決める。
            let following = scroll.metrics().distance_to_end() < 40.0;
            count.set(count.get() + 1);
            let Ok(label) = ui.label(&format!("Line {}", count.get())) else {
                return;
            };
            content.append(&label);
            if following {
                scroll.scroll_to_end();
            }
        }
    });
    let top = ui.button("Top")?;
    top.on_click({
        let scroll = scroll.clone();
        move || scroll.scroll_to(0.0, 0.0)
    });
    let end = ui.button("End")?;
    end.on_click({
        let scroll = scroll.clone();
        move || scroll.scroll_to_end()
    });
    controls.append(&append);
    controls.append(&top);
    controls.append(&end);
    controls.append(&position);
    pane.append(&controls);
    pane.append(&scroll);
    Ok(())
}

/// フォーカスとツールチップ。ボタンにポインターを重ねると説明が出る。
fn build_focus(ui: &Ui, pane: &naui::Stack) -> Result<()> {
    parts::section(ui, pane, "Focus")?;
    let actions = parts::row(ui)?;
    let field = ui.text_input("")?;
    field.set_placeholder("Text field");
    field.set_sizing(Sizing::new().width(Length::Fixed(200.0)));
    let focus = ui.button("Focus the field →")?;
    focus.set_tooltip(Some("request_focus on the field to the right"));
    focus.on_click({
        let field = field.clone();
        move || {
            field.request_focus();
        }
    });
    actions.append(&focus);
    actions.append(&field);
    pane.append(&actions);
    Ok(())
}

/// `Stack` の子を後から足したり外したりする例。
///
/// コールバックの中で Ui を clone して子を作り、insert / remove / clear で
/// 並びを変える。
fn build_dynamic_children(ui: &Ui, pane: &naui::Stack) -> Result<()> {
    parts::section(ui, pane, "Adding and removing children")?;
    let items = ui.stack(Orientation::Vertical)?;
    items.set_spacing(4.0);
    items.set_align(Align::Start);
    let count = Rc::new(Cell::new(0));

    let actions = parts::row(ui)?;

    let add = ui.button("Append")?;
    add.on_click({
        // コールバックへ持ち込むのは clone した Ui。中身は同じ。
        let ui = ui.clone();
        let items = items.clone();
        let count = count.clone();
        move || {
            count.set(count.get() + 1);
            let Ok(label) = ui.label(&format!("Item {}", count.get())) else {
                return;
            };
            items.append(&label);
        }
    });
    actions.append(&add);

    let insert = ui.button("Insert at top")?;
    insert.on_click({
        let ui = ui.clone();
        let items = items.clone();
        let count = count.clone();
        move || {
            count.set(count.get() + 1);
            let Ok(label) = ui.label(&format!("Item {} (top)", count.get())) else {
                return;
            };
            items.insert(0, &label);
        }
    });
    actions.append(&insert);

    let remove = ui.button("Remove first")?;
    remove.on_click({
        let items = items.clone();
        move || items.remove(0)
    });
    actions.append(&remove);

    let clear = ui.button("Clear")?;
    clear.on_click({
        let items = items.clone();
        move || items.clear()
    });
    actions.append(&clear);

    pane.append(&actions);
    pane.append(&items);
    Ok(())
}
