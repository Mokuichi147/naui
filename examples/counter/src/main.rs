//! miui の最小サンプル。
//!
//! ```sh
//! cargo run -p counter
//! ```

use miui::prelude::*;

#[derive(Default)]
struct Counter {
    count: i32,
}

#[derive(Clone)]
enum Msg {
    Increment,
    Decrement,
    Reset,
}

impl Application for Counter {
    type Message = Msg;

    fn view(&self) -> Element<Msg> {
        Element::new(
            column()
                .spacing(16.0)
                .padding(Insets::all(32.0))
                .justify(MainAxis::Center)
                .align(CrossAxis::Center)
                .child(Text::new(self.count.to_string()).title())
                .child(
                    row()
                        .spacing(8.0)
                        .child(
                            Button::new("−")
                                .min_width(64.0)
                                .on_press(Msg::Decrement)
                                .enabled(self.count > 0),
                        )
                        .child(Button::new("＋").min_width(64.0).accent().on_press(Msg::Increment)),
                )
                .child(
                    Button::new("リセット")
                        .subtle()
                        .on_press(Msg::Reset)
                        .enabled(self.count != 0),
                ),
        )
    }

    fn update(&mut self, message: Msg) {
        match message {
            Msg::Increment => self.count += 1,
            Msg::Decrement => self.count -= 1,
            Msg::Reset => self.count = 0,
        }
    }

    fn title(&self) -> Option<String> {
        Some(format!("counter — {}", self.count))
    }
}

fn main() {
    miui::run(Counter::default(), Settings::new("counter").size(360.0, 300.0));
}
