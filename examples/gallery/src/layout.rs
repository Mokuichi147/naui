use naui::{GridCell, Length, Orientation, Padding, Result, ScrollPolicy, Sizing, Track, Ui};

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
