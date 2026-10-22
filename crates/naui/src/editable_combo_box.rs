use crate::sys;
use naui_core::{EventResponse, KeyEvent};

/// 候補から選ぶことも、自由に打ち込むこともできる入力欄。
///
/// 値は文字列で、作った直後は空。
#[derive(Clone)]
pub struct EditableComboBox(pub(crate) sys::EditableComboBox);

impl_widget!(EditableComboBox);

impl EditableComboBox {
    /// キーが押されたときの通知。`Handled` を返すと、そのキーは
    /// 入力欄にもウィンドウにも渡らない (改行や確定を止められる)。
    ///
    /// IME で変換している間のキーは届かない。
    #[inline]
    pub fn on_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.0.on_key_down(f)
    }

    /// フォーカスが入ったとき (`true`) と出たとき (`false`) の通知。
    ///
    /// ウィンドウの中でフォーカスが移ったときだけ呼ばれる。ウィンドウ
    /// 自体がアクティブでなくなっても呼ばれない。
    #[inline]
    pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_focus_change(f)
    }

    /// 候補を作り直す。**入力されている文字列は変わらず**、通知も出ない。
    #[inline]
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        self.0.set_items(items)
    }

    /// 候補の数。
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 入力されている文字列。
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// プログラムから文字列を差し替える。`on_change` は呼ばれない。
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    /// 入力されている文字列と**そのまま一致する**候補の位置。
    ///
    /// 打ち込まれた文字列がどの候補とも一致しなければ `None`。
    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// 範囲内の候補を通知せずに選ぶ。範囲外なら何もしない。
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// 通知せずに文字列を空にする。
    #[inline]
    pub fn clear(&self) {
        self.0.clear()
    }

    /// 利用者が候補を選んだのと同じように、範囲内の候補を選んで通知する。
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    #[inline]
    pub fn set_placeholder(&self, text: &str) {
        self.0.set_placeholder(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 文字列が変わるたびに、その時点の中身で呼ばれる。
    /// 打鍵と候補の選択のどちらでも呼ばれる。設定し直すと以前のものは外れる。
    #[inline]
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change(f)
    }
}
