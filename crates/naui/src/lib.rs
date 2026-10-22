#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub use naui_core::{
    accept_attribute, clamp_split_position, days_in_month, default_extension, is_leap_year, media,
    with_default_extension, Align, CloseResponse, Color, DatePickerMode, DateTime, DialogButtons,
    DialogResponse, DrawCommand, Error, EventResponse, FileEntry, FileFilter, FilePickerMode, Fit,
    GridCell, Key, KeyEvent, Length, ListItem, MenuItem, MenuShortcut, MenuSpec, Modifiers,
    NavItem, NumberSpec, Orientation, Padding, Painter, Path, PathSegment, PlaybackState, Point,
    PointerButton, PointerEvent, PointerPhase, PopupItem, Rect, Result, ScrollMetrics,
    ScrollPolicy, SelectionMode, Sender, Settings, SidebarItem, SidebarSection, Sizing, Sleep,
    SortOrder, TableColumn, TableRow, Task, Tasks, TextColor, TextStyle, Theme, Time, Timer,
    ToastSpec, ToolbarIcon, ToolbarItem, Track, TreeItem, WeakWidget, WindowState,
    DEFAULT_SIDEBAR_WIDTH, DEFAULT_SPLIT_POSITION, ROW_WINDOW_THRESHOLD, SIDEBAR_MIN_WIDTH,
    WHEEL_STEP,
};

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub use naui_macos::{
    run, Audio, Breadcrumbs, Button, Canvas, Checkbox, Clipboard, ColorPicker, ComboBox,
    DatePicker, Dialog, Dock, EditableComboBox, Expander, FilePicker, FileSaver, Grid, Image,
    Label, Link, List, ListRow, Menu, MenuBar, Navbar, NumberInput, Pagination, PasswordInput,
    PopupMenu, ProgressBar, RadioGroup, Scroll, SearchInput, Sidebar, Slider, Spacer, SplitView,
    Stack, Table, TableCells, Tabs, TextArea, TextInput, TimePicker, Toast, Toggle, Toolbar, Tree,
    Ui, Video, WeakWindow, Widget, Window,
};
#[cfg(target_arch = "wasm32")]
pub use naui_web::{
    run, Audio, Breadcrumbs, Button, Canvas, Checkbox, Clipboard, ColorPicker, ComboBox,
    DatePicker, Dialog, Dock, EditableComboBox, Expander, FilePicker, FileSaver, Grid, Image,
    Label, Link, List, ListRow, Menu, MenuBar, Navbar, NumberInput, Pagination, PasswordInput,
    PopupMenu, ProgressBar, RadioGroup, Scroll, SearchInput, Sidebar, Slider, Spacer, SplitView,
    Stack, Table, TableCells, Tabs, TextArea, TextInput, TimePicker, Toast, Toggle, Toolbar, Tree,
    Ui, Video, WeakWindow, Widget, Window,
};
#[cfg(all(not(target_arch = "wasm32"), target_os = "windows"))]
pub use naui_windows::{
    run, Audio, Breadcrumbs, Button, Canvas, Checkbox, Clipboard, ColorPicker, ComboBox,
    DatePicker, Dialog, Dock, EditableComboBox, Expander, FilePicker, FileSaver, Grid, Image,
    Label, Link, List, ListRow, Menu, MenuBar, Navbar, NumberInput, Pagination, PasswordInput,
    PopupMenu, ProgressBar, RadioGroup, Scroll, SearchInput, Sidebar, Slider, Spacer, SplitView,
    Stack, Table, TableCells, Tabs, TextArea, TextInput, TimePicker, Toast, Toggle, Toolbar, Tree,
    Ui, Video, WeakWindow, Widget, Window,
};

#[cfg(all(
    not(target_arch = "wasm32"),
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
pub use naui_gtk::{
    run, Audio, Breadcrumbs, Button, Canvas, Checkbox, Clipboard, ColorPicker, ComboBox,
    DatePicker, Dialog, Dock, EditableComboBox, Expander, FilePicker, FileSaver, Grid, Image,
    Label, Link, List, ListRow, Menu, MenuBar, Navbar, NumberInput, Pagination, PasswordInput,
    PopupMenu, ProgressBar, RadioGroup, Scroll, SearchInput, Sidebar, Slider, Spacer, SplitView,
    Stack, Table, TableCells, Tabs, TextArea, TextInput, TimePicker, Toast, Toggle, Toolbar, Tree,
    Ui, Video, WeakWindow, Widget, Window,
};

pub mod guide {
    //! 使い方の手引き。レイアウト、ウィジェットごとの振る舞いと環境の違い、
    //! 別スレッドと非同期をまとめている。
    #![doc = include_str!("guide.md")]

    // 手引きの中の `[`Ui`]` のようなリンクを、クレートの直下の名前で引くため。
    #[allow(unused_imports)]
    use crate::*;
}

/// Web だけの DOM 操作 (CSS のクラスと属性)。
///
/// ほかの環境に対応物が無いので、Web 向けにビルドしたときだけある。
/// 使うところは `#[cfg(target_arch = "wasm32")]` で囲う。
///
/// ```ignore
/// #[cfg(target_arch = "wasm32")]
/// {
///     use naui::web::WebWidgetExt;
///     button.add_class("primary");                 // 文書側の CSS で見た目を変える
///     button.set_attribute("data-testid", "save"); // テスト用の目印
/// }
/// ```
///
/// `style` と `data-naui-*` は naui が使うので、`set_attribute` /
/// `remove_attribute` では書き換えられない (`false` が返る)。`title` は
/// `set_tooltip` が書く属性と同じなので、後から書いたほうが残る。
#[cfg(target_arch = "wasm32")]
pub mod web {
    pub use naui_web::WebWidgetExt;
}

/// `entry!` が使う wasm-bindgen の再公開。直接使うものではない。
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub use naui_web::wasm_bindgen;

/// ネイティブと Web の入口をまとめて作る。
///
/// アプリの起動処理は環境ごとに形が違う。ネイティブは `main` から呼ぶだけだが、
/// Web はブラウザから呼ばれる関数を `#[wasm_bindgen(start)]` で公開する必要が
/// ある。このマクロが両方を作るので、**アプリ側に `cfg` を書かずに済む**
/// (wasm-bindgen への依存も要らない)。
///
/// ```ignore
/// naui::entry!(naui::Settings::new("gallery"), build);
///
/// fn build(ui: &naui::Ui) -> naui::Result<()> { /* ... */ Ok(()) }
/// ```
///
/// 展開されるのは次の 2 つ。
///
/// - `pub fn start() -> Result<()>` — どの環境でもある起動処理。ネイティブの
///   `main` からはこれを呼ぶ。
/// - Web だけ: ブラウザが読み込み後に呼ぶ入口。失敗は JS の例外として投げるので、
///   ブラウザのコンソールに理由が出る。
#[macro_export]
macro_rules! entry {
    ($settings:expr, $build:expr $(,)?) => {
        /// ネイティブ / Web 共通の起動処理。
        pub fn start() -> $crate::Result<()> {
            $crate::run($settings, $build)
        }

        #[cfg(target_arch = "wasm32")]
        #[doc(hidden)]
        mod __naui_entry {
            // wasm-bindgen が展開するコードは `wasm_bindgen::` を参照するので、
            // naui が再公開しているものをその名前で見えるようにする。
            use $crate::wasm_bindgen;

            /// 読み込みが終わったブラウザが呼ぶ入口。
            #[wasm_bindgen::prelude::wasm_bindgen(start)]
            pub fn start() -> ::core::result::Result<(), wasm_bindgen::JsValue> {
                super::start().map_err(|e| wasm_bindgen::JsValue::from_str(&e.to_string()))
            }
        }
    };
}

/// バックエンド間で API がずれていないことを、コンパイル時に検査する。
///
/// バックエンドは別々のクレートなので、シグネチャの食い違いは型検査でしか
/// 捕まえられない。この関数はどのターゲットでもコンパイルされ、公開 API を
/// 一通り呼ぶ。**実行はされない。**
#[doc(hidden)]
#[allow(dead_code)]
fn __api_contract(ui: &Ui) -> Result<()> {
    // `Ui` は clone できる。コールバックの中でウィジェットを作るために、
    // 4 バックエンドすべてで同じ形にそろえてある。
    let ui: &Ui = &ui.clone();

    let window: Window = ui.window("t", 100.0, 100.0)?;
    window.set_title("t");
    let _: String = window.title();
    window.set_size(1.0, 1.0);
    window.show();
    window.close();
    let _: bool = window.is_visible();
    window.set_theme(Theme::Dark)?;
    window.on_key_down(|_event: &KeyEvent| EventResponse::Handled);
    window.on_close_request(|| CloseResponse::KeepOpen);
    window.set_min_size(1.0, 1.0);
    let _: (f64, f64) = window.size();
    window.on_resize(|(_width, _height): (f64, f64)| {});
    window.set_position(1.0, 1.0);
    let _: Option<(f64, f64)> = window.position();
    window.set_state(WindowState::Maximized);
    let _: WindowState = window.state();
    window.on_state_change(|_state: WindowState| {});
    window.set_resizable(false);
    let _: bool = window.is_resizable();
    let weak_window = window.downgrade();
    let _: Option<Window> = weak_window.upgrade();

    let _: Theme = ui.theme();
    ui.set_theme(Theme::Dark)?;
    ui.set_theme(Theme::System)?;

    let stack: Stack = ui.stack(Orientation::Vertical)?;
    stack.set_spacing(1.0);
    stack.set_padding(Padding::all(1.0));
    stack.set_align(Align::Center);
    stack.set_sizing(Sizing::fill());
    let _: usize = stack.len();
    let _: bool = stack.is_empty();
    stack.insert(0, &ui.label("t")?);
    stack.remove(0);
    stack.clear();

    // --- レイアウト -------------------------------------------------------
    let grid: Grid = ui.grid()?;
    grid.set_spacing(1.0, 1.0);
    grid.set_padding(Padding::all(1.0));
    grid.set_column_track(0, Track::Fixed(1.0));
    grid.set_row_track(0, Track::FILL);
    grid.set_sizing(Sizing::AUTO);
    let _: usize = grid.columns();
    let _: usize = grid.rows();
    let _: usize = grid.len();
    let _: bool = grid.is_empty();
    grid.replace(&ui.label("t")?, GridCell::new(0, 0));
    grid.remove(GridCell::new(0, 0));
    grid.clear();

    let scroll: Scroll = ui.scroll()?;
    scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
    scroll.set_sizing(Sizing::new().width(Length::Fill).min_height(1.0));
    let metrics: ScrollMetrics = scroll.metrics();
    let _: f64 = metrics.distance_to_end();
    scroll.scroll_to(0.0, 1.0);
    scroll.scroll_to_end();
    scroll.on_scroll(|_metrics: ScrollMetrics| {});

    let expander: Expander = ui.expander("t")?;
    let _: String = expander.text();
    expander.set_text("t");
    let _: bool = expander.is_expanded();
    expander.set_expanded(true);
    expander.set_enabled(true);
    expander.on_toggle(|_expanded: bool| {});
    expander.set_sizing(Sizing::fill_width());

    let split_view: SplitView = ui.split_view(Orientation::Horizontal)?;
    let _: Orientation = split_view.orientation();
    let _: f64 = split_view.position();
    split_view.set_position(1.0);
    split_view.drag_to(1.0);
    split_view.set_min_sizes(1.0, 1.0);
    split_view.on_resize(|_position: f64| {});
    split_view.set_sizing(Sizing::fill());

    let spacer: Spacer = ui.spacer()?;
    spacer.set_sizing(Sizing::fill_height());

    let label: Label = ui.label("t")?;
    let _: String = label.text();
    label.set_text("t");
    label.set_wrap(true);
    label.set_style(TextStyle::Title);
    label.set_color(TextColor::Danger);
    label.set_selectable(true);
    let _: bool = label.is_selectable();

    let button: Button = ui.button("t")?;
    button.set_text("t");
    button.set_enabled(true);
    button.on_click(|| {});
    #[cfg(target_arch = "wasm32")]
    {
        use crate::web::WebWidgetExt;
        button.add_class("t");
        button.remove_class("t");
        let _: bool = button.has_class("t");
        let _: bool = button.set_attribute("data-t", "t");
        let _: bool = button.remove_attribute("data-t");
        let _: Option<String> = button.attribute("data-t");
    }

    // どのウィジェットにもある操作。手書きの Widget 実装 (macOS の Stack /
    // TextArea / List など) も漏れないよう、何種類かで呼ぶ。
    button.set_visible(false);
    let _: bool = button.is_visible();
    let _: bool = button.request_focus();
    button.set_tooltip(Some("t"));
    button.set_tooltip(None);
    button.set_accessible_label(Some("t"));
    button.set_accessible_label(None);
    let any_stack: Stack = ui.stack(Orientation::Vertical)?;
    any_stack.set_visible(true);
    let _: bool = any_stack.request_focus();
    any_stack.set_tooltip(None);
    let any_area: TextArea = ui.text_area("")?;
    any_area.set_visible(true);
    let _: bool = any_area.request_focus();
    any_area.set_tooltip(None);
    let any_list: List = ui.list()?;
    any_list.set_visible(true);
    let _: bool = any_list.is_visible();
    any_list.set_tooltip(None);

    let checkbox: Checkbox = ui.checkbox("t")?;
    let _: bool = checkbox.is_checked();
    checkbox.set_checked(true);
    checkbox.set_enabled(true);
    checkbox.on_toggle(|_v: bool| {});

    let toggle: Toggle = ui.toggle("t")?;
    let _: bool = toggle.is_on();
    toggle.set_on(true);
    toggle.set_enabled(true);
    toggle.on_toggle(|_v: bool| {});
    toggle.set_sizing(Sizing::fill_width());

    let time_picker: TimePicker = ui.time_picker()?;
    let _: Time = time_picker.value();
    time_picker.set_value(Time::new(7, 30));
    time_picker.set_range(Some(Time::new(9, 0)), Some(Time::new(18, 0)));
    time_picker.set_range(None, None);
    time_picker.set_enabled(true);
    time_picker.on_change(|_value: Time| {});
    time_picker.set_sizing(Sizing::fill_width());

    let color_picker: ColorPicker = ui.color_picker()?;
    let _: Color = color_picker.value();
    color_picker.set_value(Color::rgb(1, 2, 3));
    color_picker.pick(Color::WHITE);
    color_picker.set_enabled(true);
    color_picker.on_change(|_value: Color| {});
    color_picker.set_sizing(Sizing::fill_width());

    let canvas: Canvas = ui.canvas()?;
    canvas.on_draw(|painter: &mut Painter| {
        let _: f64 = painter.width();
        let _: f64 = painter.height();
        let _: Rect = painter.bounds();
        painter.set_opacity(0.5);
        painter.set_dash(&[4.0, 2.0]);
        painter.set_text_align(Align::Center);
        painter.fill_rect(Rect::new(0.0, 0.0, 1.0, 1.0), Color::BLACK);
        painter.stroke_rect(Rect::new(0.0, 0.0, 1.0, 1.0), Color::BLACK, 1.0);
        painter.fill_circle(Point::new(0.0, 0.0), 1.0, Color::BLACK);
        painter.stroke_circle(Point::new(0.0, 0.0), 1.0, Color::BLACK, 1.0);
        painter.line(
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Color::BLACK,
            1.0,
        );
        painter.polyline(&[Point::new(0.0, 0.0)], Color::BLACK, 1.0);
        painter.fill_polygon(&[Point::new(0.0, 0.0)], Color::BLACK);
        let path: Path = Path::new()
            .move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(1.0, 0.0))
            .quad_to(Point::new(1.0, 1.0), Point::new(0.0, 1.0))
            .cubic_to(
                Point::new(0.0, 0.0),
                Point::new(1.0, 1.0),
                Point::new(0.0, 0.0),
            )
            .arc(Point::new(0.0, 0.0), 1.0, 0.0, 1.0)
            .close();
        painter.fill_path(&path, Color::BLACK);
        painter.stroke_path(&path, Color::BLACK, 1.0);
        painter.text("t", Point::new(0.0, 0.0), 12.0, Color::BLACK);
        let _: &[DrawCommand] = painter.commands();
    });
    canvas.on_pointer(|event: PointerEvent| {
        let _: PointerPhase = event.phase;
        let _: Point = event.point;
        let _: Option<PointerButton> = event.button;
    });
    canvas.on_wheel(|event: PointerEvent| {
        let _: f64 = event.wheel_x;
        let _: f64 = event.wheel_y;
        let _ = (
            PointerPhase::Wheel,
            PointerButton::Primary,
            PointerButton::Secondary,
        );
        let _ = (
            PointerButton::Middle,
            PointerButton::Back,
            PointerButton::Forward,
        );
        let _: f64 = WHEEL_STEP;
        EventResponse::Handled
    });
    let _: PointerEvent = PointerEvent::new(PointerPhase::Down, Point::new(0.0, 0.0))
        .button(Some(PointerButton::Primary));
    let _: PointerEvent = PointerEvent::wheel(Point::new(0.0, 0.0), 0.0, WHEEL_STEP);
    canvas.redraw();
    let _: (f64, f64) = canvas.size();
    canvas.set_sizing(Sizing::fill());

    let combo_box: ComboBox = ui.combo_box()?;
    combo_box.set_items(&["a", "b"]);
    let _: usize = combo_box.len();
    let _: bool = combo_box.is_empty();
    let _: Option<usize> = combo_box.selected();
    combo_box.set_selected(0);
    combo_box.clear_selection();
    combo_box.select(1);
    combo_box.set_enabled(true);
    combo_box.on_select(|_index: usize| {});
    combo_box.set_sizing(Sizing::fill_width());

    let editable_combo_box: EditableComboBox = ui.editable_combo_box()?;
    editable_combo_box.set_items(&["a", "b"]);
    let _: usize = editable_combo_box.len();
    let _: bool = editable_combo_box.is_empty();
    let _: String = editable_combo_box.text();
    editable_combo_box.set_text("t");
    let _: Option<usize> = editable_combo_box.selected();
    editable_combo_box.set_selected(0);
    editable_combo_box.clear();
    editable_combo_box.select(1);
    editable_combo_box.set_placeholder("t");
    editable_combo_box.set_enabled(true);
    editable_combo_box.on_change(|_text: &str| {});
    editable_combo_box.set_sizing(Sizing::fill_width());

    let radio_group: RadioGroup = ui.radio_group()?;
    radio_group.set_items(&["a", "b"]);
    let _: usize = radio_group.len();
    let _: bool = radio_group.is_empty();
    let _: Option<usize> = radio_group.selected();
    radio_group.set_selected(0);
    radio_group.clear_selection();
    radio_group.select(1);
    radio_group.set_orientation(Orientation::Horizontal);
    radio_group.set_enabled(true);
    radio_group.on_select(|_index: usize| {});
    radio_group.set_sizing(Sizing::fill_width());

    let date_picker: DatePicker = ui.date_picker(DatePickerMode::Date)?;
    let _: DatePickerMode = date_picker.mode();
    let _: DateTime = date_picker.value();
    date_picker.set_value(DateTime::date(2026, 8, 22));
    date_picker.set_range(Some(DateTime::date(2026, 1, 1)), None);
    date_picker.set_range(None, None);
    date_picker.set_enabled(true);
    date_picker.on_change(|_value: DateTime| {});
    date_picker.set_sizing(Sizing::fill_width());
    let _: DatePicker = ui.date_picker(DatePickerMode::Time)?;
    let _: DatePicker = ui.date_picker(DatePickerMode::DateTime)?;

    let input: TextInput = ui.text_input("t")?;
    let _: String = input.text();
    input.set_text("t");
    input.set_placeholder("t");
    input.set_enabled(true);
    input.on_change(|_s: &str| {});
    input.on_submit(|_s: &str| {});
    input.set_read_only(true);
    let _: bool = input.is_read_only();

    let text_area: TextArea = ui.text_area("t")?;
    let _: String = text_area.text();
    text_area.set_text("t\nt");
    text_area.set_placeholder("t");
    text_area.set_enabled(true);
    text_area.on_change(|_s: &str| {});
    text_area.set_read_only(true);
    let _: bool = text_area.is_read_only();
    text_area.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(1.0)));

    let password: PasswordInput = ui.password_input()?;
    let _: String = password.text();
    password.set_text("t");
    password.set_placeholder("t");
    password.set_enabled(true);
    password.on_change(|_s: &str| {});
    password.on_submit(|_s: &str| {});
    password.set_sizing(Sizing::fill_width());

    let search: SearchInput = ui.search_input()?;
    let _: String = search.text();
    search.set_text("t");
    search.set_placeholder("t");
    search.set_enabled(true);
    search.on_change(|_s: &str| {});
    search.on_search(|_s: &str| {});
    search.set_sizing(Sizing::fill_width());

    let number: NumberInput = ui.number_input(1.0)?;
    let _: f64 = number.value();
    let _: NumberSpec = number.spec();
    number.set_value(2.0);
    number.set_range(Some(0.0), Some(10.0));
    number.set_range(None, None);
    number.set_step(0.5);
    number.set_decimals(2);
    number.set_enabled(true);
    number.on_change(|_v: f64| {});
    number.on_submit(|_v: f64| {});
    number.set_sizing(Sizing::fill_width());

    // 文字入力系は 6 つとも on_key_down を持つ。
    let on_key = |event: &KeyEvent| {
        let _: Key = event.key;
        let _: Modifiers = event.modifiers;
        let _: bool = event.modifiers.primary();
        let _: bool = event.repeat;
        EventResponse::Continue
    };
    input.on_key_down(on_key);
    text_area.on_key_down(on_key);
    password.on_key_down(on_key);
    search.on_key_down(on_key);
    number.on_key_down(on_key);
    editable_combo_box.on_key_down(on_key);

    // 同じ 6 つが on_focus_change も持つ。
    let on_focus = |_focused: bool| {};
    input.on_focus_change(on_focus);
    text_area.on_focus_change(on_focus);
    password.on_focus_change(on_focus);
    search.on_focus_change(on_focus);
    number.on_focus_change(on_focus);
    editable_combo_box.on_focus_change(on_focus);

    let slider: Slider = ui.slider(0.0, 1.0)?;
    let _: f64 = slider.value();
    slider.set_value(0.5);
    slider.set_enabled(true);
    slider.on_change(|_v: f64| {});

    let progress: ProgressBar = ui.progress_bar()?;
    let _: f64 = progress.value();
    progress.set_value(0.5);
    progress.set_indeterminate(true);
    let _: bool = progress.is_indeterminate();

    // --- ナビゲーション ---------------------------------------------------
    let items = [NavItem::new("t"), NavItem::new("t").enabled(false)];

    let tabs: Tabs = ui.tabs()?;
    tabs.add_tab("t", &label);
    tabs.remove_tab(0);
    tabs.clear();
    let _: usize = tabs.len();
    let _: bool = tabs.is_empty();
    let _: Option<usize> = tabs.selected();
    tabs.set_selected(0);
    tabs.select(0);
    tabs.on_select(|_index: usize| {});

    let navbar: Navbar = ui.navbar("t")?;
    navbar.set_title("t");
    let _: String = navbar.title();
    navbar.set_items(&items);
    let _: usize = navbar.len();
    let _: bool = navbar.is_empty();
    let _: Option<usize> = navbar.selected();
    navbar.set_selected(0);
    navbar.select(0);
    navbar.on_select(|_index: usize| {});

    let dock: Dock = ui.dock()?;
    dock.set_items(&items);
    let _: usize = dock.len();
    let _: bool = dock.is_empty();
    let _: Option<usize> = dock.selected();
    dock.set_selected(0);
    dock.select(0);
    dock.on_select(|_index: usize| {});

    let menu: Menu = ui.menu()?;
    menu.set_items(&items);
    let _: usize = menu.len();
    let _: bool = menu.is_empty();
    let _: Option<usize> = menu.selected();
    menu.set_selected(0);
    menu.select(0);
    menu.on_select(|_index: usize| {});

    // --- ツールバー (ウィンドウに取り付ける。Widget ではない) -------------
    let toolbar: Toolbar = ui.toolbar()?;
    toolbar.set_items(&[
        ToolbarItem::new(ToolbarIcon::New, "t"),
        ToolbarItem::separator(),
        ToolbarItem::new(ToolbarIcon::Save, "t").enabled(false),
    ]);
    let _: &str = ToolbarIcon::Open.sf_symbol();
    let _: &str = ToolbarIcon::Open.icon_name();
    let _: char = ToolbarIcon::Open.fluent_glyph();
    let _: &str = ToolbarIcon::Open.svg_path();
    let _: usize = toolbar.len();
    let _: bool = toolbar.is_empty();
    let _: bool = toolbar.is_item_enabled(0);
    toolbar.set_item_enabled(2, true);
    toolbar.set_enabled(true);
    toolbar.activate(0);
    toolbar.on_activate(|_index: usize| {});
    window.set_toolbar(&toolbar);
    window.clear_toolbar();

    // --- メニューバー (ウィンドウに取り付ける。Widget ではない) -----------
    let menu_bar: MenuBar = ui.menu_bar()?;
    menu_bar.set_menus(&[
        MenuSpec::new(
            "t",
            [
                MenuItem::new("t").shortcut(MenuShortcut::new('n')),
                MenuItem::separator(),
                MenuItem::new("t")
                    .shortcut(MenuShortcut::new('z').shift(true).alt(true))
                    .enabled(false),
            ],
        ),
        // 文字列の並びからも作れる。
        MenuSpec::new("t", ["t", "t"]),
    ]);
    let _: usize = MenuSpec::new("t", ["t"]).len();
    let _: bool = MenuSpec::new("t", ["t"]).is_empty();
    let _: Vec<MenuItem> = MenuItem::list(["t", "t"]);
    let _: bool = MenuItem::from("t").is_separator();
    let _: bool = MenuShortcut::new('s').is_valid();
    let _: String = MenuShortcut::new('s').key_equivalent();
    let _: String = MenuShortcut::new('s').accelerator();
    let _: String = MenuShortcut::new('s').label();
    let _: i32 = MenuShortcut::new('s').virtual_key();
    let _: bool = MenuShortcut::new('s').matches("s", true, false, false);
    let _: usize = menu_bar.len();
    let _: bool = menu_bar.is_empty();
    let _: usize = menu_bar.menu_len(0);
    let _: bool = menu_bar.is_item_enabled(0, 0);
    menu_bar.set_item_enabled(0, 2, true);
    menu_bar.set_enabled(true);
    menu_bar.activate(0, 0);
    menu_bar.on_activate(|_menu: usize, _item: usize| {});
    window.set_menu_bar(&menu_bar);
    window.clear_menu_bar();

    // --- サイドバー (ウィンドウに取り付ける。Widget ではない) -----------
    let sidebar: Sidebar = ui.sidebar()?;
    sidebar.set_sections(&[
        SidebarSection::untitled([
            SidebarItem::new("t").icon(ToolbarIcon::Settings),
            SidebarItem::new("t").enabled(false),
        ]),
        SidebarSection::new("t", ["t", "t"]),
    ]);
    sidebar.set_items(&SidebarItem::list(["t", "t"]));
    let _: usize = SidebarSection::untitled(["t"]).len();
    let _: bool = SidebarSection::new("t", ["t"]).is_empty();
    let _: usize = sidebar.len();
    let _: bool = sidebar.is_empty();
    let _: Option<usize> = sidebar.selected();
    sidebar.set_selected(0);
    sidebar.clear_selection();
    sidebar.select(0);
    sidebar.on_select(|_index: usize| {});
    sidebar.set_width(DEFAULT_SIDEBAR_WIDTH);
    let _: f64 = sidebar.width();
    let _: f64 = SIDEBAR_MIN_WIDTH;
    sidebar.on_resize(|_width: f64| {});
    sidebar.set_collapsed(false);
    let _: bool = sidebar.is_collapsed();
    sidebar.on_collapse(|_collapsed: bool| {});
    window.set_sidebar(&sidebar);
    window.clear_sidebar();

    // --- リスト -----------------------------------------------------------
    let list: List = ui.list()?;
    list.set_items(&[
        ListItem::new("t"),
        ListItem::new("t").detail("t"),
        ListItem::new("t").enabled(false),
    ]);
    let _: usize = list.len();
    let _: bool = list.is_empty();
    list.set_selection_mode(SelectionMode::Multiple);
    let _: SelectionMode = list.selection_mode();
    let _: Option<usize> = list.selected();
    let _: Vec<usize> = list.selection();
    list.set_selected(0);
    list.set_selection(&[0, 1]);
    list.clear_selection();
    list.select(0);
    list.select_many(&[0, 1]);
    list.on_select(|_indices: &[usize]| {});
    list.scroll_to_row(0);
    let custom_content = ui.label("任意の行内容")?;
    let custom_row = ListRow::new(&custom_content).selectable(false);
    let _: bool = custom_row.is_selectable();
    custom_row.on_activate(|| {});
    list.set_rows(&[custom_row]);
    list.on_row_activate(|_index: usize| {});
    list.on_context_menu({
        let list = list.clone();
        let menu = ui.popup_menu()?;
        move |_index: usize, at: Point| menu.open_at(&list, at.x, at.y)
    });

    // --- テーブル ---------------------------------------------------------
    let table: Table = ui.table()?;
    table.set_columns(&[
        TableColumn::new("t"),
        TableColumn::new("t").width(1.0).align(Align::End),
        TableColumn::from("t").sortable(true),
    ]);
    let _: usize = table.column_count();
    table.set_rows(&TableRow::list([["t", "t"]]));
    let _: usize = table.len();
    let _: bool = table.is_empty();
    let cells = TableCells::new().text("t").cell(&custom_content);
    let _: bool = cells.is_selectable();
    let _: usize = cells.len();
    let _: bool = cells.is_empty();
    cells.on_activate(|| {});
    table.set_row_builder(1, move |_index: usize| Ok(cells.clone().selectable(false)));
    table.set_row_selectable(|_index: usize| true);
    table.refresh();
    table.set_row_height(1.0);
    let _: f64 = table.row_height();
    table.set_selection_mode(SelectionMode::Multiple);
    let _: SelectionMode = table.selection_mode();
    let _: Option<usize> = table.selected();
    let _: Vec<usize> = table.selection();
    table.set_selected(0);
    table.set_selection(&[0]);
    table.clear_selection();
    table.select(0);
    table.select_many(&[0]);
    table.on_select(|_indices: &[usize]| {});
    table.scroll_to_row(0);
    table.on_sort(|_column: usize, _order: SortOrder| {});
    let _: Option<(usize, SortOrder)> = table.sort();
    table.set_sort(Some((2, SortOrder::Descending)));
    table.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(1.0)));

    // --- ツリー -----------------------------------------------------------
    let tree: Tree = ui.tree()?;
    tree.set_items(&[
        TreeItem::new("t"),
        TreeItem::new("t")
            .detail("t")
            .expanded(true)
            .children([TreeItem::new("t"), TreeItem::from("t")]),
        TreeItem::new("t").enabled(false).child(TreeItem::new("t")),
    ]);
    let _: usize = tree.len();
    let _: bool = tree.is_empty();
    let _: Option<Vec<usize>> = tree.selected();
    tree.set_selected(&[1, 0]);
    tree.clear_selection();
    tree.select(&[0]);
    tree.on_select(|_path: &[usize]| {});
    tree.scroll_to_item(&[0]);
    let _: bool = tree.is_expanded(&[1]);
    tree.set_expanded(&[1], true);
    tree.expand(&[1]);
    tree.collapse(&[1]);
    tree.expand_all();
    tree.collapse_all();
    tree.on_expand(|_path: &[usize], _expanded: bool| {});
    tree.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(1.0)));

    // --- ポップアップ (コンテキスト) メニュー -----------------------------
    let popup: PopupMenu = ui.popup_menu()?;
    popup.set_items(&[
        PopupItem::new("t"),
        PopupItem::separator(),
        PopupItem::new("t").enabled(false),
    ]);
    let _: usize = popup.len();
    let _: bool = popup.is_empty();
    popup.attach(&label);
    popup.select(0);
    popup.close();
    popup.on_select(|_index: usize| {});
    // `open_at` はメニューを実際に出す (macOS では閉じるまで戻らない) ので、
    // 呼ばずに型だけ確かめる。
    let _: fn(&PopupMenu, &dyn Widget, f64, f64) = PopupMenu::open_at;

    let breadcrumbs: Breadcrumbs = ui.breadcrumbs()?;
    breadcrumbs.set_items(&items);
    let _: usize = breadcrumbs.len();
    let _: bool = breadcrumbs.is_empty();
    let _: Option<usize> = breadcrumbs.selected();
    breadcrumbs.set_selected(0);
    breadcrumbs.select(0);
    breadcrumbs.on_select(|_index: usize| {});

    let pagination: Pagination = ui.pagination(3)?;
    pagination.set_page_count(5);
    let _: usize = pagination.page_count();
    let _: usize = pagination.page();
    pagination.set_page(1);
    pagination.select(1);
    pagination.go_previous();
    pagination.go_next();
    pagination.on_change(|_page: usize| {});

    let link: Link = ui.link("t", "https://example.com")?;
    let _: String = link.text();
    link.set_text("t");
    let _: String = link.href();
    link.set_href("t");
    link.set_enabled(true);
    link.on_click(|| {});

    // --- ファイル / フォルダーの選択 ---------------------------------------
    // --- メディア ---------------------------------------------------------
    let image: Image = ui.image("t.png")?;
    let _: String = image.source();
    image.set_source("t.png");
    let _: bool = image.is_loaded();
    image.set_fit(Fit::Cover);
    image.set_alt("t");

    let video: Video = ui.video("t.mp4")?;
    video.set_fit(Fit::Contain);
    let audio: Audio = ui.audio("t.m4a")?;
    // 再生の API は動画と音声で同じ形。両方を同じ順で呼んで確かめる。
    macro_rules! contract_playback {
        ($m:expr) => {{
            let m = $m;
            let _: String = m.source();
            m.set_source("t");
            m.play();
            m.pause();
            let _: PlaybackState = m.state();
            let _: bool = m.is_playing();
            m.seek(1.0);
            let _: f64 = m.position();
            let _: Option<f64> = m.duration();
            m.set_volume(0.5);
            let _: f64 = m.volume();
            m.set_muted(true);
            let _: bool = m.is_muted();
            m.set_loop(true);
            let _: bool = m.is_loop();
            m.set_autoplay(false);
            m.set_controls(true);
            m.on_state_change(|_state: PlaybackState| {});
            m.on_position_change(|_seconds: f64| {});
        }};
    }
    contract_playback!(&video);
    contract_playback!(&audio);

    // --- ダイアログ -------------------------------------------------------
    let dialog: Dialog = ui.dialog("t")?;
    dialog.set_title("t");
    let _: String = dialog.title();
    dialog.set_message("t");
    let _: String = dialog.message();
    dialog.set_child(&ui.label("t")?);
    dialog.set_buttons(DialogButtons::new().primary("t").secondary("t").cancel("t"));
    let _: DialogButtons = dialog.buttons();
    dialog.on_response(|_response: DialogResponse| {});
    let _: bool = dialog.is_open();
    dialog.close();
    // `open()` はダイアログを出すので、契約の確認では呼ばない。

    // --- トースト ---------------------------------------------------------
    let toast: Toast = ui.toast("t")?;
    toast.set_message("t");
    let _: String = toast.message();
    toast.set_action("t");
    let _: String = toast.action();
    toast.set_timeout(1.0);
    let _: f64 = toast.timeout();
    let _: ToastSpec = toast.spec();
    toast.on_action(|| {});
    toast.on_dismiss(|| {});
    let _: bool = toast.is_visible();
    toast.show();
    toast.dismiss();

    let picker: FilePicker = ui.file_picker("t")?;
    picker.set_text("t");
    picker.set_enabled(true);
    picker.set_mode(FilePickerMode::Files);
    let _: FilePickerMode = picker.mode();
    picker.set_filters(&[FileFilter::new("t", ["png"])]);
    let _: Vec<FileEntry> = picker.selection();
    picker.on_select(|_entries: &[FileEntry]| {});
    // `open()` はダイアログを出すので、契約の確認では呼ばない。

    let saver: FileSaver = ui.file_saver("t")?;
    saver.set_text("t");
    saver.set_enabled(true);
    saver.set_file_name("t");
    let _: String = saver.file_name();
    saver.set_filters(&[FileFilter::new("t", ["txt"])]);
    saver.set_contents(b"t");
    let _: usize = saver.contents_len();
    let _: Option<FileEntry> = saver.destination();
    saver.on_save(|_entry: &FileEntry| {});
    saver.on_error(|_error: &Error| {});
    // `open()` はダイアログを出すので、契約の確認では呼ばない。

    grid.attach(&label, GridCell::new(0, 0));
    grid.attach(&button, GridCell::new(1, 0).span(2, 1));
    scroll.set_child(&grid);
    expander.set_child(&checkbox);
    split_view.set_start(&ui.stack(Orientation::Vertical)?);
    split_view.set_end(&ui.stack(Orientation::Vertical)?);

    stack.append(&spacer);
    stack.append(&scroll);
    stack.append(&image);
    stack.append(&video);
    stack.append(&audio);
    stack.append(&label);
    stack.append(&button);
    stack.append(&checkbox);
    stack.append(&combo_box);
    stack.append(&editable_combo_box);
    stack.append(&radio_group);
    stack.append(&date_picker);
    stack.append(&time_picker);
    stack.append(&color_picker);
    stack.append(&canvas);
    stack.append(&input);
    stack.append(&text_area);
    stack.append(&slider);
    stack.append(&progress);
    stack.append(&tabs);
    stack.append(&navbar);
    stack.append(&dock);
    stack.append(&menu);
    stack.append(&list);
    stack.append(&tree);
    stack.append(&breadcrumbs);
    stack.append(&pagination);
    stack.append(&link);
    stack.append(&picker);
    stack.append(&saver);
    stack.append(&expander);
    stack.append(&split_view);
    window.set_child(&stack);

    // --- 別スレッドと非同期 -----------------------------------------------
    let tasks: Tasks = ui.tasks();
    let _: Tasks = tasks.clone();

    let sender: Sender<String> = tasks.channel(|_value: String| {});
    let _: Sender<String> = sender.clone();
    let _: Result<()> = sender.send(String::new());
    let _: bool = sender.is_open();

    let task: Task = tasks.spawn(async {});
    let _: bool = task.is_finished();
    task.cancel();

    let timer: Timer = tasks.after(std::time::Duration::from_millis(1), || {});
    let _: bool = timer.is_active();
    timer.cancel();
    let _: Timer = tasks.every(std::time::Duration::from_secs(1), || {});
    let _: Sleep = tasks.sleep(std::time::Duration::from_millis(1));

    let clipboard: Clipboard = ui.clipboard();
    clipboard.set_text("t", |_result: Result<()>| {});
    clipboard.read_text(|_text: Option<String>| {});
    let _: Result<()> = ui.open_url("https://example.com/");

    // どのウィジェットにも弱参照がある (通知のクロージャから自身を触るため)。
    let weak_input: WeakWidget<TextInput> = input.downgrade();
    let _: WeakWidget<TextInput> = weak_input.clone();
    let _: Option<TextInput> = weak_input.upgrade();
    let _: Option<Stack> = stack.downgrade().upgrade();
    let _: Option<Grid> = grid.downgrade().upgrade();
    let _: Option<Scroll> = scroll.downgrade().upgrade();
    let _: Option<Expander> = expander.downgrade().upgrade();
    let _: Option<SplitView> = split_view.downgrade().upgrade();
    let _: Option<Spacer> = spacer.downgrade().upgrade();
    let _: Option<Label> = label.downgrade().upgrade();
    let _: Option<Button> = button.downgrade().upgrade();
    let _: Option<Checkbox> = checkbox.downgrade().upgrade();
    let _: Option<Toggle> = toggle.downgrade().upgrade();
    let _: Option<TimePicker> = time_picker.downgrade().upgrade();
    let _: Option<ColorPicker> = color_picker.downgrade().upgrade();
    let _: Option<Canvas> = canvas.downgrade().upgrade();
    let _: Option<ComboBox> = combo_box.downgrade().upgrade();
    let _: Option<EditableComboBox> = editable_combo_box.downgrade().upgrade();
    let _: Option<RadioGroup> = radio_group.downgrade().upgrade();
    let _: Option<DatePicker> = date_picker.downgrade().upgrade();
    let _: Option<TextArea> = text_area.downgrade().upgrade();
    let _: Option<PasswordInput> = password.downgrade().upgrade();
    let _: Option<SearchInput> = search.downgrade().upgrade();
    let _: Option<NumberInput> = number.downgrade().upgrade();
    let _: Option<Slider> = slider.downgrade().upgrade();
    let _: Option<ProgressBar> = progress.downgrade().upgrade();
    let _: Option<Tabs> = tabs.downgrade().upgrade();
    let _: Option<Navbar> = navbar.downgrade().upgrade();
    let _: Option<Dock> = dock.downgrade().upgrade();
    let _: Option<Menu> = menu.downgrade().upgrade();
    let _: Option<List> = list.downgrade().upgrade();
    let _: Option<Table> = table.downgrade().upgrade();
    let _: Option<Tree> = tree.downgrade().upgrade();
    let _: Option<Breadcrumbs> = breadcrumbs.downgrade().upgrade();
    let _: Option<Pagination> = pagination.downgrade().upgrade();
    let _: Option<Link> = link.downgrade().upgrade();
    let _: Option<Image> = image.downgrade().upgrade();
    let _: Option<Video> = video.downgrade().upgrade();
    let _: Option<Audio> = audio.downgrade().upgrade();
    let _: Option<FilePicker> = picker.downgrade().upgrade();
    let _: Option<FileSaver> = saver.downgrade().upgrade();

    ui.quit();
    Ok(())
}
