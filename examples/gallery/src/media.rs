use std::cell::Cell;
use std::rc::Rc;

use naui::{
    Align, FileFilter, Fit, GridCell, NavItem, Orientation, Padding, PlaybackState, Result, Sizing,
    Track, Ui,
};

/// 同梱のサンプル画像の場所。
#[cfg(not(target_arch = "wasm32"))]
const SAMPLE_IMAGE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sample.png");
#[cfg(target_arch = "wasm32")]
const SAMPLE_IMAGE: &str = "assets/sample.png";

/// 表示形式と、そこへ振り分ける拡張子。
const MEDIA_FORMS: [(&str, &[&str]); 3] = [
    (
        "画像",
        &["png", "jpg", "jpeg", "gif", "bmp", "webp", "heic", "tiff"],
    ),
    ("動画", &["mp4", "m4v", "mov", "webm", "mkv", "avi"]),
    ("音声", &["m4a", "mp3", "aac", "wav", "flac", "ogg"]),
];

const MEDIA_DISPLAY_HEIGHT: f64 = 315.0;

/// Image、Video、Audio のソース切り替えと再生操作。
pub(crate) fn build(ui: &Ui) -> Result<naui::Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(10.0);
    pane.set_padding(Padding::all(12.0));
    pane.set_align(Align::Start);

    pane.append(&ui.label("Image / Video / Audio")?);
    pane.append(&ui.label("ファイルまたは URL の拡張子から表示形式を切り替えます。")?);

    let (image_pane, image) = build_image_pane(ui)?;
    let (video_pane, video) = build_video_pane(ui)?;
    let (audio_pane, audio) = build_audio_pane(ui)?;

    // MediaPlayerElement をタブのコンテンツにすると環境によって初期化が不安定に
    // なるため、選択中の形式だけを同じ Grid セルへ置く。
    let forms = ui.grid()?;
    forms.set_column_track(0, Track::FILL);
    forms.set_row_track(0, Track::FILL);
    forms.attach(&image_pane, GridCell::new(0, 0));

    let status = ui.label("表示形式: Image (同梱サンプル)")?;

    let show = Rc::new({
        let image = image.clone();
        let video = video.clone();
        let audio = audio.clone();
        let forms = forms.clone();
        let image_pane = image_pane.clone();
        let video_pane = video_pane.clone();
        let audio_pane = audio_pane.clone();
        let status = status.clone();
        move |form: usize, source: &str| {
            match form {
                0 => forms.replace(&image_pane, GridCell::new(0, 0)),
                1 => forms.replace(&video_pane, GridCell::new(0, 0)),
                _ => forms.replace(&audio_pane, GridCell::new(0, 0)),
            }
            match form {
                0 => image.set_source(source),
                1 => video.set_source(source),
                _ => audio.set_source(source),
            }
            status.set_text(&format!("表示形式: {}", MEDIA_FORMS[form].0));
        }
    });

    let source = ui.text_input("")?;
    source.set_placeholder("ファイルのパス または https://…");
    source.set_sizing(Sizing::fill_width());

    let load = ui.button("URLを読み込む")?;
    load.on_click({
        let show = show.clone();
        let source = source.clone();
        let status = status.clone();
        move || {
            let value = source.text();
            if value.trim().is_empty() {
                status.set_text("表示形式: パスまたは URL を入力してください");
            } else if let Some(form) = media_form_of(&value) {
                show(form, &value);
            } else {
                status.set_text("表示形式: 拡張子から判定できません");
            }
        }
    });

    let pick = ui.file_picker("メディアを選ぶ")?;
    let extensions: Vec<&str> = MEDIA_FORMS
        .iter()
        .flat_map(|(_, extensions)| extensions.iter().copied())
        .collect();
    pick.set_filters(&[FileFilter::new("画像・動画・音声", extensions)]);
    pick.on_select({
        let show = show.clone();
        let source = source.clone();
        let status = status.clone();
        move |entries| {
            let Some(entry) = entries.first() else {
                return;
            };
            let Some(value) = entry.source() else {
                status.set_text(&format!(
                    "表示形式: {} の場所を取得できません",
                    entry.name()
                ));
                return;
            };
            source.set_text(value);
            match media_form_of(entry.name()) {
                Some(form) => show(form, value),
                None => status.set_text(&format!("表示形式: {} は不明です", entry.name())),
            }
        }
    });

    let source_row = ui.stack(Orientation::Horizontal)?;
    source_row.set_spacing(8.0);
    source_row.set_sizing(Sizing::fill_width());
    source_row.append(&source);
    source_row.append(&load);
    source_row.append(&pick);
    pane.append(&source_row);
    pane.append(&status);

    forms.set_sizing(Sizing::fill());
    pane.append(&forms);
    Ok(pane)
}

fn media_form_of(source: &str) -> Option<usize> {
    let extension = source
        .split(['?', '#'])
        .next()?
        .rsplit(['/', '\\'])
        .next()?
        .rsplit_once('.')
        .filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty())?
        .1
        .to_ascii_lowercase();
    MEDIA_FORMS
        .iter()
        .position(|(_, extensions)| extensions.contains(&extension.as_str()))
}

fn build_image_pane(ui: &Ui) -> Result<(naui::Grid, naui::Image)> {
    let pane = ui.grid()?;
    pane.set_spacing(0.0, 8.0);
    pane.set_padding(Padding::all(8.0));
    pane.set_sizing(Sizing::fill());
    pane.set_column_track(0, Track::FILL);
    pane.set_row_track(0, Track::FILL);

    let image = ui.image(SAMPLE_IMAGE)?;
    image.set_alt("斜めのグラデーションと市松模様のサンプル画像");
    image.set_sizing(Sizing::fill().max_height(MEDIA_DISPLAY_HEIGHT));
    pane.attach(&image, GridCell::new(0, 0));

    let fits = [
        ("全体を表示", Fit::Contain),
        ("枠を埋める", Fit::Cover),
        ("引き伸ばす", Fit::Fill),
        ("原寸", Fit::None),
    ];
    let selector = ui.navbar("画像の収め方")?;
    selector.set_items(&NavItem::list(fits.map(|(name, _)| name)));
    selector.set_selected(0);
    selector.on_select({
        let image = image.clone();
        move |index| {
            if let Some((_, fit)) = fits.get(index) {
                image.set_fit(*fit);
            }
        }
    });
    selector.set_sizing(Sizing::fill_width());
    pane.attach(&selector, GridCell::new(0, 1));
    Ok((pane, image))
}

fn build_video_pane(ui: &Ui) -> Result<(naui::Stack, naui::Video)> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(8.0);
    pane.set_padding(Padding::all(8.0));
    pane.set_align(Align::Start);
    pane.set_sizing(Sizing::fill());

    let frame = ui.grid()?;
    frame.set_column_track(0, Track::FILL);
    frame.set_row_track(0, Track::FILL);
    frame.set_sizing(Sizing::fill().max_height(MEDIA_DISPLAY_HEIGHT));

    let video = ui.video("")?;
    video.set_sizing(Sizing::fill().max_height(MEDIA_DISPLAY_HEIGHT));
    frame.attach(&video, GridCell::new(0, 0));
    pane.append(&frame);

    let controls = ui.stack(Orientation::Vertical)?;
    controls.set_spacing(8.0);
    controls.set_sizing(Sizing::fill_width());

    let buttons = ui.stack(Orientation::Horizontal)?;
    buttons.set_spacing(8.0);
    let play = ui.button("再生")?;
    play.on_click({
        let video = video.clone();
        move || video.play()
    });
    let pause = ui.button("一時停止")?;
    pause.on_click({
        let video = video.clone();
        move || video.pause()
    });
    buttons.append(&play);
    buttons.append(&pause);
    controls.append(&buttons);

    let state = ui.label("状態: 未再生")?;
    video.on_state_change({
        let state = state.clone();
        move |value| state.set_text(&format!("状態: {}", state_name(value)))
    });
    controls.append(&state);

    let position = ui.label("再生位置: -")?;
    let seek = ui.slider(0.0, 1.0)?;
    let syncing = Rc::new(Cell::new(false));
    seek.on_change({
        let video = video.clone();
        let syncing = syncing.clone();
        move |ratio| {
            if !syncing.get() {
                if let Some(duration) = video.duration() {
                    video.seek(duration * ratio);
                }
            }
        }
    });
    video.on_position_change({
        let video = video.clone();
        let seek = seek.clone();
        let position = position.clone();
        let syncing = syncing.clone();
        move |seconds| {
            let Some(duration) = video.duration().filter(|duration| *duration > 0.0) else {
                return;
            };
            syncing.set(true);
            seek.set_value((seconds / duration).clamp(0.0, 1.0));
            syncing.set(false);
            position.set_text(&format!("再生位置: {seconds:.1} / {duration:.1} 秒"));
        }
    });
    controls.append(&position);
    controls.append(&seek);

    let volume_label = ui.label("音量: 100%")?;
    let volume = ui.slider(0.0, 1.0)?;
    volume.set_value(1.0);
    volume.on_change({
        let video = video.clone();
        let volume_label = volume_label.clone();
        move |value| {
            video.set_volume(value);
            volume_label.set_text(&format!("音量: {:.0}%", value * 100.0));
        }
    });
    controls.append(&volume_label);
    controls.append(&volume);

    let toggles = ui.stack(Orientation::Horizontal)?;
    toggles.set_spacing(12.0);
    let muted = ui.checkbox("消音")?;
    muted.on_toggle({
        let video = video.clone();
        move |on| video.set_muted(on)
    });
    let looping = ui.checkbox("繰り返し")?;
    looping.on_toggle({
        let video = video.clone();
        move |on| video.set_loop(on)
    });
    toggles.append(&muted);
    toggles.append(&looping);
    controls.append(&toggles);
    pane.append(&controls);
    Ok((pane, video))
}

fn build_audio_pane(ui: &Ui) -> Result<(naui::Stack, naui::Audio)> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(8.0);
    pane.set_padding(Padding::all(8.0));
    pane.append(&ui.label("音声を再生して内容を確認します。")?);

    let audio = ui.audio("")?;
    audio.set_sizing(Sizing::fill_width());
    pane.append(&audio);

    let buttons = ui.stack(Orientation::Horizontal)?;
    buttons.set_spacing(8.0);
    let play = ui.button("再生")?;
    play.on_click({
        let audio = audio.clone();
        move || audio.play()
    });
    let pause = ui.button("一時停止")?;
    pause.on_click({
        let audio = audio.clone();
        move || audio.pause()
    });
    buttons.append(&play);
    buttons.append(&pause);
    pane.append(&buttons);

    let state = ui.label("状態: 未再生")?;
    audio.on_state_change({
        let state = state.clone();
        move |value| state.set_text(&format!("状態: {}", state_name(value)))
    });
    pane.append(&state);
    Ok((pane, audio))
}

fn state_name(state: PlaybackState) -> &'static str {
    match state {
        PlaybackState::Idle => "未再生",
        PlaybackState::Buffering => "読み込み中",
        PlaybackState::Playing => "再生中",
        PlaybackState::Paused => "一時停止",
        PlaybackState::Ended => "再生終了",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_type_is_detected_from_paths_and_urls() {
        assert_eq!(media_form_of("/tmp/photo.PNG"), Some(0));
        assert_eq!(
            media_form_of("https://example.test/movie.mp4?token=1"),
            Some(1)
        );
        assert_eq!(media_form_of("voice.ogg#preview"), Some(2));
        assert_eq!(media_form_of("README"), None);
    }
}
