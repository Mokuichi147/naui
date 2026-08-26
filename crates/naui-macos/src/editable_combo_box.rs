//! 自由に入力できるコンボボックス (AppKit の `NSComboBox`)。
//!
//! [`ComboBox`](crate::ComboBox) は `NSPopUpButton` なので候補からしか選べない。
//! こちらは文字を打ち込める `NSComboBox` を使い、値を**文字列**で扱う。
//! 候補はあくまで入力の補助で、一致しない文字列もそのまま値になる。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSComboBox, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::trampoline::{ComboObserver, TextHandler};
use crate::widgets::{impl_widget, Widget};

/// 一覧を開いたときに一度に見える候補の数。AppKit の既定 (5) より少し多くする。
const VISIBLE_ITEMS: isize = 8;

struct EditableComboBoxInner {
    native: Retained<NSComboBox>,
    /// 候補の控え。`selected` の一致判定と `set_selected` の書き込みに使う。
    items: RefCell<Vec<String>>,
    on_change: TextHandler,
    /// 最後に通知した文字列。AppKit は「候補が選ばれた」と「欄が書き換わった」を
    /// 別々に伝えてくるので、同じ値の二重通知をここで落とす。
    last: RefCell<String>,
    /// プログラムからの書き換えの間だけ通知を止める。
    silent: Cell<bool>,
    /// AppKit の delegate は weak なので、ハンドル側で生かしておく。
    observer: RefCell<Option<Retained<ComboObserver>>>,
}

/// 候補から選ぶことも、自由に打ち込むこともできる入力欄 (`NSComboBox`)。
///
/// 値は文字列で、作った直後は空。
#[derive(Clone)]
pub struct EditableComboBox(Rc<EditableComboBoxInner>);
impl_widget!(EditableComboBox);

impl EditableComboBox {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        let native = NSComboBox::initWithFrame(NSComboBox::alloc(mtm), frame);
        native.setNumberOfVisibleItems(VISIBLE_ITEMS);
        // 打ちかけの文字を候補で補完する。打ち切れば候補外の文字列も残せる。
        native.setCompletes(true);

        let inner = Rc::new(EditableComboBoxInner {
            native,
            items: RefCell::new(Vec::new()),
            on_change: TextHandler::default(),
            last: RefCell::new(String::new()),
            silent: Cell::new(false),
            observer: RefCell::new(None),
        });

        let observer = ComboObserver::new(mtm, {
            let weak = Rc::downgrade(&inner);
            move |from_list| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                EditableComboBox(inner).handle(from_list);
            }
        });
        unsafe {
            inner
                .native
                .setDelegate(Some(ProtocolObject::from_ref(&*observer)))
        };
        *inner.observer.borrow_mut() = Some(observer);

        Self(inner)
    }

    /// 候補を作り直す。**入力されている文字列は変わらず**、通知も出ない。
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        *self.0.items.borrow_mut() = items.iter().map(|s| s.as_ref().to_string()).collect();
        self.without_notifying(|this| {
            this.0.native.removeAllItems();
            for item in items {
                unsafe {
                    this.0
                        .native
                        .addItemWithObjectValue(&NSString::from_str(item.as_ref()))
                };
            }
            // 候補が入れ替わると、今の文字列と一致する候補も変わる。
            this.sync_selection();
        });
    }

    /// 候補の数。
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 入力されている文字列。
    pub fn text(&self) -> String {
        self.0.native.stringValue().to_string()
    }

    /// プログラムから文字列を差し替える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        self.without_notifying(|this| {
            this.write_text(text);
            this.sync_selection();
        });
        *self.0.last.borrow_mut() = text.to_string();
    }

    /// 入力されている文字列と**そのまま一致する**候補の位置。
    ///
    /// 打ち込まれた文字列がどの候補とも一致しなければ `None`。
    pub fn selected(&self) -> Option<usize> {
        let text = self.text();
        self.0.items.borrow().iter().position(|item| *item == text)
    }

    /// 範囲内の候補を通知せずに選ぶ。範囲外なら何もしない。
    pub fn set_selected(&self, index: usize) {
        let Some(text) = self.0.items.borrow().get(index).cloned() else {
            return;
        };
        self.set_text(&text);
    }

    /// 通知せずに文字列を空にする。
    pub fn clear(&self) {
        self.set_text("");
    }

    /// 利用者が候補を選んだのと同じように、範囲内の候補を選んで通知する。
    pub fn select(&self, index: usize) {
        let Some(text) = self.0.items.borrow().get(index).cloned() else {
            return;
        };
        self.set_text(&text);
        self.0.on_change.emit(&text);
    }

    pub fn set_placeholder(&self, text: &str) {
        self.0
            .native
            .setPlaceholderString(Some(&NSString::from_str(text)));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// 文字列が変わるたびに、その時点の中身で呼ばれる。
    /// 打鍵と候補の選択のどちらでも呼ばれる。設定し直すと以前のものは外れる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// AppKit の実コントロール。バックエンド固有の脱出口として公開している。
    pub fn native_combo_box(&self) -> Retained<NSComboBox> {
        self.0.native.clone()
    }

    /// AppKit から届いた 2 種類の通知を 1 本の `on_change` にまとめる。
    fn handle(&self, from_list: bool) {
        if self.0.silent.get() {
            return;
        }
        if from_list {
            // 候補を押した直後、テキスト欄はまだ書き換わっていない。
            // 先に控えから書き入れて、通知の最中でも `text()` が読めるようにする。
            let index = self.0.native.indexOfSelectedItem();
            let picked = usize::try_from(index)
                .ok()
                .and_then(|index| self.0.items.borrow().get(index).cloned());
            if let Some(text) = picked {
                self.without_notifying(|this| this.write_text(&text));
            }
        }
        let text = self.text();
        if *self.0.last.borrow() == text {
            return;
        }
        *self.0.last.borrow_mut() = text.clone();
        self.0.on_change.emit(&text);
    }

    /// 欄の文字列だけを書く (`setStringValue:` は通知を出さない)。
    fn write_text(&self, text: &str) {
        self.0.native.setStringValue(&NSString::from_str(text));
    }

    /// 今の文字列に合わせて、一覧側の選択も合わせる。
    fn sync_selection(&self) {
        let previous = self.0.native.indexOfSelectedItem();
        match self.selected() {
            Some(index) => self.0.native.selectItemAtIndex(index as isize),
            // 候補と一致しなくなったら、一覧の選択も外す。
            None if previous >= 0 => self.0.native.deselectItemAtIndex(previous),
            None => {}
        }
    }

    /// AppKit から届く通知を止めたまま操作する。
    fn without_notifying<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        let previous = self.0.silent.replace(true);
        let result = f(self);
        self.0.silent.set(previous);
        result
    }
}
