//! 折りたたみ (見出しの `NSButton` + 中身の `NSStackView`)。
//!
//! AppKit に「見出しを押して中身を出し入れする」1 つのコントロールは無い。
//! 開閉の三角 (disclosure) を持つ `NSButton` と中身を縦に並べるのが AppKit の
//! 標準の組み方 (システム設定の「詳細」と同じ形) なので、`NSStackView` へ
//! 見出しと中身を並べる。
//!
//! たたむときは中身のビューを隠す。`NSStackView` は隠れた子を
//! レイアウトから外す (`detachesHiddenViews` の既定) ので、たたむと
//! 見出しの高さまで縮む。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{
    NSButton, NSButtonType, NSCellImagePosition, NSControlStateValueOff, NSControlStateValueOn,
    NSImage, NSLayoutAttribute, NSLayoutConstraint, NSStackView, NSStackViewDistribution,
    NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSArray, NSString};

use crate::trampoline::{ActionTarget, ValueHandler};
use crate::widgets::{impl_widget, Widget};

/// 見出しと中身のすき間。
const SPACING: f64 = 6.0;

/// 開いているときと閉じているときの三角 (SF Symbols)。
const SYMBOL_EXPANDED: &str = "chevron.down";
const SYMBOL_COLLAPSED: &str = "chevron.right";

struct ExpanderInner {
    native: Retained<NSStackView>,
    header: Retained<NSButton>,
    /// 中身のハンドル。トランポリンごと生かしておくために持つ。
    child: RefCell<Option<Box<dyn Widget>>>,
    /// 中身を折りたたみの幅へ結び付ける制約。置き換えるときに張り直す。
    fill_constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
    expanded: Cell<bool>,
    handler: ValueHandler<bool>,
    /// AppKit の target は弱参照なので、ここで生かしておく。
    target: RefCell<Option<Retained<ActionTarget>>>,
}

/// 見出しを押して中身を出し入れするコンテナ。
#[derive(Clone)]
pub struct Expander(Rc<ExpanderInner>);
impl_widget!(Expander);

impl Expander {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = NSStackView::new(mtm);
        native.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        // 中身の hugging priority に従って余りを配る (`Stack` と同じ理由)。
        native.setDistribution(NSStackViewDistribution::GravityAreas);
        native.setAlignment(NSLayoutAttribute::Leading);
        native.setSpacing(SPACING);

        let header = unsafe {
            NSButton::buttonWithTitle_target_action(&NSString::from_str(text), None, None, mtm)
        };
        // 押すたびに入り切りが変わるボタン。枠は出さず、三角と文字だけを出す。
        header.setButtonType(NSButtonType::PushOnPushOff);
        header.setBordered(false);
        header.setImagePosition(NSCellImagePosition::ImageLeading);

        let this = Self(Rc::new(ExpanderInner {
            native,
            header,
            child: RefCell::new(None),
            fill_constraints: RefCell::new(Vec::new()),
            expanded: Cell::new(false),
            handler: ValueHandler::default(),
            target: RefCell::new(None),
        }));
        this.0.native.addArrangedSubview(&this.0.header);
        this.0.write_native(false);

        // 中継はハンドルと同じ寿命で持つ。作り直すと、通知の中から
        // 開閉したときに実行中の中継そのものを解放してしまう。
        let target = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(&this.0);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let expanded = inner.header.state() != NSControlStateValueOff;
                inner.expanded.set(expanded);
                inner.write_native(expanded);
                inner.handler.emit(expanded);
            }
        });
        unsafe {
            this.0.header.setTarget(Some(&target));
            this.0.header.setAction(Some(sel!(invoke:)));
        }
        *this.0.target.borrow_mut() = Some(target);
        this
    }

    /// 見出しの文字。
    pub fn text(&self) -> String {
        self.0.header.title().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.0.header.setTitle(&NSString::from_str(text));
    }

    /// 開いているかどうか。
    pub fn is_expanded(&self) -> bool {
        self.0.expanded.get()
    }

    /// プログラムから開閉する。`on_toggle` は呼ばれない。
    pub fn set_expanded(&self, expanded: bool) {
        self.0.expanded.set(expanded);
        self.0.write_native(expanded);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.header.setEnabled(enabled);
    }

    /// 折りたたむ中身。呼ぶたびに置き換わる。
    pub fn set_child(&self, child: &dyn Widget) {
        if let Some(previous) = self.0.child.borrow_mut().take() {
            let view = previous.native_view();
            self.0.native.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        let mut constraints = self.0.fill_constraints.borrow_mut();
        if !constraints.is_empty() {
            NSLayoutConstraint::deactivateConstraints(&NSArray::from_retained_slice(&constraints));
            constraints.clear();
        }

        let view = child.native_view();
        crate::layout::prepare_child(&view);
        if !crate::layout::wants_fill(&view, false) {
            crate::layout::keep_auto_size(&view, false);
        }
        self.0.native.addArrangedSubview(&view);
        // 中身は開いた場所の幅いっぱいに置く (`Scroll::set_child` と同じ扱い)。
        // 中身の中で左右のどこへ寄せるかは、中身のコンテナが決める。
        let at_most = view
            .widthAnchor()
            .constraintLessThanOrEqualToAnchor(&self.0.native.widthAnchor());
        let equal = view
            .widthAnchor()
            .constraintEqualToAnchor(&self.0.native.widthAnchor());
        equal.setPriority(crate::layout::CROSS_FILL_PRIORITY);
        for constraint in [&at_most, &equal] {
            constraint.setActive(true);
        }
        constraints.push(at_most);
        constraints.push(equal);
        drop(constraints);

        view.setHidden(!self.is_expanded());
        *self.0.child.borrow_mut() = Some(child.boxed_clone());
    }

    /// 利用者が開閉するたびに、変わった後の状態で呼ばれる。
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.handler.set(f);
    }

    /// 見出しのクリックを発生させる (テストや自動操作用)。
    pub fn click(&self) {
        unsafe { self.0.header.performClick(None) };
    }

    /// 見出しのボタン。バックエンド固有の脱出口として公開している。
    pub fn native_header(&self) -> Retained<NSButton> {
        self.0.header.clone()
    }
}

impl ExpanderInner {
    /// 開閉の状態をネイティブへ書く。
    fn write_native(&self, expanded: bool) {
        self.header.setState(if expanded {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        let symbol = NSString::from_str(if expanded {
            SYMBOL_EXPANDED
        } else {
            SYMBOL_COLLAPSED
        });
        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&symbol, None);
        self.header.setImage(image.as_deref());
        if let Some(child) = self.child.borrow().as_ref() {
            child.native_view().setHidden(!expanded);
        }
    }
}
