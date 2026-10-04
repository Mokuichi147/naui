//! どのウィジェットにもある操作 (表示・非表示 / フォーカス / ツールチップ)。
//!
//! 対象は親へ置いているビュー ([`Widget::native_view`](crate::Widget::native_view))。
//! 中身を別のビューで包んでいるウィジェット (`TextArea` の `NSScrollView` など)
//! でも、フォーカスは中の入力欄へ届くように子孫をたどる。

use std::cell::RefCell;
use std::thread::LocalKey;

use objc2::rc::{Retained, Weak};
use objc2::Message;
use objc2_app_kit::NSView;
use objc2_foundation::NSString;

type HiddenViews = RefCell<Vec<Weak<NSView>>>;

thread_local! {
    /// アプリが `set_visible(false)` で隠したビュー。
    static APP_HIDDEN: HiddenViews = const { RefCell::new(Vec::new()) };
    /// コンテナが隠しているビュー (たたんだ `Expander` の中身)。
    ///
    /// どちらも `hidden` を使うので、片方だけを覚えていると、もう片方が
    /// 表示へ戻したときにもう片方の指定まで消えてしまう。両方を覚えておき、
    /// どちらかが隠していれば隠す。
    static CONTAINER_HIDDEN: HiddenViews = const { RefCell::new(Vec::new()) };
}

fn contains(list: &'static LocalKey<HiddenViews>, view: &NSView) -> bool {
    list.with(|hidden| {
        hidden
            .borrow()
            .iter()
            .filter_map(Weak::load)
            .any(|other| std::ptr::eq(&*other, view))
    })
}

fn mark(list: &'static LocalKey<HiddenViews>, view: &NSView, hidden: bool) {
    list.with(|list| {
        let mut list = list.borrow_mut();
        // 消えたビューの分もここで片づける。
        list.retain(|weak| {
            weak.load()
                .is_some_and(|other| !std::ptr::eq(&*other, view))
        });
        if hidden {
            list.push(Weak::from(view));
        }
    });
}

/// 2 つの指定から `hidden` を決め直す。
fn sync(view: &NSView) {
    let hidden = contains(&APP_HIDDEN, view) || contains(&CONTAINER_HIDDEN, view);
    if view.isHidden() == hidden {
        return;
    }
    view.setHidden(hidden);
    // 大きさが変わるので、親の連なりへ伝える (Grid の Auto 行など)。
    crate::layout::invalidate_ancestors(view);
}

/// 表示するかどうか。隠すと `NSStackView` の中ではレイアウトから外れる。
pub(crate) fn set_visible(view: &NSView, visible: bool) {
    mark(&APP_HIDDEN, view, !visible);
    sync(view);
}

pub(crate) fn is_visible(view: &NSView) -> bool {
    !contains(&APP_HIDDEN, view)
}

/// コンテナの都合で隠す / 戻す。アプリの [`set_visible`] とは別に覚える。
pub(crate) fn set_hidden_by_container(view: &NSView, hidden: bool) {
    mark(&CONTAINER_HIDDEN, view, hidden);
    sync(view);
}

/// キーボードフォーカスを移す。移せたら `true`。
///
/// 自分が受け取れなければ、子孫のうち最初に受け取れるものへ移す。
/// ウィンドウに載っていない・隠れている・無効なときは移せない。
pub(crate) fn request_focus(view: &NSView) -> bool {
    let Some(window) = view.window() else {
        return false;
    };
    if view.isHiddenOrHasHiddenAncestor() {
        return false;
    }
    let Some(target) = focus_target(view) else {
        return false;
    };
    window.makeFirstResponder(Some(&target))
}

fn focus_target(view: &NSView) -> Option<Retained<NSView>> {
    if view.isHidden() {
        return None;
    }
    if view.acceptsFirstResponder() {
        return Some(view.retain());
    }
    view.subviews()
        .iter()
        .find_map(|child| focus_target(&child))
}

/// ポインターを重ねたときに出す説明。`None` で外す。
pub(crate) fn set_tooltip(view: &NSView, text: Option<&str>) {
    let text = text.map(NSString::from_str);
    view.setToolTip(text.as_deref());
}
