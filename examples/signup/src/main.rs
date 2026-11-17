//! A sign-up form. Its UI is tested in `tests/ui.rs`:
//!
//! ```sh
//! cargo run -p signup
//! cargo test -p signup
//! ```

// Windows のリリースビルドでは、起動時にコンソール画面を出さない。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> naui::Result<()> {
    naui::run(naui::Settings::new("signup"), |ui| {
        let app = signup::build(ui)?;
        app.window.show();
        Ok(())
    })
}
