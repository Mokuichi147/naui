use std::cell::Cell;
use std::rc::Rc;

use naui::{
    Align, FileFilter, Fit, GridCell, Orientation, Padding, PlaybackState, Result, Sizing, Track,
    Ui,
};

use crate::parts::{self, Form, Notice};

/// 同梱のサンプル画像の場所。
///
/// 配布物では実行ファイルからの相対位置に置く (release.yml と packaging/)。
/// 書庫と Windows のインストーラーは exe の隣の `assets/`、macOS の `.app` は
/// `Contents/Resources/assets/`、`.deb` は `/usr/share/naui-gallery/assets/`。
/// どれも無ければ `cargo run` とみなし、ソースツリーのものを使う。
#[cfg(not(target_arch = "wasm32"))]
fn sample_image() -> String {
    const NAME: &str = "assets/sample.png";
    let bundled = std::env::current_exe().ok().and_then(|exe| {
        let dir = exe.parent()?;
        let mut bases = vec![dir.to_path_buf()];
        if let Some(parent) = dir.parent() {
            bases.push(parent.join("Resources"));
            bases.push(parent.join("share/naui-gallery"));
        }
        bases
            .into_iter()
            .map(|base| base.join(NAME))
            .find(|path| path.is_file())
    });
    match bundled {
        Some(path) => path.to_string_lossy().into_owned(),
        None => concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sample.png").to_owned(),
    }
}

#[cfg(target_arch = "wasm32")]
fn sample_image() -> String {
    "assets/sample.png".to_owned()
}

/// 表示形式と、そこへ振り分ける拡張子。
const MEDIA_FORMS: [(&str, &[&str]); 3] = [
    (
        "Image",
        &["png", "jpg", "jpeg", "gif", "bmp", "webp", "heic", "tiff"],
    ),
    ("Video", &["mp4", "m4v", "mov", "webm", "mkv", "avi"]),
    ("Audio", &["m4a", "mp3", "aac", "wav", "flac", "ogg"]),
];

const MEDIA_DISPLAY_HEIGHT: f64 = 315.0;

/// Image、Video、Audio のソース切り替えと再生操作。
///
/// 画面と一緒に、再生を止める関数を返す。**画面から外しても再生は止まらない**
/// (ネイティブのプレーヤーは表示されていなくても鳴り続ける) ので、ほかの区分へ
/// 移るときに呼ぶ。
pub(crate) fn build(ui: &Ui, notice: &Notice) -> Result<(naui::Stack, impl Fn() + 'static)> {
    let pane = parts::pane(ui)?;

    // ファイルまたは URL の拡張子から表示形式を切り替える。
    parts::section(ui, &pane, "Image / Video / Audio")?;

    let (image_pane, image) = build_image_pane(ui)?;
    let (video_pane, video) = build_video_pane(ui, notice)?;
    let (audio_pane, audio) = build_audio_pane(ui, notice)?;

    // MediaPlayerElement をタブのコンテンツにすると環境によって初期化が不安定に
    // なるため、選択中の形式だけを同じ Grid セルへ置く。
    let forms = ui.grid()?;
    forms.set_column_track(0, Track::FILL);
    forms.set_row_track(0, Track::FILL);
    forms.attach(&image_pane, GridCell::new(0, 0));

    let stop = {
        let video = video.clone();
        let audio = audio.clone();
        move || {
            video.pause();
            audio.pause();
        }
    };

    let show = Rc::new({
        let image = image.clone();
        let video = video.clone();
        let audio = audio.clone();
        let stop = stop.clone();
        let forms = forms.clone();
        let image_pane = image_pane.clone();
        let video_pane = video_pane.clone();
        let audio_pane = audio_pane.clone();
        let notice = notice.clone();
        move |form: usize, source: &str| {
            // 外すほうのプレーヤーが鳴り続けないよう、差し替える前に止める。
            stop();
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
            notice.show(&format!(
                "Showing as {}",
                MEDIA_FORMS[form].0.to_lowercase()
            ));
        }
    });

    let source = ui.text_input("")?;
    source.set_placeholder("File path or https://…");
    source.set_sizing(Sizing::fill_width());

    let load = ui.button("Load")?;
    load.on_click({
        let show = show.clone();
        let source = source.clone();
        let notice = notice.clone();
        move || {
            let value = source.text();
            if value.trim().is_empty() {
                notice.show("Enter a path or URL");
            } else if let Some(form) = media_form_of(&value) {
                show(form, &value);
            } else {
                notice.show("Unknown file extension");
            }
        }
    });

    let pick = ui.file_picker("Choose…")?;
    let extensions: Vec<&str> = MEDIA_FORMS
        .iter()
        .flat_map(|(_, extensions)| extensions.iter().copied())
        .collect();
    pick.set_filters(&[FileFilter::new("Images, videos and audio", extensions)]);
    pick.on_select({
        let show = show.clone();
        let source = source.clone();
        let notice = notice.clone();
        move |entries| {
            let Some(entry) = entries.first() else {
                return;
            };
            let Some(value) = entry.source() else {
                notice.show(&format!("Could not locate {}", entry.name()));
                return;
            };
            source.set_text(value);
            match media_form_of(entry.name()) {
                Some(form) => show(form, value),
                None => notice.show(&format!("Unknown type: {}", entry.name())),
            }
        }
    });

    let source_row = parts::row(ui)?;
    source_row.set_sizing(Sizing::fill_width());
    source_row.append(&source);
    source_row.append(&load);
    source_row.append(&pick);
    pane.append(&source_row);

    forms.set_sizing(Sizing::fill());
    pane.append(&forms);
    Ok((pane, stop))
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

    let image = ui.image(&sample_image())?;
    image.set_alt("Sample image with a diagonal gradient and a checkerboard");
    image.set_sizing(Sizing::fill().max_height(MEDIA_DISPLAY_HEIGHT));
    pane.attach(&image, GridCell::new(0, 0));

    let fits = [
        ("Contain", Fit::Contain),
        ("Cover", Fit::Cover),
        ("Fill", Fit::Fill),
        ("None", Fit::None),
    ];
    let (selector_row, selector) = parts::choice(ui, "Fit", &fits.map(|(name, _)| name))?;
    selector.on_select({
        let image = image.clone();
        move |index| {
            if let Some((_, fit)) = fits.get(index) {
                image.set_fit(*fit);
            }
        }
    });
    pane.attach(&selector_row, GridCell::new(0, 1));
    Ok((pane, image))
}

fn build_video_pane(ui: &Ui, notice: &Notice) -> Result<(naui::Stack, naui::Video)> {
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

    // 再生位置と音量は「名前 | スライダー | 値」の行に並べる。
    let controls = Form::new(ui, &pane)?;

    let buttons = parts::row(ui)?;
    let play = ui.button("Play")?;
    play.on_click({
        let video = video.clone();
        move || video.play()
    });
    let pause = ui.button("Pause")?;
    pause.on_click({
        let video = video.clone();
        move || video.pause()
    });
    buttons.append(&play);
    buttons.append(&pause);

    video.on_state_change({
        let notice = notice.clone();
        move |value| notice.show(&format!("Video: {}", state_name(value)))
    });

    let position = parts::readout(ui, "—")?;
    let seek = ui.slider(0.0, 1.0)?;
    seek.set_sizing(Sizing::fill_width());
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
    // video 自身と、video を捕まえている seek は弱参照で捕まえる。
    video.on_position_change({
        let video = video.downgrade();
        let seek = seek.downgrade();
        let position = position.clone();
        let syncing = syncing.clone();
        move |seconds| {
            let (Some(video), Some(seek)) = (video.upgrade(), seek.upgrade()) else {
                return;
            };
            let Some(duration) = video.duration().filter(|duration| *duration > 0.0) else {
                return;
            };
            syncing.set(true);
            seek.set_value((seconds / duration).clamp(0.0, 1.0));
            syncing.set(false);
            position.set_text(&format!("{seconds:.1} / {duration:.1} s"));
        }
    });

    let volume_label = parts::readout(ui, "100%")?;
    let volume = ui.slider(0.0, 1.0)?;
    volume.set_sizing(Sizing::fill_width());
    volume.set_value(1.0);
    volume.on_change({
        let video = video.clone();
        let volume_label = volume_label.clone();
        move |value| {
            video.set_volume(value);
            volume_label.set_text(&format!("{:.0}%", value * 100.0));
        }
    });

    let toggles = parts::row(ui)?;
    toggles.set_spacing(12.0);
    let muted = ui.checkbox("Mute")?;
    muted.on_toggle({
        let video = video.clone();
        move |on| video.set_muted(on)
    });
    let looping = ui.checkbox("Loop")?;
    looping.on_toggle({
        let video = video.clone();
        move |on| video.set_loop(on)
    });
    toggles.append(&muted);
    toggles.append(&looping);
    buttons.append(&toggles);
    controls.row(ui, "Playback", &buttons)?;
    controls.row_with(ui, "Position", &seek, &position)?;
    controls.row_with(ui, "Volume", &volume, &volume_label)?;
    Ok((pane, video))
}

fn build_audio_pane(ui: &Ui, notice: &Notice) -> Result<(naui::Stack, naui::Audio)> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(8.0);
    pane.set_padding(Padding::all(8.0));

    let audio = ui.audio("")?;
    audio.set_sizing(Sizing::fill_width());
    pane.append(&audio);

    let buttons = parts::row(ui)?;
    let play = ui.button("Play")?;
    play.on_click({
        let audio = audio.clone();
        move || audio.play()
    });
    let pause = ui.button("Pause")?;
    pause.on_click({
        let audio = audio.clone();
        move || audio.pause()
    });
    buttons.append(&play);
    buttons.append(&pause);
    pane.append(&buttons);

    audio.on_state_change({
        let notice = notice.clone();
        move |value| notice.show(&format!("Audio: {}", state_name(value)))
    });
    Ok((pane, audio))
}

fn state_name(state: PlaybackState) -> &'static str {
    match state {
        PlaybackState::Idle => "idle",
        PlaybackState::Buffering => "buffering",
        PlaybackState::Playing => "playing",
        PlaybackState::Paused => "paused",
        PlaybackState::Ended => "ended",
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
