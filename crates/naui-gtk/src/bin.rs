//! 大きさの指定 ([`Sizing`]) を GTK4 のレイアウトへ写すための土台。
//!
//! GTK4 のウィジェットは「最小の大きさ」(`set_size_request`)・「余りを受け取るか」
//! (`set_hexpand`)・「配られた場所での寄せ方」(`set_halign`) を持つが、
//! **「これ以上は大きくならない」という上限を持たない**。
//!
//! そこで naui のウィジェットは、必ず [`SizeBin`] という薄い入れ物に入れてから
//! コンテナへ渡す。`SizeBin` は GTK4 の `measure` だけを差し替えたウィジェットで、
//! 中身が申告する「自然な大きさ」を上限で頭打ちにする。実際の配置は
//! `GtkBinLayout` に任せているので、レイアウト計算は GTK4 が行う。

use gtk::glib;
use gtk::prelude::*;
use naui_core::{Align, Length, Sizing, Track};

// glib のサブクラス化マクロが `unsafe impl` を作るため、この中だけ許可する。
#[allow(unsafe_code)]
mod imp {
    use std::cell::Cell;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use naui_core::Sizing;

    #[derive(Default)]
    pub struct SizeBin {
        /// アプリが指定した大きさ。寄せ方を決め直すときに読む。
        pub sizing: Cell<Sizing>,
        /// 幅の上限 (論理ピクセル)。負なら上限なし。
        pub max_width: Cell<i32>,
        /// 高さの上限 (論理ピクセル)。負なら上限なし。
        pub max_height: Cell<i32>,
        /// 中身が幅を縮められないか (`GtkSpinButton` など)。
        pub rigid_width: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SizeBin {
        const NAME: &'static str = "NauiSizeBin";
        type Type = super::SizeBin;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for SizeBin {
        fn constructed(&self) {
            self.parent_constructed();
            self.max_width.set(-1);
            self.max_height.set(-1);
        }

        fn dispose(&self) {
            // GTK4 では、親が壊れる前に子を外しておく必要がある。
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for SizeBin {
        /// 中身の申告を通しつつ、上限のある軸の「自然な大きさ」を決め直す。
        ///
        /// 親は「自然な大きさまでは配る、空きが足りなければそれ以下」という
        /// 決め方をするので、上限は自然な大きさとして渡す。
        ///
        /// | その軸の指定 | 自然な大きさ |
        /// | --- | --- |
        /// | [`Length::Fill`] + 上限 | **上限そのもの** (空きがあれば上限まで広がる) |
        /// | [`Length::Auto`] / [`Length::Fixed`] + 上限 | 中身と上限の小さいほう |
        ///
        /// 最小 (`minimum`) は 2 つの理由で下げる。
        ///
        /// 1. 上限より下げないと、空きが上限より狭いときに中身がはみ出す。
        /// 2. [`Length::Fill`] は「大きさを親が決める」という指定なので、
        ///    **中身の都合で親を押し広げない**。これをしないと、`Fill` を
        ///    指定した中身がウィンドウの縮められる下限を決めてしまう
        ///    (Web バックエンドが `min-width: 0` を書いているのと同じ理由)。
        ///    下限が要るときは [`Sizing::min_width`] などで指定する。
        ///    そちらは `size_request` として GTK4 が改めて下限に効かせる。
        ///
        /// ただし**縮められない中身** ([`rigid_width`](Self::rigid_width)) は別で、
        /// 中身の最小をそのまま通す。GTK4 は最小より狭い場所を配られても中身を
        /// 縮めず、はみ出して描いてしまうため。
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.obj().first_child() else {
                return (0, 0, -1, -1);
            };
            let (child_minimum, child_natural, min_baseline, nat_baseline) =
                child.measure(orientation, for_size);
            let (mut minimum, mut natural) = (child_minimum, child_natural);
            let horizontal = orientation == gtk::Orientation::Horizontal;
            let sizing = self.sizing.get();
            let (cap, length, rigid) = if horizontal {
                (self.max_width.get(), sizing.width, self.rigid_width.get())
            } else {
                (self.max_height.get(), sizing.height, false)
            };
            if length.is_fill() {
                minimum = 0;
            }
            if cap >= 0 {
                natural = if length.is_fill() {
                    // 上限は「通常時に確保したい大きさ」も兼ねる。
                    cap
                } else {
                    natural.min(cap)
                };
                minimum = minimum.min(cap);
            }
            if rigid {
                minimum = child_minimum;
                natural = natural.max(child_minimum);
            }
            (minimum, natural, min_baseline, nat_baseline)
        }

        /// 中身を自分と同じ場所いっぱいに置く。
        ///
        /// `GtkBinLayout` に任せたいところだが、**レイアウトマネージャーを
        /// 付けると GTK4 は `measure` をそちらへ回してしまい**、上限が効かなく
        /// なる。置き方そのものは 1 行で済むので、ここで両方を受け持つ。
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.obj().first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

glib::wrapper! {
    /// naui のウィジェット 1 つを包み、大きさの上限を足す入れ物。
    pub struct SizeBin(ObjectSubclass<imp::SizeBin>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SizeBin {
    /// 中身を 1 つ持つ入れ物を作る。
    ///
    /// 寄せ方の初期値は `Center` にしておく。GTK4 の既定は `Fill` (配られた場所
    /// いっぱいに広げる) だが、naui の既定は「中身に合わせる」([`Length::Auto`])
    /// なので、そのままでは意味がずれる。
    pub(crate) fn wrap(child: &impl IsA<gtk::Widget>) -> Self {
        let bin: SizeBin = glib::Object::new();
        bin.set_halign(gtk::Align::Center);
        bin.set_valign(gtk::Align::Center);
        child.as_ref().set_parent(&bin);
        bin
    }

    fn imp(&self) -> &imp::SizeBin {
        gtk::subclass::prelude::ObjectSubclassIsExt::imp(self)
    }

    /// 中身が幅を縮められないことを伝える。
    ///
    /// `GtkSpinButton` のように「欄とボタンが並ぶ最小の幅」を持つ中身は、
    /// それより狭い場所を配られても縮まず、はみ出して描かれてしまう。
    /// これを立てた入れ物は、上限より中身の最小を優先する。
    pub(crate) fn mark_rigid_width(&self) {
        self.imp().rigid_width.set(true);
        self.queue_resize();
    }

    /// アプリが指定した大きさ。
    pub(crate) fn sizing(&self) -> Sizing {
        self.imp().sizing.get()
    }

    /// 大きさの指定を反映する。呼ぶたびに以前の指定は外れる。
    pub(crate) fn apply_sizing(&self, sizing: Sizing) {
        let imp = self.imp();
        imp.sizing.set(sizing);

        let width = axis(sizing.width, sizing.min_width, sizing.max_width);
        let height = axis(sizing.height, sizing.min_height, sizing.max_height);

        imp.max_width.set(width.cap);
        imp.max_height.set(height.cap);
        // `Fill` の軸は最小を 0 として申告する (大きさは親が決める) 以上、
        // 配られた場所からはみ出して描いてはいけない。CSS の
        // `min-width: 0` と `overflow: hidden` を組みで使うのと同じ。
        self.set_overflow(if sizing.width.is_fill() || sizing.height.is_fill() {
            gtk::Overflow::Hidden
        } else {
            gtk::Overflow::Visible
        });
        self.set_size_request(width.request, height.request);
        self.apply_expand(true, width.expand);
        self.apply_expand(false, height.expand);
        self.set_halign(width.align);
        self.set_valign(height.align);
        self.queue_resize();
    }

    /// 余りを受け取るかどうかを指定する。
    ///
    /// [`Length::Auto`] のときは自分で決めず、`hexpand-set` を下ろして
    /// **子から伝わってくるまま**にする。`Stack` の中に `Fill` の子が
    /// 入っているとき、その `Stack` 自身も余りを受け取れるようにするため。
    fn apply_expand(&self, horizontal: bool, expand: Option<bool>) {
        let (value, is_set) = if horizontal {
            ("hexpand", "hexpand-set")
        } else {
            ("vexpand", "vexpand-set")
        };
        match expand {
            Some(expand) => self.set_property(value, expand),
            None => self.set_property(is_set, false),
        }
    }

    /// ウィンドウ直下など、親いっぱいに広がってほしい場所へ入れる。
    ///
    /// 自分で大きさを指定していない軸だけを `Fill` にする。
    pub(crate) fn fill_parent(&self) {
        let sizing = self.sizing();
        if matches!(sizing.width, Length::Auto) && sizing.max_width.is_none() {
            self.set_halign(gtk::Align::Fill);
        }
        if matches!(sizing.height, Length::Auto) && sizing.max_height.is_none() {
            self.set_valign(gtk::Align::Fill);
        }
    }

    /// グリッドの列 / 行の決め方を受け取る。
    ///
    /// `GtkGrid` は列や行そのものに幅を持たせられない。代わりに
    /// **その列に入っている子**へ指定を写す。列の幅は中に入っている子の
    /// いちばん大きいものに合わせて決まるので、これで列の幅が決まる。
    ///
    /// 自分で大きさを指定している軸には触れない (子の指定が優先)。
    pub(crate) fn apply_track(&self, horizontal: bool, track: Track) {
        let sizing = self.sizing();
        let own = if horizontal {
            sizing.width
        } else {
            sizing.height
        };
        if !matches!(own, Length::Auto) {
            return;
        }
        match track {
            Track::Auto => {}
            Track::Fixed(value) => {
                let value = to_px(value);
                let imp = self.imp();
                if horizontal {
                    imp.max_width.set(value);
                    self.set_size_request(value, self.height_request());
                    self.set_halign(gtk::Align::Fill);
                } else {
                    imp.max_height.set(value);
                    self.set_size_request(self.width_request(), value);
                    self.set_valign(gtk::Align::Fill);
                }
            }
            Track::Fill(_) => {
                // 重みは `GtkGrid` に無い。余りは広がる列で等分される。
                if horizontal {
                    self.set_hexpand(true);
                    self.set_halign(gtk::Align::Fill);
                } else {
                    self.set_vexpand(true);
                    self.set_valign(gtk::Align::Fill);
                }
            }
        }
        self.queue_resize();
    }

    /// コンテナの寄せ方を受け取る。
    ///
    /// 自分の指定が優先されるのは、その軸が上限なしの [`Length::Fill`] のときだけ。
    /// [`Length::Fixed`] や上限付きの `Fill` は「広げてはいけない」ので、
    /// コンテナが `Fill` を渡してきても `Center` に読み替える。
    pub(crate) fn set_cross_align(&self, align: Align, vertical_container: bool) {
        let sizing = self.sizing();
        let (length, max) = if vertical_container {
            (sizing.width, sizing.max_width)
        } else {
            (sizing.height, sizing.max_height)
        };
        if length.is_fill() && max.is_none() {
            return;
        }
        let mut align = to_gtk_align(align);
        if align == gtk::Align::Fill && !matches!(length, Length::Auto) {
            align = gtk::Align::Center;
        }
        if vertical_container {
            self.set_halign(align);
        } else {
            self.set_valign(align);
        }
    }
}

/// 1 つの軸について、GTK4 へ渡す値の組。
struct AxisPlan {
    /// `set_size_request` に渡す最小の大きさ。-1 なら指定しない。
    request: i32,
    /// `measure` で頭打ちにする上限。-1 なら上限なし。
    cap: i32,
    /// 余りを受け取るか。`None` なら子から伝わってくるままにする。
    expand: Option<bool>,
    /// 配られた場所の中でのふるまい。
    align: gtk::Align,
}

/// 1 つの軸の指定を GTK4 の値へ翻訳する。
///
/// | naui | GTK4 |
/// | --- | --- |
/// | [`Length::Auto`] | 何もしない (中身の自然な大きさ) |
/// | [`Length::Fixed`] | `size_request` と上限を同じ値にし、寄せ方は `Center` |
/// | [`Length::Fill`] (上限なし) | `expand` を立て、寄せ方は `Fill` |
/// | [`Length::Fill`] (上限あり) | `expand` を立て、上限で頭打ち (寄せ方は `Center`) |
///
/// 寄せ方を `Fill` にすると、GTK4 は配られた場所いっぱいに広げてしまい上限が
/// 効かない。上限を指定した軸だけ `Center` にしているのはこのため。
fn axis(length: Length, min: Option<f64>, max: Option<f64>) -> AxisPlan {
    let min = min.map(to_px);
    let max = max.map(to_px);
    match length {
        Length::Fixed(value) => {
            let value = to_px(value);
            AxisPlan {
                request: min.map_or(value, |m| m.max(value)),
                cap: max.map_or(value, |m| m.min(value)),
                expand: Some(false),
                align: gtk::Align::Center,
            }
        }
        Length::Fill => AxisPlan {
            request: min.unwrap_or(-1),
            cap: max.unwrap_or(-1),
            expand: Some(true),
            align: if max.is_some() {
                gtk::Align::Center
            } else {
                gtk::Align::Fill
            },
        },
        Length::Auto => AxisPlan {
            request: min.unwrap_or(-1),
            cap: max.unwrap_or(-1),
            expand: None,
            align: gtk::Align::Center,
        },
    }
}

fn to_px(value: f64) -> i32 {
    value.round().clamp(0.0, i32::MAX as f64) as i32
}

/// naui の寄せ方を GTK4 の `align` へ写す。
pub(crate) fn to_gtk_align(align: Align) -> gtk::Align {
    match align {
        Align::Start => gtk::Align::Start,
        Align::Center => gtk::Align::Center,
        Align::End => gtk::Align::End,
        Align::Fill => gtk::Align::Fill,
    }
}

/// 余白を、そのウィジェットの外周のマージンとして反映する。
///
/// GTK4 のコンテナは「内側の余白」を持たない代わりに、どのウィジェットも
/// 4 辺のマージンを持つ。コンテナ自身にマージンを付けると中身の置ける範囲が
/// そのぶん狭くなり、naui の [`Padding`](naui_core::Padding) と同じ意味になる。
pub(crate) fn apply_padding(widget: &impl IsA<gtk::Widget>, padding: naui_core::Padding) {
    let widget = widget.as_ref();
    widget.set_margin_top(to_px(padding.top));
    widget.set_margin_end(to_px(padding.right));
    widget.set_margin_bottom(to_px(padding.bottom));
    widget.set_margin_start(to_px(padding.left));
}
