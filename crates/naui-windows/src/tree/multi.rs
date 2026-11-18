//! The surface `Tree` uses for multiple selection (WinUI 3).
//!
//! WinUI's `TreeView` has only one kind of multiple selection: a check box on every
//! row, where selecting a branch also selects all its descendants and selecting all
//! the children selects their parent (`ViewModel::UpdateSelection` in WinUI; it
//! cannot be turned off). naui's [`SelectionMode::Multiple`] means "select with Ctrl
//! or Shift, each item on its own", the same as `List`, so for multiple selection the
//! tree is laid out in a `ListView` with `SelectionMode="Extended"` instead, inside the
//! same frame.
//!
//! | Part | Built from |
//! | --- | --- |
//! | Rows | A `ListViewItem` per item, built in advance for every item |
//! | Indentation | The left margin of the row's content (16 per level, as on GTK and the Web) |
//! | Expand / collapse | A borderless `Button` with the `TreeViewItem` chevrons (`\u{E76C}` / `\u{E70D}`) |
//! | Selection | The `ListView`'s own Ctrl / Shift handling (`Extended`) |
//!
//! Collapsing a branch takes its rows out of the `ListView` (and expanding puts them
//! back), so the `ListView` holds only the visible rows. Selected items inside a
//! collapsed branch are kept by naui and written back when they come back, following
//! the same rule as rows outside the window of a long `List`
//! ([`keeps_hidden_selection`]).
//!
//! [`SelectionMode::Multiple`]: naui_core::SelectionMode::Multiple

use std::rc::Rc;

use naui_core::{keeps_hidden_selection, Result, TreeItem};
use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, FontIcon, Grid as XamlGrid, ListView, ListViewItem,
    SelectionChangedEventHandler,
};
use naui_winui3::Microsoft::UI::Xaml::Input::KeyEventHandler;
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{
    FlowDirection, FocusState, FrameworkElement, HorizontalAlignment, RoutedEventHandler,
    Thickness, UIElement,
};
use windows::System::VirtualKey;
use windows_core::{IInspectable, Interface, HSTRING};

use super::Tree;
use crate::layout::ListScrollTarget;
use crate::list::{hold_unselectable_press, item_content, new_row_item, transparent_grid};
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::Widget;

/// The indentation per level (logical pixels). The same as GTK and the Web.
const INDENT: f64 = 16.0;

/// The chevron of a collapsed branch (`TreeViewItem`'s `CollapsedGlyph`).
const GLYPH_COLLAPSED: &str = "\u{E76C}";
/// The chevron of an expanded branch (`TreeViewItem`'s `ExpandedGlyph`).
const GLYPH_EXPANDED: &str = "\u{E70D}";

/// The `ListView`. Rows are added and removed as branches are expanded and collapsed,
/// so the item transitions are cleared (otherwise every row slides in on expanding).
const LIST_XAML: &str = r##"<ListView
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent" BorderThickness="0" Padding="0"
    SelectionMode="Extended" HorizontalContentAlignment="Stretch">
    <ListView.ItemContainerTransitions>
        <TransitionCollection/>
    </ListView.ItemContainerTransitions>
</ListView>"##;

/// One row's content: the chevron in the first column, the text in the second.
///
/// The chevron is a borderless `Button`, so pressing it does not select the row (the
/// button takes the press) and it is not a tab stop of its own, as in `TreeViewItem`.
/// Leaves keep its space with the button hidden, so their text lines up.
const ROW_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent">
    <Grid.ColumnDefinitions>
        <ColumnDefinition Width="Auto"/>
        <ColumnDefinition Width="*"/>
    </Grid.ColumnDefinitions>
    <Button Width="24" Height="24" MinWidth="0" MinHeight="0" Padding="0" Margin="0,0,4,0"
        Background="Transparent" BorderThickness="0" IsTabStop="False"
        VerticalAlignment="Center">
        <FontIcon FontFamily="{ThemeResource SymbolThemeFontFamily}" FontSize="12"/>
    </Button>
</Grid>"##;

/// One row of the multiple selection surface.
pub(super) struct MultiRow {
    path: Vec<usize>,
    item: ListViewItem,
    /// The chevron. `None` for leaves.
    glyph: Option<FontIcon>,
}

/// The `ListView` that stands in for the `TreeView` during multiple selection.
pub(super) struct MultiView {
    list_view: ListView,
    /// The rows of every item, depth-first (parents first), that is, in lexicographic
    /// order of paths.
    rows: Vec<MultiRow>,
    /// The rows in the `ListView` now, as positions in `rows`, from the top.
    shown: Vec<usize>,
    /// The registration with the wheel helper, made on `Loaded`.
    wheel: Option<Rc<ListScrollTarget>>,
    /// The destination of a scroll requested before the `ListView` was loaded.
    pending_scroll: Option<Vec<usize>>,
}

impl MultiView {
    fn row_of(&self, path: &[usize]) -> Option<usize> {
        self.rows
            .binary_search_by(|row| row.path.as_slice().cmp(path))
            .ok()
    }
}

impl Tree {
    /// Shows the multiple selection surface in the frame, creating it the first time.
    pub(super) fn enter_multi(&self) -> Result<()> {
        if self.0.multi.borrow().is_none() {
            let list_view = self.create_multi_list()?;
            *self.0.multi.borrow_mut() = Some(MultiView {
                list_view,
                rows: Vec::new(),
                shown: Vec::new(),
                wheel: None,
                pending_scroll: None,
            });
        }
        let list_view = self.multi_list_view();
        if let Some(list_view) = list_view {
            let _ = self.0.native.SetChild(&list_view);
        }
        self.multi_rebuild()
    }

    /// Puts the `TreeView` back in the frame.
    pub(super) fn leave_multi(&self) {
        let _ = self.0.native.SetChild(&self.0.tree_view);
    }

    fn multi_list_view(&self) -> Option<ListView> {
        self.0
            .multi
            .borrow()
            .as_ref()
            .map(|multi| multi.list_view.clone())
    }

    /// Creates the `ListView` and subscribes to it.
    fn create_multi_list(&self) -> Result<ListView> {
        let list_view = XamlReader::Load(&HSTRING::from(LIST_XAML))
            .and_then(|element| element.cast::<ListView>())
            .map_err(|e| to_error("create the tree's ListView", e))?;

        // 選択を書き戻すと `SelectionChanged` がその場で起きるので、再入は取りこぼしと
        // して扱える `try_with_mut` で受ける (その間は `silent` が立っている)。
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let changed = SelectionChangedEventHandler::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    let tree = Tree(inner);
                    if !tree.0.silent.get() {
                        tree.on_multi_selection();
                    }
                }
            });
            Ok(())
        });
        list_view
            .SelectionChanged(&changed)
            .map_err(|e| to_error("subscribe to the tree's ListView", e))?;

        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let loaded = RoutedEventHandler::new(move |_, _| {
            let _ = state.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    Tree(inner).on_multi_loaded();
                }
            });
            Ok(())
        });
        list_view
            .Loaded(&loaded)
            .map_err(|e| to_error("subscribe to the tree's ListView Loaded", e))?;
        Ok(list_view)
    }

    /// Registers the `ListView`'s `ScrollViewer` with the wheel helper, writes the
    /// selection again (each time it is put on screen) and scrolls to what was asked
    /// before it was loaded.
    fn on_multi_loaded(&self) {
        let pending = {
            let mut multi = self.0.multi.borrow_mut();
            let Some(multi) = multi.as_mut() else {
                return;
            };
            if multi.wheel.is_none() {
                if let Some(scroll) = crate::layout::scroll_viewer_within(&multi.list_view) {
                    multi.wheel = Some(crate::layout::register_list_scroll(
                        scroll,
                        self.0.hovered.clone(),
                    ));
                }
            }
            multi.pending_scroll.take()
        };
        // 載る前に行へ書いた `IsSelected` は、載るときに `ListView` に消される
        // (`List` と同じ)。覚えている選択を書き直す。
        self.multi_write_selection();
        if let Some(path) = pending {
            self.multi_scroll_to(&path);
        }
    }

    /// Builds the rows of every item and shows those that are visible.
    pub(super) fn multi_rebuild(&self) -> Result<()> {
        let mut rows = Vec::new();
        {
            let items = self.0.items.borrow();
            let mut failure = None;
            TreeItem::walk(&items, |path, item| {
                if failure.is_some() {
                    return;
                }
                let siblings = match path.len() {
                    1 => items.len(),
                    _ => TreeItem::at(&items, &path[..path.len() - 1])
                        .map_or(0, |parent| parent.children.len()),
                };
                match self.build_multi_row(path, item, TreeItem::selectable(&items, path), siblings)
                {
                    Ok(row) => rows.push(row),
                    Err(error) => failure = Some(error),
                }
            });
            if let Some(error) = failure {
                return Err(error);
            }
        }
        let Some(list_view) = self.multi_list_view() else {
            return Ok(());
        };
        self.without_notifying(|_| {
            let _ = list_view.Items().and_then(|items| items.Clear());
        });
        if let Some(multi) = self.0.multi.borrow_mut().as_mut() {
            multi.rows = rows;
            multi.shown.clear();
        }
        self.multi_sync();
        Ok(())
    }

    /// Builds one row.
    fn build_multi_row(
        &self,
        path: &[usize],
        item: &TreeItem,
        selectable: bool,
        siblings: usize,
    ) -> Result<MultiRow> {
        let native = new_row_item()?;
        let _ = native.SetHorizontalContentAlignment(HorizontalAlignment::Stretch);
        let line = XamlReader::Load(&HSTRING::from(ROW_XAML))
            .and_then(|element| element.cast::<XamlGrid>())
            .map_err(|e| to_error("create the tree row", e))?;
        let depth = path.len().saturating_sub(1) as f64;
        let _ = line.SetMargin(Thickness {
            Left: INDENT * depth,
            Top: 0.0,
            Right: 0.0,
            Bottom: 0.0,
        });
        let twisty = line
            .Children()
            .and_then(|children| children.GetAt(0))
            .and_then(|child| child.cast::<XamlButton>())
            .map_err(|e| to_error("get the tree row's chevron", e))?;
        let glyph = twisty
            .Content()
            .and_then(|content| content.cast::<FontIcon>())
            .map_err(|e| to_error("get the tree row's chevron", e))?;
        let glyph = match item.is_leaf() {
            // 葉は山形の場所だけを空けて、文字の左端をそろえる。
            true => {
                let _ = twisty.SetOpacity(0.0);
                let _ = twisty.SetIsHitTestVisible(false);
                None
            }
            false => {
                let state = UiThreadCell::new(Rc::downgrade(&self.0));
                let target = path.to_vec();
                let clicked = RoutedEventHandler::new(move |_, _| {
                    if let Some(inner) = state.try_with_mut(|weak| weak.upgrade()).flatten() {
                        let tree = Tree(inner);
                        let expanded = tree.is_expanded(&target);
                        tree.toggle(&target, !expanded);
                    }
                    Ok(())
                });
                twisty
                    .Click(&clicked)
                    .map_err(|e| to_error("subscribe to the tree row's chevron", e))?;
                Some(glyph)
            }
        };

        // 文字は `List` と同じ組み方。選べない項目は、押下を受け止める入れ物で包む
        // (行ごと無効にすると、山形まで押せなくなる)。
        let content =
            item_content(&item.label, item.detail.as_deref(), selectable)?.native_element();
        let content: UIElement = match selectable {
            true => content,
            false => {
                let holder = transparent_grid()?;
                hold_unselectable_press(&holder)?;
                holder
                    .Children()
                    .and_then(|children| children.Append(&content))
                    .map_err(|e| to_error("add to the tree row", e))?;
                holder
                    .cast::<UIElement>()
                    .map_err(|e| to_error("convert the tree row", e))?
            }
        };
        if let Ok(framework) = content.cast::<FrameworkElement>() {
            let _ = XamlGrid::SetColumn(&framework, 1);
        }
        line.Children()
            .and_then(|children| children.Append(&content))
            .map_err(|e| to_error("add to the tree row", e))?;
        native
            .SetContent(
                &line
                    .cast::<IInspectable>()
                    .map_err(|e| to_error("convert the tree row", e))?,
            )
            .map_err(|e| to_error("set the tree row content", e))?;

        // 支援技術には、木の中の深さと兄弟の中の位置を伝える。
        let _ = AutomationProperties::SetLevel(&native, path.len() as i32);
        let _ = AutomationProperties::SetPositionInSet(
            &native,
            path.last().map_or(1, |index| *index as i32 + 1),
        );
        let _ = AutomationProperties::SetSizeOfSet(&native, siblings as i32);

        self.listen_multi_keys(&native, path)?;
        Ok(MultiRow {
            path: path.to_vec(),
            item: native,
            glyph,
        })
    }

    /// ← / → on a row collapse and expand it, as in `TreeView`: → expands a collapsed
    /// branch or moves to its first child, ← collapses an expanded branch or moves to
    /// the parent. They are mirrored right to left.
    fn listen_multi_keys(&self, native: &ListViewItem, path: &[usize]) -> Result<()> {
        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let target = path.to_vec();
        let row = native.clone();
        let handler = KeyEventHandler::new(move |_, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let rtl = row.FlowDirection() == Ok(FlowDirection::RightToLeft);
            let key = match (args.Key().unwrap_or(VirtualKey::None), rtl) {
                (VirtualKey::Left, false) | (VirtualKey::Right, true) => VirtualKey::Left,
                (VirtualKey::Right, false) | (VirtualKey::Left, true) => VirtualKey::Right,
                _ => return Ok(()),
            };
            let handled = state
                .try_with_mut(|weak| {
                    weak.upgrade()
                        .is_some_and(|inner| Tree(inner).on_multi_arrow(&target, key))
                })
                .unwrap_or(false);
            if handled {
                let _ = args.SetHandled(true);
            }
            Ok(())
        });
        native
            .KeyDown(&handler)
            .map_err(|e| to_error("subscribe to tree row keys", e))?;
        Ok(())
    }

    /// ← (`VirtualKey::Left`) or → on the row at `path`. Returns whether it was used.
    fn on_multi_arrow(&self, path: &[usize], key: VirtualKey) -> bool {
        let branch = TreeItem::at(&self.0.items.borrow(), path).is_some_and(|item| !item.is_leaf());
        let expanded = self.is_expanded(path);
        match key {
            VirtualKey::Right if branch && !expanded => {
                self.toggle(path, true);
                true
            }
            VirtualKey::Right if branch => {
                let mut child = path.to_vec();
                child.push(0);
                self.focus_multi_row(&child)
            }
            VirtualKey::Left if branch && expanded => {
                self.toggle(path, false);
                true
            }
            VirtualKey::Left if path.len() > 1 => self.focus_multi_row(&path[..path.len() - 1]),
            _ => false,
        }
    }

    /// Moves the keyboard focus to the row at `path` (the selection stays).
    fn focus_multi_row(&self, path: &[usize]) -> bool {
        let item = {
            let multi = self.0.multi.borrow();
            let Some(multi) = multi.as_ref() else {
                return false;
            };
            multi
                .row_of(path)
                .map(|index| multi.rows[index].item.clone())
        };
        item.is_some_and(|item| item.Focus(FocusState::Keyboard).unwrap_or(false))
    }

    /// Makes the `ListView` hold exactly the visible rows, adding and removing only
    /// those that changed (so the scroll position stays), updates the chevrons and
    /// writes the selection back.
    pub(super) fn multi_sync(&self) {
        let wanted: Vec<usize> = {
            let multi = self.0.multi.borrow();
            let Some(multi) = multi.as_ref() else {
                return;
            };
            multi
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| self.is_path_shown(&row.path))
                .map(|(index, _)| index)
                .collect()
        };
        {
            let multi = self.0.multi.borrow();
            let Some(multi) = multi.as_ref() else {
                return;
            };
            let Ok(items) = multi.list_view.Items() else {
                return;
            };
            let previous = self.0.silent.replace(true);
            // どちらも深さ優先の順 (行の番号の昇順) なので、並べて突き合わせる。
            let (mut old, mut new, mut at) = (0, 0, 0u32);
            let shown = &multi.shown;
            while old < shown.len() || new < wanted.len() {
                match (shown.get(old), wanted.get(new)) {
                    (Some(a), Some(b)) if a == b => {
                        old += 1;
                        new += 1;
                        at += 1;
                    }
                    (Some(a), Some(b)) if a < b => {
                        let _ = items.RemoveAt(at);
                        old += 1;
                    }
                    (Some(_), None) => {
                        let _ = items.RemoveAt(at);
                        old += 1;
                    }
                    (_, Some(b)) => {
                        if let Ok(element) = multi.rows[*b].item.cast::<IInspectable>() {
                            let _ = items.InsertAt(at, &element);
                        }
                        new += 1;
                        at += 1;
                    }
                    (None, None) => break,
                }
            }
            self.0.silent.set(previous);
            let expanded = self.0.expanded.borrow();
            for row in &multi.rows {
                if let Some(glyph) = &row.glyph {
                    let open = expanded.contains(&row.path);
                    let _ = glyph.SetGlyph(&HSTRING::from(match open {
                        true => GLYPH_EXPANDED,
                        false => GLYPH_COLLAPSED,
                    }));
                }
            }
        }
        if let Some(multi) = self.0.multi.borrow_mut().as_mut() {
            multi.shown = wanted;
        }
        self.multi_write_selection();
    }

    /// Writes the remembered selection into the rows in the `ListView` (no
    /// notification). Hidden items have no row there; they are written when they come
    /// back.
    pub(super) fn multi_write_selection(&self) {
        let multi = self.0.multi.borrow();
        let Some(multi) = multi.as_ref() else {
            return;
        };
        let selected = self.0.selected.borrow();
        let previous = self.0.silent.replace(true);
        for &index in &multi.shown {
            let row = &multi.rows[index];
            let _ = row
                .item
                .SetIsSelected(selected.binary_search(&row.path).is_ok());
        }
        self.0.silent.set(previous);
    }

    /// When the user changed the selection in the `ListView`.
    fn on_multi_selection(&self) {
        let native: Vec<Vec<usize>> = {
            let multi = self.0.multi.borrow();
            let Some(multi) = multi.as_ref() else {
                return;
            };
            multi
                .shown
                .iter()
                .map(|&index| &multi.rows[index])
                .filter(|row| row.item.IsSelected().unwrap_or(false))
                .map(|row| row.path.clone())
                .collect()
        };
        let before = self.0.selected.borrow().clone();
        let (shown, hidden): (Vec<_>, Vec<_>) = before
            .iter()
            .cloned()
            .partition(|path| self.is_path_shown(path));
        // 見えている選択が覚えているとおりなら、何も変わっていない。枝を閉じて行を
        // 抜いた分や、自分で書いた分が遅れて届いたもの。ここで選び直しとして扱うと、
        // 隠れた選択を落としてしまう。
        if native == shown {
            return;
        }
        // Ctrl は足し引きなので隠れた選択を残し、ふつうのクリックと Shift の範囲は
        // 選び直しなので落とす (`List` の窓の外の行と同じ決まり)。
        let gesture = crate::table::selection_gesture();
        let mut picked = native.clone();
        if keeps_hidden_selection(&shown, &native, gesture) {
            picked.extend(hidden);
        }
        let picked = self
            .0
            .mode
            .get()
            .normalize_paths(&self.0.items.borrow(), &picked);
        *self.0.selected.borrow_mut() = picked.clone();
        // Ctrl + A や Shift の範囲は選べない項目にも届くので、ここで外して書き戻す。
        let visible: Vec<Vec<usize>> = picked
            .iter()
            .filter(|path| self.is_path_shown(path))
            .cloned()
            .collect();
        if visible != native {
            self.multi_write_selection();
        }
        if picked != before {
            let first = picked.first().cloned().unwrap_or_default();
            self.0.handler.emit(&first);
        }
    }

    /// Scrolls the row at `path` into view (its ancestors are already expanded).
    pub(super) fn multi_scroll_to(&self, path: &[usize]) {
        let mut multi = self.0.multi.borrow_mut();
        let Some(multi) = multi.as_mut() else {
            return;
        };
        let Some(index) = multi.row_of(path) else {
            return;
        };
        if !multi.list_view.IsLoaded().unwrap_or(false) {
            multi.pending_scroll = Some(path.to_vec());
            return;
        }
        if let Ok(element) = multi.rows[index].item.cast::<IInspectable>() {
            let _ = multi.list_view.ScrollIntoView(&element);
        }
    }

    /// The `ListView` of the multiple selection surface. `None` while the tree has
    /// single selection and has never had multiple selection.
    pub(super) fn native_multi_list_view(&self) -> Option<ListView> {
        self.multi_list_view()
    }
}
