//! フォーカスの出入りの通知 (`on_focus_change`)。
//!
//! 各バックエンドは、ウィンドウの中でフォーカスのある要素が変わるたびに、
//! そこから親へたどって最初に見つかった登録済みのウィジェットを
//! [`FocusOwner::move_to`] へ渡す。持ち主が変わったときだけ、前の持ち主へ
//! `false`、新しい持ち主へ `true` が届く。
//!
//! 持ち主で数えるので、ウィジェットの中で部品から部品へ移っても (数値入力の
//! 欄と増減のボタンなど) 通知は出ない。ウィンドウ自体がアクティブで
//! なくなっても、そのウィンドウの中のフォーカスは変わらないので通知しない。

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use crate::Slot;

/// フォーカスの出入りの通知先。`true` で入り、`false` で出る。
///
/// 呼んでいる間はクロージャを取り出しておくので、通知の中から同じ
/// ウィジェットを操作したり `on_focus_change` で差し替えたりしてもよい。
#[derive(Default)]
pub struct FocusHandler(RefCell<Slot<dyn FnMut(bool)>>);

impl FocusHandler {
    pub fn set(&self, f: impl FnMut(bool) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub fn emit(&self, focused: bool) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(focused);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// ウィンドウ 1 つぶんの、いまフォーカスを持っている登録済みのウィジェット。
///
/// 通知の中でフォーカスを動かしても (入ったとたんに別の欄へ移すなど)、
/// 通知は順番待ちにして 1 つずつ送るので、取りこぼしも順序の入れ替わりも
/// 起きない。
#[derive(Default)]
pub struct FocusOwner {
    current: RefCell<Option<Rc<FocusHandler>>>,
    pending: RefCell<VecDeque<(Rc<FocusHandler>, bool)>>,
    emitting: Cell<bool>,
}

impl FocusOwner {
    /// フォーカスが `next` の中へ移った (`None` はどの登録済みウィジェットの
    /// 中でもない) ことを伝える。持ち主が変わったときだけ通知する。
    ///
    /// 前の持ち主への `false` を先に、新しい持ち主への `true` を後に送る。
    pub fn move_to(&self, next: Option<Rc<FocusHandler>>) {
        {
            let mut current = self.current.borrow_mut();
            let same = match (current.as_ref(), next.as_ref()) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if same {
                return;
            }
            let previous = std::mem::replace(&mut *current, next.clone());
            let mut pending = self.pending.borrow_mut();
            pending.extend(previous.map(|previous| (previous, false)));
            pending.extend(next.map(|next| (next, true)));
        }
        if self.emitting.replace(true) {
            // 通知の中から呼ばれた。外側の送り出しが続きを送る。
            return;
        }
        loop {
            let Some((handler, focused)) = self.pending.borrow_mut().pop_front() else {
                break;
            };
            handler.emit(focused);
        }
        self.emitting.set(false);
    }

    /// 通知せずに持ち主を置き直す。
    ///
    /// 通知先を置いた時点で、フォーカスがすでにそのウィジェットの中にある
    /// ときに使う。入ったことは知らせないが、そこから出たときには `false` が
    /// 届くようになる。
    pub fn reset(&self, current: Option<Rc<FocusHandler>>) {
        *self.current.borrow_mut() = current;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorder(name: &'static str, log: &Rc<RefCell<Vec<String>>>) -> Rc<FocusHandler> {
        let handler = Rc::new(FocusHandler::default());
        let log = log.clone();
        handler.set(move |focused| log.borrow_mut().push(format!("{name}:{focused}")));
        handler
    }

    #[test]
    fn only_a_change_of_owner_is_reported() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let a = recorder("a", &log);
        let b = recorder("b", &log);
        let owner = FocusOwner::default();

        owner.move_to(Some(a.clone()));
        owner.move_to(Some(a.clone()));
        owner.move_to(Some(b.clone()));
        owner.move_to(None);
        owner.move_to(None);
        assert_eq!(*log.borrow(), ["a:true", "a:false", "b:true", "b:false"]);
    }

    #[test]
    fn a_reset_owner_is_told_when_the_focus_leaves() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let a = recorder("a", &log);
        let owner = FocusOwner::default();
        owner.reset(Some(a.clone()));
        assert!(log.borrow().is_empty(), "置き直しは通知しない");
        owner.move_to(Some(a));
        owner.move_to(None);
        assert_eq!(*log.borrow(), ["a:false"]);
    }

    #[test]
    fn the_handler_may_move_the_focus_again() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let owner = Rc::new(FocusOwner::default());
        let a = recorder("a", &log);
        let b = Rc::new(FocusHandler::default());
        b.set({
            let log = log.clone();
            let owner = owner.clone();
            let a = a.clone();
            move |focused| {
                log.borrow_mut().push(format!("b:{focused}"));
                // 入ったとたんに別の欄へ移す (検証で戻すときなど)。
                if focused {
                    owner.move_to(Some(a.clone()));
                }
            }
        });
        owner.move_to(Some(b));
        assert_eq!(*log.borrow(), ["b:true", "b:false", "a:true"]);
    }
}
