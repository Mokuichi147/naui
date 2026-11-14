//! Children added to another container without being taken out of the one they are in.
//!
//! WinUI refuses such a child (`crates/naui/src/guide.md`, "Changing and moving widgets
//! after adding"). These cases check that every entry that takes a child keeps that
//! promise: the refused child stays where it is and nothing is written to it, the
//! refusing container keeps what it had, and neither container's later operations
//! touch the child.
//!
//! Each case collects every broken promise before failing, so one run shows all of
//! them.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::time::Duration;

use naui_core::{
    Align, GridCell, Orientation, Padding, Pane, PopoverPosition, Result, ScrollPolicy,
    SidebarItem, Sizing, TableColumn, Track,
};
use naui_windows::{ListRow, TableCells, Ui, Widget};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Border, ContentControl, Grid as XamlGrid, ItemsControl, ListView, ListViewItem, Panel,
    TextBlock, TwoPaneView,
};
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{DependencyObject, FrameworkElement};
use windows_core::{IInspectable, IUnknown, Interface};

use crate::cases::{pump_until, SCROLL_LAYOUT_TIMEOUT};

// ------------------------------------------------------------------ 補助

/// The identity of a COM object.
fn key<T: Interface>(object: &T) -> usize {
    object
        .cast::<IUnknown>()
        .map_or(0, |unknown| Interface::as_raw(&unknown) as usize)
}

/// Where the child is and what naui writes on a child it places: the parents and
/// the placement properties.
fn placement(widget: &dyn Widget) -> String {
    let element = widget.native_element();
    let Ok(framework) = element.cast::<FrameworkElement>() else {
        return "FrameworkElement ではない".to_string();
    };
    let logical = framework.Parent().ok().map(|parent| key(&parent));
    let visual = element
        .cast::<DependencyObject>()
        .ok()
        .and_then(|object| VisualTreeHelper::GetParent(&object).ok())
        .map(|parent| key(&parent));
    format!(
        "Parent={logical:?} GetParent={visual:?} 横={:?} 縦={:?} 幅={:?} 高さ={:?} \
         余白={:?} 行={:?} 列={:?} 行の結合={:?} 列の結合={:?} 表示={:?}",
        framework.HorizontalAlignment().ok(),
        framework.VerticalAlignment().ok(),
        framework.Width().ok(),
        framework.Height().ok(),
        framework.Margin().ok(),
        XamlGrid::GetRow(&framework).ok(),
        XamlGrid::GetColumn(&framework).ok(),
        XamlGrid::GetRowSpan(&framework).ok(),
        XamlGrid::GetColumnSpan(&framework).ok(),
        element.Visibility().ok(),
    )
}

/// Whether anything under `root` satisfies `found`. Walks the panels' children, a
/// border's child, a content control's content, a `TwoPaneView`'s panes, an items
/// control's items and the visual children.
fn any_under(root: &IInspectable, found: &dyn Fn(&IInspectable) -> bool, depth: usize) -> bool {
    if found(root) {
        return true;
    }
    if depth > 64 {
        return false;
    }
    let mut children: Vec<IInspectable> = Vec::new();
    if let Ok(panel) = root.cast::<Panel>() {
        if let Ok(items) = panel.Children() {
            for index in 0..items.Size().unwrap_or(0) {
                children.extend(items.GetAt(index).ok().and_then(|child| child.cast().ok()));
            }
        }
    }
    if let Ok(border) = root.cast::<Border>() {
        children.extend(border.Child().ok().and_then(|child| child.cast().ok()));
    }
    if let Ok(control) = root.cast::<ContentControl>() {
        children.extend(control.Content().ok());
    }
    if let Ok(panes) = root.cast::<TwoPaneView>() {
        for pane in [panes.Pane1(), panes.Pane2()] {
            children.extend(pane.ok().and_then(|pane| pane.cast().ok()));
        }
    }
    if let Ok(control) = root.cast::<ItemsControl>() {
        if let Ok(items) = control.Items() {
            for index in 0..items.Size().unwrap_or(0) {
                children.extend(items.GetAt(index).ok());
            }
        }
    }
    if let Ok(object) = root.cast::<DependencyObject>() {
        let count = VisualTreeHelper::GetChildrenCount(&object).unwrap_or(0);
        for index in 0..count {
            children.extend(
                VisualTreeHelper::GetChild(&object, index)
                    .ok()
                    .and_then(|child| child.cast().ok()),
            );
        }
    }
    children
        .iter()
        .any(|child| any_under(child, found, depth + 1))
}

fn contains(root: &IInspectable, widget: &dyn Widget) -> bool {
    let target = key(&widget.native_element());
    any_under(root, &|object| key(object) == target, 0)
}

fn shows_text(root: &IInspectable, text: &str) -> bool {
    any_under(
        root,
        &|object| {
            object
                .cast::<TextBlock>()
                .and_then(|block| block.Text())
                .is_ok_and(|shown| shown == text)
        },
        0,
    )
}

fn root_of(widget: &dyn Widget) -> Option<IInspectable> {
    widget.native_element().cast().ok()
}

/// Runs `f`, turning a panic into a problem.
fn guarded(problems: &mut Vec<String>, what: &str, f: impl FnOnce(&mut Vec<String>)) {
    let mut found = Vec::new();
    let outcome = catch_unwind(AssertUnwindSafe(|| f(&mut found)));
    problems.append(&mut found);
    if outcome.is_err() {
        problems.push(format!("{what}: 途中で panic した"));
    }
}

fn report(problems: &[String]) {
    for problem in problems {
        println!("     違反: {problem}");
    }
    assert!(
        problems.is_empty(),
        "{} 件の違反 (上の「違反:」の行)",
        problems.len()
    );
}

// ------------------------------------------------------------ 置く入口

/// Puts a child into a container.
type Put = Box<dyn Fn(&dyn Widget)>;

/// One entry that takes a child, wrapped so that every container is driven the
/// same way.
struct Holder {
    put: Put,
    /// What to look in for the children the container shows.
    root: Box<dyn Fn() -> Option<IInspectable>>,
    /// What must stay the same when it refuses a child (the number of children and
    /// so on).
    state: Box<dyn Fn() -> String>,
    /// The operations that read the children it remembers.
    exercise: Box<dyn Fn()>,
    /// Whether putting the child it already holds is a thing to do (`set_child`).
    again: bool,
    close: Box<dyn Fn()>,
}

impl Holder {
    fn new(
        put: impl Fn(&dyn Widget) + 'static,
        root: impl Fn() -> Option<IInspectable> + 'static,
    ) -> Self {
        Self {
            put: Box::new(put),
            root: Box::new(root),
            state: Box::new(String::new),
            exercise: Box::new(|| {}),
            again: false,
            close: Box::new(|| {}),
        }
    }

    fn state(mut self, f: impl Fn() -> String + 'static) -> Self {
        self.state = Box::new(f);
        self
    }

    fn exercise(mut self, f: impl Fn() + 'static) -> Self {
        self.exercise = Box::new(f);
        self
    }

    fn again(mut self) -> Self {
        self.again = true;
        self
    }

    fn close(mut self, f: impl Fn() + 'static) -> Self {
        self.close = Box::new(f);
        self
    }

    fn holds(&self, widget: &dyn Widget) -> bool {
        (self.root)().is_some_and(|root| contains(&root, widget))
    }
}

type Make = fn(&Ui) -> Result<Holder>;

/// The entries that place a child on the spot.
const PLACERS: &[(&str, Make)] = &[
    ("Stack::append", stack_append),
    ("Stack::insert", stack_insert),
    ("Grid::attach", grid_attach),
    ("Grid::replace", grid_replace),
    ("Scroll::set_child", scroll_child),
    ("Group::set_child", group_child),
    ("Expander::set_child", expander_child),
    ("Popover::set_child", popover_child),
    ("Dialog::set_child", dialog_child),
    ("Sheet::set_child", sheet_child),
    ("Window::set_child", window_child),
    (
        "Window::set_child (サイドバーあり)",
        window_child_with_sidebar,
    ),
    ("AdaptivePanes::set_list", panes_list),
    ("AdaptivePanes::set_detail", panes_detail),
    ("SplitView::set_start", split_start),
    ("SplitView::set_end", split_end),
    ("Tabs::add_tab", tabs_add),
];

fn stack_holder(ui: &Ui, insert: bool) -> Result<Holder> {
    let stack = ui.stack(Orientation::Vertical)?;
    let (put, root, state, exercise) = (stack.clone(), stack.clone(), stack.clone(), stack);
    Ok(Holder::new(
        move |child| {
            if insert {
                put.insert(put.len(), child)
            } else {
                put.append(child)
            }
        },
        move || root_of(&root),
    )
    .state(move || format!("len={}", state.len()))
    .exercise(move || {
        exercise.set_align(Align::Center);
        exercise.set_spacing(3.0);
        exercise.set_padding(Padding::all(2.0));
        exercise.set_orientation(Orientation::Horizontal);
        exercise.set_orientation(Orientation::Vertical);
        // 断った子が入るはずだった位置。
        exercise.remove(1);
    }))
}

fn stack_append(ui: &Ui) -> Result<Holder> {
    stack_holder(ui, false)
}

fn stack_insert(ui: &Ui) -> Result<Holder> {
    stack_holder(ui, true)
}

fn grid_attach(ui: &Ui) -> Result<Holder> {
    let grid = ui.grid()?;
    let next = Rc::new(std::cell::Cell::new(0));
    let (put, root, state, exercise) = (grid.clone(), grid.clone(), grid.clone(), grid);
    Ok(Holder::new(
        move |child| {
            put.attach(child, GridCell::new(next.get(), 0));
            next.set(next.get() + 1);
        },
        move || root_of(&root),
    )
    .state(move || {
        format!(
            "len={} columns={} rows={}",
            state.len(),
            state.columns(),
            state.rows()
        )
    })
    .exercise(move || {
        exercise.set_spacing(4.0, 4.0);
        exercise.set_padding(Padding::all(2.0));
        exercise.set_column_track(0, Track::Fixed(80.0));
        exercise.set_row_track(0, Track::FILL);
        // 断った子が入るはずだったマス。
        exercise.remove(GridCell::new(1, 0));
    }))
}

fn grid_replace(ui: &Ui) -> Result<Holder> {
    let grid = ui.grid()?;
    let (put, root, state, exercise) = (grid.clone(), grid.clone(), grid.clone(), grid);
    Ok(Holder::new(
        move |child| put.replace(child, GridCell::new(0, 0)),
        move || root_of(&root),
    )
    .state(move || {
        format!(
            "len={} columns={} rows={}",
            state.len(),
            state.columns(),
            state.rows()
        )
    })
    .exercise(move || {
        exercise.set_spacing(4.0, 4.0);
        exercise.set_column_track(0, Track::Fixed(60.0));
        exercise.set_row_track(0, Track::Auto);
    })
    .again())
}

fn scroll_child(ui: &Ui) -> Result<Holder> {
    let scroll = ui.scroll()?;
    let (put, root, exercise) = (scroll.clone(), scroll.clone(), scroll);
    Ok(
        Holder::new(move |child| put.set_child(child), move || root_of(&root))
            .exercise(move || {
                exercise.set_policy(ScrollPolicy::Always, ScrollPolicy::Auto);
                exercise.set_policy(ScrollPolicy::Never, ScrollPolicy::Never);
                exercise.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
            })
            .again(),
    )
}

fn group_child(ui: &Ui) -> Result<Holder> {
    let group = ui.group("見出し")?;
    let (put, root, exercise) = (group.clone(), group.clone(), group);
    Ok(
        Holder::new(move |child| put.set_child(child), move || root_of(&root))
            .exercise(move || {
                exercise.set_padding(Padding::all(4.0));
                exercise.set_text("別の見出し");
            })
            .again(),
    )
}

fn expander_child(ui: &Ui) -> Result<Holder> {
    let expander = ui.expander("開閉")?;
    let (put, root, exercise) = (expander.clone(), expander.clone(), expander);
    Ok(
        Holder::new(move |child| put.set_child(child), move || root_of(&root))
            .exercise(move || {
                exercise.set_expanded(true);
                exercise.set_expanded(false);
            })
            .again(),
    )
}

fn popover_child(ui: &Ui) -> Result<Holder> {
    let popover = ui.popover()?;
    let (put, root, exercise) = (popover.clone(), popover.clone(), popover);
    Ok(Holder::new(
        move |child| put.set_child(child),
        move || {
            root.native_flyout()
                .Content()
                .ok()
                .and_then(|content| content.cast().ok())
        },
    )
    .exercise(move || {
        exercise.set_position(PopoverPosition::Top);
        exercise.set_position(PopoverPosition::Bottom);
    })
    .again())
}

fn dialog_child(ui: &Ui) -> Result<Holder> {
    let dialog = ui.dialog("題")?;
    let (put, root, exercise) = (dialog.clone(), dialog.clone(), dialog);
    Ok(Holder::new(
        move |child| put.set_child(child),
        move || root.native_dialog().cast().ok(),
    )
    .exercise(move || {
        exercise.set_message("本文");
        exercise.set_title("別の題");
        exercise.set_message("");
    })
    .again())
}

fn sheet_child(ui: &Ui) -> Result<Holder> {
    let sheet = ui.sheet("題")?;
    let (put, root, exercise) = (sheet.clone(), sheet.clone(), sheet);
    Ok(Holder::new(
        move |child| put.set_child(child),
        move || root.native_dialog().cast().ok(),
    )
    .exercise(move || exercise.set_title("別の題"))
    .again())
}

fn window_holder(ui: &Ui, with_sidebar: bool) -> Result<Holder> {
    let window = ui.window("外さずに移した子", 320.0, 240.0)?;
    let sidebar = ui.sidebar()?;
    sidebar.set_items(&SidebarItem::list(["一般"]));
    if with_sidebar {
        window.set_sidebar(&sidebar);
    }
    let (put, root, exercise, close) = (window.clone(), window.clone(), window.clone(), window);
    Ok(Holder::new(
        move |child| put.set_child(child),
        move || {
            root.native_window()
                .Content()
                .ok()
                .and_then(|content| content.cast().ok())
        },
    )
    .exercise(move || {
        if with_sidebar {
            exercise.clear_sidebar();
            exercise.set_sidebar(&sidebar);
        } else {
            exercise.set_sidebar(&sidebar);
            exercise.clear_sidebar();
        }
    })
    .close(move || close.close())
    .again())
}

fn window_child(ui: &Ui) -> Result<Holder> {
    window_holder(ui, false)
}

fn window_child_with_sidebar(ui: &Ui) -> Result<Holder> {
    window_holder(ui, true)
}

fn panes_holder(ui: &Ui, list: bool) -> Result<Holder> {
    let panes = ui.adaptive_panes()?;
    let (put, root, exercise) = (panes.clone(), panes.clone(), panes);
    Ok(Holder::new(
        move |child| {
            if list {
                put.set_list(child)
            } else {
                put.set_detail(child)
            }
        },
        move || root_of(&root),
    )
    .exercise(move || {
        exercise.show(Pane::Detail);
        exercise.go_back();
        exercise.set_breakpoint(300.0);
        exercise.set_list_width(150.0);
    })
    .again())
}

fn panes_list(ui: &Ui) -> Result<Holder> {
    panes_holder(ui, true)
}

fn panes_detail(ui: &Ui) -> Result<Holder> {
    panes_holder(ui, false)
}

fn split_holder(ui: &Ui, start: bool) -> Result<Holder> {
    let split = ui.split_view(Orientation::Horizontal)?;
    let (put, root, exercise) = (split.clone(), split.clone(), split);
    Ok(Holder::new(
        move |child| {
            if start {
                put.set_start(child)
            } else {
                put.set_end(child)
            }
        },
        move || root_of(&root),
    )
    .exercise(move || {
        exercise.set_position(100.0);
        exercise.set_min_sizes(20.0, 20.0);
        exercise.drag_to(80.0);
    })
    .again())
}

fn split_start(ui: &Ui) -> Result<Holder> {
    split_holder(ui, true)
}

fn split_end(ui: &Ui) -> Result<Holder> {
    split_holder(ui, false)
}

fn tabs_add(ui: &Ui) -> Result<Holder> {
    let tabs = ui.tabs()?;
    let (put, root, state, exercise) = (tabs.clone(), tabs.clone(), tabs.clone(), tabs);
    Ok(Holder::new(
        move |child| put.add_tab("タブ", child),
        move || root_of(&root),
    )
    .state(move || format!("len={} selected={:?}", state.len(), state.selected()))
    .exercise(move || {
        let count = exercise.len();
        if count > 1 {
            exercise.set_selected(count - 1);
        }
        exercise.set_selected(0);
        // 断った子が入るはずだったタブ。
        exercise.remove_tab(1);
        exercise.set_selected(0);
    }))
}

// ------------------------------------------------------------ 移す先

/// The containers the child is added to without being taken out first.
#[derive(Clone, Copy, Debug)]
enum Target {
    Stack,
    Grid,
}

const TARGETS: [Target; 2] = [Target::Stack, Target::Grid];

/// A `Stack` or a `Grid` with one child already in it, so the child added next does
/// not get the values a refusing container would write (row 0, the first column,
/// `Left` / `Center` alignment).
fn target(ui: &Ui, kind: Target) -> Result<Holder> {
    let first = ui.label("先客")?;
    let spare = ui.label("差し替え")?;
    match kind {
        Target::Stack => {
            let stack = ui.stack(Orientation::Vertical)?;
            // 縦の Stack の End は、子の横の配置を Right にする。
            stack.set_align(Align::End);
            stack.append(&first);
            let (put, root, state, exercise) = (stack.clone(), stack.clone(), stack.clone(), stack);
            Ok(
                Holder::new(move |child| put.append(child), move || root_of(&root))
                    .state(move || format!("len={}", state.len()))
                    .exercise(move || {
                        exercise.set_align(Align::Center);
                        exercise.set_spacing(5.0);
                        exercise.set_padding(Padding::all(3.0));
                        exercise.set_orientation(Orientation::Horizontal);
                        exercise.set_orientation(Orientation::Vertical);
                        exercise.remove(1);
                        exercise.insert(0, &spare);
                        exercise.remove(0);
                        exercise.clear();
                    }),
            )
        }
        Target::Grid => {
            let grid = ui.grid()?;
            grid.attach(&first, GridCell::new(0, 0));
            let (put, root, state, exercise) = (grid.clone(), grid.clone(), grid.clone(), grid);
            Ok(Holder::new(
                move |child| put.attach(child, GridCell::new(1, 1)),
                move || root_of(&root),
            )
            .state(move || {
                format!(
                    "len={} columns={} rows={}",
                    state.len(),
                    state.columns(),
                    state.rows()
                )
            })
            .exercise(move || {
                exercise.set_spacing(6.0, 6.0);
                exercise.set_padding(Padding::all(3.0));
                exercise.set_column_track(1, Track::Fixed(40.0));
                exercise.set_row_track(1, Track::Fixed(30.0));
                exercise.remove(GridCell::new(1, 1));
                exercise.replace(&spare, GridCell::new(1, 1));
                exercise.clear();
            }))
        }
    }
}

// ------------------------------------------------------------ その場で置く入口

/// (a) A child in a container is added to a `Stack` / `Grid` without being taken
/// out: the new parent refuses it, and neither its operations nor the old
/// container's touch the child.
///
/// (b) A child in a `Stack` / `Grid` is given to the container: it refuses it,
/// keeps what it had, and its operations do not touch the child.
pub(crate) fn placers_refuse_a_child_with_a_parent(ui: &Ui) -> Result<()> {
    let mut problems = Vec::new();
    for (name, make) in PLACERS {
        for kind in TARGETS {
            let what = format!("(a) {name} の子を {kind:?} へ");
            guarded(&mut problems, &what, |problems| {
                let Ok(holder) = make(ui) else {
                    problems.push(format!("{what}: 作れない"));
                    return;
                };
                let Ok(target) = target(ui, kind) else {
                    problems.push(format!("{what}: 移す先を作れない"));
                    return;
                };
                let (Ok(p), Ok(c)) = (ui.label("p"), ui.label("c")) else {
                    return;
                };
                (holder.put)(&p);
                (holder.put)(&c);
                let before = placement(&c);
                let state = (target.state)();
                (target.put)(&c);
                if placement(&c) != before {
                    problems.push(format!(
                        "{what}: 移す先が子を取った・書き換えた\n         前 {before}\n         後 {}",
                        placement(&c)
                    ));
                }
                if (target.state)() != state {
                    problems.push(format!(
                        "{what}: 移す先の記録が変わった ({state} → {})",
                        (target.state)()
                    ));
                }
                if target.holds(&c) {
                    problems.push(format!("{what}: 移す先に子が入った"));
                }
                (target.exercise)();
                if placement(&c) != before {
                    problems.push(format!(
                        "{what}: 移す先の操作が子を書き換えた\n         前 {before}\n         後 {}",
                        placement(&c)
                    ));
                }
                // 元のコンテナの操作は、自分の子に対する正当なもの。落ちないことだけを見る。
                (holder.exercise)();
                (holder.close)();
            });

            let what = format!("(b) {kind:?} の中の子を {name} へ");
            guarded(&mut problems, &what, |problems| {
                let Ok(holder) = make(ui) else {
                    problems.push(format!("{what}: 作れない"));
                    return;
                };
                let Ok(target) = target(ui, kind) else {
                    problems.push(format!("{what}: 移す先を作れない"));
                    return;
                };
                let (Ok(p), Ok(c)) = (ui.label("p"), ui.label("c")) else {
                    return;
                };
                c.set_sizing(Sizing::AUTO);
                (target.put)(&c);
                (holder.put)(&p);
                if !holder.holds(&p) {
                    problems.push(format!("{what}: 親の無い子 p を置けない"));
                }
                if holder.again {
                    (holder.put)(&p);
                    if !holder.holds(&p) {
                        problems.push(format!("{what}: 同じ子 p を入れ直すと外れる"));
                    }
                }
                let before = placement(&c);
                let state = (holder.state)();
                (holder.put)(&c);
                if placement(&c) != before {
                    problems.push(format!(
                        "{what}: 断るはずの子を取った・書き換えた\n         前 {before}\n         後 {}",
                        placement(&c)
                    ));
                }
                if (holder.state)() != state {
                    problems.push(format!(
                        "{what}: 断るはずの子を記録した ({state} → {})",
                        (holder.state)()
                    ));
                }
                if !holder.holds(&p) {
                    problems.push(format!("{what}: 前の子 p が外れた"));
                }
                if holder.holds(&c) {
                    problems.push(format!("{what}: 断るはずの子 c を置いた"));
                }
                (holder.exercise)();
                if placement(&c) != before {
                    problems.push(format!(
                        "{what}: 断ったコンテナの操作が子を書き換えた\n         前 {before}\n         後 {}",
                        placement(&c)
                    ));
                }
                if !holder.holds(&p) {
                    problems.push(format!("{what}: 操作のあと前の子 p が外れた"));
                }
                (holder.close)();
            });
        }
    }

    // 同じ Grid の別のマスにいる子で `replace` する。
    guarded(
        &mut problems,
        "Grid::replace (同じ Grid の別のマス)",
        |problems| {
            let (Ok(grid), Ok(a), Ok(c)) = (ui.grid(), ui.label("a"), ui.label("c")) else {
                return;
            };
            grid.attach(&a, GridCell::new(0, 0));
            grid.attach(&c, GridCell::new(1, 0));
            let before = placement(&c);
            grid.replace(&c, GridCell::new(0, 0));
            let root = root_of(&grid).expect("Grid");
            if !contains(&root, &a) {
                problems
                    .push("Grid::replace (同じ Grid の別のマス): 前の子 a が外れた".to_string());
            }
            if placement(&c) != before || grid.len() != 2 {
                problems.push(format!(
                "Grid::replace (同じ Grid の別のマス): 別のマスの子を動かした (len={})\n         前 {before}\n         後 {}",
                grid.len(),
                placement(&c)
            ));
            }
        },
    );

    report(&problems);
    Ok(())
}

// ------------------------------------------------------------ 組み立てるときに置く入口

fn list_holds(list: &naui_windows::List, widget: &dyn Widget) -> bool {
    list.native_list_view()
        .cast::<IInspectable>()
        .is_ok_and(|root| contains(&root, widget))
}

fn table_holds(table: &naui_windows::Table, widget: &dyn Widget) -> bool {
    table
        .native_list_view()
        .cast::<IInspectable>()
        .is_ok_and(|root| contains(&root, widget))
}

/// The rows of a `ListView` whose holder has nothing in it.
fn empty_rows(list_view: &ListView) -> usize {
    let Ok(items) = list_view.Items() else {
        return 0;
    };
    (0..items.Size().unwrap_or(0))
        .filter_map(|index| items.GetAt(index).ok())
        .filter_map(|item| item.cast::<ListViewItem>().ok())
        .filter(|item| {
            item.Content()
                .ok()
                .and_then(|content| content.cast::<Panel>().ok())
                .and_then(|panel| panel.Children().ok())
                .and_then(|children| children.Size().ok())
                .is_none_or(|size| size == 0)
        })
        .count()
}

/// List rows and table cells: a child with no parent goes in and comes back after
/// rebuilding; a child in a `Stack` / `Grid` leaves the row empty, is not written
/// to, and is not taken even after it leaves that container.
pub(crate) fn rows_take_only_free_children(ui: &Ui) -> Result<()> {
    let mut problems = Vec::new();

    guarded(
        &mut problems,
        "List::set_rows (親の無い子)",
        |problems| {
            let (Ok(list), Ok(c)) = (ui.list(), ui.label("c")) else {
                return;
            };
            let mut check = |when: &str| {
                if !list_holds(&list, &c) {
                    problems.push(format!("List::set_rows (親の無い子): {when}のあと行に無い"));
                }
            };
            list.set_rows(&[ListRow::new(&c)]);
            check("set_rows ");
            list.refresh();
            check("refresh ");
            list.set_row_height(40.0);
            check("set_row_height(40) ");
            list.set_row_height(0.0);
            check("set_row_height(0) ");
            list.set_rows(&[ListRow::new(&c)]);
            check("同じ子での set_rows ");
        },
    );

    guarded(
        &mut problems,
        "List::set_row_builder (親の無い子)",
        |problems| {
            let (Ok(list), Ok(c)) = (ui.list(), ui.label("c")) else {
                return;
            };
            let held = c.clone();
            list.set_row_builder(1, move |_| Ok(ListRow::new(&held)));
            let mut check = |when: &str| {
                if !list_holds(&list, &c) {
                    problems.push(format!(
                        "List::set_row_builder (親の無い子): {when}のあと行に無い"
                    ));
                }
            };
            check("set_row_builder ");
            list.refresh();
            check("refresh ");
            list.set_row_height(48.0);
            check("set_row_height ");
        },
    );

    guarded(
        &mut problems,
        "Table::set_row_builder (親の無い子)",
        |problems| {
            let (Ok(table), Ok(c)) = (ui.table(), ui.label("c")) else {
                return;
            };
            let columns = [
                TableColumn::new("名前"),
                TableColumn::new("中身").align(Align::Fill),
            ];
            table.set_columns(&columns);
            let held = c.clone();
            table.set_row_builder(1, move |_| Ok(TableCells::new().text("x").cell(&held)));
            let mut check = |when: &str| {
                if !table_holds(&table, &c) {
                    problems.push(format!(
                        "Table::set_row_builder (親の無い子): {when}のあとセルに無い"
                    ));
                }
            };
            check("set_row_builder ");
            table.set_columns(&columns);
            check("set_columns ");
            table.refresh();
            check("refresh ");
            table.set_row_height(40.0);
            check("set_row_height ");
        },
    );

    for kind in TARGETS {
        let what = format!("List::set_rows ({kind:?} の中の子)");
        guarded(&mut problems, &what, |problems| {
            let (Ok(list), Ok(target), Ok(c), Ok(text)) = (
                ui.list(),
                target(ui, kind),
                ui.label("c"),
                ui.label("同じ一覧の文字"),
            ) else {
                return;
            };
            c.set_sizing(Sizing::AUTO);
            (target.put)(&c);
            let before = placement(&c);
            list.set_rows(&[ListRow::new(&c), ListRow::new(&text)]);
            let mut check = |when: &str| {
                if list_holds(&list, &c) {
                    problems.push(format!("{what}: {when}のあと行に置いた"));
                }
                if placement(&c) != before {
                    problems.push(format!(
                        "{what}: {when}のあと子を書き換えた\n         前 {before}\n         後 {}",
                        placement(&c)
                    ));
                }
                if !list_holds(&list, &text) {
                    problems.push(format!("{what}: {when}のあとほかの行が無い"));
                }
            };
            check("set_rows ");
            list.refresh();
            check("refresh ");
            list.set_row_height(40.0);
            check("set_row_height ");
            // 移す先が手放して親が無くなっても、断った子は拾わない。
            (target.exercise)();
            list.refresh();
            if list_holds(&list, &c) {
                problems.push(format!(
                    "{what}: 移す先が手放したあとの refresh で、断った子を置いた"
                ));
            }
        });

        let what = format!("List::set_row_builder ({kind:?} の中の子)");
        guarded(&mut problems, &what, |problems| {
            let (Ok(list), Ok(target), Ok(c)) = (ui.list(), target(ui, kind), ui.label("c")) else {
                return;
            };
            c.set_sizing(Sizing::AUTO);
            (target.put)(&c);
            let before = placement(&c);
            let held = c.clone();
            let ui = ui.clone();
            list.set_row_builder(2, move |index| match index {
                0 => Ok(ListRow::new(&held)),
                _ => Ok(ListRow::new(&ui.label("同じ一覧の文字")?)),
            });
            let mut check = |when: &str| {
                if list_holds(&list, &c) {
                    problems.push(format!("{what}: {when}のあと行に置いた"));
                }
                if placement(&c) != before {
                    problems.push(format!(
                        "{what}: {when}のあと子を書き換えた\n         前 {before}\n         後 {}",
                        placement(&c)
                    ));
                }
                let shown = list
                    .native_list_view()
                    .cast::<IInspectable>()
                    .is_ok_and(|root| shows_text(&root, "同じ一覧の文字"));
                if !shown {
                    problems.push(format!("{what}: {when}のあとほかの行が無い"));
                }
            };
            check("set_row_builder ");
            list.refresh();
            check("refresh ");
            list.set_row_height(40.0);
            check("set_row_height ");
        });

        let what = format!("Table::set_row_builder ({kind:?} の中の子)");
        guarded(&mut problems, &what, |problems| {
            let (Ok(table), Ok(target), Ok(c)) = (ui.table(), target(ui, kind), ui.label("c"))
            else {
                return;
            };
            c.set_sizing(Sizing::AUTO);
            (target.put)(&c);
            let before = placement(&c);
            let columns = [
                TableColumn::new("名前"),
                TableColumn::new("中身").align(Align::Fill),
            ];
            table.set_columns(&columns);
            let held = c.clone();
            table.set_row_builder(1, move |_| {
                Ok(TableCells::new().text("同じ行の文字").cell(&held))
            });
            let mut check = |when: &str| {
                if table_holds(&table, &c) {
                    problems.push(format!("{what}: {when}のあとセルに置いた"));
                }
                if placement(&c) != before {
                    problems.push(format!(
                        "{what}: {when}のあと子を書き換えた\n         前 {before}\n         後 {}",
                        placement(&c)
                    ));
                }
                let shown = table
                    .native_list_view()
                    .cast::<IInspectable>()
                    .is_ok_and(|root| shows_text(&root, "同じ行の文字"));
                if !shown {
                    problems.push(format!("{what}: {when}のあと同じ行の文字のセルが無い"));
                }
            };
            check("set_row_builder ");
            table.set_columns(&columns);
            check("set_columns ");
            table.refresh();
            check("refresh ");
            table.set_row_height(40.0);
            check("set_row_height ");
        });
    }

    report(&problems);
    Ok(())
}

/// In a shown list whose rows are built only around what is visible: a child given
/// but not placed yet is not taken back from the container it went into, a later
/// owner keeps it, and rows built from a builder still fill in after scrolling and
/// `set_row_height`.
pub(crate) fn rows_take_only_free_children_on_screen(ui: &Ui) -> Result<()> {
    const ROWS: usize = 1000;
    let window = ui.window("行の引き取り", 320.0, 240.0)?;
    let list = ui.list()?;
    list.set_sizing(Sizing::fill());
    window.set_child(&list);
    let mut problems = Vec::new();
    let outcome = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
        window.show();
        let scroll = list
            .native_element()
            .cast::<Border>()
            .and_then(|border| border.Child())
            .and_then(|child| {
                child.cast::<naui_winui3::Microsoft::UI::Xaml::Controls::ScrollViewer>()
            })
            .expect("リストの ScrollViewer");
        pump_until(SCROLL_LAYOUT_TIMEOUT, || {
            scroll.ViewportHeight().unwrap_or(0.0) > 0.0
        });
        if scroll.ViewportHeight().unwrap_or(0.0) <= 0.0 {
            println!("     (ScrollViewer に実寸が付かない環境なので、確かめずに通す)");
            return Ok(());
        }
        let settle = || pump_until(Duration::from_millis(400), || false);
        let last = ui.label("最後の行")?;
        let mut rows: Vec<ListRow> = (0..ROWS - 1)
            .map(|index| {
                ui.label(&format!("行 {index}"))
                    .map(|label| ListRow::new(&label))
            })
            .collect::<Result<_>>()?;
        rows.push(ListRow::new(&last));
        list.set_rows(&rows);
        settle();
        if list_holds(&list, &last) {
            println!("     (最後の行まで組み立てる環境なので、確かめずに通す)");
            return Ok(());
        }

        // 渡しただけでまだ置かれていない子を、Stack へ入れてから行を組み立てる。
        let stack = target(ui, Target::Stack)?;
        (stack.put)(&last);
        let before = placement(&last);
        list.scroll_to_row(ROWS - 1);
        settle();
        if list_holds(&list, &last) {
            problems.push("組み立てる前に Stack へ入れた子を、行が取り返した".to_string());
        }
        if placement(&last) != before {
            problems.push(format!(
                "組み立てる前に Stack へ入れた子を、行が書き換えた\n         前 {before}\n         後 {}",
                placement(&last)
            ));
        }

        // 後から渡した持ち主だけが置く。
        list.scroll_to_row(0);
        settle();
        (stack.exercise)();
        let other = ui.list()?;
        other.set_rows(&[ListRow::new(&last)]);
        if !list_holds(&other, &last) {
            problems.push("親の無い子を後から渡した List に置けない".to_string());
        }
        list.scroll_to_row(ROWS - 1);
        settle();
        if list_holds(&list, &last) || !list_holds(&other, &last) {
            problems.push("前の持ち主が組み立てたとき、後の持ち主から子を取った".to_string());
        }
        other.set_rows(&[]);
        list.refresh();
        settle();
        if list_holds(&list, &last) {
            problems.push("後の持ち主が手放したあと、前の持ち主が作り直しで子を置いた".to_string());
        }

        // 新しいウィジェットを返す組み立て関数の行は、行き来しても作り直しても中身がある。
        let fresh = ui.clone();
        list.set_row_builder(ROWS, move |index| {
            Ok(ListRow::new(
                &fresh.label(&format!("組み立てた行 {index}"))?,
            ))
        });
        settle();
        list.scroll_to_row(800);
        settle();
        list.scroll_to_row(0);
        settle();
        let empty = empty_rows(&list.native_list_view());
        if empty > 0 {
            problems.push(format!(
                "組み立て関数の行を行き来すると {empty} 行が空になる"
            ));
        }
        list.set_row_height(40.0);
        settle();
        let empty = empty_rows(&list.native_list_view());
        if empty > 0 {
            problems.push(format!(
                "組み立て関数の行が set_row_height のあと {empty} 行空になる"
            ));
        }
        Ok(())
    }));
    window.close();
    match outcome {
        Ok(result) => result?,
        Err(_) => problems.push("表示したリストの確かめが途中で panic した".to_string()),
    }
    report(&problems);
    Ok(())
}
