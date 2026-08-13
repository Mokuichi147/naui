//! ウィジェットの抽象と、レイアウト / 描画 / イベントの各コンテキスト。
//!
//! ツリーは更新のたびに `view()` から作り直される (Elm 風) が、ホバーや
//! フォーカス、キャレット位置といった「UI 側の状態」はランタイムが `Id` を
//! キーに保持するため、作り直しても失われない。

use std::any::Any;
use std::collections::{HashMap, HashSet};

use crate::event::{Event, Modifiers};
use crate::geometry::{Point, Rect, Size};
use crate::layout::BoxConstraints;
use crate::painter::{Painter, TextMeasurer};
use crate::theme::Theme;

/// ウィジェットの同一性。ツリー内の位置 (親 → 兄弟番号 → 型名 → key) から決まる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Id(pub u64);

impl Id {
    pub const ROOT: Id = Id(0xcbf2_9ce4_8422_2325);

    fn child(self, index: u32, type_name: &str, key: Option<&str>) -> Id {
        // FNV-1a。乱数シードに依存しないので実行ごとに安定する。
        let mut h = self.0;
        let mut mix = |bytes: &[u8]| {
            for b in bytes {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        mix(&index.to_le_bytes());
        mix(type_name.as_bytes());
        if let Some(k) = key {
            mix(b"#");
            mix(k.as_bytes());
        }
        Id(h)
    }
}

/// マウスカーソルの見た目。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorIcon {
    #[default]
    Default,
    Pointer,
    Text,
    ResizeHorizontal,
    NotAllowed,
}

/// ウィジェットが `Id` ごとに保持する内部状態 (キャレット位置、スクロール量など)。
#[derive(Default)]
pub struct StateStore {
    map: HashMap<Id, Box<dyn Any>>,
}

impl StateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_default<T: Any + Default>(&mut self, id: Id) -> &mut T {
        self.map
            .entry(id)
            .or_insert_with(|| Box::<T>::default())
            .downcast_mut::<T>()
            .expect("同じ Id に別の型の状態が格納されている")
    }

    pub fn get<T: Any>(&self, id: Id) -> Option<&T> {
        self.map.get(&id).and_then(|b| b.downcast_ref::<T>())
    }

    /// 今フレームに存在しなかった `Id` の状態を破棄する。
    pub fn retain_alive(&mut self, alive: &HashSet<Id>) {
        self.map.retain(|k, _| alive.contains(k));
    }
}

/// ポインタ / フォーカスの現在状態。
#[derive(Debug, Clone, Default)]
pub struct Interaction {
    pub pointer: Point,
    pub hovered: Option<Id>,
    /// 押下中のウィジェット (ポインタキャプチャ)。
    pub active: Option<Id>,
    pub focused: Option<Id>,
    pub modifiers: Modifiers,
    /// キーボード操作由来のフォーカスか (true のときだけフォーカスリングを描く)。
    pub focus_visible: bool,
    /// タブ順の構築用に、フレーム中に現れたフォーカス可能な Id を順に記録する。
    pub focusables: Vec<Id>,
}

impl Interaction {
    pub fn is_hovered(&self, id: Id) -> bool {
        self.hovered == Some(id)
    }
    pub fn is_active(&self, id: Id) -> bool {
        self.active == Some(id)
    }
    pub fn is_focused(&self, id: Id) -> bool {
        self.focused == Some(id)
    }
    pub fn shows_focus_ring(&self, id: Id) -> bool {
        self.focus_visible && self.focused == Some(id)
    }
}

/// レイアウト時のコンテキスト。
pub struct LayoutCx<'a> {
    pub text: &'a mut dyn TextMeasurer,
    pub theme: &'a Theme,
    pub scale_factor: f32,
    store: &'a mut StateStore,
    alive: &'a mut HashSet<Id>,
    focusables: &'a mut Vec<Id>,
    stack: Vec<(Id, u32)>,
}

impl<'a> LayoutCx<'a> {
    pub fn new(
        text: &'a mut dyn TextMeasurer,
        theme: &'a Theme,
        store: &'a mut StateStore,
        alive: &'a mut HashSet<Id>,
        focusables: &'a mut Vec<Id>,
        scale_factor: f32,
    ) -> Self {
        Self {
            text,
            theme,
            scale_factor,
            store,
            alive,
            focusables,
            stack: vec![(Id::ROOT, 0)],
        }
    }

    /// レイアウト中のウィジェット自身の `Id`。
    pub fn current_id(&self) -> Id {
        self.stack.last().map(|(id, _)| *id).unwrap_or(Id::ROOT)
    }

    /// Tab によるフォーカス移動の対象として登録する。
    /// レイアウト順に呼ばれるため、そのままタブ順になる。
    pub fn register_focusable(&mut self) {
        let id = self.current_id();
        self.focusables.push(id);
    }

    fn alloc_id(&mut self, type_name: &str, key: Option<&str>) -> Id {
        let (parent, counter) = self.stack.last_mut().expect("id スタックが空");
        let index = *counter;
        *counter += 1;
        let id = parent.child(index, type_name, key);
        self.alive.insert(id);
        id
    }

    fn push_id(&mut self, id: Id) {
        self.stack.push((id, 0));
    }

    fn pop_id(&mut self) {
        self.stack.pop();
    }

    pub fn state<T: Any + Default>(&mut self, id: Id) -> &mut T {
        self.store.get_or_default::<T>(id)
    }
}

/// 描画時のコンテキスト。
pub struct PaintCx<'a> {
    pub painter: &'a mut dyn Painter,
    pub theme: &'a Theme,
    pub interaction: &'a Interaction,
    pub store: &'a StateStore,
}

impl<'a> PaintCx<'a> {
    pub fn is_hovered(&self, id: Id) -> bool {
        self.interaction.is_hovered(id)
    }
    pub fn is_active(&self, id: Id) -> bool {
        self.interaction.is_active(id)
    }
    pub fn is_focused(&self, id: Id) -> bool {
        self.interaction.is_focused(id)
    }
    pub fn shows_focus_ring(&self, id: Id) -> bool {
        self.interaction.shows_focus_ring(id)
    }
    pub fn state<T: Any>(&self, id: Id) -> Option<&T> {
        self.store.get::<T>(id)
    }
}

/// イベント処理時のコンテキスト。
pub struct EventCx<'a, M> {
    pub theme: &'a Theme,
    pub interaction: &'a mut Interaction,
    pub text: &'a mut dyn TextMeasurer,
    store: &'a mut StateStore,
    messages: &'a mut Vec<M>,
    handled: bool,
    redraw: bool,
    cursor: CursorIcon,
}

impl<'a, M> EventCx<'a, M> {
    pub fn new(
        theme: &'a Theme,
        interaction: &'a mut Interaction,
        text: &'a mut dyn TextMeasurer,
        store: &'a mut StateStore,
        messages: &'a mut Vec<M>,
    ) -> Self {
        Self {
            theme,
            interaction,
            text,
            store,
            messages,
            handled: false,
            redraw: false,
            cursor: CursorIcon::Default,
        }
    }

    /// アプリケーションへメッセージを送る。
    pub fn emit(&mut self, msg: M) {
        self.messages.push(msg);
        self.redraw = true;
    }

    /// このイベントを消費したことを示す (親はそれ以上配送しない)。
    pub fn consume(&mut self) {
        self.handled = true;
        self.redraw = true;
    }

    pub fn is_handled(&self) -> bool {
        self.handled
    }

    pub fn request_redraw(&mut self) {
        self.redraw = true;
    }

    pub fn needs_redraw(&self) -> bool {
        self.redraw
    }

    pub fn set_cursor(&mut self, cursor: CursorIcon) {
        self.cursor = cursor;
    }

    pub fn cursor(&self) -> CursorIcon {
        self.cursor
    }

    pub fn state<T: Any + Default>(&mut self, id: Id) -> &mut T {
        self.store.get_or_default::<T>(id)
    }

    pub fn focus(&mut self, id: Id, visible: bool) {
        self.interaction.focused = Some(id);
        self.interaction.focus_visible = visible;
        self.redraw = true;
    }

    pub fn is_hovered(&self, id: Id) -> bool {
        self.interaction.is_hovered(id)
    }
    pub fn is_active(&self, id: Id) -> bool {
        self.interaction.is_active(id)
    }
    pub fn is_focused(&self, id: Id) -> bool {
        self.interaction.is_focused(id)
    }
}

/// 描画 / イベント時にウィジェットへ渡される、自分自身の位置と同一性。
#[derive(Debug, Clone, Copy)]
pub struct Node {
    /// ウィンドウ絶対座標での自分の矩形。
    pub rect: Rect,
    pub id: Id,
}

/// すべての UI 部品が実装するトレイト。`M` はアプリのメッセージ型。
pub trait Widget<M> {
    /// `Id` 生成に使う型名。既定実装で十分だが、同じ位置に異なる部品が来る
    /// 場合に状態が混ざらないよう、各ウィジェットで返すのが望ましい。
    fn type_name(&self) -> &'static str {
        "widget"
    }

    /// 制約からサイズを決める。子を持つ場合はここで子の配置も行う。
    fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size;

    fn paint(&self, cx: &mut PaintCx, node: Node);

    fn event(&mut self, cx: &mut EventCx<M>, event: &Event, node: Node) {
        let _ = (cx, event, node);
    }
}

/// レイアウト結果を保持する、ツリー上のノード。
pub struct Element<M> {
    widget: Box<dyn Widget<M>>,
    /// 親からの相対位置とサイズ。
    rect: Rect,
    id: Id,
    key: Option<&'static str>,
}

impl<M> Element<M> {
    pub fn new(widget: impl Widget<M> + 'static) -> Self {
        Self {
            widget: Box::new(widget),
            rect: Rect::ZERO,
            id: Id::ROOT,
            key: None,
        }
    }

    /// 兄弟の増減があっても状態を保つためのキーを付ける。
    pub fn key(mut self, key: &'static str) -> Self {
        self.key = Some(key);
        self
    }

    pub fn id(&self) -> Id {
        self.id
    }

    pub fn size(&self) -> Size {
        self.rect.size
    }

    pub fn offset(&self) -> Point {
        self.rect.origin
    }

    /// 親からの相対位置を設定する。
    pub fn place(&mut self, origin: Point) {
        self.rect.origin = origin;
    }

    pub fn layout(&mut self, cx: &mut LayoutCx, bc: BoxConstraints) -> Size {
        let id = cx.alloc_id(self.widget.type_name(), self.key);
        self.id = id;
        cx.push_id(id);
        let size = self.widget.layout(cx, bc);
        cx.pop_id();
        self.rect.size = size;
        size
    }

    /// `parent_origin` は親のウィンドウ絶対座標。
    pub fn paint(&self, cx: &mut PaintCx, parent_origin: Point) {
        let rect = self.rect.translate(parent_origin.x, parent_origin.y);
        if rect.intersect(cx.painter.clip_rect()).is_empty() {
            return;
        }
        self.widget.paint(cx, Node { rect, id: self.id });
    }

    pub fn event(&mut self, cx: &mut EventCx<M>, event: &Event, parent_origin: Point) {
        if cx.is_handled() {
            return;
        }
        let rect = self.rect.translate(parent_origin.x, parent_origin.y);
        self.widget.event(cx, event, Node { rect, id: self.id });
    }

    /// 絶対座標での矩形 (親原点を与える)。
    pub fn abs_rect(&self, parent_origin: Point) -> Rect {
        self.rect.translate(parent_origin.x, parent_origin.y)
    }
}

impl<M> std::fmt::Debug for Element<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Element")
            .field("type", &self.widget.type_name())
            .field("rect", &self.rect)
            .field("id", &self.id)
            .finish()
    }
}
