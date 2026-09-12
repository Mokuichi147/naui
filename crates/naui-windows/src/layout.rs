//! 大きさの指定と、レイアウト用のコンテナ (Grid / Scroll / Spacer)。
//!
//! 計算するのは WinUI 3 のレイアウトパスで、naui 側は
//! `Width` / `MinWidth` / `RowDefinition` などのプロパティを設定するだけ。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{Align, GridCell, Length, Padding, Result, ScrollPolicy, Sizing, Track};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    ColumnDefinition, Grid as XamlGrid, RowDefinition, ScrollBarVisibility, ScrollMode,
    ScrollViewer,
};
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{
    DependencyObject, FrameworkElement, GridLength, GridUnitType, HorizontalAlignment, Thickness,
    UIElement, VerticalAlignment, Visibility, Window as XamlWindow,
};
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::widgets::{impl_widget, Widget};

/// `Fill` を指定されたことを覚えておく目印 (`FrameworkElement.Tag`)。
///
/// WinUI の `HorizontalAlignment` は指定しなくても `Stretch` なので、
/// プロパティを読むだけでは「`Fill` と言われた」のか「既定のまま」なのかを
/// 区別できない。グリッドのマスの中でだけこの違いが要るため、目印を残す。
const FILL_TAG: &str = "naui:fill:";
const PARENT_TAG: &str = "parent=";

fn set_fill_marker(element: &FrameworkElement, sizing: Sizing) {
    let mut value = String::from(FILL_TAG);
    if sizing.width.is_fill() {
        value.push('w');
    }
    if sizing.height.is_fill() {
        value.push('h');
    }
    let parent = tag_text(element)
        .as_deref()
        .and_then(parent_value)
        .map(str::to_owned);
    if let Some(parent) = parent {
        value.push('|');
        value.push_str(PARENT_TAG);
        value.push_str(&parent);
    }
    set_tag_text(element, value);
}

/// この要素がその方向へ `Fill` を指定されたか。
pub(crate) fn wants_fill(element: &FrameworkElement, horizontal: bool) -> bool {
    let Some(text) = tag_text(element) else {
        return false;
    };
    let Some(flags) = fill_flags(&text) else {
        return false;
    };
    flags.contains(if horizontal { 'w' } else { 'h' })
}

fn tag_text(element: &FrameworkElement) -> Option<String> {
    let tag = element.Tag().ok()?;
    let value = tag.cast::<windows::Foundation::IPropertyValue>().ok()?;
    value.GetString().ok().map(|text| text.to_string())
}

fn set_tag_text(element: &FrameworkElement, text: String) {
    if let Ok(tag) = windows::Foundation::PropertyValue::CreateString(&HSTRING::from(text)) {
        let _ = element.SetTag(&tag);
    }
}

fn fill_flags(text: &str) -> Option<&str> {
    text.strip_prefix(FILL_TAG)
        .map(|value| value.split('|').next().unwrap_or_default())
}

fn parent_value(text: &str) -> Option<&str> {
    text.split('|')
        .find_map(|part| part.strip_prefix(PARENT_TAG))
}

fn rewrite_parent_marker(element: &FrameworkElement, parent: Option<&str>) {
    let current = tag_text(element);
    // Tag が naui の目印でない場合、削除はアプリ側の値を触らない。
    if parent.is_none() && current.as_deref().and_then(fill_flags).is_none() {
        return;
    }
    let flags = current.as_deref().and_then(fill_flags).unwrap_or_default();
    let mut value = String::from(FILL_TAG);
    value.push_str(flags);
    if let Some(parent) = parent {
        value.push('|');
        value.push_str(PARENT_TAG);
        value.push_str(parent);
    }
    set_tag_text(element, value);
}

fn align_tag(align: Align) -> &'static str {
    match align {
        Align::Start => "start",
        Align::Center => "center",
        Align::End => "end",
        Align::Fill => "fill",
    }
}

fn parse_align(value: &str) -> Option<Align> {
    match value {
        "start" => Some(Align::Start),
        "center" => Some(Align::Center),
        "end" => Some(Align::End),
        "fill" => Some(Align::Fill),
        _ => None,
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

/// Stack の親情報を子へ記録し、現在の寄せ方も適用する。
pub(crate) fn set_stack_parent(element: &UIElement, align: Align, vertical_stack: bool) {
    let Ok(element) = element.cast::<FrameworkElement>() else {
        return;
    };
    let parent = format!(
        "stack:{}:{}",
        if vertical_stack { "v" } else { "h" },
        align_tag(align)
    );
    rewrite_parent_marker(&element, Some(&parent));
    apply_stack_alignment(&element, align, vertical_stack);
}

/// Scroll の親情報を子へ記録し、スクロールしない軸の Stretch も適用する。
pub(crate) fn set_scroll_parent(
    element: &UIElement,
    horizontal_scroll_enabled: bool,
    vertical_scroll_enabled: bool,
) {
    let Ok(element) = element.cast::<FrameworkElement>() else {
        return;
    };
    let parent = format!(
        "scroll:{}:{}",
        if horizontal_scroll_enabled { "1" } else { "0" },
        if vertical_scroll_enabled { "1" } else { "0" }
    );
    rewrite_parent_marker(&element, Some(&parent));
    stretch_non_scrolling_child(&element, horizontal_scroll_enabled, vertical_scroll_enabled);
}

/// 親コンテナのレイアウトを、Sizing が上書きした後にもう一度適用する。
pub(crate) fn reapply_parent_layout(element: &FrameworkElement) {
    let Some(parent) = tag_text(element)
        .as_deref()
        .and_then(parent_value)
        .map(str::to_owned)
    else {
        return;
    };
    let mut parts = parent.split(':');
    match parts.next() {
        Some("stack") => {
            let Some(vertical_stack) = parts.next().and_then(|axis| match axis {
                "v" => Some(true),
                "h" => Some(false),
                _ => None,
            }) else {
                return;
            };
            let Some(align) = parts.next().and_then(parse_align) else {
                return;
            };
            apply_stack_alignment(element, align, vertical_stack);
        }
        Some("scroll") => {
            let Some(horizontal_scroll_enabled) = parts.next().and_then(parse_bool) else {
                return;
            };
            let Some(vertical_scroll_enabled) = parts.next().and_then(parse_bool) else {
                return;
            };
            stretch_non_scrolling_child(
                element,
                horizontal_scroll_enabled,
                vertical_scroll_enabled,
            );
        }
        _ => {}
    }
}

/// 親コンテナの記録を外す。
pub(crate) fn clear_parent_layout(element: &UIElement) {
    let Ok(element) = element.cast::<FrameworkElement>() else {
        return;
    };
    rewrite_parent_marker(&element, None);
}

/// 大きさの指定を要素へ反映する。呼ぶたびに以前の指定は置き換わる。
pub(crate) fn apply_sizing(element: &UIElement, sizing: Sizing) {
    let Ok(element) = element.cast::<FrameworkElement>() else {
        return;
    };
    set_fill_marker(&element, sizing);
    // WinUI では NaN が「中身に合わせる」を表す。
    let _ = element.SetWidth(sizing.width.fixed_value().unwrap_or(f64::NAN));
    let _ = element.SetHeight(sizing.height.fixed_value().unwrap_or(f64::NAN));
    let _ = element.SetMinWidth(sizing.min_width.unwrap_or(0.0));
    let _ = element.SetMinHeight(sizing.min_height.unwrap_or(0.0));
    let _ = element.SetMaxWidth(sizing.max_width.unwrap_or(f64::INFINITY));
    let _ = element.SetMaxHeight(sizing.max_height.unwrap_or(f64::INFINITY));

    let _ = element.SetHorizontalAlignment(match sizing.width {
        Length::Fill => HorizontalAlignment::Stretch,
        _ => HorizontalAlignment::Left,
    });
    let _ = element.SetVerticalAlignment(match sizing.height {
        Length::Fill => VerticalAlignment::Stretch,
        _ => VerticalAlignment::Top,
    });
    reapply_parent_layout(&element);
}

/// `Stack::set_align` を StackPanel の子要素へ写す。
///
/// StackPanel 自身の配置は親の中でパネルを置く位置を変えるだけなので、
/// 交差軸の寄せ方は子へ設定する。子自身の `Fill` がある場合はそれを優先する。
fn apply_stack_alignment(element: &FrameworkElement, align: Align, vertical_stack: bool) {
    if wants_fill(element, vertical_stack) {
        return;
    }
    if vertical_stack {
        let value = match align {
            Align::Start => HorizontalAlignment::Left,
            Align::Center => HorizontalAlignment::Center,
            Align::End => HorizontalAlignment::Right,
            Align::Fill => HorizontalAlignment::Stretch,
        };
        let _ = element.SetHorizontalAlignment(value);
    } else {
        let value = match align {
            Align::Start => VerticalAlignment::Top,
            Align::Center => VerticalAlignment::Center,
            Align::End => VerticalAlignment::Bottom,
            Align::Fill => VerticalAlignment::Stretch,
        };
        let _ = element.SetVerticalAlignment(value);
    }
}

fn grid_length(track: Track) -> GridLength {
    match track {
        Track::Auto => GridLength {
            Value: 1.0,
            GridUnitType: GridUnitType::Auto,
        },
        Track::Fixed(value) => GridLength {
            Value: value,
            GridUnitType: GridUnitType::Pixel,
        },
        Track::Fill(_) => GridLength {
            Value: track.weight(),
            GridUnitType: GridUnitType::Star,
        },
    }
}

// ----------------------------------------------------------------- Spacer

struct SpacerInner {
    native: XamlGrid,
}

/// 余白そのものになるウィジェット (中身が空の Grid)。
///
/// WinUI の `StackPanel` は余りを子へ配らないため、`Stack` の中では
/// 場所を取らない。余りを分けたいときは [`Grid`] の [`Track::Fill`] を使う。
#[derive(Clone)]
pub struct Spacer(Rc<SpacerInner>);
impl_widget!(Spacer, native);

impl Spacer {
    pub(crate) fn new() -> Result<Self> {
        let native = XamlGrid::new().map_err(|e| to_error("Spacer の生成", e))?;
        let this = Self(Rc::new(SpacerInner { native }));
        this.set_sizing(Sizing::fill());
        Ok(this)
    }
}

// ------------------------------------------------------------------- Grid

struct GridInner {
    native: XamlGrid,
    /// 置いた子と、その置き場所。マス単位で外すために持つ。
    children: RefCell<Vec<(GridCell, Box<dyn Widget>)>>,
    columns: Cell<usize>,
    rows: Cell<usize>,
}

/// 行と列で位置を決めるコンテナ (WinUI 3 の Grid)。
#[derive(Clone)]
pub struct Grid(Rc<GridInner>);
impl_widget!(Grid, native);

impl Grid {
    pub(crate) fn new() -> Result<Self> {
        let native = XamlGrid::new().map_err(|e| to_error("Grid の生成", e))?;
        Ok(Self(Rc::new(GridInner {
            native,
            children: RefCell::new(Vec::new()),
            columns: Cell::new(0),
            rows: Cell::new(0),
        })))
    }

    /// 列間・行間のすき間。
    pub fn set_spacing(&self, column: f64, row: f64) {
        let _ = self.0.native.SetColumnSpacing(column);
        let _ = self.0.native.SetRowSpacing(row);
    }

    /// 外周の余白。
    pub fn set_padding(&self, padding: Padding) {
        let _ = self.0.native.SetPadding(Thickness {
            Left: padding.left,
            Top: padding.top,
            Right: padding.right,
            Bottom: padding.bottom,
        });
    }

    /// 指定した場所に子を置く。足りない行と列は自動で足される。
    pub fn attach(&self, child: &dyn Widget, cell: GridCell) {
        self.ensure_size(cell.columns_needed(), cell.rows_needed());
        let element = child.native_element();
        // 置き場所は添付プロパティなので、FrameworkElement として設定する。
        if let Ok(framework) = element.cast::<FrameworkElement>() {
            let _ = XamlGrid::SetColumn(&framework, cell.column as i32);
            let _ = XamlGrid::SetRow(&framework, cell.row as i32);
            let _ = XamlGrid::SetColumnSpan(&framework, cell.column_span as i32);
            let _ = XamlGrid::SetRowSpan(&framework, cell.row_span as i32);
            // 縦は中央ぞろえ。既定の Stretch のままだと、同じ行に置いた
            // ラベルと入力欄のように高さの違うものが上端で揃ってしまう。
            let _ = framework.SetVerticalAlignment(if wants_fill(&framework, false) {
                VerticalAlignment::Stretch
            } else {
                VerticalAlignment::Center
            });
        }
        let appended = self
            .0
            .native
            .Children()
            .and_then(|children| children.Append(&element));
        if appended.is_ok() {
            self.0
                .children
                .borrow_mut()
                .push((cell, child.boxed_clone()));
        }
    }

    /// そのマスの中身を差し替える。同じマスに置かれていたものは外れる。
    ///
    /// `MediaPlayerElement` のように、TabView のコンテンツ切り替え時に
    /// WinUI 内部でテンプレート適用が走るコントロールを安全に差し替える
    /// ために使う。
    pub fn replace(&self, child: &dyn Widget, cell: GridCell) {
        self.remove(cell);
        self.attach(child, cell);
    }

    /// 指定したマスに置かれているものを外す。何も無ければ何もしない。
    ///
    /// 見るのは `cell` の列と行だけで、span は見ない。
    pub fn remove(&self, cell: GridCell) {
        let mut children = self.0.children.borrow_mut();
        let mut index = 0;
        while index < children.len() {
            if children[index].0.column != cell.column || children[index].0.row != cell.row {
                index += 1;
                continue;
            }
            // `attach` は Children へ順に足すので、位置はここの並びと同じ。
            let removed = self
                .0
                .native
                .Children()
                .and_then(|c| c.RemoveAt(index as u32));
            if removed.is_err() {
                index += 1;
                continue;
            }
            children.remove(index);
        }
    }

    /// 子をすべて外す。行と列の指定はそのまま残る。
    pub fn clear(&self) {
        if let Ok(children) = self.0.native.Children() {
            let _ = children.Clear();
        }
        self.0.children.borrow_mut().clear();
    }

    /// 列の幅の決め方。
    pub fn set_column_track(&self, index: usize, track: Track) {
        self.ensure_size(index + 1, 0);
        if let Ok(definition) = self
            .0
            .native
            .ColumnDefinitions()
            .and_then(|definitions| definitions.GetAt(index as u32))
        {
            let _ = definition.SetWidth(grid_length(track));
        }
    }

    /// 行の高さの決め方。
    pub fn set_row_track(&self, index: usize, track: Track) {
        self.ensure_size(0, index + 1);
        if let Ok(definition) = self
            .0
            .native
            .RowDefinitions()
            .and_then(|definitions| definitions.GetAt(index as u32))
        {
            let _ = definition.SetHeight(grid_length(track));
        }
    }

    /// いまある列数。
    pub fn columns(&self) -> usize {
        self.0.columns.get()
    }

    /// いまある行数。
    pub fn rows(&self) -> usize {
        self.0.rows.get()
    }

    /// 置いた子の数。
    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn ensure_size(&self, columns: usize, rows: usize) {
        while self.0.columns.get() < columns {
            let Ok(definition) = ColumnDefinition::new() else {
                break;
            };
            // 既定 (Star) ではなく「中身に合わせる」から始める。
            let _ = definition.SetWidth(grid_length(Track::Auto));
            let appended = self
                .0
                .native
                .ColumnDefinitions()
                .and_then(|definitions| definitions.Append(&definition));
            if appended.is_err() {
                break;
            }
            self.0.columns.set(self.0.columns.get() + 1);
        }
        while self.0.rows.get() < rows {
            let Ok(definition) = RowDefinition::new() else {
                break;
            };
            let _ = definition.SetHeight(grid_length(Track::Auto));
            let appended = self
                .0
                .native
                .RowDefinitions()
                .and_then(|definitions| definitions.Append(&definition));
            if appended.is_err() {
                break;
            }
            self.0.rows.set(self.0.rows.get() + 1);
        }
    }
}

// ----------------------------------------------------------------- Scroll

struct ScrollInner {
    native: ScrollViewer,
    child: RefCell<Option<Box<dyn Widget>>>,
    horizontal_scroll_enabled: Cell<bool>,
    vertical_scroll_enabled: Cell<bool>,
    /// ホイール入力時に、ポインター直下の ScrollViewer だけを選ぶための状態。
    hovered: std::sync::Arc<crate::ui_thread::UiThreadCell<usize>>,
}

thread_local! {
    static SCROLLS: RefCell<Vec<Weak<ScrollInner>>> = const { RefCell::new(Vec::new()) };
    static LIST_SCROLLS: RefCell<Vec<Weak<ListScrollTarget>>> = const { RefCell::new(Vec::new()) };
    static WHEEL_TARGETS: RefCell<Vec<windows::Win32::Foundation::HWND>> =
        const { RefCell::new(Vec::new()) };
    static WHEEL_HOOK: RefCell<Option<windows::Win32::UI::WindowsAndMessaging::HHOOK>> =
        const { RefCell::new(None) };
}

const WHEEL_SUBCLASS_ID: usize = 0x4D49_5549;

unsafe extern "system" fn low_level_mouse_hook(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetAncestor, GetForegroundWindow, GA_ROOT, MSLLHOOKSTRUCT, WM_MOUSEWHEEL,
    };

    if code >= 0 {
        let input = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let foreground_root = GetAncestor(GetForegroundWindow(), GA_ROOT);
        let over_target_window = WHEEL_TARGETS.with(|targets| {
            targets
                .borrow()
                .iter()
                .any(|target| GetAncestor(*target, GA_ROOT) == foreground_root)
        });

        if over_target_window && wparam.0 as u32 == WM_MOUSEWHEEL {
            let delta = ((input.mouseData >> 16) as i16) as f64;
            if delta != 0.0 && apply_wheel_delta(delta) {
                return windows::Win32::Foundation::LRESULT(1);
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

fn install_low_level_wheel_hook(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WH_MOUSE_LL};

    WHEEL_TARGETS.with(|targets| {
        let mut targets = targets.borrow_mut();
        if !targets.contains(&hwnd) {
            targets.push(hwnd);
        }
    });
    WHEEL_HOOK.with(|hook| {
        let mut hook = hook.borrow_mut();
        if hook.is_some() {
            return true;
        }
        let Ok(handle) =
            (unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_hook), None, 0) })
        else {
            return false;
        };
        *hook = Some(handle);
        true
    })
}

fn apply_wheel_delta(delta: f64) -> bool {
    if apply_list_wheel_delta(delta) {
        return true;
    }
    apply_scroll_wheel_delta(delta)
}

/// ポインター直下にある Scroll のうち、ビジュアルツリーで最も深いものだけを送る。
///
/// スクロール可能な内側の Scroll が端に達していても入力を消費し、外側への
/// 意図しないスクロール連鎖を防ぐ。内側にオーバーフローがない場合だけ外側へ渡す。
fn apply_scroll_wheel_delta(delta: f64) -> bool {
    SCROLLS.with(|scrolls| {
        let mut scrolls = scrolls.borrow_mut();
        scrolls.retain(|scroll| scroll.strong_count() != 0);
        let target = scrolls
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|inner| inner.vertical_scroll_enabled.get())
            .filter(|inner| inner.hovered.with_mut(|depth| *depth != 0))
            .filter(|inner| inner.native.ScrollableHeight().unwrap_or(0.0) > 0.0)
            .filter_map(|inner| visual_depth(&inner.native).map(|depth| (depth, inner)))
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, inner)| inner);

        let Some(inner) = target else {
            return false;
        };
        let current = inner.native.VerticalOffset().unwrap_or(0.0);
        let maximum = inner.native.ScrollableHeight().unwrap_or(0.0);
        let next = (current - delta).clamp(0.0, maximum);
        if next != current {
            let _ = inner.native.ScrollToVerticalOffset(next);
        }
        true
    })
}

fn visual_depth(native: &ScrollViewer) -> Option<usize> {
    let mut current = Some(native.cast::<FrameworkElement>().ok()?);
    let mut depth = 0;
    while let Some(element) = current {
        let ui_element = element.cast::<UIElement>().ok()?;
        if ui_element.Visibility().ok()? != Visibility::Visible {
            return None;
        }
        current = element
            .Parent()
            .ok()
            .and_then(|parent| parent.cast::<FrameworkElement>().ok());
        depth += 1;
    }
    Some(depth)
}

fn apply_list_wheel_delta(delta: f64) -> bool {
    LIST_SCROLLS.with(|targets| {
        let mut targets = targets.borrow_mut();
        targets.retain(|target| target.strong_count() != 0);
        for weak in targets.iter().rev() {
            let Some(target) = weak.upgrade() else {
                continue;
            };
            let hovered = target.hovered.with_mut(|depth| *depth != 0);
            if !hovered {
                continue;
            }
            let current = target.native.VerticalOffset().unwrap_or(0.0);
            let maximum = target.native.ScrollableHeight().unwrap_or(0.0);
            if maximum <= 0.0 {
                continue;
            }
            let next = (current - delta).clamp(0.0, maximum);
            if next != current {
                let _ = target.native.ScrollToVerticalOffset(next);
            }
            // List が端に達していても外側の Scroll へ連鎖させない。
            return true;
        }
        false
    })
}

unsafe extern "system" fn wheel_subclass(
    hwnd: windows::Win32::Foundation::HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    subclass_id: usize,
    _ref_data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{WM_MOUSEWHEEL, WM_POINTERWHEEL};

    if message == WM_MOUSEWHEEL || message == WM_POINTERWHEEL {
        let delta = ((wparam.0 >> 16) as i16) as f64;
        if delta != 0.0 && apply_wheel_delta(delta) {
            return windows::Win32::Foundation::LRESULT(0);
        }
    }

    if message == windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY {
        let result = DefSubclassProc(hwnd, message, wparam, lparam);
        let _ = RemoveWindowSubclass(hwnd, Some(wheel_subclass), subclass_id);
        return result;
    }

    DefSubclassProc(hwnd, message, wparam, lparam)
}

pub(crate) fn register_scroll(scroll: &Scroll) {
    SCROLLS.with(|scrolls| {
        let mut scrolls = scrolls.borrow_mut();
        scrolls.retain(|scroll| scroll.strong_count() != 0);
        scrolls.push(Rc::downgrade(&scroll.0));
    });
}

fn track_scroll_pointer(scroll: &Scroll) -> Result<()> {
    use naui_winui3::Microsoft::UI::Xaml::Input::PointerEventHandler;

    let entered_state = scroll.0.hovered.clone();
    let entered = PointerEventHandler::new(move |_, _| {
        entered_state.with_mut(|depth| *depth = depth.saturating_add(1));
        Ok(())
    });
    scroll
        .0
        .native
        .PointerEntered(&entered)
        .map_err(|e| to_error("Scroll のポインター購読", e))?;

    let exited_state = scroll.0.hovered.clone();
    let exited = PointerEventHandler::new(move |_, _| {
        exited_state.with_mut(|depth| *depth = depth.saturating_sub(1));
        Ok(())
    });
    scroll
        .0
        .native
        .PointerExited(&exited)
        .map_err(|e| to_error("Scroll のポインター購読", e))?;

    // タブ切り替えなどで PointerEntered が発生しない場合にも、次のホイール
    // 入力までに対象を確定できるよう PointerMoved で補正する。
    let moved_state = scroll.0.hovered.clone();
    let moved = PointerEventHandler::new(move |_, _| {
        moved_state.with_mut(|depth| {
            if *depth == 0 {
                *depth = 1;
            }
        });
        Ok(())
    });
    scroll
        .0
        .native
        .PointerMoved(&moved)
        .map_err(|e| to_error("Scroll のポインター購読", e))?;

    Ok(())
}

pub(crate) struct ListScrollTarget {
    native: ScrollViewer,
    hovered: std::sync::Arc<crate::ui_thread::UiThreadCell<usize>>,
}

/// その要素の中にある最初の `ScrollViewer` を、深さ優先で探す。
///
/// `ListView` や `TreeView` はテンプレートの中に自分の `ScrollViewer` を
/// 持つ。段数はテンプレート次第なので、名前ではなく型で探す。中身は
/// `Loaded` まで組み上がらないので、そこまで待ってから呼ぶ。
pub(crate) fn scroll_viewer_within<T: Interface>(root: &T) -> Option<ScrollViewer> {
    let root = root.cast::<DependencyObject>().ok()?;
    let mut queue = vec![root];
    while let Some(element) = queue.pop() {
        if let Ok(scroll) = element.cast::<ScrollViewer>() {
            return Some(scroll);
        }
        let count = VisualTreeHelper::GetChildrenCount(&element).unwrap_or(0);
        for index in (0..count).rev() {
            if let Ok(child) = VisualTreeHelper::GetChild(&element, index) {
                queue.push(child);
            }
        }
    }
    None
}

pub(crate) fn register_list_scroll(
    native: ScrollViewer,
    hovered: std::sync::Arc<crate::ui_thread::UiThreadCell<usize>>,
) -> Rc<ListScrollTarget> {
    let target = Rc::new(ListScrollTarget { native, hovered });
    LIST_SCROLLS.with(|targets| {
        let mut targets = targets.borrow_mut();
        targets.retain(|target| target.strong_count() != 0);
        targets.push(Rc::downgrade(&target));
    });
    target
}

pub(crate) fn install_wheel_subclass(window: &XamlWindow) -> bool {
    let Ok(native) = window.cast::<naui_winui3::IWindowNative>() else {
        return false;
    };
    let Ok(hwnd) = (unsafe { native.WindowHandle() }) else {
        return false;
    };
    let low_level_hook_installed = install_low_level_wheel_hook(hwnd);
    let has_scroll = SCROLLS.with(|scrolls| {
        let mut scrolls = scrolls.borrow_mut();
        scrolls.retain(|scroll| scroll.strong_count() != 0);
        !scrolls.is_empty()
    });
    if !has_scroll {
        return low_level_hook_installed;
    }
    let installed = unsafe {
        windows::Win32::UI::Shell::SetWindowSubclass(
            hwnd,
            Some(wheel_subclass),
            WHEEL_SUBCLASS_ID,
            1,
        )
        .as_bool()
    };
    installed || low_level_hook_installed
}

/// 中身がはみ出したらスクロールさせるコンテナ (ScrollViewer)。
#[derive(Clone)]
pub struct Scroll(Rc<ScrollInner>);
impl_widget!(Scroll, native);

/// 横 / 縦それぞれのスクロールを許すかどうか。
///
/// `ScrollBarVisibility` はスクロールバーの見せかたで、スクロールそのものを
/// 止めるのは `ScrollMode` のほう。両方そろえないと、バーが消えたまま
/// ホイールでは動く、という食い違いが起きる。
fn set_scroll_mode(native: &ScrollViewer, horizontal: ScrollPolicy, vertical: ScrollPolicy) {
    fn mode(policy: ScrollPolicy) -> ScrollMode {
        match policy {
            ScrollPolicy::Never => ScrollMode::Disabled,
            _ => ScrollMode::Enabled,
        }
    }
    let _ = native.SetHorizontalScrollMode(mode(horizontal));
    let _ = native.SetVerticalScrollMode(mode(vertical));
}

impl Scroll {
    pub(crate) fn new() -> Result<Self> {
        let native = ScrollViewer::new().map_err(|e| to_error("ScrollViewer の生成", e))?;
        let this = Self(Rc::new(ScrollInner {
            native,
            child: RefCell::new(None),
            horizontal_scroll_enabled: Cell::new(false),
            vertical_scroll_enabled: Cell::new(true),
            hovered: std::sync::Arc::new(crate::ui_thread::UiThreadCell::new(0)),
        }));
        register_scroll(&this);
        this.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
        track_scroll_pointer(&this)?;
        Ok(this)
    }

    /// 横 / 縦それぞれのスクロールの許可。既定は横 `Never`・縦 `Auto`。
    pub fn set_policy(&self, horizontal: ScrollPolicy, vertical: ScrollPolicy) {
        self.0
            .horizontal_scroll_enabled
            .set(horizontal.is_enabled());
        self.0
            .vertical_scroll_enabled
            .set(!matches!(vertical, ScrollPolicy::Never));
        set_scroll_mode(&self.0.native, horizontal, vertical);
        // `Control` の内容配置の既定は左上寄せなので、スクロールしない軸
        // だけはビューポートいっぱいへ広げる。スクロールする軸は内容の
        // 自然な大きさを残し、必要ならそのまま送れるようにする。
        let _ = self
            .0
            .native
            .SetHorizontalContentAlignment(if horizontal.is_enabled() {
                HorizontalAlignment::Left
            } else {
                HorizontalAlignment::Stretch
            });
        let _ = self
            .0
            .native
            .SetVerticalContentAlignment(if vertical.is_enabled() {
                VerticalAlignment::Top
            } else {
                VerticalAlignment::Stretch
            });
        if let Some(child) = self.0.child.borrow().as_ref() {
            set_scroll_parent(
                &child.native_element(),
                horizontal.is_enabled(),
                vertical.is_enabled(),
            );
        }
        let _ = self
            .0
            .native
            .SetHorizontalScrollBarVisibility(visibility(horizontal));
        let _ = self
            .0
            .native
            .SetVerticalScrollBarVisibility(visibility(vertical));
    }

    /// スクロールさせる中身。呼ぶたびに置き換わる。
    pub fn set_child(&self, child: &dyn Widget) {
        let element = child.native_element();
        if let Some(previous) = self.0.child.borrow().as_ref() {
            clear_parent_layout(&previous.native_element());
        }
        set_scroll_parent(
            &element,
            self.0.horizontal_scroll_enabled.get(),
            self.0.vertical_scroll_enabled.get(),
        );
        if self.0.native.SetContent(&element).is_ok() {
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }
}

/// スクロールしない軸の内容を、ビューポートの大きさまで広げる。
///
/// 明示的な大きさや上限を付けた子はアプリの指定を優先する。`Fill` は
/// `HorizontalAlignment` / `VerticalAlignment` が既に `Stretch` なので、
/// ここで同じ値を書き直しても意味は変わらない。
fn stretch_non_scrolling_child(
    element: &FrameworkElement,
    horizontal_scroll_enabled: bool,
    vertical_scroll_enabled: bool,
) {
    if !horizontal_scroll_enabled {
        let width = element.Width().unwrap_or(f64::NAN);
        let max_width = element.MaxWidth().unwrap_or(f64::INFINITY);
        if width.is_nan() && max_width.is_infinite() {
            let _ = element.SetHorizontalAlignment(HorizontalAlignment::Stretch);
        }
    }
    if !vertical_scroll_enabled {
        let height = element.Height().unwrap_or(f64::NAN);
        let max_height = element.MaxHeight().unwrap_or(f64::INFINITY);
        if height.is_nan() && max_height.is_infinite() {
            let _ = element.SetVerticalAlignment(VerticalAlignment::Stretch);
        }
    }
}

fn visibility(policy: ScrollPolicy) -> ScrollBarVisibility {
    match policy {
        ScrollPolicy::Auto => ScrollBarVisibility::Auto,
        ScrollPolicy::Always => ScrollBarVisibility::Visible,
        ScrollPolicy::Never => ScrollBarVisibility::Disabled,
    }
}
