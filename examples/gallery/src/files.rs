use naui::{FileEntry, FileFilter, FilePickerMode, Length, Result, Sizing, Ui};

use crate::parts::{self, Notice};

/// 選ばれたファイルやフォルダーを、短い知らせの文にする。
fn describe_entries(entries: &[FileEntry]) -> String {
    match entries {
        [] => "Nothing selected".to_string(),
        [entry] => describe_entry(entry),
        many => format!("{} items: {} and more", many.len(), many[0].name()),
    }
}

/// パスが読めればパスを、読めない環境 (ブラウザ) では名前を出す。
fn describe_entry(entry: &FileEntry) -> String {
    match entry.path() {
        Some(path) => path.display().to_string(),
        None => format!("{} (path not available here)", entry.name()),
    }
}

/// 単一ファイル、複数ファイル、フォルダーの選択と、落とされたファイルの受け取り、内容の保存。
pub(crate) fn build(ui: &Ui, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    // 環境標準の選択ダイアログを、3 つのモードで開く。ボタンの文字がモードを表す。
    parts::section(ui, &pane, "FilePicker")?;
    let pickers = parts::row(ui)?;

    // 拡張子フィルターあり。
    let single = ui.file_picker("Choose an image…")?;
    single.set_filters(&[FileFilter::new(
        "Images",
        ["png", "jpg", "jpeg", "gif", "webp"],
    )]);
    single.on_select({
        let notice = notice.clone();
        move |entries| notice.show(&format!("Selected: {}", describe_entries(entries)))
    });
    pickers.append(&single);

    let multiple = ui.file_picker("Choose files…")?;
    multiple.set_mode(FilePickerMode::Files);
    multiple.on_select({
        let notice = notice.clone();
        move |entries| notice.show(&format!("Selected: {}", describe_entries(entries)))
    });
    pickers.append(&multiple);

    let folder = ui.file_picker("Choose a folder…")?;
    folder.set_mode(FilePickerMode::Folder);
    folder.on_select({
        let notice = notice.clone();
        move |entries| notice.show(&format!("Selected: {}", describe_entries(entries)))
    });
    pickers.append(&folder);
    pane.append(&pickers);

    // OS (Finder やエクスプローラー) から落とされたファイルを受け取る。
    parts::section(ui, &pane, "on_file_drop")?;
    let drop_target = ui.label("Drop files from Finder, Explorer or a file manager here.")?;
    drop_target.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(64.0)),
    );
    drop_target.on_file_drop({
        let target = drop_target.downgrade();
        let notice = notice.clone();
        move |entries| {
            let text = format!("Dropped: {}", describe_entries(entries));
            if let Some(target) = target.upgrade() {
                target.set_text(&text);
            }
            notice.show(&text);
        }
    });
    pane.append(&drop_target);

    // 入力した内容を、環境標準の保存ダイアログで書き出す。保存のボタンは
    // 書き出す欄のすぐ下に置く。
    parts::section(ui, &pane, "FileSaver")?;

    let editor = ui.text_area("Text saved with naui.")?;
    editor.set_sizing(
        Sizing::new()
            .width(Length::Fill)
            .height(Length::Fixed(80.0)),
    );

    let saver = ui.file_saver("Save text…")?;
    saver.set_file_name("naui-note");
    saver.set_filters(&[FileFilter::new("Text", ["txt", "md"])]);
    // ボタンを押した時点の内容を書き出したいので、打つたびに渡し直す。
    saver.set_contents(editor.text().as_bytes());
    editor.on_change({
        let saver = saver.clone();
        move |text| saver.set_contents(text.as_bytes())
    });
    saver.on_save({
        let notice = notice.clone();
        move |entry| notice.show(&format!("Saved: {}", describe_entry(entry)))
    });
    saver.on_error({
        let notice = notice.clone();
        move |error| notice.show(&format!("Could not save: {error}"))
    });
    pane.append(&editor);
    pane.append(&saver);
    Ok(pane)
}
