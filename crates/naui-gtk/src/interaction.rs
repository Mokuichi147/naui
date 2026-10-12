//! どのウィジェットにもある操作 (表示・非表示 / フォーカス / ツールチップ)。
//!
//! 表示・非表示は親へ置いている [`SizeBin`] ごと切り替える。中身だけを
//! 隠すと、入れ物の余白や大きさの指定が場所を取り続けるため。フォーカスと
//! ツールチップは中身のコントロールに付ける。

use gtk::prelude::*;

use crate::bin::SizeBin;

pub(crate) fn set_visible(bin: &SizeBin, visible: bool) {
    bin.set_visible(visible);
}

pub(crate) fn is_visible(bin: &SizeBin) -> bool {
    bin.is_visible()
}

/// キーボードフォーカスを移す。移せたら `true`。
///
/// 自分が受け取れなければ、子孫のうち最初に受け取れるものへ移す。
/// 表示されていない (ウィンドウが出る前・隠れている) ときは移せない。
pub(crate) fn request_focus(widget: &gtk::Widget) -> bool {
    if !widget.is_mapped() {
        return false;
    }
    grab_first(widget)
}

fn grab_first(widget: &gtk::Widget) -> bool {
    if !widget.is_visible() || !widget.is_sensitive() {
        return false;
    }
    if widget.grab_focus() {
        return true;
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if grab_first(&current) {
            return true;
        }
        child = current.next_sibling();
    }
    false
}

/// ポインターを重ねたときに出す説明。`None` で外す。
pub(crate) fn set_tooltip(widget: &gtk::Widget, text: Option<&str>) {
    widget.set_tooltip_text(text);
}

/// 読み上げソフトに伝える名前 (`GTK_ACCESSIBLE_PROPERTY_LABEL`)。`None` で外す。
///
/// 文字を持つ `GtkButton` (トグルとリンクも) は、自分の名前を中のラベルから
/// 取るよう `LABELLED_BY` を張っている。名前は `LABELLED_BY` が `LABEL` より
/// 優先される (ARIA の aria-labelledby と aria-label と同じ) ので、付けている
/// 間はその関係を外し、外すときに中のラベルへ張り直す。
pub(crate) fn set_accessible_label(widget: &gtk::Widget, text: Option<&str>) {
    let label_child = widget
        .downcast_ref::<gtk::Button>()
        .and_then(|button| button.child())
        .filter(|child| child.is::<gtk::Label>());
    match text {
        Some(text) => {
            widget.update_property(&[gtk::accessible::Property::Label(text)]);
            if label_child.is_some() {
                widget.reset_relation(gtk::AccessibleRelation::LabelledBy);
            }
        }
        None => {
            widget.reset_property(gtk::AccessibleProperty::Label);
            if let Some(child) = label_child {
                widget.update_relation(&[gtk::accessible::Relation::LabelledBy(&[
                    child.upcast_ref()
                ])]);
            }
        }
    }
}
