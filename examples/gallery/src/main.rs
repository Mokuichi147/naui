// Windows のリリースビルドでは、起動時にコンソール画面を出さない。
// デバッグビルドは cargo run のログが見えるよう、コンソールのままにする。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> naui::Result<()> {
    gallery::start()
}
