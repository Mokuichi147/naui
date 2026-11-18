//! A sign-up form whose UI is tested on all four platforms (`tests/ui.rs`).
//!
//! The UI is built in a library function, [`build`], that returns the widgets the
//! tests operate. `main.rs` only shows the window it builds. The tests type and click
//! the way a user does, answer the dialog that asks before clearing the form, and
//! resize the window to check the adaptive layout: the form lines up its parts when
//! there is room, and the sidebar collapses when the window is narrow.

use naui::{
    Breakpoint, Button, Checkbox, Dialog, DialogButtons, DialogResponse, Label, Orientation,
    Padding, Result, Sidebar, SidebarItem, Sizing, Stack, TextInput, Ui, Window,
};

/// The width the form needs to put its parts side by side.
pub const FORM_BREAKPOINT: f64 = 420.0;

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
    /// Follows the name as it is typed.
    pub preview: Label,
    pub subscribe: Checkbox,
    pub create: Button,
    /// Clears the form after asking with [`Signup::confirm_clear`].
    pub clear: Button,
    /// Asks before clearing the form.
    pub confirm_clear: Dialog,
    pub status: Label,
}

/// What the status line says once the form has been cleared.
pub const CLEARED: &str = "The form is cleared";

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
    let clear = ui.button("Clear")?;
    form.append(&name);
    form.append(&subscribe);
    form.append(&create);
    form.append(&clear);

    let confirm_clear = ui.dialog("Clear the form?")?;
    confirm_clear.set_message("The name you typed will be lost.");
    confirm_clear.set_buttons(DialogButtons::new().primary("Clear").cancel("Keep"));

    let preview = ui.label("")?;
    let status = ui.label("")?;
    root.append(&form);
    root.append(&preview);
    root.append(&status);
    window.set_child(&root);

    let create_account = {
        let name = name.clone();
        let subscribe = subscribe.clone();
        let status = status.clone();
        move || status.set_text(&greeting(&name.text(), subscribe.is_checked()))
    };
    create.on_click(create_account.clone());
    // Enter in the name field does the same as the button.
    name.on_submit({
        let create_account = create_account.clone();
        move |_| create_account()
    });
    name.on_change({
        let preview = preview.clone();
        move |text| preview.set_text(&signing_up_as(text))
    });

    clear.on_click({
        let confirm_clear = confirm_clear.clone();
        move || confirm_clear.open()
    });
    confirm_clear.on_response({
        let name = name.clone();
        let preview = preview.clone();
        let subscribe = subscribe.clone();
        let status = status.clone();
        move |response| {
            if response == DialogResponse::Primary {
                name.set_text("");
                preview.set_text("");
                subscribe.set_checked(false);
                status.set_text(CLEARED);
            }
        }
    });

    Ok(Signup {
        window,
        sidebar,
        form,
        name,
        preview,
        subscribe,
        create,
        clear,
        confirm_clear,
        status,
    })
}

/// What the line below the form says while a name is typed.
pub fn signing_up_as(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        String::new()
    } else {
        format!("Signing up as {name}")
    }
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
