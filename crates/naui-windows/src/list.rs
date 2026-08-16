//! リスト (WinUI 3)。
//!
//! WinUI 標準の `ListBox` をそのまま使う。行は `ListBoxItem` で、
//! 中身は `TextBlock`。選べない行は `IsEnabled = false`、
//! 複数選択・キーボード操作・スクロールは `ListBox` 自身が行う。
//!
//! `ListView` は `winio-winui3` 0.4.5 のバインディングに含まれていないため、
//! 同じ WinUI 標準コントロールである `ListBox` を使っている。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{ListItem, Result, SelectionMode};
use windows_core::{IInspectable, Interface, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::{
    ListBox as XamlListBox, ListBoxItem, Orientation as XamlOrientation, StackPanel,
    SelectionChangedEventHandler, SelectionMode as XamlSelectionMode, TextBlock,
};
use winui3::Microsoft::UI::Xaml::UIElement;

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

/// 選択が変わったことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するため `UiThreadCell` に載せる。
/// 呼び出しの間だけクロージャを取り出すので、コールバックの中から
/// 同じリストを操作しても二重借用にならない。
#[derive(Clone)]
struct SelectionHandler(Arc<UiThreadCell<Option<Box<dyn FnMut(&[usize])>>>>);

impl SelectionHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, indices: &[usize]) {
        let Some(mut f) = self.0.with_mut(|slot| slot.take()) else {
            return;
        };
        f(indices);
        self.0.with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct ListInner {
    native: XamlListBox,
    /// 行そのもの。選択の読み書きはここを通す。
    rows: RefCell<Vec<ListBoxItem>>,
    items: RefCell<Vec<ListItem>>,
    mode: Cell<SelectionMode>,
    handler: SelectionHandler,
    /// プログラムから選択を変えている間だけ通知を止める。
    /// `IsSelected` の書き換えでも `SelectionChanged` が起きるため。
    silent: Rc<Cell<bool>>,
}

/// 縦に並ぶ選択できる一覧 (ListBox)。
///
/// 高さは `ListBox` 自身が持つが、行数に関係なく固定したいときは
/// `set_sizing` で指定する。
#[derive(Clone)]
pub struct List(Rc<ListInner>);
impl_widget!(List, native);

impl List {
    pub(crate) fn new() -> Result<Self> {
        let native = XamlListBox::new().map_err(|e| to_error("ListBox の生成", e))?;
        native
            .SetSelectionMode(XamlSelectionMode::Single)
            .map_err(|e| to_error("ListBox の選択方法の設定", e))?;

        let this = Self(Rc::new(ListInner {
            native,
            rows: RefCell::new(Vec::new()),
            items: RefCell::new(Vec::new()),
            mode: Cell::new(SelectionMode::Single),
            handler: SelectionHandler::new(),
            silent: Rc::new(Cell::new(false)),
        }));

        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        let state = UiThreadCell::new(Rc::downgrade(&this.0));
        let handler = SelectionChangedEventHandler::new(move |_sender, _args| {
            state.with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    let list = List(inner);
                    if !list.0.silent.get() {
                        let indices = list.selection();
                        list.0.handler.emit(&indices);
                    }
                }
            });
            Ok(())
        });
        this.0
            .native
            .SelectionChanged(&handler)
            .map_err(|e| to_error("ListBox の購読", e))?;
        Ok(this)
    }

    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    pub fn set_items(&self, items: &[ListItem]) {
        let _ = self.rebuild(items);
    }

    fn rebuild(&self, items: &[ListItem]) -> Result<()> {
        let children = self
            .0
            .native
            .Items()
            .map_err(|e| to_error("行の取得", e))?;
        self.without_notifying(|_| children.Clear())
            .map_err(|e| to_error("行の消去", e))?;
        self.0.rows.borrow_mut().clear();

        let mut rows = Vec::with_capacity(items.len());
        for item in items {
            let row = ListBoxItem::new().map_err(|e| to_error("ListBoxItem の生成", e))?;
            set_row_content(&row, item)?;
            let _ = row.SetIsEnabled(item.enabled);

            let element = row
                .cast::<IInspectable>()
                .map_err(|e| to_error("行の要素化", e))?;
            self.without_notifying(|_| children.Append(&element))
                .map_err(|e| to_error("行の追加", e))?;
            rows.push(row);
        }
        *self.0.rows.borrow_mut() = rows;
        *self.0.items.borrow_mut() = items.to_vec();
        self.write_selection(&[]);
        Ok(())
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.0.rows.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 選び方を変える。選択の意味が変わるため、選択は外れる。
    ///
    /// 複数選択は WinUI の `Extended` (Ctrl / Shift を押しながら選ぶ) に写す。
    /// `Multiple` はクリックのたびに反転する挙動で、macOS / Web と揃わないため。
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.mode.set(mode);
        let native = if mode.is_multiple() {
            XamlSelectionMode::Extended
        } else {
            XamlSelectionMode::Single
        };
        let _ = self.without_notifying(|this| this.0.native.SetSelectionMode(native));
        self.write_selection(&[]);
    }

    pub fn selection_mode(&self) -> SelectionMode {
        self.0.mode.get()
    }

    /// 選ばれている行のうち、いちばん上のもの。
    pub fn selected(&self) -> Option<usize> {
        self.selection().first().copied()
    }

    /// 選ばれている行 (昇順)。単一選択なら 0 件か 1 件。
    pub fn selection(&self) -> Vec<usize> {
        self.0
            .rows
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, row)| row.IsSelected().unwrap_or(false))
            .map(|(index, _)| index)
            .collect()
    }

    /// 通知せずに 1 行だけを選ぶ。
    pub fn set_selected(&self, index: usize) {
        self.set_selection(&[index]);
    }

    /// 通知せずに選択を置き換える。
    ///
    /// 範囲外・選べない行・重複は取り除かれ、単一選択なら先頭の 1 件だけが残る
    /// ([`SelectionMode::normalize`])。
    pub fn set_selection(&self, indices: &[usize]) {
        let picked = self.0.mode.get().normalize(&self.0.items.borrow(), indices);
        self.write_selection(&picked);
    }

    /// 通知せずに選択をすべて外す。
    pub fn clear_selection(&self) {
        self.write_selection(&[]);
    }

    /// ユーザーが選んだのと同じ経路で 1 行を選ぶ (通知あり)。
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// ユーザーが選んだのと同じ経路で選択を置き換える (通知あり)。
    pub fn select_many(&self, indices: &[usize]) {
        self.set_selection(indices);
        // 同じ選択を選び直すと `SelectionChanged` は起きないため、
        // 通知の回数をそろえてここで 1 回だけ出す。
        let actual = self.selection();
        self.0.handler.emit(&actual);
    }

    /// 選択が変わったときに、選ばれている行 (昇順) で呼ばれる。
    ///
    /// 複数選択では 0 件で呼ばれることもある。
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// 中身の `ListBox`。バックエンド固有の脱出口として公開している。
    pub fn native_list_box(&self) -> XamlListBox {
        self.0.native.clone()
    }

    /// 選択をそのまま行へ書き込む (通知は起きない)。
    fn write_selection(&self, indices: &[usize]) {
        self.without_notifying(|this| {
            for (index, row) in this.0.rows.borrow().iter().enumerate() {
                let _ = row.SetIsSelected(indices.contains(&index));
            }
        });
    }

    /// WinUI からの通知を止めたまま操作する。
    fn without_notifying<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        let previous = self.0.silent.replace(true);
        let result = f(self);
        self.0.silent.set(previous);
        result
    }
}

/// 行の中身を組み立てる。
///
/// `detail` が無ければ `TextBlock` 1 つ、あれば縦の `StackPanel` に 2 つ入れる。
/// 縦の位置合わせは WinUI のレイアウトパスが行うので、naui は組むだけ。
fn set_row_content(row: &ListBoxItem, item: &ListItem) -> Result<()> {
    let title = text_block(&item.label, false)?;
    match &item.detail {
        None => row
            .SetContent(&title)
            .map_err(|e| to_error("行への内容設定", e)),
        Some(detail) => {
            let panel = StackPanel::new().map_err(|e| to_error("行の StackPanel の生成", e))?;
            panel
                .SetOrientation(XamlOrientation::Vertical)
                .map_err(|e| to_error("行の向き設定", e))?;
            let children = panel
                .Children()
                .map_err(|e| to_error("行の中身の取得", e))?;
            children
                .Append(&title.cast::<UIElement>().map_err(|e| to_error("行の要素化", e))?)
                .map_err(|e| to_error("行への追加", e))?;
            let sub = text_block(detail, true)?;
            children
                .Append(&sub.cast::<UIElement>().map_err(|e| to_error("行の要素化", e))?)
                .map_err(|e| to_error("行への追加", e))?;
            row.SetContent(&panel)
                .map_err(|e| to_error("行への内容設定", e))
        }
    }
}

/// 行に載せる 1 本の文字。`secondary` なら小さく淡い見た目にする。
fn text_block(text: &str, secondary: bool) -> Result<TextBlock> {
    let block = TextBlock::new().map_err(|e| to_error("行ラベルの生成", e))?;
    block
        .SetText(&HSTRING::from(text))
        .map_err(|e| to_error("行ラベルの設定", e))?;
    if secondary {
        // Fluent の副次テキストに合わせる。色は WinUI のテーマに任せ、
        // 大きさと濃さだけを下げる。
        let _ = block.SetFontSize(12.0);
        let _ = block.SetOpacity(0.7);
    }
    Ok(block)
}
