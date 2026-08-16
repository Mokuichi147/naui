//! リスト (DOM)。
//!
//! **行の中身によって作りが変わる。**
//!
//! | 行 | 作り |
//! | --- | --- |
//! | 文字だけ | `<select size>` + `<option>` — ブラウザ標準のリストボックスそのもの |
//! | `detail` あり | `<ul role="listbox">` + `<li role="option">` — 2 行にするための合成 |
//!
//! `<option>` の内容モデルは**テキストのみ**で、要素も改行も置けない。
//! 2 行の行を出すには `<select>` を離れるしかない。とはいえ `<select>` は
//! 選択もキーボード操作もスクロールもブラウザが面倒を見てくれる本物の
//! コントロールなので、**その必要が無いとき (文字だけの行) は使い続ける**。
//!
//! 合成のほうでも、枠と選択の色はブラウザが持つシステム色
//! (`Field` / `SelectedItem` / `Highlight` / `GrayText`) をそのまま使い、
//! naui は配色を決めない。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{ListItem, Result, SelectionMode};
use wasm_bindgen::JsCast;
use web_sys::{
    Document, Element, HtmlElement, HtmlOptionElement, HtmlSelectElement, KeyboardEvent, MouseEvent,
};

use crate::widgets::{create, impl_widget, Listener, Widget};

/// `size` の下限。1 以下だとドロップダウンになる。
const MIN_ROWS: u32 = 2;
/// `size` の上限。これを超える行数はスクロールで見せる。
const MAX_ROWS: u32 = 8;

thread_local! {
    /// `aria-activedescendant` から行を指すための、リストごとの通し番号。
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
}

fn next_list_id() -> u32 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// 選択が変わったことの通知先。
///
/// 単一選択でも複数選択でも同じ形にするため、選ばれている行を
/// 昇順の並びで渡す。呼び出しの間だけクロージャを取り出すので、
/// コールバックの中からリストを操作しても二重借用にならない。
#[derive(Clone, Default)]
struct SelectionHandler(Rc<RefCell<Option<Box<dyn FnMut(&[usize])>>>>);

impl SelectionHandler {
    fn set(&self, f: impl FnMut(&[usize]) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, indices: &[usize]) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(indices);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// いま使っている中身。[`List::set_items`] が行の内容を見て選び直す。
enum Body {
    /// ブラウザ標準のリストボックス。
    Select {
        select: HtmlSelectElement,
        /// `change` の購読。落とすと購読も外れる。
        _listener: Option<Listener>,
    },
    /// 2 行の行を出すための合成。
    Listbox {
        list: HtmlElement,
        options: Vec<HtmlElement>,
        /// 行ごとのクリックと、リスト全体のキー操作の購読。
        _listeners: Vec<Listener>,
    },
}

struct ListInner {
    /// 外から見える枠。中身が入れ替わっても、この要素は変わらない。
    root: HtmlElement,
    document: Document,
    id: u32,
    body: RefCell<Body>,
    items: RefCell<Vec<ListItem>>,
    mode: Cell<SelectionMode>,
    /// 選ばれている行 (昇順)。`<select>` でも合成でもここが正。
    selected: RefCell<Vec<usize>>,
    /// キーボードでいま指している行。合成のときだけ使う。
    active: Cell<Option<usize>>,
    /// Shift での範囲選択の起点。
    anchor: Cell<Option<usize>>,
    handler: SelectionHandler,
}

/// 縦に並ぶ選択できる一覧。
///
/// 行が文字だけなら `<select size>`、`detail` があれば
/// `<ul role="listbox">` になる。高さは `set_sizing` で指定する
/// (`<select>` のときだけ、指定が無ければ行数から決まる)。
#[derive(Clone)]
pub struct List(Rc<ListInner>);
impl_widget!(List, root);

impl List {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let root: HtmlElement = create(doc, "div")?.unchecked_into();
        // 中身を枠いっぱいに広げる。枠は入れ替わらないので、
        // `set_sizing` の指定も親への追加もそのまま生き続ける。
        style(&root, "display", "flex");
        style(&root, "min-height", "0");

        let this = Self(Rc::new(ListInner {
            root,
            document: doc.clone(),
            id: next_list_id(),
            body: RefCell::new(Body::Select {
                select: create(doc, "select")?.unchecked_into(),
                _listener: None,
            }),
            items: RefCell::new(Vec::new()),
            mode: Cell::new(SelectionMode::Single),
            selected: RefCell::new(Vec::new()),
            active: Cell::new(None),
            anchor: Cell::new(None),
            handler: SelectionHandler::default(),
        }));
        this.build_select(&[])?;
        Ok(this)
    }

    /// 行を作り直す。インデックスの意味が変わるため、選択は外れる。
    ///
    /// `detail` を持つ行が 1 つでもあれば `<ul role="listbox">` に、
    /// 無ければ `<select size>` に組み替える。
    pub fn set_items(&self, items: &[ListItem]) {
        *self.0.items.borrow_mut() = items.to_vec();
        self.0.selected.borrow_mut().clear();
        self.0.active.set(None);
        self.0.anchor.set(None);

        let needs_listbox = items.iter().any(|item| item.detail.is_some());
        let _ = if needs_listbox {
            self.build_listbox(items)
        } else {
            self.build_select(items)
        };
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 選び方を変える。選択の意味が変わるため、選択は外れる。
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.mode.set(mode);
        self.apply_mode();
        self.write_selection(&[]);
    }

    pub fn selection_mode(&self) -> SelectionMode {
        self.0.mode.get()
    }

    /// 選ばれている行のうち、いちばん上のもの。
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.borrow().first().copied()
    }

    /// 選ばれている行 (昇順)。単一選択なら 0 件か 1 件。
    pub fn selection(&self) -> Vec<usize> {
        self.0.selected.borrow().clone()
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
        // ブラウザはプログラムからの変更でイベントを出さないため、
        // ここで 1 回だけ通知する。
        let actual = self.selection();
        self.0.handler.emit(&actual);
    }

    /// 選択が変わったときに、選ばれている行 (昇順) で呼ばれる。
    ///
    /// 複数選択では 0 件で呼ばれることもある。
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// 中身の `<select>`。バックエンド固有の脱出口として公開している。
    ///
    /// `detail` を持つ行があるときは `<ul role="listbox">` になっているため
    /// `None` を返す。枠そのものは [`Widget::native_element`] から取れる。
    pub fn native_select(&self) -> Option<HtmlSelectElement> {
        match &*self.0.body.borrow() {
            Body::Select { select, .. } => Some(select.clone()),
            Body::Listbox { .. } => None,
        }
    }

    // ------------------------------------------------------------ 組み立て

    /// ブラウザ標準の `<select size>` を作る。
    fn build_select(&self, items: &[ListItem]) -> Result<()> {
        let select: HtmlSelectElement = create(&self.0.document, "select")?.unchecked_into();
        for item in items {
            let option: HtmlOptionElement = create(&self.0.document, "option")?.unchecked_into();
            option.set_text_content(Some(&item.label));
            option.set_disabled(!item.enabled);
            let _ = select.append_child(&option);
        }
        // `size` が無いとドロップダウンになるので、必ず 2 以上を入れる。
        select.set_size((items.len() as u32).clamp(MIN_ROWS, MAX_ROWS));

        // 選択はブラウザが動かすので、その結果を読み取って通知するだけ。
        let listener = Listener::attach(select.as_ref(), "change", {
            let weak = Rc::downgrade(&self.0);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let list = List(inner);
                let picked = list.read_select_selection();
                *list.0.selected.borrow_mut() = picked.clone();
                list.0.handler.emit(&picked);
            }
        })
        .ok();

        self.swap_body(select.as_ref())?;
        *self.0.body.borrow_mut() = Body::Select {
            select,
            _listener: listener,
        };
        self.apply_mode();
        Ok(())
    }

    /// 2 行の行を出すために `role="listbox"` を組み立てる。
    fn build_listbox(&self, items: &[ListItem]) -> Result<()> {
        let list: HtmlElement = create(&self.0.document, "ul")?.unchecked_into();
        let _ = list.set_attribute("role", "listbox");
        // キーボードで入れるようにする。中の行は `aria-activedescendant` で指す。
        let _ = list.set_attribute("tabindex", "0");
        style(&list, "list-style", "none");
        style(&list, "margin", "0");
        style(&list, "padding", "0");
        style(&list, "overflow-y", "auto");
        // 行の `offsetTop` がこの要素を基準になるようにする
        // (`reveal_active` がスクロール位置を求めるのに使う)。
        style(&list, "position", "relative");
        // 枠と地の色は、ブラウザが入力欄に使うシステム色に任せる。
        style(&list, "border", "1px solid");
        style(&list, "border-color", "ButtonBorder");
        style(&list, "background-color", "Field");
        style(&list, "color", "FieldText");

        let mut options = Vec::with_capacity(items.len());
        let mut listeners = Vec::with_capacity(items.len() + 1);
        for (index, item) in items.iter().enumerate() {
            let option: HtmlElement = create(&self.0.document, "li")?.unchecked_into();
            let _ = option.set_attribute("role", "option");
            let _ = option.set_attribute("id", &self.option_id(index));
            let _ = option.set_attribute("aria-selected", "false");
            style(&option, "display", "flex");
            style(&option, "flex-direction", "column");
            style(&option, "padding", "2px 4px");

            let title: HtmlElement = create(&self.0.document, "span")?.unchecked_into();
            title.set_text_content(Some(&item.label));
            let _ = option.append_child(&title);
            if let Some(detail) = &item.detail {
                let sub: HtmlElement = create(&self.0.document, "span")?.unchecked_into();
                sub.set_text_content(Some(detail));
                // macOS / Windows の 2 行目に合わせて、小さく淡くする。
                style(&sub, "font-size", "smaller");
                style(&sub, "opacity", "0.7");
                let _ = option.append_child(&sub);
            }

            if item.enabled {
                let listener = Listener::attach_event(option.as_ref(), "click", {
                    let weak = Rc::downgrade(&self.0);
                    move |event| {
                        let Some(inner) = weak.upgrade() else {
                            return;
                        };
                        let mouse = event.dyn_ref::<MouseEvent>();
                        let toggle = mouse.is_some_and(|e| e.meta_key() || e.ctrl_key());
                        let extend = mouse.is_some_and(|e| e.shift_key());
                        List(inner).on_row_activated(index, toggle, extend);
                    }
                })?;
                listeners.push(listener);
            } else {
                let _ = option.set_attribute("aria-disabled", "true");
                // 無効な文字にブラウザが使うシステム色。
                style(&option, "color", "GrayText");
            }

            let _ = list.append_child(&option);
            options.push(option);
        }

        listeners.push(Listener::attach_event(list.as_ref(), "keydown", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if let Some(key) = event.dyn_ref::<KeyboardEvent>() {
                    if List(inner).on_key(key) {
                        event.prevent_default();
                    }
                }
            }
        })?);

        self.swap_body(&list)?;
        *self.0.body.borrow_mut() = Body::Listbox {
            list,
            options,
            _listeners: listeners,
        };
        self.apply_mode();
        Ok(())
    }

    /// 枠の中身を入れ替える。
    fn swap_body(&self, element: &HtmlElement) -> Result<()> {
        while let Some(child) = self.0.root.last_element_child() {
            let _ = self.0.root.remove_child(&child);
        }
        style(element, "flex", "1");
        style(element, "min-width", "0");
        self.0
            .root
            .append_child(element)
            .map(|_| ())
            .map_err(|e| crate::to_error("リストの組み立て", e))
    }

    fn option_id(&self, index: usize) -> String {
        format!("naui-list-{}-row-{index}", self.0.id)
    }

    /// 単一 / 複数の指定を、いまの中身へ反映する。
    fn apply_mode(&self) {
        let multiple = self.0.mode.get().is_multiple();
        match &*self.0.body.borrow() {
            Body::Select { select, .. } => select.set_multiple(multiple),
            Body::Listbox { list, .. } => {
                let _ = list.set_attribute(
                    "aria-multiselectable",
                    if multiple { "true" } else { "false" },
                );
            }
        }
    }

    // --------------------------------------------------------------- 選択

    /// `<select>` の選択を DOM から読む。
    fn read_select_selection(&self) -> Vec<usize> {
        let body = self.0.body.borrow();
        let Body::Select { select, .. } = &*body else {
            return Vec::new();
        };
        let options = select.options();
        (0..options.length())
            .filter(|&i| {
                options
                    .get_with_index(i)
                    .and_then(|node| node.dyn_into::<HtmlOptionElement>().ok())
                    .is_some_and(|option| option.selected())
            })
            .map(|i| i as usize)
            .collect()
    }

    /// 選択を覚えて、そのまま中身へ書き込む (通知は起きない)。
    fn write_selection(&self, indices: &[usize]) {
        *self.0.selected.borrow_mut() = indices.to_vec();
        let body = self.0.body.borrow();
        match &*body {
            Body::Select { select, .. } => {
                let options = select.options();
                for i in 0..options.length() {
                    if let Some(option) = options
                        .get_with_index(i)
                        .and_then(|node| node.dyn_into::<HtmlOptionElement>().ok())
                    {
                        option.set_selected(indices.contains(&(i as usize)));
                    }
                }
            }
            Body::Listbox { options, .. } => {
                for (index, option) in options.iter().enumerate() {
                    let picked = indices.contains(&index);
                    let _ =
                        option.set_attribute("aria-selected", if picked { "true" } else { "false" });
                    if picked {
                        // 選択の色はブラウザのシステム色をそのまま使う。
                        // 新しい名前に対応していれば、後の指定が勝つ。
                        style(option, "background-color", "Highlight");
                        style(option, "background-color", "SelectedItem");
                        style(option, "color", "HighlightText");
                        style(option, "color", "SelectedItemText");
                    } else {
                        style(option, "background-color", "");
                        style(option, "color", "");
                        if option.has_attribute("aria-disabled") {
                            style(option, "color", "GrayText");
                        }
                    }
                }
            }
        }
    }

    /// 合成のリストで行が押されたとき。
    fn on_row_activated(&self, index: usize, toggle: bool, extend: bool) {
        let multiple = self.0.mode.get().is_multiple();
        let picked = if multiple && extend {
            let anchor = self.0.anchor.get().unwrap_or(index);
            self.range(anchor, index)
        } else if multiple && toggle {
            let mut picked = self.selection();
            match picked.iter().position(|&i| i == index) {
                Some(at) => {
                    picked.remove(at);
                }
                None => picked.push(index),
            }
            self.0.anchor.set(Some(index));
            picked
        } else {
            self.0.anchor.set(Some(index));
            vec![index]
        };
        self.0.active.set(Some(index));
        self.commit(&picked);
    }

    /// キー操作。処理したら `true` を返す (ブラウザの既定動作を止める)。
    fn on_key(&self, event: &KeyboardEvent) -> bool {
        let len = self.len();
        if len == 0 {
            return false;
        }
        let current = self.0.active.get().or_else(|| self.selected());
        let target = match event.key().as_str() {
            "ArrowDown" => self.step(current, 1),
            "ArrowUp" => self.step(current, -1),
            "Home" => self.first_enabled(0, 1),
            "End" => self.first_enabled(len as isize - 1, -1),
            // 複数選択では Space で、いま指している行を入れたり外したりする。
            " " if self.0.mode.get().is_multiple() => {
                let Some(index) = current else {
                    return false;
                };
                self.on_row_activated(index, true, false);
                return true;
            }
            _ => return false,
        };
        let Some(target) = target else {
            return false;
        };
        self.0.active.set(Some(target));
        if self.0.mode.get().is_multiple() && event.shift_key() {
            let anchor = self.0.anchor.get().unwrap_or(target);
            let picked = self.range(anchor, target);
            self.commit(&picked);
        } else {
            self.0.anchor.set(Some(target));
            self.commit(&[target]);
        }
        true
    }

    /// `from` から `step` の向きへ、次に選べる行を探す。
    fn step(&self, from: Option<usize>, step: isize) -> Option<usize> {
        let start = match from {
            Some(index) => index as isize + step,
            None if step > 0 => 0,
            None => self.len() as isize - 1,
        };
        self.first_enabled(start, step)
    }

    /// `start` から `step` の向きに進んで、最初に選べる行を返す。
    fn first_enabled(&self, start: isize, step: isize) -> Option<usize> {
        let items = self.0.items.borrow();
        let mut at = start;
        while at >= 0 && (at as usize) < items.len() {
            if items[at as usize].enabled {
                return Some(at as usize);
            }
            at += step;
        }
        None
    }

    /// `a` から `b` までの、選べる行の並び。
    fn range(&self, a: usize, b: usize) -> Vec<usize> {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let items = self.0.items.borrow();
        (lo..=hi.min(items.len().saturating_sub(1)))
            .filter(|&i| items[i].enabled)
            .collect()
    }

    /// ユーザー操作の結果を確定し、通知する。
    fn commit(&self, indices: &[usize]) {
        let picked = self.0.mode.get().normalize(&self.0.items.borrow(), indices);
        self.write_selection(&picked);
        self.reveal_active();
        self.0.handler.emit(&picked);
    }

    /// キーボードで指している行を、スクロール領域の中へ入れる。
    fn reveal_active(&self) {
        let Some(active) = self.0.active.get() else {
            return;
        };
        let body = self.0.body.borrow();
        let Body::Listbox { list, options, .. } = &*body else {
            return;
        };
        let _ = list.set_attribute("aria-activedescendant", &self.option_id(active));
        let Some(option) = options.get(active) else {
            return;
        };
        let top = option.offset_top();
        let bottom = top + option.offset_height();
        let view_top = list.scroll_top();
        let view_bottom = view_top + list.client_height();
        if top < view_top {
            list.set_scroll_top(top);
        } else if bottom > view_bottom {
            list.set_scroll_top(bottom - list.client_height());
        }
    }
}
