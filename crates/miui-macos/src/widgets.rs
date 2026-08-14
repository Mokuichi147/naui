//! AppKit の実コントロールを包むハンドル群。
//!
//! どのハンドルも `Rc<Inner>` で、`Inner` が Retained なネイティブオブジェクトと
//! トランポリンを保持する。ハンドルを clone してもネイティブは 1 つ。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use miui_core::{Align, Orientation, Padding};
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{
    NSButton, NSButtonType, NSControlStateValueOff, NSControlStateValueOn, NSLayoutAttribute,
    NSLayoutConstraint, NSProgressIndicator, NSProgressIndicatorStyle, NSSlider, NSStackView,
    NSStackViewDistribution, NSTextField, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSEdgeInsets, NSString};

use crate::trampoline::{ActionTarget, TextObserver};

/// miui のウィジェットが実装する共通インタフェース。
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
            pub fn set_sizing(&self, sizing: miui_core::Sizing) {
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
}

/// 編集できないテキスト (NSTextField のラベル構成)。
#[derive(Clone)]
pub struct Label(Rc<LabelInner>);
impl_widget!(Label);

impl Label {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = NSTextField::labelWithString(&NSString::from_str(text), mtm);
        Self(Rc::new(LabelInner { native }))
    }

    pub fn text(&self) -> String {
        self.0.native.stringValue().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.setStringValue(&NSString::from_str(text));
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
            self.0.native.setDelegate(Some(
                objc2::runtime::ProtocolObject::from_ref(&*observer),
            ))
        };
        *self.0.observer.borrow_mut() = Some(observer);
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
    /// 子のハンドルを保持し、トランポリンごと生かしておく。
    children: RefCell<Vec<Box<dyn Widget>>>,
    /// 交差軸に `Fill` を指定された子を、スタックの幅 / 高さへ結び付ける制約。
    /// 余白が変わると定数も変わるので保持しておく。
    fill_constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
    padding: Cell<Padding>,
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
            native.setDistribution(NSStackViewDistribution::Fill);
            native.setAlignment(if orientation.is_vertical() {
                NSLayoutAttribute::CenterX
            } else {
                NSLayoutAttribute::CenterY
            });
        }
        Self(Rc::new(StackInner {
            native,
            children: RefCell::new(Vec::new()),
            fill_constraints: RefCell::new(Vec::new()),
            padding: Cell::new(Padding::ZERO),
        }))
    }

    pub fn set_spacing(&self, spacing: f64) {
        self.0.native.setSpacing(spacing);
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
    /// 子が交差軸に [`miui_core::Length::Fill`] を指定していれば、
    /// スタックの幅 (縦並びのとき) または高さに合わせて広げる。
    /// 主軸方向の `Fill` は hugging priority を通じて NSStackView が扱う。
    pub fn append(&self, child: &dyn Widget) {
        let view = child.native_view();
        crate::layout::prepare_child(&view);
        self.0.native.addArrangedSubview(&view);

        let vertical = self.is_vertical();
        // 縦並びの交差軸は横方向。
        let cross_is_horizontal = vertical;
        if crate::layout::wants_fill(&view, cross_is_horizontal) {
            let inset = self.cross_inset();
            let constraint = if vertical {
                view.widthAnchor()
                    .constraintEqualToAnchor_constant(&self.0.native.widthAnchor(), -inset)
            } else {
                view.heightAnchor()
                    .constraintEqualToAnchor_constant(&self.0.native.heightAnchor(), -inset)
            };
            constraint.setActive(true);
            self.0.fill_constraints.borrow_mut().push(constraint);
        }

        self.0.children.borrow_mut().push(child.boxed_clone());
    }

    /// 追加済みの子の数。
    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
