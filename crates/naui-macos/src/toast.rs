//! トースト (AppKit)。
//!
//! AppKit にトーストにあたるコントロールは無い。macOS の通知センターは
//! **アプリの外**へ出るもので、「いま見ている画面の上に短く出して自分で
//! 消える」という naui のトーストとは別物なので、`NSVisualEffectView` を
//! ウィンドウの中身へ重ねて組み立てる (`NSPopover` と同じ材質)。
//!
//! 出すのは**いちばん手前のウィンドウ**で、載せる先はその `contentView`。
//! 消えるまでの時間は `NSTimer` が数える。
//!
//! ## 他の環境との違い
//!
//! GTK4 では `AdwToast` がそのまま対応するが、macOS では上のとおり
//! naui が組み立てたビューになる。位置 (下端の中央) と角丸もここで決めている。

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::ToastSpec;
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSButton, NSColor, NSLayoutConstraint, NSShadow, NSStackView, NSTextField,
    NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
};
use objc2_foundation::{NSArray, NSEdgeInsets, NSPoint, NSRect, NSSize, NSString, NSTimer};

use crate::trampoline::ActionTarget;

/// 下端からの距離。
const BOTTOM_MARGIN: f64 = 24.0;
/// 左右の端に残す余白。ウィンドウが狭いときはトーストのほうが縮む。
const SIDE_MARGIN: f64 = 24.0;
/// 角の丸み。
const CORNER_RADIUS: f64 = 10.0;
/// 影のぼかし。重なっていることが分かるように、`NSPopover` と同じく影を落とす。
const SHADOW_BLUR: f64 = 12.0;
/// 影を落とす向き (下へ)。
const SHADOW_OFFSET: NSSize = NSSize {
    width: 0.0,
    height: -3.0,
};
/// 重ねたビューに付ける名前。ネイティブ側からトーストを見分けるために使う。
const IDENTIFIER: &str = "naui.toast";
/// 文字とボタンの間隔。
const SPACING: f64 = 12.0;
/// 中身の周りの余白。
const PADDING: NSEdgeInsets = NSEdgeInsets {
    top: 10.0,
    left: 16.0,
    bottom: 10.0,
    right: 16.0,
};

thread_local! {
    /// いま出ているトースト。同時に出るのは 1 つで、新しいものが置き換える。
    static CURRENT: RefCell<Option<Toast>> = const { RefCell::new(None) };
}

/// クロージャ 1 本の置き場。
///
/// 呼び出しの間だけ取り出すのは、通知の中からトーストを出し直しても
/// `RefCell` が二重借用にならないようにするため (`Dialog` と同じ作り)。
#[derive(Clone, Default)]
struct Callback(Rc<RefCell<Option<Box<dyn FnMut()>>>>);

impl Callback {
    fn set(&self, f: impl FnMut() + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f();
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// 出している間だけ持つもの。
struct Presentation {
    /// ウィンドウへ重ねてあるビュー。影を落とすので、角を丸めて中身を
    /// 切り取る `NSVisualEffectView` とは分けてある (同じビューで両方を
    /// やると、切り取りに影まで巻き込まれて消える)。
    container: Retained<NSView>,
    /// 文字とボタンを横に並べる行。
    row: Retained<NSStackView>,
    /// 出している文字。
    label: Retained<NSTextField>,
    /// 操作ボタン。置いていなければ持たない。
    button: Option<Retained<NSButton>>,
    /// 自動で消すためのタイマー。消えない指定なら持たない。
    timer: Option<Retained<NSTimer>>,
}

struct ToastInner {
    mtm: MainThreadMarker,
    spec: RefCell<ToastSpec>,
    shown: RefCell<Option<Presentation>>,
    /// 操作ボタンとタイマーの中継。**トーストと同じだけ生かしておく。**
    ///
    /// `NSControl` の target は弱参照なので、どこかで持つ必要がある。
    /// 出すたびに作り直さないのは、中継の中から消したとき
    /// (ボタンを押した・時間が来た) に、**実行中の中継そのものを
    /// 解放してしまわない**ようにするため。
    action_target: RefCell<Option<Retained<ActionTarget>>>,
    timer_target: RefCell<Option<Retained<ActionTarget>>>,
    on_action: Callback,
    on_dismiss: Callback,
}

/// 一時的な通知 (`NSVisualEffectView` を重ねたもの)。
///
/// ウィジェットではないので、コンテナへは入れない (`Dialog` と同じ)。
#[derive(Clone)]
pub struct Toast(Rc<ToastInner>);

impl Toast {
    pub(crate) fn new(mtm: MainThreadMarker, message: &str) -> Self {
        let this = Self(Rc::new(ToastInner {
            mtm,
            spec: RefCell::new(ToastSpec::new(message)),
            shown: RefCell::new(None),
            action_target: RefCell::new(None),
            timer_target: RefCell::new(None),
            on_action: Callback::default(),
            on_dismiss: Callback::default(),
        }));
        this.install_targets();
        this
    }

    /// 操作ボタンとタイマーの中継を作る。以後はこれを使い回す。
    fn install_targets(&self) {
        let mtm = self.0.mtm;
        let for_action = Rc::downgrade(&self.0);
        *self.0.action_target.borrow_mut() = Some(ActionTarget::new(mtm, move || {
            if let Some(inner) = for_action.upgrade() {
                Toast(inner).finish(true);
            }
        }));
        let for_timer = Rc::downgrade(&self.0);
        *self.0.timer_target.borrow_mut() = Some(ActionTarget::new(mtm, move || {
            if let Some(inner) = for_timer.upgrade() {
                Toast(inner).finish(false);
            }
        }));
    }

    /// 出す文字列。出している間に呼ぶと、その場で書き換わる
    /// (消えるまでの時間は数え直さない)。
    pub fn set_message(&self, message: &str) {
        self.0.spec.borrow_mut().set_message(message);
        if let Some(shown) = self.0.shown.borrow().as_ref() {
            shown.label.setStringValue(&NSString::from_str(message));
        }
    }

    pub fn message(&self) -> String {
        self.0.spec.borrow().message().to_string()
    }

    /// 操作ボタンの文字列。**空文字列を渡すとボタンを外す。**
    ///
    /// 出している間に呼ぶと、その場で付け外しされる。
    pub fn set_action(&self, label: &str) {
        self.0.spec.borrow_mut().set_action(label);
        let action = self.0.spec.borrow().action().map(str::to_string);
        if let Some(shown) = self.0.shown.borrow_mut().as_mut() {
            self.apply_action(shown, action.as_deref());
        }
    }

    /// 操作ボタンの文字列。置いていなければ空文字列。
    pub fn action(&self) -> String {
        self.0
            .spec
            .borrow()
            .action()
            .unwrap_or_default()
            .to_string()
    }

    /// 自動で消えるまでの秒数。**0 を渡すと自動では消えない。**
    ///
    /// 次に [`show`](Self::show) したときから効く。
    pub fn set_timeout(&self, seconds: f64) {
        self.0.spec.borrow_mut().set_timeout(seconds);
    }

    pub fn timeout(&self) -> f64 {
        self.0.spec.borrow().timeout()
    }

    /// いまの設定。
    pub fn spec(&self) -> ToastSpec {
        self.0.spec.borrow().clone()
    }

    /// 操作ボタンが押されたときに呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// 押されるとトーストは消えるので、続けて `on_dismiss` も呼ばれる。
    pub fn on_action(&self, f: impl FnMut() + 'static) {
        self.0.on_action.set(f);
    }

    /// 消えたときに呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// 呼ばれるのは**時間で消えたとき**と**操作ボタンで消えたとき**。
    /// [`dismiss`](Self::dismiss) で消したときと、別のトーストに
    /// 置き換えられたときは呼ばれない (アプリ自身の操作は通知しない、
    /// という [`Dialog::close`](crate::Dialog::close) と同じ決まり)。
    pub fn on_dismiss(&self, f: impl FnMut() + 'static) {
        self.0.on_dismiss.set(f);
    }

    /// トーストを出す。
    ///
    /// **同時に出るのは 1 つ**で、ほかのトーストが出ていれば置き換える
    /// (置き換えられたほうの `on_dismiss` は呼ばれない)。
    /// 出せるウィンドウがまだ無いときは何もしない。
    pub fn show(&self) {
        let Some(host) = host_view(self.0.mtm) else {
            return;
        };
        // 自分自身を出し直すときも、いったん片付けてから組み立て直す。
        if let Some(previous) = CURRENT.with(|slot| slot.borrow_mut().take()) {
            previous.take_down();
        }
        let presentation = self.build(&host);
        *self.0.shown.borrow_mut() = Some(presentation);
        CURRENT.with(|slot| *slot.borrow_mut() = Some(self.clone()));
    }

    /// 出しているトーストを消す。`on_dismiss` は呼ばれない。
    pub fn dismiss(&self) {
        if !self.is_visible() {
            return;
        }
        self.forget_current();
        self.take_down();
    }

    /// いま出ているか。
    pub fn is_visible(&self) -> bool {
        self.0.shown.borrow().is_some()
    }

    /// 重ねてあるビュー。出していなければ `None`。
    ///
    /// バックエンド固有の脱出口として公開している。
    pub fn native_view(&self) -> Option<Retained<NSView>> {
        self.0
            .shown
            .borrow()
            .as_ref()
            .map(|shown| shown.container.clone())
    }

    /// 操作ボタンを付け直す。`label` が `None` なら外す。
    fn apply_action(&self, shown: &mut Presentation, label: Option<&str>) {
        if let Some(button) = shown.button.take() {
            shown.row.removeArrangedSubview(&button);
            button.removeFromSuperview();
        }
        let Some(label) = label else {
            return;
        };
        let button = self.action_button(label);
        shown.row.addArrangedSubview(&button);
        shown.button = Some(button);
    }

    /// 押すと `on_action` を通して消えるボタン。
    fn action_button(&self, label: &str) -> Retained<NSButton> {
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(label),
                None,
                None,
                self.0.mtm,
            )
        };
        if let Some(target) = self.0.action_target.borrow().as_ref() {
            unsafe {
                button.setTarget(Some(target));
                button.setAction(Some(sel!(invoke:)));
            }
        }
        button
    }

    /// ビューを組み立てて `host` へ重ね、時間を数え始める。
    fn build(&self, host: &NSView) -> Presentation {
        let mtm = self.0.mtm;
        let spec = self.0.spec.borrow().clone();
        // 影を落とす外側。角丸で中身を切り取るビューと分けるのは、
        // 切り取りが影まで消してしまうため。
        let container = NSView::new(mtm);
        crate::layout::prepare_child(&container);
        container.setIdentifier(Some(&NSString::from_str(IDENTIFIER)));
        container.setWantsLayer(true);
        let shadow = NSShadow::new();
        shadow.setShadowBlurRadius(SHADOW_BLUR);
        shadow.setShadowOffset(SHADOW_OFFSET);
        let shadow_color = NSColor::shadowColor();
        shadow.setShadowColor(Some(&shadow_color));
        container.setShadow(Some(&shadow));

        // 背景。`NSPopover` と同じ材質にして、OS のテーマへ追従させる。
        let surface = NSVisualEffectView::initWithFrame(
            NSVisualEffectView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
        );
        surface.setMaterial(NSVisualEffectMaterial::Popover);
        // ウィンドウの中身の上に重ねるので、背後ではなく手前を混ぜる。
        surface.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
        // ウィンドウが背面にあっても薄くならないようにする。
        surface.setState(NSVisualEffectState::Active);
        surface.setWantsLayer(true);
        if let Some(layer) = surface.layer() {
            layer.setCornerRadius(CORNER_RADIUS);
            layer.setMasksToBounds(true);
        }
        crate::layout::prepare_child(&surface);
        container.addSubview(&surface);

        let row = NSStackView::new(mtm);
        // 制約で container につなぐので、frame から作られる制約は外す。
        // 付けたままだと 0 × 0 の frame がそのまま大きさになり、
        // トーストが見えないまま重なる。
        crate::layout::prepare_child(row.as_ref());
        row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        row.setSpacing(SPACING);
        row.setEdgeInsets(PADDING);
        surface.addSubview(&row);

        let label = NSTextField::labelWithString(&NSString::from_str(spec.message()), mtm);
        row.addArrangedSubview(&label);

        let button = spec.action().map(|action| {
            let button = self.action_button(action);
            row.addArrangedSubview(&button);
            button
        });

        host.addSubview(&container);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            surface
                .leadingAnchor()
                .constraintEqualToAnchor(&container.leadingAnchor()),
            surface
                .trailingAnchor()
                .constraintEqualToAnchor(&container.trailingAnchor()),
            surface
                .topAnchor()
                .constraintEqualToAnchor(&container.topAnchor()),
            surface
                .bottomAnchor()
                .constraintEqualToAnchor(&container.bottomAnchor()),
            row.leadingAnchor()
                .constraintEqualToAnchor(&surface.leadingAnchor()),
            row.trailingAnchor()
                .constraintEqualToAnchor(&surface.trailingAnchor()),
            row.topAnchor()
                .constraintEqualToAnchor(&surface.topAnchor()),
            row.bottomAnchor()
                .constraintEqualToAnchor(&surface.bottomAnchor()),
            container
                .centerXAnchor()
                .constraintEqualToAnchor(&host.centerXAnchor()),
            container
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&host.bottomAnchor(), -BOTTOM_MARGIN),
            // 文字が長くてもウィンドウからはみ出さない。
            container
                .widthAnchor()
                .constraintLessThanOrEqualToAnchor_constant(
                    &host.widthAnchor(),
                    -SIDE_MARGIN * 2.0,
                ),
        ]));

        let timer = self.0.timer_target.borrow().as_ref().and_then(|target| {
            spec.timeout_millis().map(|_| unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    spec.timeout(),
                    target,
                    sel!(invoke:),
                    None,
                    false,
                )
            })
        });

        Presentation {
            container,
            row,
            label,
            button,
            timer,
        }
    }

    /// 消えたことをアプリへ知らせる。`action` は操作ボタンで消えたか。
    fn finish(&self, action: bool) {
        if !self.is_visible() {
            return;
        }
        self.forget_current();
        self.take_down();
        if action {
            self.0.on_action.emit();
        }
        self.0.on_dismiss.emit();
    }

    /// ビューを外し、タイマーを止める。通知はしない。
    fn take_down(&self) {
        let Some(shown) = self.0.shown.borrow_mut().take() else {
            return;
        };
        if let Some(timer) = shown.timer {
            timer.invalidate();
        }
        shown.container.removeFromSuperview();
    }

    /// 「いま出ているトースト」が自分なら、その座を空ける。
    fn forget_current(&self) {
        CURRENT.with(|slot| {
            let mine = slot
                .borrow()
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(&current.0, &self.0));
            if mine {
                slot.borrow_mut().take();
            }
        });
    }
}

/// トーストを載せるビュー。まだウィンドウが無ければ `None`。
///
/// 出し先は naui が作ったウィンドウのうちいちばん手前のもの
/// ([`crate::window::frontmost`])。AppKit が内部で作るウィンドウを
/// 掴まないよう、`NSApplication` の `windows` は直接見ない。
fn host_view(mtm: MainThreadMarker) -> Option<Retained<NSView>> {
    crate::window::frontmost(mtm)?.contentView()
}
