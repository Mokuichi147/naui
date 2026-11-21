//! naui-charts の例。折れ線・棒・散布図と、10 万点の折れ線を並べる。
//!
//! ```sh
//! cargo run -p charts
//! ```

// Windows のリリースビルドでは、起動時にコンソール画面を出さない。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> naui::Result<()> {
    charts::start()
}
