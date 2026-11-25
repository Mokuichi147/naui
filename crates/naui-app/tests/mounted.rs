//! A [`Mounted`] component goes into a container as it is, through naui's
//! `AsWidget`, and the common methods reach its root widget. Run with
//! `naui::testing` on every platform's real event loop.
//!
//! ```sh
//! cargo test -p naui-app --test mounted                                   # macOS, Linux, Windows
//! cargo test -p naui-app --test mounted --target wasm32-unknown-unknown   # the Web, in a browser
//! ```

use std::convert::Infallible;

use naui::{testing, AsWidget, Canvas, Orientation, Result, Sizing, Ui, Widget};
use naui_app::{App, Component, Dispatch, Effect, Mounted, Program};

naui::test_cases!(a_mounted_component_goes_into_a_container);

/// A component whose view is a `Canvas`: its size stays zero until it is laid out
/// in a shown window.
struct Probe;

impl Program for Probe {
    type Message = ();
    type View = Canvas;

    fn update(&mut self, _: ()) -> Effect<()> {
        Effect::none()
    }

    fn render(&self, _: &Canvas) {}
}

impl Component for Probe {
    fn mount(ui: &Ui, _: &Dispatch<()>) -> naui::Result<Canvas> {
        let canvas = ui.canvas()?;
        canvas.set_sizing(Sizing::fixed(40.0, 16.0));
        Ok(canvas)
    }

    fn root(view: &Canvas) -> &dyn Widget {
        view
    }
}

/// The parent: an app with no view of its own, to hand out a dispatch.
struct Parent;

impl Program for Parent {
    type Message = ();
    type View = ();

    fn update(&mut self, _: ()) -> Effect<()> {
        Effect::none()
    }

    fn render(&self, _: &()) {}
}

/// `stack.append(&mounted)` places the component's root, and `set_sizing` and
/// `set_visible` on the `Mounted` reach it.
async fn a_mounted_component_goes_into_a_container(ui: Ui) -> Result<()> {
    let window = ui.window("mounted", 480.0, 320.0)?;
    window.show();
    let stack = ui.stack(Orientation::Vertical)?;
    window.set_child(&stack);
    let app = App::new(Parent, |_| Ok::<_, Infallible>(())).unwrap();
    let mounted = Mounted::<Probe>::new(&ui, &app.dispatch(), |message| message)?;

    stack.append(&mounted);
    assert_eq!(stack.len(), 1);
    let canvas = mounted.view().clone();
    testing::wait_until(&ui, || canvas.size() == (40.0, 16.0)).await?;

    // 共通のメソッドは root へ届く。
    mounted.set_sizing(Sizing::fixed(60.0, 24.0));
    testing::wait_until(&ui, || canvas.size() == (60.0, 24.0)).await?;
    mounted.set_visible(false);
    assert!(!canvas.is_visible());
    assert!(!mounted.is_visible());
    mounted.set_visible(true);

    mounted.unmount(|_root| stack.remove(0));
    assert_eq!(stack.len(), 0);
    window.close();
    Ok(())
}
