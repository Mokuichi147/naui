use crate::sys;
use crate::Widget;
use naui_core::Orientation;

/// 2 つの区画を、動かせる仕切りで分けるコンテナ (`GtkPaned`)。
#[derive(Clone)]
pub struct SplitView(pub(crate) sys::SplitView);

impl_widget!(SplitView);

impl SplitView {
    /// 並べる向き。`Horizontal` なら区画が横に並ぶ (仕切りは縦)。
    #[inline]
    pub fn orientation(&self) -> Orientation {
        self.0.orientation()
    }

    /// 左 (または上) の区画。呼ぶたびに置き換わる。
    #[inline]
    pub fn set_start(&self, child: &dyn Widget) {
        self.0.set_start(child.as_backend())
    }

    /// 右 (または下) の区画。呼ぶたびに置き換わる。
    #[inline]
    pub fn set_end(&self, child: &dyn Widget) {
        self.0.set_end(child.as_backend())
    }

    /// いまの仕切りの位置 (start 側の大きさ、論理ピクセル)。
    #[inline]
    pub fn position(&self) -> f64 {
        self.0.position()
    }

    /// 仕切りを動かす。`on_resize` は呼ばれない。
    #[inline]
    pub fn set_position(&self, position: f64) {
        self.0.set_position(position)
    }

    /// 利用者がドラッグしたのと同じく仕切りを動かす。`on_resize` を呼ぶ。
    #[inline]
    pub fn drag_to(&self, position: f64) {
        self.0.drag_to(position)
    }

    /// 両側の区画の最小の大きさ。既定はどちらも 0。
    #[inline]
    pub fn set_min_sizes(&self, start: f64, end: f64) {
        self.0.set_min_sizes(start, end)
    }

    /// 利用者が仕切りを動かすたび、動いた後の位置で呼ばれる。
    #[inline]
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_resize(f)
    }
}
