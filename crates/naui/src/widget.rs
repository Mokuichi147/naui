//! ウィジェットに共通のトレイトと、包む型を作るマクロ。

use crate::sys;

pub(crate) mod sealed {
    pub trait Sealed {}
}

/// naui のウィジェットが実装する共通インタフェース。
///
/// コンテナへ入れるメソッド ([`Stack::append`](crate::Stack::append) など) が
/// `&dyn Widget` で受け取る。naui の外では実装できない。
pub trait Widget: sealed::Sealed + 'static {
    /// 包んでいるバックエンドのウィジェット。コンテナへ入れるときに使う。
    #[doc(hidden)]
    fn as_backend(&self) -> &dyn sys::Widget;
}

/// ウィジェットでない型 (`Window` や `ListRow` など) の共通部分。
macro_rules! impl_backend {
    ($t:ident) => {
        impl $t {
            /// 包んでいるバックエンドの型。naui に無いネイティブの操作
            /// (`native_*`) は、これを通して呼ぶ。
            ///
            /// 型はビルド対象のバックエンドごとに違う
            /// (`naui::macos` / `naui::windows` / `naui::gtk` / `naui::web` の中の同じ名前の型)。
            #[inline]
            pub fn backend(&self) -> &crate::sys::$t {
                &self.0
            }
        }
    };
}

/// ウィジェットの共通部分。[`Widget`] の実装と、全ウィジェットにある 7 つのメソッド。
macro_rules! impl_widget {
    ($t:ident) => {
        impl_backend!($t);

        impl crate::widget::sealed::Sealed for $t {}

        impl crate::Widget for $t {
            #[inline]
            fn as_backend(&self) -> &dyn crate::sys::Widget {
                &self.0
            }
        }

        impl $t {
            /// 大きさを指定する。呼ぶたびに以前の指定は外れる。
            ///
            /// 実際の大きさを決めるのは GTK4 のレイアウトなので、ここで渡すのは
            /// 「最小はここまで」「余りを受け取る」「上限で止める」という指定だけ。
            ///
            /// 交差軸の寄せ方はコンテナ ([`Stack::set_align`](crate::Stack::set_align)) も決めるため、
            /// **コンテナへ入れる前**に呼ぶのが確実。
            #[inline]
            pub fn set_sizing(&self, sizing: naui_core::Sizing) {
                self.0.set_sizing(sizing)
            }

            /// 表示するかどうか。既定は表示。
            ///
            /// 隠したものは `Stack` の中では場所を空けない。`Grid` ではマスが残る。
            #[inline]
            pub fn set_visible(&self, visible: bool) {
                self.0.set_visible(visible)
            }

            /// [`set_visible`](Self::set_visible) で指定した表示。親が隠れていても
            /// `true` のまま。
            #[inline]
            pub fn is_visible(&self) -> bool {
                self.0.is_visible()
            }

            /// キーボードフォーカスを移す。移せたら `true`。
            ///
            /// 自分が受け取れないとき (コンテナなど) は、中で最初に受け取れるものへ
            /// 移す。表示前・隠れている・無効なときは `false`。
            #[inline]
            pub fn request_focus(&self) -> bool {
                self.0.request_focus()
            }

            /// ポインターを重ねたときに出す説明。`None` で外す。
            #[inline]
            pub fn set_tooltip(&self, text: Option<&str>) {
                self.0.set_tooltip(text)
            }

            /// 読み上げソフトに伝える名前。`None` で外す (見えている文字が使われる)。
            ///
            /// アイコンだけのボタンのように、見えている文字が無いか意味を
            /// 表しきれないときに付ける。
            #[inline]
            pub fn set_accessible_label(&self, text: Option<&str>) {
                self.0.set_accessible_label(text)
            }

            /// 弱参照を作る。通知のクロージャからこのウィジェット自身を触るときに
            /// 捕まえる (`clone()` で捕まえると循環して解放されない)。
            #[inline]
            pub fn downgrade(&self) -> naui_core::WeakWidget<Self> {
                let weak = self.0.downgrade();
                naui_core::WeakWidget::new(move || weak.upgrade().map(Self))
            }
        }
    };
}
