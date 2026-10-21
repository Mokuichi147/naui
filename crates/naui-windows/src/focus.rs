//! `on_focus_change` (文字入力のウィジェット)。
//!
//! ウィンドウの根に `GotFocus` を 1 つ付ける。`GotFocus` はバブルする
//! ルーティングイベントなので、どの要素がフォーカスを得ても根まで届く。
//! 出どころ (`OriginalSource`) から親へたどって、最初に見つかった登録済みの
//! ウィジェットをそのウィンドウのフォーカスの持ち主とする ([`FocusOwner`])。
//! 持ち主が変わったときだけ通知するので、`NumberBox` の中の入力欄へ移る
//! ときのように、ウィジェットの中で移っても通知は出ない。
//!
//! WinUI はウィンドウがアクティブでなくなってもフォーカスのある要素を
//! 変えないので、そのときは通知しない (ほかの環境と同じ)。
//!
//! 通知先はウィジェットが持ち ([`Focus`])、登録表は要素と通知先を弱参照で
//! 持つ。ウィジェットが解放されれば、表の項目もそのときに外れる。表が
//! 通知先を持つと、ウィジェットを捨てても通知先とそれが捕まえている状態が
//! 残ってしまう。終了の経路では [`release_handlers`] で見張りが覚えている
//! 持ち主を手放す ([`Registry`] を参照)。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak as RcWeak};
use std::sync::Arc;

use naui_core::{FocusHandler, FocusOwner};
use naui_winui3::Microsoft::UI::Xaml::Media::VisualTreeHelper;
use naui_winui3::Microsoft::UI::Xaml::{DependencyObject, RoutedEventHandler, UIElement};
use windows_core::{Interface, Weak};

use crate::keys::same;
use crate::ui_thread::{Registry, UiThreadCell};

/// ウィンドウの根 1 つぶんの見張り。
#[derive(Default)]
struct Tracker {
    owner: FocusOwner,
    /// 最後にフォーカスを得た要素。通知先を置いたときに、すでにその中に
    /// フォーカスがあるかを調べるのに使う。
    focused: RefCell<Option<Weak<DependencyObject>>>,
}

type Entries = Registry<(Weak<UIElement>, RcWeak<FocusHandler>)>;

thread_local! {
    static WIDGETS: Entries = const { Registry::new() };
    /// 生きている見張り。見張りそのものは根の購読が持っている。
    static TRACKERS: RefCell<Vec<RcWeak<Tracker>>> = const { RefCell::new(Vec::new()) };
}

/// ウィジェットに持たせる通知先。
#[derive(Default)]
pub(crate) struct Focus {
    handler: Rc<FocusHandler>,
    registered: Cell<bool>,
}

impl Focus {
    /// 通知先を置く。呼ぶたびに置き換わる。
    pub(crate) fn set(&self, element: &UIElement, f: impl FnMut(bool) + 'static) {
        self.handler.set(f);
        if self.registered.get() {
            return;
        }
        let Ok(weak) = element.downgrade() else {
            return;
        };
        self.registered.set(true);
        WIDGETS.with(|entries| {
            let mut entries = entries.borrow_mut();
            entries
                .retain(|(weak, handler)| handler.strong_count() > 0 && weak.upgrade().is_some());
            entries.push((weak, Rc::downgrade(&self.handler)));
        });
        adopt_current_focus(&self.handler);
    }
}

impl Drop for Focus {
    fn drop(&mut self) {
        if !self.registered.get() {
            return;
        }
        let me = Rc::as_ptr(&self.handler);
        // スレッドの後始末で表が先に消えていれば、外すものも無い。
        let _ = WIDGETS.try_with(|entries| {
            if let Ok(mut entries) = entries.try_borrow_mut() {
                entries.retain(|(_, handler)| handler.as_ptr() != me);
            }
        });
    }
}

/// フォーカスがすでに新しく登録したウィジェットの中にあれば、通知せずに
/// 持ち主として覚える (ウィンドウを出したときに最初の入力欄へ移るなど)。
/// そうしないと、あとで出たときの `false` が届かない。
fn adopt_current_focus(handler: &Rc<FocusHandler>) {
    let trackers: Vec<Rc<Tracker>> = TRACKERS.with(|trackers| {
        let mut trackers = trackers.borrow_mut();
        trackers.retain(|weak| weak.strong_count() > 0);
        trackers.iter().filter_map(RcWeak::upgrade).collect()
    });
    for tracker in trackers {
        let focused = tracker.focused.borrow().as_ref().and_then(Weak::upgrade);
        if owner_of(focused).is_some_and(|owner| Rc::ptr_eq(&owner, handler)) {
            tracker.owner.reset(Some(handler.clone()));
        }
    }
}

/// 見張りが覚えている持ち主 (通知先を掴んでいる) と、登録表の弱参照を
/// 手放す。終了の経路で、XAML の後片づけより前に呼ぶ。
pub(crate) fn release_handlers() {
    let trackers: Vec<Rc<Tracker>> = TRACKERS.with(|trackers| {
        trackers
            .borrow()
            .iter()
            .filter_map(RcWeak::upgrade)
            .collect()
    });
    for tracker in trackers {
        tracker.owner.reset(None);
    }
    // 借用の外で捨てる。
    drop(WIDGETS.with(Registry::take));
}

fn lookup(object: &DependencyObject) -> Option<Rc<FocusHandler>> {
    WIDGETS.with(|entries| {
        entries
            .borrow()
            .iter()
            .find(|(weak, _)| weak.upgrade().is_some_and(|other| same(&other, object)))
            .and_then(|(_, handler)| handler.upgrade())
    })
}

/// `object` を含む登録済みのウィジェット。
fn owner_of(object: Option<DependencyObject>) -> Option<Rc<FocusHandler>> {
    let mut current = object;
    while let Some(object) = current {
        if let Some(handler) = lookup(&object) {
            return Some(handler);
        }
        current = VisualTreeHelper::GetParent(&object).ok();
    }
    None
}

/// ウィンドウの根にフォーカスの見張りを付ける。根を作り直すたびに呼ぶ。
pub(crate) fn install(root: &UIElement) -> windows_core::Result<()> {
    let tracker = Rc::new(Tracker::default());
    TRACKERS.with(|trackers| trackers.borrow_mut().push(Rc::downgrade(&tracker)));
    let tracker = Arc::new(UiThreadCell::new(tracker));
    let handler = RoutedEventHandler::new(move |_, args| {
        let Some(tracker) = tracker.try_with_mut(|tracker| tracker.clone()) else {
            return Ok(());
        };
        let source = args
            .as_ref()
            .and_then(|args| args.OriginalSource().ok())
            .and_then(|source| source.cast::<DependencyObject>().ok());
        *tracker.focused.borrow_mut() = source.as_ref().and_then(|s| s.downgrade().ok());
        // 通知の中で要素を操作してもよいよう、借用の外で知らせる。
        tracker.owner.move_to(owner_of(source));
        Ok(())
    });
    root.GotFocus(&handler)?;
    Ok(())
}

/// 文字入力のウィジェットへ `on_focus_change` を足す。`Inner` に
/// `focus: Focus` を持たせておくこと。
macro_rules! impl_focus_change {
    ($t:ty) => {
        impl $t {
            /// フォーカスが入ったとき (`true`) と出たとき (`false`) の通知。
            ///
            /// ウィンドウの中でフォーカスが移ったときだけ呼ばれる。ウィンドウ
            /// 自体がアクティブでなくなっても呼ばれない。
            pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
                let element = <$t as crate::widgets::Widget>::native_element(self);
                self.0.focus.set(&element, f);
            }
        }
    };
}

pub(crate) use impl_focus_change;
