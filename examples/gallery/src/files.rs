use naui::{FileEntry, FileFilter, FilePickerMode, Length, Result, Sizing, Ui};

use crate::parts::{self, Notice};

/// 選ばれたファイルやフォルダーを、短い知らせの文にする。
fn describe_entries(entries: &[FileEntry]) -> String {
    match entries {
        [] => "選択されていません".to_string(),
        [entry] => describe_entry(entry),
        many => format!("{} 件: {} ほか", many.len(), many[0].name()),
    }
}

/// パスが読めればパスを、読めない環境 (ブラウザ) では名前を出す。
fn describe_entry(entry: &FileEntry) -> String {
    match entry.path() {
        Some(path) => path.display().to_string(),
        None => format!("{} (この環境ではパス非公開)", entry.name()),
    }
}

/// 単一ファイル、複数ファイル、フォルダーの選択と、内容の保存。
pub(crate) fn build(ui: &Ui, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    parts::section(
        ui,
        &pane,
        "FilePicker",
        &["環境標準の選択ダイアログを、3つのモードで開きます。"],
    )?;

    parts::group(ui, &pane, "ファイルを1つ選択", &["拡張子フィルターあり。"])?;
    let single = ui.file_picker("画像を1つ選ぶ")?;
    single.set_filters(&[FileFilter::new(
        "画像",
        ["png", "jpg", "jpeg", "gif", "webp"],
    )]);
    single.on_select({
        let notice = notice.clone();
        move |entries| notice.show(&format!("選択: {}", describe_entries(entries)))
    });
    pane.append(&single);

    parts::group(ui, &pane, "ファイルを複数選択", &[])?;
    let multiple = ui.file_picker("ファイルを複数選ぶ")?;
    multiple.set_mode(FilePickerMode::Files);
    multiple.on_select({
        let notice = notice.clone();
        move |entries| notice.show(&format!("選択: {}", describe_entries(entries)))
    });
    pane.append(&multiple);

    parts::group(ui, &pane, "フォルダーを選択", &[])?;
    let folder = ui.file_picker("フォルダーを選ぶ")?;
    folder.set_mode(FilePickerMode::Folder);
    folder.on_select({
        let notice = notice.clone();
        move |entries| notice.show(&format!("選択: {}", describe_entries(entries)))
    });
    pane.append(&folder);

    parts::section(
        ui,
        &pane,
        "FileSaver",
        &["入力した内容を、環境標準の保存ダイアログで書き出します。"],
    )?;

    let editor = ui.text_area("naui で保存したテキストです。")?;
    editor.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(80.0)),
    );

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
        let notice = notice.clone();
        move |entry| notice.show(&format!("保存しました: {}", describe_entry(entry)))
    });
    saver.on_error({
        let notice = notice.clone();
        move |error| notice.show(&format!("保存できません: {error}"))
    });
    pane.append(&editor);
    pane.append(&saver);
    Ok(pane)
}
