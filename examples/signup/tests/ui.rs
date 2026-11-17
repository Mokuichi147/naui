//! The sign-up form's UI, tested the way a user uses it, on all four platforms.
//!
//! ```sh
//! cargo test -p signup                                     # macOS, Linux, Windows
//! cargo test -p signup --target wasm32-unknown-unknown     # the Web, in a browser
//! ```

use naui::{
    testing, Orientation, Result, Ui, DEFAULT_SIDEBAR_WIDTH, SIDEBAR_COLLAPSE_CONTENT_WIDTH,
    SIDEBAR_MIN_WIDTH,
};
use signup::{build, FORM_BREAKPOINT, PADDING};

naui::test_cases!(
    create_greets_by_name,
    create_asks_for_a_name,
    subscribing_is_mentioned,
    disabled_button_ignores_clicks,
    form_lines_up_when_wide,
    sidebar_collapses_when_narrow,
);

/// Clicking "Create account" greets the name typed in.
async fn create_greets_by_name(ui: Ui) -> Result<()> {
    let app = build(&ui)?;
    app.name.set_text("Ada");
    app.create.click();
    assert_eq!(app.status.text(), "Welcome, Ada");
    app.window.close();
    Ok(())
}

async fn create_asks_for_a_name(ui: Ui) -> Result<()> {
    let app = build(&ui)?;
    app.create.click();
    assert_eq!(app.status.text(), "Enter a name first");
    app.window.close();
    Ok(())
}

/// Clicking the check box flips it, and the greeting follows.
async fn subscribing_is_mentioned(ui: Ui) -> Result<()> {
    let app = build(&ui)?;
    app.name.set_text("Ada");
    app.subscribe.click();
    assert!(app.subscribe.is_checked());
    app.create.click();
    assert_eq!(app.status.text(), "Welcome, Ada. News is on its way");

    app.subscribe.click();
    assert!(!app.subscribe.is_checked());
    app.create.click();
    assert_eq!(app.status.text(), "Welcome, Ada");
    app.window.close();
    Ok(())
}

/// A user cannot click a disabled button, and neither can `click`.
async fn disabled_button_ignores_clicks(ui: Ui) -> Result<()> {
    let app = build(&ui)?;
    app.create.set_enabled(false);
    app.create.click();
    assert_eq!(app.status.text(), "");
    app.window.close();
    Ok(())
}

/// The form lines up its parts only when the window leaves it room.
async fn form_lines_up_when_wide(ui: Ui) -> Result<()> {
    let app = build(&ui)?;
    app.window.show();

    // 中身の幅 = 窓 - サイドバー - 両側の余白。しきい値には遊びがあるので、
    // 境目から離れた幅で確かめる。
    let beside_sidebar = |form_width: f64| form_width + DEFAULT_SIDEBAR_WIDTH + 2.0 * PADDING;
    testing::resize(
        &ui,
        &app.window,
        beside_sidebar(FORM_BREAKPOINT + 40.0),
        420.0,
    )
    .await?;
    testing::wait_until(&ui, || {
        app.form.current_orientation() == Orientation::Horizontal
    })
    .await?;

    testing::resize(
        &ui,
        &app.window,
        beside_sidebar(FORM_BREAKPOINT - 60.0),
        420.0,
    )
    .await?;
    testing::wait_until(&ui, || {
        app.form.current_orientation() == Orientation::Vertical
    })
    .await?;
    assert!(!app.sidebar.is_collapsed(), "the sidebar still fits");
    app.window.close();
    Ok(())
}

/// The sidebar makes way for the content in a narrow window, and comes back.
async fn sidebar_collapses_when_narrow(ui: Ui) -> Result<()> {
    let app = build(&ui)?;
    app.window.show();
    testing::settle(&ui).await;
    assert!(!app.sidebar.is_collapsed());

    // macOS はサイドバーを最小の幅まで縮めてから畳むので、そこより狭くする。
    // ほかの環境は既定の幅のままでも中身が足りなくなれば畳む。
    let collapses_everywhere = SIDEBAR_MIN_WIDTH + SIDEBAR_COLLAPSE_CONTENT_WIDTH - 60.0;
    testing::resize(&ui, &app.window, collapses_everywhere, 420.0).await?;
    testing::wait_until(&ui, || app.sidebar.is_collapsed()).await?;

    testing::resize(&ui, &app.window, 760.0, 420.0).await?;
    testing::wait_until(&ui, || !app.sidebar.is_collapsed()).await?;
    app.window.close();
    Ok(())
}
