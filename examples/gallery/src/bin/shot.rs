//! ギャラリーを 4 スタイル × ライト / ダークで描画し、BMP に書き出す。
//!
//! ```sh
//! cargo run -p gallery --bin shot -- ./shots
//! ```

use miui::core::theme::{ColorMode, PlatformStyle};
use miui::headless::{to_bmp, Headless};

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "shots".into());
    std::fs::create_dir_all(&dir).expect("出力ディレクトリを作成できません");

    let app = gallery::Gallery::default();
    let mut headless = Headless::new();
    let scale = 2.0f32;
    let (w, h) = (900u32, 780u32);

    for style in [
        PlatformStyle::Fluent,
        PlatformStyle::Cupertino,
        PlatformStyle::Adwaita,
        PlatformStyle::Web,
    ] {
        for mode in [ColorMode::Light, ColorMode::Dark] {
            let theme = miui::theme::for_style(style, mode);
            let buffer = headless.render(
                &app,
                &theme,
                (w as f32 * scale) as u32,
                (h as f32 * scale) as u32,
                scale,
            );
            let name = format!(
                "{dir}/{}-{}.bmp",
                match style {
                    PlatformStyle::Fluent => "fluent",
                    PlatformStyle::Cupertino => "macos",
                    PlatformStyle::Adwaita => "adwaita",
                    PlatformStyle::Web => "web",
                },
                if mode.is_dark() { "dark" } else { "light" }
            );
            let bmp = to_bmp(
                &buffer,
                (w as f32 * scale) as u32,
                (h as f32 * scale) as u32,
            );
            std::fs::write(&name, bmp).expect("BMP を書き出せません");
            println!("{name}");
        }
    }
}
