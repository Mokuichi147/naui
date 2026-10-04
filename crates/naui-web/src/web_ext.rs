//! Web だけの DOM 操作 (CSS のクラスと属性)。
//!
//! 見た目の作り込みや、テスト用の目印 (`data-testid` など) のように、
//! **Web でしか意味の無いもの**をアプリが付けるための口。ほかの環境に
//! 対応物が無いので naui の共通 API には入れず、このトレイトに分けてある。
//! どのウィジェットにも使える。
//!
//! ```ignore
//! #[cfg(target_arch = "wasm32")]
//! {
//!     use naui::web::WebWidgetExt;
//!     button.add_class("primary");
//!     button.set_attribute("data-testid", "save");
//! }
//! ```

use crate::widgets::Widget;

/// naui が書くので、アプリからは書き換えられない属性。
///
/// - `style`: 大きさの指定 (`set_sizing`) などをインラインのスタイルで書く。
///   見た目はクラスと文書側の CSS で変える。
/// - `data-naui-*`: 親コンテナの種類や非表示の目印として naui が使う。
fn is_reserved(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "style" || name.starts_with("data-naui-")
}

/// ウィジェットの要素の CSS のクラスと属性を扱う。Web でだけ使える。
///
/// 対象は親へ置いている要素 ([`Widget::native_element`])。naui は
/// クラスを使わないので、クラスは自由に付け外しできる。属性のうち `role` や
/// `aria-*`・`tabindex` などは naui も書くので、上書きするとウィジェットの
/// ふるまいや読み上げが変わることがある。
pub trait WebWidgetExt: Widget {
    /// CSS のクラスを足す。
    fn add_class(&self, name: &str) {
        let _ = self.native_element().class_list().add_1(name);
    }

    /// CSS のクラスを外す。
    fn remove_class(&self, name: &str) {
        let _ = self.native_element().class_list().remove_1(name);
    }

    /// CSS のクラスが付いているか。
    fn has_class(&self, name: &str) -> bool {
        self.native_element().class_list().contains(name)
    }

    /// 属性を書く。書けたら `true`。
    ///
    /// `style` と `data-naui-*` は naui が使うので書かない (`false` を返す)。
    fn set_attribute(&self, name: &str, value: &str) -> bool {
        if is_reserved(name) {
            return false;
        }
        self.native_element().set_attribute(name, value).is_ok()
    }

    /// 属性を外す。外せたら (もともと無かったときも) `true`。
    ///
    /// `style` と `data-naui-*` は naui が使うので外さない (`false` を返す)。
    fn remove_attribute(&self, name: &str) -> bool {
        if is_reserved(name) {
            return false;
        }
        self.native_element().remove_attribute(name).is_ok()
    }

    /// 属性の値。無ければ `None`。
    fn attribute(&self, name: &str) -> Option<String> {
        self.native_element().get_attribute(name)
    }
}

impl<T: Widget + ?Sized> WebWidgetExt for T {}
