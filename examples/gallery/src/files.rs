use naui::{FileFilter, FilePickerMode, Length, Orientation, Padding, Result, Sizing, Ui};

use crate::describe_entries;

/// 単一ファイル、複数ファイル、フォルダーの選択と、内容の保存。
pub(crate) fn build(ui: &Ui) -> Result<naui::Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(14.0);
    pane.set_padding(Padding::all(12.0));

    pane.append(&ui.label("FilePicker")?);
    pane.append(&ui.label("環境標準の選択ダイアログを、3つのモードで開きます。")?);

    pane.append(&ui.label("ファイルを1つ選択 — 拡張子フィルターあり")?);
    let single_status = ui.label("選択: なし")?;
    let single = ui.file_picker("画像を1つ選ぶ")?;
    single.set_filters(&[FileFilter::new(
        "画像",
        ["png", "jpg", "jpeg", "gif", "webp"],
    )]);
    single.on_select({
        let single_status = single_status.clone();
        move |entries| single_status.set_text(&format!("選択: {}", describe_entries(entries)))
    });
    pane.append(&single);
    pane.append(&single_status);

    pane.append(&ui.label("ファイルを複数選択")?);
    let multiple_status = ui.label("選択: なし")?;
    let multiple = ui.file_picker("ファイルを複数選ぶ")?;
    multiple.set_mode(FilePickerMode::Files);
    multiple.on_select({
        let multiple_status = multiple_status.clone();
        move |entries| multiple_status.set_text(&format!("選択: {}", describe_entries(entries)))
    });
    pane.append(&multiple);
    pane.append(&multiple_status);

    pane.append(&ui.label("フォルダーを選択")?);
    let folder_status = ui.label("選択: なし")?;
    let folder = ui.file_picker("フォルダーを選ぶ")?;
    folder.set_mode(FilePickerMode::Folder);
    folder.on_select({
        let folder_status = folder_status.clone();
        move |entries| folder_status.set_text(&format!("選択: {}", describe_entries(entries)))
    });
    pane.append(&folder);
    pane.append(&folder_status);

    pane.append(&ui.label("FileSaver")?);
    pane.append(&ui.label("入力した内容を、環境標準の保存ダイアログで書き出します。")?);

    let editor = ui.text_area("naui で保存したテキストです。")?;
    editor.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(80.0)),
    );

    let save_status = ui.label("保存: まだ")?;
    let saver = ui.file_saver("テキストを保存")?;
    saver.set_file_name("naui-メモ");
    saver.set_filters(&[FileFilter::new("テキスト", ["txt", "md"])]);
    // ボタンを押した時点の内容を書き出したいので、打つたびに渡し直す。
    saver.set_contents(editor.text().as_bytes());
    editor.on_change({
        let saver = saver.clone();
        move |text| saver.set_contents(text.as_bytes())
    });
    saver.on_save({
        let save_status = save_status.clone();
        move |entry| {
            let place = match entry.path() {
                Some(path) => path.display().to_string(),
                None => format!("{} (この環境ではパス非公開)", entry.name()),
            };
            save_status.set_text(&format!("保存: {place}"));
        }
    });
    saver.on_error({
        let save_status = save_status.clone();
        move |error| save_status.set_text(&format!("保存できません: {error}"))
    });
    pane.append(&editor);
    pane.append(&saver);
    pane.append(&save_status);
    Ok(pane)
}
