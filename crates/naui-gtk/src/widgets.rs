//! GTK4 の実コントロールを包むハンドル群。
//!
//! どのハンドルも `Rc<Inner>` で、`Inner` が GTK4 のオブジェクトとアプリの
//! クロージャを保持する。ハンドルを clone してもネイティブは 1 つ。
//!
//! シグナルは**作った時点で 1 回だけ**つなぎ、アプリのクロージャは
//! [`Notifier`] に差し替える形で持つ。GTK4 のシグナルハンドラは `Fn`
//! (何度でも呼べる) を要求するのに対し、naui の API は `FnMut` を受けるため。
//!
//! 描画・レイアウト・IME・アクセシビリティ・テーマ追従はすべて GTK4 が行う。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use naui_core::{Align, Orientation, Padding};

use crate::bin::{apply_padding, SizeBin};
use crate::callback::{Notifier, TextNotifier};

/// naui のウィジェットが実装する共通インタフェース。
pub trait Widget: 'static {
    /// 対応する GTK4 のウィジェット。バックエンド固有の脱出口として公開している。
    ///
    /// 返るのは**中身のコントロール**で、コンテナへ入るのはこれを包んだ
    /// [`SizeBin`] のほう。
    fn native_widget(&self) -> gtk::Widget;

    #[doc(hidden)]
    fn size_bin(&self) -> SizeBin;

    #[doc(hidden)]
    fn boxed_clone(&self) -> Box<dyn Widget>;
}

macro_rules! impl_widget {
    ($t:ty) => {
        impl Widget for $t {
            fn native_widget(&self) -> gtk::Widget {
                self.0.native.clone().upcast()
            }
            fn size_bin(&self) -> SizeBin {
                self.0.bin.clone()
            }
            fn boxed_clone(&self) -> Box<dyn Widget> {
                Box::new(self.clone())
            }
        }

        crate::widgets::impl_sizing!($t);
    };
}

/// `Widget` を手書きしている型に、大きさの指定だけを足す。
macro_rules! impl_sizing {
    ($t:ty) => {
        impl $t {
            /// 大きさを指定する。呼ぶたびに以前の指定は外れる。
            ///
            /// 実際の大きさを決めるのは GTK4 のレイアウトなので、ここで渡すのは
            /// 「最小はここまで」「余りを受け取る」「上限で止める」という指定だけ。
            ///
            /// 交差軸の寄せ方はコンテナ ([`Stack::set_align`]) も決めるため、
            /// **コンテナへ入れる前**に呼ぶのが確実。
            pub fn set_sizing(&self, sizing: naui_core::Sizing) {
                <$t as Widget>::size_bin(self).apply_sizing(sizing);
            }
        }
    };
}

pub(crate) use {impl_sizing, impl_widget};

/// シグナルを止めたまま値を書き換える。
///
/// GTK4 はプログラムから値を変えても `changed` を出すが、naui では
/// **利用者の操作でだけ** `on_change` が呼ばれる (macOS / Web と同じ)。
pub(crate) fn without_signal<T>(
    object: &impl IsA<glib::Object>,
    handler: &RefCell<Option<glib::SignalHandlerId>>,
    f: impl FnOnce() -> T,
) -> T {
    let object = object.as_ref();
    let handler = handler.borrow();
    if let Some(id) = handler.as_ref() {
        object.block_signal(id);
    }
    let out = f();
    if let Some(id) = handler.as_ref() {
        object.unblock_signal(id);
    }
    out
}

// ------------------------------------------------------------------ Label

struct LabelInner {
    native: gtk::Label,
    bin: SizeBin,
}

/// 編集できないテキスト (`GtkLabel`)。
#[derive(Clone)]
pub struct Label(Rc<LabelInner>);
impl_widget!(Label);

impl Label {
    pub(crate) fn new(text: &str) -> Self {
        let native = gtk::Label::new(Some(text));
        // GtkLabel の既定は中央ぞろえだが、naui のラベルは他の環境と同じく左詰め。
        native.set_xalign(0.0);
        let bin = SizeBin::wrap(&native);
        let this = Self(Rc::new(LabelInner { native, bin }));
        this.set_wrap(false);
        this
    }

    pub fn text(&self) -> String {
        self.0.native.text().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.set_text(text);
    }

    /// 長い文字列を折り返すかどうか。既定は折り返さない。
    ///
    /// 折り返さないときは 1 行のまま、入りきらない分を末尾の省略記号 (…) で
    /// 切る (`PangoEllipsizeMode` の `End`)。**省略記号を付けると `GtkLabel` の
    /// 最小幅も小さくなる**ので、狭いコンテナへ入れてもコンテナごと押し広げて
    /// しまうことがなくなる。
    pub fn set_wrap(&self, wrap: bool) {
        self.0.native.set_wrap(wrap);
        self.0.native.set_wrap_mode(pango::WrapMode::WordChar);
        self.0.native.set_ellipsize(if wrap {
            pango::EllipsizeMode::None
        } else {
            pango::EllipsizeMode::End
        });
    }
}

// ----------------------------------------------------------------- Button

struct ButtonInner {
    native: gtk::Button,
    bin: SizeBin,
    on_click: Notifier<()>,
}

/// 押しボタン (`GtkButton`)。
#[derive(Clone)]
pub struct Button(Rc<ButtonInner>);
impl_widget!(Button);

impl Button {
    pub(crate) fn new(text: &str) -> Self {
        let native = gtk::Button::with_label(text);
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(ButtonInner {
            native,
            bin,
            on_click: Notifier::default(),
        });
        let weak = Rc::downgrade(&inner);
        inner.native.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.on_click.emit(());
            }
        });
        Self(inner)
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.set_label(text);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 押されるたびに呼ばれる。呼び直すと以前のものは外れる。
    pub fn on_click(&self, mut f: impl FnMut() + 'static) {
        self.0.on_click.set(move |()| f());
    }
}

// --------------------------------------------------------------- Checkbox

struct CheckboxInner {
    native: gtk::CheckButton,
    bin: SizeBin,
    on_toggle: Notifier<bool>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// チェックボックス (`GtkCheckButton`)。
#[derive(Clone)]
pub struct Checkbox(Rc<CheckboxInner>);
impl_widget!(Checkbox);

impl Checkbox {
    pub(crate) fn new(label: &str) -> Self {
        let native = gtk::CheckButton::with_label(label);
        crate::indicator::watch(&native);
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(CheckboxInner {
            native,
            bin,
            on_toggle: Notifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_toggled(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_toggle.emit(native.is_active());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    pub fn is_checked(&self) -> bool {
        self.0.native.is_active()
    }

    /// プログラムから状態を変える。`on_toggle` は呼ばれない。
    pub fn set_checked(&self, checked: bool) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_active(checked);
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 利用者が切り替えるたびに、切り替え後の状態で呼ばれる。
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_toggle.set(f);
    }
}

// -------------------------------------------------------------- TextInput

struct TextInputInner {
    native: gtk::Entry,
    bin: SizeBin,
    on_change: TextNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 1 行のテキスト入力 (`GtkEntry`)。
#[derive(Clone)]
pub struct TextInput(Rc<TextInputInner>);
impl_widget!(TextInput);

impl TextInput {
    pub(crate) fn new(text: &str) -> Self {
        let native = gtk::Entry::new();
        native.set_text(text);
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(TextInputInner {
            native,
            bin,
            on_change: TextNotifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_changed(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_change.emit(native.text().as_str());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    pub fn text(&self) -> String {
        self.0.native.text().to_string()
    }

    /// プログラムから中身を差し替える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_text(text);
        });
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.native.set_placeholder_text(Some(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 利用者が打つたびに、そのときの中身で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }
}

// ----------------------------------------------------------- PasswordInput

struct PasswordInputInner {
    native: gtk::PasswordEntry,
    bin: SizeBin,
    on_change: TextNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// パスワード入力 (`GtkPasswordEntry`)。
///
/// API の形は [`TextInput`] と同じで、違うのは**打った文字が伏せ字になる**
/// ことだけ。伏せ字を一時的に外すボタン (peek icon) は `GtkPasswordEntry` に
/// あるが、4 環境の共通部分に無いので出さない。
#[derive(Clone)]
pub struct PasswordInput(Rc<PasswordInputInner>);
impl_widget!(PasswordInput);

impl PasswordInput {
    pub(crate) fn new() -> Self {
        let native = gtk::PasswordEntry::new();
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(PasswordInputInner {
            native,
            bin,
            on_change: TextNotifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_changed(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_change.emit(native.text().as_str());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    /// いま入力されている文字列。
    pub fn text(&self) -> String {
        self.0.native.text().to_string()
    }

    /// プログラムから中身を差し替える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_text(text);
        });
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.native.set_placeholder_text(Some(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 利用者が打つたびに、そのときの中身で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }
}

// ------------------------------------------------------------- SearchInput

struct SearchInputInner {
    native: gtk::SearchEntry,
    bin: SizeBin,
    on_change: TextNotifier,
    on_search: TextNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 検索の入力欄 (`GtkSearchEntry`)。
///
/// 虫めがねの印と、打ち始めると出る取り消しボタン (✕) は GTK が出す。
/// `on_change` は打つたび (`changed` シグナル)、`on_search` は Enter で
/// 確定したとき (`activate` シグナル) に呼ばれる。GtkSearchEntry には
/// 打鍵をまとめてから出す `search-changed` もあるが、待ち時間の分だけ
/// 他の環境とずれるので使わない。
#[derive(Clone)]
pub struct SearchInput(Rc<SearchInputInner>);
impl_widget!(SearchInput);

impl SearchInput {
    pub(crate) fn new() -> Self {
        let native = gtk::SearchEntry::new();
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(SearchInputInner {
            native,
            bin,
            on_change: TextNotifier::default(),
            on_search: TextNotifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_changed(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_change.emit(native.text().as_str());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_activate(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_search.emit(native.text().as_str());
                }
            });
        }
        Self(inner)
    }

    /// いま入力されている文字列。
    pub fn text(&self) -> String {
        self.0.native.text().to_string()
    }

    /// プログラムから中身を差し替える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_text(text);
        });
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.native.set_placeholder_text(Some(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 利用者が打つたびに、そのときの中身で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// Enter で確定したときに、そのときの中身で呼ばれる。
    pub fn on_search(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_search.set(f);
    }
}

// --------------------------------------------------------------- TextArea

struct TextAreaInner {
    native: gtk::TextView,
    /// `GtkTextView` は自分でスクロールしないので、スクロール領域に載せる。
    _scroller: gtk::ScrolledWindow,
    /// `GtkTextView` に placeholder は無いため、空のときだけ重ねて出すラベル。
    placeholder: gtk::Label,
    bin: SizeBin,
    on_change: TextNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 改行を含む文字列を入力できる欄 (`GtkTextView`)。
///
/// スクロールと同じく中身に合わせた高さを持たないので、
/// [`TextArea::set_sizing`] で指定しておく。
#[derive(Clone)]
pub struct TextArea(Rc<TextAreaInner>);
impl_widget!(TextArea);

impl TextArea {
    pub(crate) fn new(text: &str) -> Self {
        let native = gtk::TextView::new();
        native.set_wrap_mode(gtk::WrapMode::WordChar);
        native.buffer().set_text(text);

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        scroller.set_has_frame(true);
        scroller.set_child(Some(&native));

        // placeholder は入力の邪魔にならないよう、クリックを受けない。
        let placeholder = gtk::Label::new(None);
        placeholder.set_halign(gtk::Align::Start);
        placeholder.set_valign(gtk::Align::Start);
        placeholder.set_margin_top(3);
        placeholder.set_margin_start(3);
        placeholder.set_can_target(false);
        placeholder.set_visible(false);
        placeholder.add_css_class("dim-label");

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&scroller));
        overlay.add_overlay(&placeholder);

        let bin = SizeBin::wrap(&overlay);
        let inner = Rc::new(TextAreaInner {
            native,
            _scroller: scroller,
            placeholder,
            bin,
            on_change: TextNotifier::default(),
            handler: RefCell::new(None),
        });

        // placeholder の出し入れは、アプリのクロージャとは別に常時つないでおく。
        // `set_text` で通知を止めても、表示だけは正しく追従する。
        {
            let placeholder = inner.placeholder.clone();
            inner.native.buffer().connect_changed(move |buffer| {
                placeholder.set_visible(is_empty(buffer) && !placeholder.text().is_empty());
            });
        }
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.buffer().connect_changed(move |buffer| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_change.emit(&buffer_text(buffer));
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    pub fn text(&self) -> String {
        buffer_text(&self.0.native.buffer())
    }

    /// プログラムから中身を差し替える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        let buffer = self.0.native.buffer();
        without_signal(&buffer, &self.0.handler, || buffer.set_text(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0.placeholder.set_text(text);
        let empty = is_empty(&self.0.native.buffer());
        self.0.placeholder.set_visible(empty && !text.is_empty());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 利用者が打つたびに、そのときの中身で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }
}

fn buffer_text(buffer: &gtk::TextBuffer) -> String {
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string()
}

fn is_empty(buffer: &gtk::TextBuffer) -> bool {
    buffer.char_count() == 0
}

// ----------------------------------------------------------------- Slider

struct SliderInner {
    native: gtk::Scale,
    bin: SizeBin,
    on_change: Notifier<f64>,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// スライダー (`GtkScale`)。
#[derive(Clone)]
pub struct Slider(Rc<SliderInner>);
impl_widget!(Slider);

impl Slider {
    pub(crate) fn new(min: f64, max: f64) -> Self {
        // 刻みが 0 だと GtkScale が値を動かせないので、幅から決める。
        let step = if max > min { (max - min) / 100.0 } else { 1.0 };
        let native = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
        // 値の数字は naui の API に無いので出さない (NSSlider / `<input>` と同じ)。
        native.set_draw_value(false);
        // GtkScale の自然な幅はつまみ 1 個ぶんしかなく、そのままでは操作できない。
        native.set_width_request(160);
        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(SliderInner {
            native,
            bin,
            on_change: Notifier::default(),
            handler: RefCell::new(None),
        });
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.native.connect_value_changed(move |native| {
                if let Some(inner) = weak.upgrade() {
                    inner.on_change.emit(native.value());
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);
        Self(inner)
    }

    pub fn value(&self) -> f64 {
        self.0.native.value()
    }

    /// プログラムから値を変える。`on_change` は呼ばれない。
    pub fn set_value(&self, value: f64) {
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.set_value(value);
        });
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// つまみが動くたびに、その値で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_change.set(f);
    }
}

// ------------------------------------------------------------ ProgressBar

struct ProgressInner {
    native: gtk::ProgressBar,
    bin: SizeBin,
}

/// 進捗バー (`GtkProgressBar`)。
#[derive(Clone)]
pub struct ProgressBar(Rc<ProgressInner>);
impl_widget!(ProgressBar);

impl ProgressBar {
    pub(crate) fn new() -> Self {
        let native = gtk::ProgressBar::new();
        native.set_fraction(0.0);
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(ProgressInner { native, bin }))
    }

    /// 0.0..=1.0。
    pub fn set_value(&self, value: f64) {
        self.0.native.set_fraction(value.clamp(0.0, 1.0));
    }

    pub fn value(&self) -> f64 {
        self.0.native.fraction()
    }
}

// ------------------------------------------------------------------ Stack

struct StackInner {
    native: gtk::Box,
    bin: SizeBin,
    vertical: bool,
    align: Cell<Align>,
    /// 子のハンドルを保持し、クロージャの受け皿ごと生かしておく。
    children: RefCell<Vec<Box<dyn Widget>>>,
}

/// 縦 / 横に子を並べるコンテナ (`GtkBox`)。
#[derive(Clone)]
pub struct Stack(Rc<StackInner>);
impl_widget!(Stack);

impl Stack {
    pub(crate) fn new(orientation: Orientation) -> Self {
        let vertical = orientation.is_vertical();
        let native = gtk::Box::new(
            if vertical {
                gtk::Orientation::Vertical
            } else {
                gtk::Orientation::Horizontal
            },
            0,
        );
        let bin = SizeBin::wrap(&native);
        Self(Rc::new(StackInner {
            native,
            bin,
            vertical,
            align: Cell::new(Align::default()),
            children: RefCell::new(Vec::new()),
        }))
    }

    pub fn set_spacing(&self, spacing: f64) {
        self.0
            .native
            .set_spacing(spacing.round().clamp(0.0, i32::MAX as f64) as i32);
    }

    pub fn set_padding(&self, padding: Padding) {
        apply_padding(&self.0.native, padding);
    }

    /// 交差軸方向の寄せ方。既定は [`Align::Center`]。
    ///
    /// 交差軸に [`Length::Fill`](naui_core::Length::Fill) を指定した子は、
    /// 自分の指定を優先する。
    pub fn set_align(&self, align: Align) {
        self.0.align.set(align);
        for child in self.0.children.borrow().iter() {
            child.size_bin().set_cross_align(align, self.0.vertical);
        }
    }

    pub fn append(&self, child: &dyn Widget) {
        let bin = child.size_bin();
        bin.set_cross_align(self.0.align.get(), self.0.vertical);
        self.0.native.append(&bin);
        self.0.children.borrow_mut().push(child.boxed_clone());
    }

    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
