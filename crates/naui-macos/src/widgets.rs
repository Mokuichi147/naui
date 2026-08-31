//! AppKit の実コントロールを包むハンドル群。
//!
//! どのハンドルも `Rc<Inner>` で、`Inner` が Retained なネイティブオブジェクトと
//! トランポリンを保持する。ハンドルを clone してもネイティブは 1 つ。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Align, Orientation, Padding};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBorderType, NSButton, NSButtonType, NSColor,
    NSControlStateValueOff, NSControlStateValueOn, NSFont, NSLayoutAttribute, NSLayoutConstraint,
    NSLineBreakMode, NSProgressIndicator, NSProgressIndicatorStyle, NSScrollView, NSSearchField,
    NSSecureTextField, NSSlider, NSStackView, NSStackViewDistribution, NSTextField, NSTextView,
    NSUserInterfaceLayoutOrientation, NSView, NSViewFrameDidChangeNotification,
};
use objc2_foundation::{
    NSArray, NSEdgeInsets, NSNotificationCenter, NSPoint, NSRect, NSSize, NSString,
};

use crate::trampoline::{
    ActionTarget, SearchHandlers, SearchObserver, TextObserver, TextViewObserver,
};

/// naui のウィジェットが実装する共通インタフェース。
pub trait Widget: 'static {
    /// 対応する AppKit のビュー。バックエンド固有の脱出口として公開している。
    fn native_view(&self) -> Retained<NSView>;

    #[doc(hidden)]
    fn boxed_clone(&self) -> Box<dyn Widget>;
}

macro_rules! impl_widget {
    ($t:ty) => {
        impl Widget for $t {
            fn native_view(&self) -> Retained<NSView> {
                let view: &NSView = self.0.native.as_ref();
                view.retain()
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
            /// 実際の大きさを決めるのは Auto Layout なので、ここで渡すのは
            /// 「固定する」「親の余りを受け取る」といった制約だけ。
            ///
            /// 交差軸の `Fill` と、グリッドのマス内で広がる指定は、
            /// コンテナへ入れる**前**に呼ぶこと。AppKit では制約とセルの配置を
            /// `append` / `attach` の時点で張るため、後から変えても反映されない。
            pub fn set_sizing(&self, sizing: naui_core::Sizing) {
                let view = <$t as Widget>::native_view(self);
                crate::layout::apply_sizing(&view, sizing);
            }
        }
    };
}

pub(crate) use {impl_sizing, impl_widget};

// ------------------------------------------------------------------ Label

struct LabelInner {
    native: Retained<NSTextField>,
    mtm: MainThreadMarker,
    /// 折り返しているかどうか。
    wraps: Cell<bool>,
    /// 折り返す幅を frame に追従させる中継。折り返していない間は `None`。
    ///
    /// 中継はハンドルと同じ寿命で持つ (`Toast` と同じ決まり)。
    width_target: RefCell<Option<Retained<ActionTarget>>>,
}

/// 編集できないテキスト (NSTextField のラベル構成)。
#[derive(Clone)]
pub struct Label(Rc<LabelInner>);
impl_widget!(Label);

impl Label {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = NSTextField::labelWithString(&NSString::from_str(text), mtm);
        let this = Self(Rc::new(LabelInner {
            native,
            mtm,
            wraps: Cell::new(false),
            width_target: RefCell::new(None),
        }));
        this.set_wrap(false);
        this
    }

    pub fn text(&self) -> String {
        self.0.native.stringValue().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.setStringValue(&NSString::from_str(text));
    }

    /// 長い文字列を折り返すかどうか。既定は折り返さない。
    ///
    /// 折り返さないときは 1 行のまま、入りきらない分を末尾の省略記号 (…) で
    /// 切る (`NSLineBreakMode` の `ByTruncatingTail`)。
    ///
    /// 折り返す幅を決めるのは親なので、`Stack` の中では
    /// [`set_sizing`](Self::set_sizing) で幅を与える
    /// (`Sizing::fill_width()` など)。`NSTextField` は自分の幅が決まって
    /// 初めて高さを返せるため、naui は frame の変化を見て
    /// `preferredMaxLayoutWidth` を追従させている。
    pub fn set_wrap(&self, wrap: bool) {
        self.0.wraps.set(wrap);
        self.0.native.setUsesSingleLineMode(!wrap);
        self.0.native.setLineBreakMode(if wrap {
            NSLineBreakMode::ByWordWrapping
        } else {
            NSLineBreakMode::ByTruncatingTail
        });
        self.0
            .native
            .setMaximumNumberOfLines(if wrap { 0 } else { 1 });

        let center = NSNotificationCenter::defaultCenter();
        if let Some(previous) = self.0.width_target.borrow_mut().take() {
            unsafe {
                center.removeObserver_name_object(
                    &previous,
                    Some(NSViewFrameDidChangeNotification),
                    Some(&self.0.native),
                );
            }
        }
        if !wrap {
            self.0.native.setPreferredMaxLayoutWidth(0.0);
            return;
        }

        self.0.sync_preferred_width();
        self.0.native.setPostsFrameChangedNotifications(true);
        let target = ActionTarget::new(self.0.mtm, {
            let weak = Rc::downgrade(&self.0);
            move || {
                if let Some(inner) = weak.upgrade() {
                    inner.sync_preferred_width();
                }
            }
        });
        unsafe {
            center.addObserver_selector_name_object(
                &target,
                sel!(invoke:),
                Some(NSViewFrameDidChangeNotification),
                Some(&self.0.native),
            );
        }
        *self.0.width_target.borrow_mut() = Some(target);
    }
}

impl LabelInner {
    /// 折り返す幅を、いまの frame の幅へそろえる。
    ///
    /// 値が変わったときだけ書く。`preferredMaxLayoutWidth` を書くと intrinsic
    /// size が無効になって次のレイアウトが走るので、毎回書くと回り続ける。
    fn sync_preferred_width(&self) {
        if !self.wraps.get() {
            return;
        }
        let width = self.native.frame().size.width;
        if width <= 0.0 {
            return;
        }
        if (self.native.preferredMaxLayoutWidth() - width).abs() < 0.5 {
            return;
        }
        self.native.setPreferredMaxLayoutWidth(width);
    }
}

// ----------------------------------------------------------------- Button

struct ButtonInner {
    native: Retained<NSButton>,
    /// クリック時のクロージャを保持するオブジェクト。
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// 押しボタン (NSButton)。
#[derive(Clone)]
pub struct Button(Rc<ButtonInner>);
impl_widget!(Button);

impl Button {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = unsafe {
            NSButton::buttonWithTitle_target_action(&NSString::from_str(text), None, None, mtm)
        };
        Self(Rc::new(ButtonInner {
            native,
            target: RefCell::new(None),
        }))
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.setTitle(&NSString::from_str(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// クリックされたときに呼ばれる。設定し直すと以前のものは外れる。
    pub fn on_click(&self, f: impl FnMut() + 'static) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let target = ActionTarget::new(mtm, f);
        unsafe {
            self.0.native.setTarget(Some(&target));
            self.0.native.setAction(Some(sel!(invoke:)));
        }
        *self.0.target.borrow_mut() = Some(target);
    }

    /// クリックを発生させる (テストや自動操作用)。
    pub fn click(&self) {
        unsafe { self.0.native.performClick(None) };
    }
}

// --------------------------------------------------------------- Checkbox

struct CheckboxInner {
    native: Retained<NSButton>,
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// チェックボックス (NSButton の Switch タイプ)。
#[derive(Clone)]
pub struct Checkbox(Rc<CheckboxInner>);
impl_widget!(Checkbox);

impl Checkbox {
    pub(crate) fn new(mtm: MainThreadMarker, label: &str) -> Self {
        let native = unsafe {
            NSButton::checkboxWithTitle_target_action(&NSString::from_str(label), None, None, mtm)
        };
        native.setButtonType(NSButtonType::Switch);
        Self(Rc::new(CheckboxInner {
            native,
            target: RefCell::new(None),
        }))
    }

    pub fn is_checked(&self) -> bool {
        let state = self.0.native.state();
        state != NSControlStateValueOff
    }

    pub fn set_checked(&self, checked: bool) {
        let state = if checked {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        };
        self.0.native.setState(state);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// 状態が変わったときに、変更後の値で呼ばれる。
    pub fn on_toggle(&self, mut f: impl FnMut(bool) + 'static) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let native = self.0.native.clone();
        let target = ActionTarget::new(mtm, move || {
            let state = native.state();
            f(state != NSControlStateValueOff);
        });
        unsafe {
            self.0.native.setTarget(Some(&target));
            self.0.native.setAction(Some(sel!(invoke:)));
        }
        *self.0.target.borrow_mut() = Some(target);
    }

    /// クリックを発生させる (テストや自動操作用)。
    pub fn click(&self) {
        unsafe { self.0.native.performClick(None) };
    }
}

// -------------------------------------------------------------- TextInput

struct TextInputInner {
    native: Retained<NSTextField>,
    observer: RefCell<Option<Retained<TextObserver>>>,
}

/// 1 行テキスト入力 (NSTextField)。日本語入力は AppKit の IME がそのまま効く。
#[derive(Clone)]
pub struct TextInput(Rc<TextInputInner>);
impl_widget!(TextInput);

impl TextInput {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = NSTextField::textFieldWithString(&NSString::from_str(text), mtm);
        Self(Rc::new(TextInputInner {
            native,
            observer: RefCell::new(None),
        }))
    }

    pub fn text(&self) -> String {
        self.0.native.stringValue().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.setStringValue(&NSString::from_str(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0
            .native
            .setPlaceholderString(Some(&NSString::from_str(text)));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// 1 文字入力するたびに、その時点の文字列で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let observer = TextObserver::new(mtm, f);
        unsafe {
            self.0
                .native
                .setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*observer)))
        };
        *self.0.observer.borrow_mut() = Some(observer);
    }
}

// ----------------------------------------------------------- PasswordInput

struct PasswordInputInner {
    native: Retained<NSSecureTextField>,
    observer: RefCell<Option<Retained<TextObserver>>>,
}

/// パスワード入力 (`NSSecureTextField`)。
///
/// API の形は [`TextInput`] と同じで、違うのは**打った文字が伏せ字になる**
/// ことだけ。伏せ字を一時的に外す仕掛けは AppKit に無いので持たない。
#[derive(Clone)]
pub struct PasswordInput(Rc<PasswordInputInner>);
impl_widget!(PasswordInput);

impl PasswordInput {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSSecureTextField::new(mtm);
        Self(Rc::new(PasswordInputInner {
            native,
            observer: RefCell::new(None),
        }))
    }

    /// いま入力されている文字列。
    pub fn text(&self) -> String {
        self.0.native.stringValue().to_string()
    }

    /// 文字列を置き換える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        self.0.native.setStringValue(&NSString::from_str(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0
            .native
            .setPlaceholderString(Some(&NSString::from_str(text)));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// 1 文字入力するたびに、その時点の文字列で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let observer = TextObserver::new(mtm, f);
        unsafe {
            self.0
                .native
                .setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*observer)))
        };
        *self.0.observer.borrow_mut() = Some(observer);
    }
}

// ------------------------------------------------------------- SearchInput

struct SearchInputInner {
    native: Retained<NSSearchField>,
    handlers: Rc<SearchHandlers>,
    // NSSearchField の delegate は weak なので、こちら側で生かしておく。
    observer: RefCell<Option<Retained<SearchObserver>>>,
}

/// 検索の入力欄 (`NSSearchField`)。
///
/// 虫めがねの印と、打ち始めると出る取り消しボタン (✕) は AppKit が出す。
/// [`on_change`](SearchInput::on_change) は打つたび、
/// [`on_search`](SearchInput::on_search) は Enter で確定したときに呼ばれる。
#[derive(Clone)]
pub struct SearchInput(Rc<SearchInputInner>);
impl_widget!(SearchInput);

impl SearchInput {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSSearchField::new(mtm);
        let this = Self(Rc::new(SearchInputInner {
            native,
            handlers: Rc::new(SearchHandlers::default()),
            observer: RefCell::new(None),
        }));
        // デリゲートは 2 つの通知で共有するので、生成のときに一度だけ張る。
        let observer = SearchObserver::new(mtm, this.0.handlers.clone());
        unsafe {
            this.0
                .native
                .setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*observer)))
        };
        *this.0.observer.borrow_mut() = Some(observer);
        this
    }

    /// いま入力されている文字列。
    pub fn text(&self) -> String {
        self.0.native.stringValue().to_string()
    }

    /// 文字列を置き換える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        self.0.native.setStringValue(&NSString::from_str(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0
            .native
            .setPlaceholderString(Some(&NSString::from_str(text)));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// 1 文字入力するたびに、その時点の文字列で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.handlers.set_change(f);
    }

    /// Enter で確定したときに、その時点の文字列で呼ばれる。
    pub fn on_search(&self, f: impl FnMut(&str) + 'static) {
        self.0.handlers.set_search(f);
    }
}

// --------------------------------------------------------------- TextArea

// プレースホルダー用のラベル。
//
// NSTextView の上に重ねるため、そのままでは文字の載っている場所を
// クリックしてもキャレットが立たない。`hitTest:` で nil を返すと、
// AppKit はこのビューを飛ばして下の NSTextView へ当たり判定を渡す。
define_class!(
    #[unsafe(super(NSTextField))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiPlaceholderLabel"]
    /// クリックを下のビューへ通すラベル。
    struct PlaceholderLabel;

    unsafe impl NSObjectProtocol for PlaceholderLabel {}

    impl PlaceholderLabel {
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl PlaceholderLabel {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this: Allocated<Self> = Self::alloc(mtm);
        let this: Retained<Self> = unsafe { msg_send![this, init] };
        // NSTextField::labelWithString と同じ構成を手で作る。
        this.setEditable(false);
        this.setSelectable(false);
        this.setBezeled(false);
        this.setDrawsBackground(false);
        this.setTextColor(Some(&NSColor::placeholderTextColor()));
        this.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSize())));
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this
    }
}

/// 差し替えできる 1 本の「文字が変わった」通知先。
///
/// プレースホルダーの出し入れは naui 側でも購読する必要があるため、
/// デリゲートは生成時から常に付けておき、アプリのクロージャはここへ入れる。
/// 通知の最中に `on_change` を呼び直しても二重借用にならないよう、
/// 呼び出しの間だけ取り出す ([`crate::trampoline::SelectHandler`] と同じ形)。
#[derive(Clone, Default)]
struct TextHandler(Rc<RefCell<Option<Box<dyn FnMut(&str)>>>>);

impl TextHandler {
    fn set(&self, f: impl FnMut(&str) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, text: &str) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(text);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

struct TextAreaInner {
    /// 外から見えるビュー。複数行入力はこのスクロールビューごと 1 つ。
    scroll: Retained<NSScrollView>,
    text_view: Retained<NSTextView>,
    placeholder: Retained<PlaceholderLabel>,
    handler: TextHandler,
    /// デリゲートは weak 参照なので保持する。
    _observer: Retained<TextViewObserver>,
}

/// 複数行テキスト入力 (NSScrollView に載せた NSTextView)。
///
/// 改行を含む文字列をそのまま扱い、折り返し・スクロール・IME・取り消しは
/// AppKit が行う。**スクロールビューは中身に合わせた高さを持たない**ため、
/// `set_sizing` で高さを指定すること ([`crate::Scroll`] や [`crate::List`] と同じ)。
#[derive(Clone)]
pub struct TextArea(Rc<TextAreaInner>);

impl Widget for TextArea {
    fn native_view(&self) -> Retained<NSView> {
        let view: &NSView = self.0.scroll.as_ref();
        view.retain()
    }
    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }
}

crate::widgets::impl_sizing!(TextArea);

impl TextArea {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        // 1 行入力 (NSTextField) と同じ、へこんだ枠にする。
        scroll.setBorderType(NSBorderType::BezelBorder);

        let text_view = NSTextView::new(mtm);
        // NSScrollView の中の NSTextView は、Auto Layout ではなく
        // autoresizing で幅を追わせるのが AppKit の標準の組み方。
        // 高さは中身に合わせて伸び、はみ出した分をスクロールが担う。
        let content = scroll.contentSize();
        text_view.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), content));
        text_view.setMinSize(NSSize::new(0.0, 0.0));
        text_view.setMaxSize(NSSize::new(f64::MAX, f64::MAX));
        text_view.setVerticallyResizable(true);
        text_view.setHorizontallyResizable(false);
        text_view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        if let Some(container) = unsafe { text_view.textContainer() } {
            // 幅はテキストビューに追従させ、高さは無制限にする。
            // これで横は折り返し、縦はスクロールになる。
            container.setContainerSize(NSSize::new(content.width, f64::MAX));
            container.setWidthTracksTextView(true);
        }
        // 書式付きテキストは扱わない。貼り付けも書式を落として素の文字にする。
        text_view.setRichText(false);
        text_view.setAllowsUndo(true);
        text_view.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSize())));
        text_view.setString(&NSString::from_str(text));
        scroll.setDocumentView(Some(&text_view));

        // NSTextView にプレースホルダーは無いので、薄い文字のラベルを重ねる。
        let placeholder = PlaceholderLabel::new(mtm);
        placeholder.setHidden(!text.is_empty());
        let view: &NSView = text_view.as_ref();
        view.addSubview(&placeholder);
        // 文字の描き始めは「テキストの余白 + 行の余白」の内側。
        let inset = text_view.textContainerInset();
        let padding = unsafe { text_view.textContainer() }
            .map(|container| container.lineFragmentPadding())
            .unwrap_or(0.0);
        let constraints = [
            placeholder
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&view.leadingAnchor(), inset.width + padding),
            placeholder
                .topAnchor()
                .constraintEqualToAnchor_constant(&view.topAnchor(), inset.height),
            placeholder
                .trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(
                    &view.trailingAnchor(),
                    -(inset.width + padding),
                ),
        ];
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));

        // デリゲートは常に付けておく。プレースホルダーの出し入れが
        // アプリのクロージャの有無に左右されないようにするため。
        let handler = TextHandler::default();
        let observer = TextViewObserver::new(mtm, {
            let placeholder = placeholder.clone();
            let handler = handler.clone();
            move |text: &str| {
                placeholder.setHidden(!text.is_empty());
                handler.emit(text);
            }
        });
        text_view.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*observer)));

        Self(Rc::new(TextAreaInner {
            scroll,
            text_view,
            placeholder,
            handler,
            _observer: observer,
        }))
    }

    /// いまの文字列。改行はそのまま含まれる。
    pub fn text(&self) -> String {
        self.0.text_view.string().to_string()
    }

    /// 文字列を置き換える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        self.0.text_view.setString(&NSString::from_str(text));
        // setString: は textDidChange: を出さないので、ここで合わせる。
        self.0.placeholder.setHidden(!text.is_empty());
    }

    /// 何も入力されていないときに薄く出る文字。
    pub fn set_placeholder(&self, text: &str) {
        self.0.placeholder.setStringValue(&NSString::from_str(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.text_view.setEditable(enabled);
        // 編集できないことが見て分かるよう、無効なコントロールの色にする。
        // 戻すときは NSTextView の既定である textColor へ (labelColor ではない)。
        let color = if enabled {
            NSColor::textColor()
        } else {
            NSColor::disabledControlTextColor()
        };
        self.0.text_view.setTextColor(Some(&color));
    }

    /// 1 文字入力するたびに、その時点の文字列で呼ばれる。
    ///
    /// 改行の入力でも呼ばれる。`set_text` では呼ばれない。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.handler.set(f);
    }

    /// 中身の `NSTextView`。バックエンド固有の脱出口として公開している。
    pub fn native_text_view(&self) -> Retained<NSTextView> {
        self.0.text_view.clone()
    }
}

// ----------------------------------------------------------------- Slider

struct SliderInner {
    native: Retained<NSSlider>,
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// スライダー (NSSlider)。
#[derive(Clone)]
pub struct Slider(Rc<SliderInner>);
impl_widget!(Slider);

impl Slider {
    pub(crate) fn new(mtm: MainThreadMarker, min: f64, max: f64) -> Self {
        let native = NSSlider::new(mtm);
        {
            native.setMinValue(min);
            native.setMaxValue(max);
            native.setContinuous(true);
        }
        Self(Rc::new(SliderInner {
            native,
            target: RefCell::new(None),
        }))
    }

    pub fn value(&self) -> f64 {
        self.0.native.doubleValue()
    }

    pub fn set_value(&self, value: f64) {
        self.0.native.setDoubleValue(value);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// つまみが動くたびに、その値で呼ばれる。
    pub fn on_change(&self, mut f: impl FnMut(f64) + 'static) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let native = self.0.native.clone();
        let target = ActionTarget::new(mtm, move || {
            f(native.doubleValue());
        });
        unsafe {
            self.0.native.setTarget(Some(&target));
            self.0.native.setAction(Some(sel!(invoke:)));
        }
        *self.0.target.borrow_mut() = Some(target);
    }
}

// ------------------------------------------------------------ ProgressBar

struct ProgressInner {
    native: Retained<NSProgressIndicator>,
}

/// 進捗バー (NSProgressIndicator)。
#[derive(Clone)]
pub struct ProgressBar(Rc<ProgressInner>);
impl_widget!(ProgressBar);

impl ProgressBar {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let native = NSProgressIndicator::new(mtm);
        {
            native.setStyle(NSProgressIndicatorStyle::Bar);
            native.setIndeterminate(false);
            native.setMinValue(0.0);
            native.setMaxValue(1.0);
        }
        Self(Rc::new(ProgressInner { native }))
    }

    /// 0.0..=1.0。
    pub fn set_value(&self, value: f64) {
        self.0.native.setDoubleValue(value.clamp(0.0, 1.0));
    }

    pub fn value(&self) -> f64 {
        self.0.native.doubleValue()
    }
}

// ------------------------------------------------------------------ Stack

struct StackInner {
    native: Retained<NSStackView>,
    /// Auto の子が余りを受け取らないよう、末尾で余りを受けるビュー。
    _tail_spacer: Retained<NSView>,
    tail_spacer_active: Cell<bool>,
    /// 子のハンドルを保持し、トランポリンごと生かしておく。
    children: RefCell<Vec<Box<dyn Widget>>>,
    /// 交差軸に `Fill` を指定された子を、スタックの幅 / 高さへ結び付ける制約。
    /// 余白が変わると定数も変わるので保持しておく。
    fill_constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
    padding: Cell<Padding>,
    spacing: Cell<f64>,
}

/// 縦 / 横に子を並べるコンテナ (NSStackView)。
#[derive(Clone)]
pub struct Stack(Rc<StackInner>);
impl_widget!(Stack);

impl Stack {
    pub(crate) fn new(mtm: MainThreadMarker, orientation: Orientation) -> Self {
        let native = NSStackView::new(mtm);
        {
            native.setOrientation(if orientation.is_vertical() {
                NSUserInterfaceLayoutOrientation::Vertical
            } else {
                NSUserInterfaceLayoutOrientation::Horizontal
            });
            // `Fill` は AppKit が主軸の hugging priority を必須扱いにし、
            // Auto の子のどれかへ余りを配ってしまう。GravityAreas は
            // 子の hugging priority に従うため、Fill / Spacer だけが余りを受ける。
            native.setDistribution(NSStackViewDistribution::GravityAreas);
            native.setAlignment(if orientation.is_vertical() {
                NSLayoutAttribute::CenterX
            } else {
                NSLayoutAttribute::CenterY
            });
        }
        let spacing = native.spacing();
        let tail_spacer = NSView::new(mtm);
        crate::layout::prepare_child(&tail_spacer);
        // 明示的な `Fill` / `Spacer` よりは優先度を上げ、Auto の子よりは
        // 下げる。これで、指定が無いときだけ末尾の受け皿が余りを吸う。
        for horizontal in [true, false] {
            let orientation = if horizontal {
                objc2_app_kit::NSLayoutConstraintOrientation::Horizontal
            } else {
                objc2_app_kit::NSLayoutConstraintOrientation::Vertical
            };
            tail_spacer.setContentHuggingPriority_forOrientation(2.0, orientation);
            tail_spacer.setContentCompressionResistancePriority_forOrientation(1.0, orientation);
        }
        native.addArrangedSubview(&tail_spacer);
        Self(Rc::new(StackInner {
            native,
            _tail_spacer: tail_spacer,
            tail_spacer_active: Cell::new(true),
            children: RefCell::new(Vec::new()),
            fill_constraints: RefCell::new(Vec::new()),
            padding: Cell::new(Padding::ZERO),
            spacing: Cell::new(spacing),
        }))
    }

    pub fn set_spacing(&self, spacing: f64) {
        self.0.spacing.set(spacing);
        self.0.native.setSpacing(spacing);
        if let Some(last) = self.0.children.borrow().last() {
            let view = last.native_view();
            self.0.native.setCustomSpacing_afterView(0.0, &view);
        }
        self.invalidate_natural_size();
    }

    pub fn set_padding(&self, padding: Padding) {
        self.0.padding.set(padding);
        self.0.native.setEdgeInsets(NSEdgeInsets {
            top: padding.top,
            left: padding.left,
            bottom: padding.bottom,
            right: padding.right,
        });
        // 交差軸いっぱいに広げている子は、余白のぶんだけ狭くなる。
        let inset = self.cross_inset();
        for constraint in self.0.fill_constraints.borrow().iter() {
            constraint.setConstant(-inset);
        }
        self.invalidate_natural_size();
    }

    /// 交差軸方向に取られる余白の合計。
    fn cross_inset(&self) -> f64 {
        let padding = self.0.padding.get();
        if self.is_vertical() {
            padding.left + padding.right
        } else {
            padding.top + padding.bottom
        }
    }

    fn is_vertical(&self) -> bool {
        self.0.native.orientation() == NSUserInterfaceLayoutOrientation::Vertical
    }

    /// NSStackView 自身は intrinsic size を公開しないため、子や余白の変更を
    /// Grid の Auto 行へ明示的に伝える。
    fn invalidate_natural_size(&self) {
        self.0.native.invalidateIntrinsicContentSize();
        self.0.native.setNeedsLayout(true);
        if let Some(parent) = unsafe { self.0.native.superview() } {
            parent.invalidateIntrinsicContentSize();
            parent.setNeedsLayout(true);
        }
    }

    pub fn set_align(&self, align: Align) {
        let vertical = self.is_vertical();
        let attr = match (align, vertical) {
            (Align::Fill, _) => NSLayoutAttribute::NotAnAttribute,
            (Align::Start, true) => NSLayoutAttribute::Leading,
            (Align::Center, true) => NSLayoutAttribute::CenterX,
            (Align::End, true) => NSLayoutAttribute::Trailing,
            (Align::Start, false) => NSLayoutAttribute::Top,
            (Align::Center, false) => NSLayoutAttribute::CenterY,
            (Align::End, false) => NSLayoutAttribute::Bottom,
        };
        self.0.native.setAlignment(attr);
    }

    /// 末尾に子を追加する。
    ///
    /// 子が交差軸に [`naui_core::Length::Fill`] を指定していれば、
    /// スタックの幅 (縦並びのとき) または高さに合わせて広げる。
    /// 主軸方向の `Fill` は hugging priority を通じて NSStackView が扱う。
    pub fn append(&self, child: &dyn Widget) {
        let view = child.native_view();
        crate::layout::prepare_child(&view);
        let index = self.0.children.borrow().len();
        let vertical = self.is_vertical();
        let wants_main_fill = crate::layout::wants_fill(&view, !vertical);
        if !wants_main_fill {
            crate::layout::keep_auto_size(&view, !vertical);
        }
        if wants_main_fill && self.0.tail_spacer_active.replace(false) {
            self.0.native.removeArrangedSubview(&self.0._tail_spacer);
            self.0._tail_spacer.removeFromSuperview();
        }
        if let Some(last) = self.0.children.borrow().last() {
            let previous = last.native_view();
            self.0
                .native
                .setCustomSpacing_afterView(self.0.spacing.get(), &previous);
        }
        self.0
            .native
            .insertArrangedSubview_atIndex(&view, index as isize);
        if self.0.tail_spacer_active.get() {
            self.0.native.setCustomSpacing_afterView(0.0, &view);
        }

        // 縦並びの交差軸は横方向。
        let cross_is_horizontal = vertical;
        if crate::layout::wants_fill(&view, cross_is_horizontal) {
            let inset = self.cross_inset();
            let (child, parent) = if vertical {
                (view.widthAnchor(), self.0.native.widthAnchor())
            } else {
                (view.heightAnchor(), self.0.native.heightAnchor())
            };
            // 「はみ出さない」は必須、「親に合わせる」は 1 段下。上限
            // (`max_width` など) を付けた子は、上限のほうが勝つ。
            let at_most = child.constraintLessThanOrEqualToAnchor_constant(&parent, -inset);
            let equal = child.constraintEqualToAnchor_constant(&parent, -inset);
            equal.setPriority(crate::layout::CROSS_FILL_PRIORITY);
            for constraint in [&at_most, &equal] {
                constraint.setActive(true);
            }
            let mut constraints = self.0.fill_constraints.borrow_mut();
            constraints.push(at_most);
            constraints.push(equal);
        }

        self.0.children.borrow_mut().push(child.boxed_clone());
        self.invalidate_natural_size();
    }

    /// 追加済みの子の数。
    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
