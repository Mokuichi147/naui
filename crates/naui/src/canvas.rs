use crate::sys;
use naui_core::{EventResponse, Painter, PointerEvent};

/// アプリが自分で描く面 (`GtkDrawingArea`)。
///
/// 中身から大きさは決まらないので、`set_sizing` で指定する (`Scroll` と同じ)。
#[derive(Clone)]
pub struct Canvas(pub(crate) sys::Canvas);

impl_widget!(Canvas);

impl Canvas {
    /// 描く内容を決めるクロージャ。描き直すたびに新しい [`Painter`] で呼ばれる。
    ///
    /// 呼ばれるのは [`redraw`](Self::redraw) の後と、面の大きさが変わったとき
    /// (と、GTK が再描画を求めたとき)。
    #[inline]
    pub fn on_draw(&self, f: impl FnMut(&mut Painter) + 'static) {
        self.0.on_draw(f)
    }

    /// 描き直しを頼む。その場では描かず、次の描画のときに `on_draw` が呼ばれる。
    #[inline]
    pub fn redraw(&self) {
        self.0.redraw()
    }

    /// ポインター (マウス・タッチ・ペン) の押下・移動・解放。位置は面の左上を
    /// 原点にした論理ピクセル。移動は押していない間 (ホバー) も届く。押下と
    /// 解放にはどのボタンかが付く ([`PointerEvent::button`])。
    #[inline]
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.on_pointer(f)
    }

    /// ホイール (トラックパッドの 2 本指のスクロールも含む)。量は論理ピクセル。
    ///
    /// `Handled` を返すと外側のスクロールへは回らない (指を離した後の慣性も
    /// 起きない)。`Continue` なら外側の `GtkScrolledWindow` がいつもどおり
    /// スクロールする。
    #[inline]
    pub fn on_wheel(&self, f: impl FnMut(PointerEvent) -> EventResponse + 'static) {
        self.0.on_wheel(f)
    }

    /// いまの面の大きさ (幅, 高さ)。まだレイアウトされていなければ 0。
    #[inline]
    pub fn size(&self) -> (f64, f64) {
        self.0.size()
    }
}
