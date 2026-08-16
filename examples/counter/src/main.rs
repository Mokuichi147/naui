//! naui の最小サンプル。ネイティブのボタンとラベルを 1 つずつ使う。
//!
//! ```sh
//! cargo run -p counter
//! ```

use std::cell::Cell;
use std::rc::Rc;

use naui::{Orientation, Padding, Settings};

fn main() -> naui::Result<()> {
    naui::run(Settings::new("counter"), |ui| {
        let window = ui.window("counter", 320.0, 200.0)?;

        let stack = ui.stack(Orientation::Vertical)?;
        stack.set_spacing(12.0);
        stack.set_padding(Padding::all(24.0));

        let label = ui.label("0")?;
        let row = ui.stack(Orientation::Horizontal)?;
        row.set_spacing(8.0);

        let count = Rc::new(Cell::new(0i32));
        let decrement = ui.button("−")?;
        let increment = ui.button("＋")?;

        // ラベルとカウンタを共有して、押されたら書き換える。
        let update = {
            let label = label.clone();
            let count = count.clone();
            move |delta: i32| {
                count.set(count.get() + delta);
                label.set_text(&count.get().to_string());
            }
        };
        decrement.on_click({
            let update = update.clone();
            move || update(-1)
        });
        increment.on_click({
            let update = update.clone();
            move || update(1)
        });

        row.append(&decrement);
        row.append(&increment);
        stack.append(&label);
        stack.append(&row);

        window.set_child(&stack);
        window.show();
        Ok(())
    })
}
