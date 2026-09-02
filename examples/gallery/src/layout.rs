use std::cell::Cell;
use std::rc::Rc;

use naui::{
    Align, GridCell, Length, Orientation, Padding, Result, ScrollPolicy, Sizing, Track, Ui,
};

/// Stack、Grid、Scroll、Spacer の配置特性。
pub(crate) fn build(ui: &Ui) -> Result<naui::Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(12.0);
    pane.set_padding(Padding::all(12.0));

    pane.append(&ui.label("Stack")?);
    pane.append(&ui.label("Vertical は縦、Horizontal は横へ順番に並べます。")?);
    let horizontal = ui.stack(Orientation::Horizontal)?;
    horizontal.set_spacing(8.0);
    horizontal.append(&ui.button("左")?);
    horizontal.append(&ui.button("中央")?);
    horizontal.append(&ui.button("右")?);
    pane.append(&horizontal);

    pane.append(&ui.label("子の出し入れ")?);
    pane.append(&ui.label(
        "コールバックの中で Ui を clone して子を作り、insert / remove / clear で並びを変えます。",
    )?);
    build_dynamic_children(ui, &pane)?;

    pane.append(&ui.label("Grid")?);
    pane.append(&ui.label("固定幅と Fill の列、複数列にまたがるセルを確認できます。")?);
    let grid = ui.grid()?;
    grid.set_spacing(10.0, 8.0);
    grid.set_column_track(0, Track::Fixed(120.0));
    grid.set_column_track(1, Track::FILL);
    grid.set_sizing(Sizing::fill_width());
    grid.attach(&ui.label("固定幅 120")?, GridCell::new(0, 0));
    grid.attach(&ui.label("残りの幅を受け取る Fill")?, GridCell::new(1, 0));
    grid.attach(
        &ui.label("2 列にまたがるセル")?,
        GridCell::new(0, 1).span(2, 1),
    );
    pane.append(&grid);

    pane.append(&ui.label("Spacer")?);
    pane.append(&ui.label("余った幅を吸収し、後ろの要素を端へ押します。")?);
    let spacer_example = ui.stack(Orientation::Horizontal)?;
    spacer_example.set_sizing(Sizing::fill_width());
    spacer_example.append(&ui.label("左端")?);
    spacer_example.append(&ui.spacer()?);
    spacer_example.append(&ui.label("右端")?);
    pane.append(&spacer_example);

    pane.append(&ui.label("Expander")?);
    pane.append(&ui.label("見出しを押すと中身を出し入れします。閉じている間は場所も空けません。")?);
    let details_status = ui.label("Expander: 閉じています")?;
    let details_body = ui.stack(Orientation::Vertical)?;
    details_body.set_spacing(8.0);
    // 交差軸の既定は中央ぞろえなので、チェックボックスの左端をそろえる。
    details_body.set_align(Align::Start);
    details_body.append(&ui.checkbox("バックアップを作る")?);
    details_body.append(&ui.checkbox("保存時に整形する")?);
    let details = ui.expander("詳細設定")?;
    details.set_child(&details_body);
    details.set_sizing(Sizing::fill_width());
    details.on_toggle({
        let status = details_status.clone();
        move |expanded| {
            status.set_text(if expanded {
                "Expander: 開いています"
            } else {
                "Expander: 閉じています"
            });
        }
    });
    pane.append(&details);
    pane.append(&details_status);

    pane.append(&ui.label("SplitView")?);
    pane.append(
        &ui.label(
            "仕切りをドラッグすると区画の大きさが変わります。余った幅は右側が受け取ります。",
        )?,
    );
    let split_status = ui.label(&format!(
        "SplitView: 仕切りは {} px",
        naui::DEFAULT_SPLIT_POSITION
    ))?;
    let sidebar = ui.stack(Orientation::Vertical)?;
    sidebar.set_spacing(6.0);
    sidebar.set_padding(Padding::all(10.0));
    sidebar.set_align(Align::Start);
    sidebar.append(&ui.label("サイドバー")?);
    for name in ["受信箱", "送信済み", "下書き"] {
        sidebar.append(&ui.button(name)?);
    }
    let body = ui.stack(Orientation::Vertical)?;
    body.set_spacing(6.0);
    body.set_padding(Padding::all(10.0));
    body.set_align(Align::Start);
    body.append(&ui.label("本文")?);
    // 区画は狭くなるので、この説明だけは折り返す (Label の既定は 1 行)。
    // 折り返す幅は親が決めるため、幅も与えておく。
    let body_note = ui.label("仕切りを左へ動かすと、この文は区画の幅で折り返します。")?;
    body_note.set_wrap(true);
    body_note.set_sizing(Sizing::fill_width());
    body.append(&body_note);
    let split = ui.split_view(Orientation::Horizontal)?;
    split.set_start(&sidebar);
    split.set_end(&body);
    split.set_min_sizes(120.0, 140.0);
    split.on_resize({
        let status = split_status.clone();
        move |position| status.set_text(&format!("SplitView: 仕切りは {position:.0} px"))
    });
    // Scroll と同じく中身の高さでは決まらないので、大きさを指定する。
    split.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(160.0)),
    );
    pane.append(&split);
    pane.append(&split_status);

    pane.append(&ui.label("Scroll")?);
    pane.append(&ui.label("高さを固定し、はみ出した内容だけをスクロールします。")?);
    let content = ui.stack(Orientation::Vertical)?;
    content.set_spacing(5.0);
    content.set_padding(Padding::all(8.0));
    for index in 1..=20 {
        content.append(&ui.label(&format!("スクロール項目 {index}"))?);
    }
    let scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_child(&content);
    scroll.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(220.0)),
    );
    pane.append(&scroll);
    Ok(pane)
}

/// `Stack` の子を後から足したり外したりする例。
fn build_dynamic_children(ui: &Ui, pane: &naui::Stack) -> Result<()> {
    let items = ui.stack(Orientation::Vertical)?;
    items.set_spacing(4.0);
    items.set_align(Align::Start);
    let count = Rc::new(Cell::new(0));

    let actions = ui.stack(Orientation::Horizontal)?;
    actions.set_spacing(8.0);

    let add = ui.button("末尾へ足す")?;
    add.on_click({
        // コールバックへ持ち込むのは clone した Ui。中身は同じ。
        let ui = ui.clone();
        let items = items.clone();
        let count = count.clone();
        move || {
            count.set(count.get() + 1);
            let Ok(label) = ui.label(&format!("項目 {}", count.get())) else {
                return;
            };
            items.append(&label);
        }
    });
    actions.append(&add);

    let insert = ui.button("先頭へ差し込む")?;
    insert.on_click({
        let ui = ui.clone();
        let items = items.clone();
        let count = count.clone();
        move || {
            count.set(count.get() + 1);
            let Ok(label) = ui.label(&format!("項目 {} (先頭)", count.get())) else {
                return;
            };
            items.insert(0, &label);
        }
    });
    actions.append(&insert);

    let remove = ui.button("先頭を外す")?;
    remove.on_click({
        let items = items.clone();
        move || items.remove(0)
    });
    actions.append(&remove);

    let clear = ui.button("空にする")?;
    clear.on_click({
        let items = items.clone();
        move || items.clear()
    });
    actions.append(&clear);

    pane.append(&actions);
    pane.append(&items);
    Ok(())
}
