//! A sign-up form whose UI is tested on all four platforms (`tests/ui.rs`).
//!
//! The UI is built in a library function, [`build`], that returns the widgets the
//! tests operate. `main.rs` only shows the window it builds. The tests click the
//! buttons the way a user does and resize the window to check the adaptive layout:
//! the form lines up its parts when there is room, and the sidebar collapses when
//! the window is narrow.

use naui::{
    Breakpoint, Button, Checkbox, Label, Orientation, Padding, Result, Sidebar, SidebarItem,
    Sizing, Stack, TextInput, Ui, Window,
};

/// The width the form needs to put its parts side by side.
pub const FORM_BREAKPOINT: f64 = 480.0;

/// The padding around the form.
pub const PADDING: f64 = 16.0;

/// The widgets the app is made of.
pub struct Signup {
    pub window: Window,
    pub sidebar: Sidebar,
    /// Stacks its parts vertically, and lines them up when it is at least
    /// [`FORM_BREAKPOINT`] wide.
    pub form: Stack,
    pub name: TextInput,
    pub subscribe: Checkbox,
    pub create: Button,
    pub status: Label,
}

/// Builds the window and its contents. The window is not shown yet.
pub fn build(ui: &Ui) -> Result<Signup> {
    let window = ui.window("Sign up", 760.0, 420.0)?;
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["Account", "Preferences"]));
    sidebar.set_selected(0);
    window.set_sidebar(&sidebar);

    let root = ui.stack(Orientation::Vertical)?;
    root.set_sizing(Sizing::fill());
    root.set_padding(Padding::all(PADDING));
    root.set_spacing(12.0);

    let form = ui.stack(Orientation::Vertical)?;
    form.set_sizing(Sizing::fill_width());
    form.set_spacing(8.0);
    form.set_breakpoint(Some(Breakpoint::new(
        FORM_BREAKPOINT,
        Orientation::Horizontal,
    )));
    let name = ui.text_input("")?;
    name.set_placeholder("Name");
    let subscribe = ui.checkbox("Send me news")?;
    let create = ui.button("Create account")?;
    form.append(&name);
    form.append(&subscribe);
    form.append(&create);

    let status = ui.label("")?;
    root.append(&form);
    root.append(&status);
    window.set_child(&root);

    create.on_click({
        let name = name.clone();
        let subscribe = subscribe.clone();
        let status = status.clone();
        move || status.set_text(&greeting(&name.text(), subscribe.is_checked()))
    });

    Ok(Signup {
        window,
        sidebar,
        form,
        name,
        subscribe,
        create,
        status,
    })
}

/// What the status line says after "Create account".
pub fn greeting(name: &str, subscribed: bool) -> String {
    let name = name.trim();
    if name.is_empty() {
        return "Enter a name first".to_string();
    }
    if subscribed {
        format!("Welcome, {name}. News is on its way")
    } else {
        format!("Welcome, {name}")
    }
}
