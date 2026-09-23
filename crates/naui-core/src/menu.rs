//! メニューバー (OS のアプリケーションメニュー) の項目とショートカット。
//!
//! メニューバーは「見出しの並び」と「その下にぶら下がる項目の並び」という
//! 2 段の構造を持つ。見出し 1 つぶんが [`MenuSpec`]、その中の 1 行が
//! [`MenuItem`] で、項目に割り当てるキー操作が [`MenuShortcut`]。
//!
//! 項目の識別はアプリ側が持つ順序で行い、通知も
//! **(見出しのインデックス, 区切り線を含めた項目のインデックス)** で返る。

/// メニュー項目に割り当てるキーボードショートカット。
///
/// 主修飾キー (macOS は ⌘、Windows・Linux・Web は Ctrl) と英数字 1 文字の
/// 組み合わせで表す。「⌘S / Ctrl+S」のように**環境ごとに主修飾キーが違う**
/// ので、naui は組み合わせの意味だけを受け取り、その環境の書き方へ写す。
/// ⇧ と ⌥ (Alt) は足せる。
///
/// ```
/// # use naui_core::MenuShortcut;
/// let save = MenuShortcut::new('s');
/// assert_eq!(save.label(), "Ctrl+S");
/// assert_eq!(save.accelerator(), "<Control>s");
///
/// let redo = MenuShortcut::new('z').shift(true);
/// assert_eq!(redo.label(), "Ctrl+Shift+Z");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MenuShortcut {
    /// 組み合わせる文字。英数字 1 文字で、小文字にそろえて持つ。
    pub key: char,
    /// ⇧ (Shift) を含めるか。
    pub shift: bool,
    /// ⌥ / Alt を含めるか。
    pub alt: bool,
}

impl MenuShortcut {
    /// 主修飾キーと `key` の組み合わせを作る。
    ///
    /// `key` は英数字 1 文字。大文字を渡しても小文字にそろえる
    /// (⇧ を含めたいときは [`shift`](Self::shift) を使う)。
    pub fn new(key: char) -> Self {
        Self {
            key: key.to_ascii_lowercase(),
            shift: false,
            alt: false,
        }
    }

    /// ⇧ (Shift) を含めるかどうかを指定する (既定は含めない)。
    pub fn shift(mut self, shift: bool) -> Self {
        self.shift = shift;
        self
    }

    /// ⌥ / Alt を含めるかどうかを指定する (既定は含めない)。
    pub fn alt(mut self, alt: bool) -> Self {
        self.alt = alt;
        self
    }

    /// 英数字 1 文字として使えるか。使えない文字は環境ごとに扱いが違う。
    pub fn is_valid(self) -> bool {
        self.key.is_ascii_alphanumeric()
    }

    /// macOS (AppKit) の `keyEquivalent` に渡す文字列。
    ///
    /// ⌘ と ⇧・⌥ は `keyEquivalentModifierMask` で指定するため、ここには
    /// 文字だけが入る。
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// assert_eq!(MenuShortcut::new('S').key_equivalent(), "s");
    /// ```
    pub fn key_equivalent(self) -> String {
        self.key.to_ascii_lowercase().to_string()
    }

    /// Linux (GTK4) の `gtk_accelerator_parse` が読む書き方。
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// let s = MenuShortcut::new('o').shift(true).alt(true);
    /// assert_eq!(s.accelerator(), "<Control><Shift><Alt>o");
    /// ```
    pub fn accelerator(self) -> String {
        let mut out = String::from("<Control>");
        if self.shift {
            out.push_str("<Shift>");
        }
        if self.alt {
            out.push_str("<Alt>");
        }
        out.push(self.key.to_ascii_lowercase());
        out
    }

    /// Windows の仮想キーコード。英数字以外では `0`。
    ///
    /// `VK_A`..`VK_Z` と `VK_0`..`VK_9` は ASCII の大文字・数字と同じ値。
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// assert_eq!(MenuShortcut::new('s').virtual_key(), 0x53);
    /// assert_eq!(MenuShortcut::new('1').virtual_key(), 0x31);
    /// ```
    pub fn virtual_key(self) -> i32 {
        if self.is_valid() {
            self.key.to_ascii_uppercase() as i32
        } else {
            0
        }
    }

    /// 画面に出す文字列 (`Ctrl+Shift+S` の形)。
    ///
    /// macOS と GTK4 はメニュー項目の右端の表示を自分で作るので、これを使う
    /// のは表示を naui が持つ環境 (Windows・Web) だけ。
    pub fn label(self) -> String {
        let mut out = String::from("Ctrl+");
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        out.push(self.key.to_ascii_uppercase());
        out
    }

    /// 押されたキーがこのショートカットかどうか。
    ///
    /// 主修飾キーを naui が見張る環境 (Windows・Web) で使う。`key` は
    /// 押された文字で、大文字・小文字は問わない。`primary` は ⌘ か Ctrl の
    /// どちらかが押されていること。
    ///
    /// ```
    /// # use naui_core::MenuShortcut;
    /// let s = MenuShortcut::new('s');
    /// assert!(s.matches("S", true, false, false));
    /// assert!(!s.matches("s", false, false, false)); // 修飾キーが無い
    /// assert!(!s.matches("s", true, true, false));   // ⇧ が余っている
    /// ```
    pub fn matches(self, key: &str, primary: bool, shift: bool, alt: bool) -> bool {
        primary
            && shift == self.shift
            && alt == self.alt
            && key.len() == self.key.len_utf8()
            && key
                .chars()
                .next()
                .is_some_and(|c| c.eq_ignore_ascii_case(&self.key))
    }
}

/// メニューバーの 1 項目。
///
/// 押されるとその場でコマンドが走る。区切り線は選べないので、通知として
/// そのインデックスが返ることはない。
///
/// ```
/// # use naui_core::{MenuItem, MenuShortcut};
/// let items = [
///     MenuItem::new("開く").shortcut(MenuShortcut::new('o')),
///     MenuItem::separator(),
///     MenuItem::new("保存").enabled(false),
/// ];
/// assert!(items[1].is_separator());
/// assert!(!items[2].enabled);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuItem {
    /// 画面に出る文字列。区切り線では使われない。
    pub label: String,
    /// 選べるかどうか。
    pub enabled: bool,
    /// 区切り線かどうか。
    pub separator: bool,
    /// 割り当てるキーボードショートカット。区切り線では使われない。
    pub shortcut: Option<MenuShortcut>,
}

impl MenuItem {
    /// 押せる項目を作る。
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            separator: false,
            shortcut: None,
        }
    }

    /// 項目のまとまりを分ける区切り線を作る。押すことはできない。
    pub fn separator() -> Self {
        Self {
            label: String::new(),
            enabled: false,
            separator: true,
            shortcut: None,
        }
    }

    /// 選べるかどうかを指定する (既定は選べる)。
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// キーボードショートカットを割り当てる (既定は無し)。
    pub fn shortcut(mut self, shortcut: MenuShortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// 区切り線かどうか。
    pub fn is_separator(&self) -> bool {
        self.separator
    }

    /// 文字列の並びから項目列を作る。
    ///
    /// ```
    /// # use naui_core::MenuItem;
    /// let items = MenuItem::list(["元に戻す", "やり直す"]);
    /// assert_eq!(items.len(), 2);
    /// ```
    pub fn list<I, S>(labels: I) -> Vec<MenuItem>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        labels.into_iter().map(MenuItem::new).collect()
    }
}

impl From<&str> for MenuItem {
    fn from(label: &str) -> Self {
        MenuItem::new(label)
    }
}

impl From<String> for MenuItem {
    fn from(label: String) -> Self {
        MenuItem::new(label)
    }
}

/// メニューバーに並ぶ 1 つのメニュー (見出しと、その中の項目)。
///
/// ```
/// # use naui_core::{MenuItem, MenuShortcut, MenuSpec};
/// let file = MenuSpec::new(
///     "ファイル",
///     [
///         MenuItem::new("新規").shortcut(MenuShortcut::new('n')),
///         MenuItem::separator(),
///         MenuItem::new("閉じる"),
///     ],
/// );
/// assert_eq!(file.title, "ファイル");
/// assert_eq!(file.len(), 3);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuSpec {
    /// メニューバーに出る見出し。
    pub title: String,
    /// 見出しの下に並ぶ項目。区切り線を含めた順序がそのままインデックス。
    pub items: Vec<MenuItem>,
}

impl MenuSpec {
    /// 見出しと項目からメニュー 1 つを作る。
    ///
    /// 項目は文字列のままでも渡せる (`["元に戻す", "やり直す"]`)。
    pub fn new<I, T>(title: impl Into<String>, items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<MenuItem>,
    {
        Self {
            title: title.into(),
            items: items.into_iter().map(Into::into).collect(),
        }
    }

    /// 区切り線を含めた項目数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_item_defaults_to_enabled() {
        let item = MenuItem::new("開く");
        assert_eq!(item.label, "開く");
        assert!(item.enabled);
        assert!(!item.is_separator());
        assert_eq!(item.shortcut, None);
        assert!(!MenuItem::new("保存").enabled(false).enabled);
    }

    #[test]
    fn separator_is_never_pressable() {
        let item = MenuItem::separator();
        assert!(item.is_separator());
        assert!(!item.enabled);
        assert!(item.label.is_empty());
        assert_eq!(item.shortcut, None);
    }

    #[test]
    fn menu_item_list_keeps_order() {
        let items = MenuItem::list(["元に戻す", "やり直す"]);
        assert_eq!(items[0], MenuItem::from("元に戻す"));
        assert_eq!(items[1].label, "やり直す");
    }

    #[test]
    fn menu_spec_takes_labels_or_items() {
        let spec = MenuSpec::new("編集", ["元に戻す", "やり直す"]);
        assert_eq!(spec.len(), 2);
        assert!(!spec.is_empty());
        assert!(MenuSpec::new("空", Vec::<MenuItem>::new()).is_empty());

        let spec = MenuSpec::new("編集", [MenuItem::new("元に戻す"), MenuItem::separator()]);
        assert!(spec.items[1].is_separator());
    }

    /// 大文字で渡しても、キーは小文字にそろえて持つ (⇧ は別に指定する)。
    #[test]
    fn shortcut_key_is_lowercased() {
        let s = MenuShortcut::new('S');
        assert_eq!(s.key, 's');
        assert!(!s.shift);
        assert!(!s.alt);
        assert_eq!(s.key_equivalent(), "s");
    }

    #[test]
    fn shortcut_writes_each_backend_notation() {
        let plain = MenuShortcut::new('s');
        assert_eq!(plain.accelerator(), "<Control>s");
        assert_eq!(plain.label(), "Ctrl+S");
        assert_eq!(plain.virtual_key(), 0x53);

        let full = MenuShortcut::new('z').shift(true).alt(true);
        assert_eq!(full.accelerator(), "<Control><Shift><Alt>z");
        assert_eq!(full.label(), "Ctrl+Shift+Alt+Z");
        assert_eq!(full.virtual_key(), 0x5A);
    }

    #[test]
    fn digits_map_to_their_ascii_code() {
        assert_eq!(MenuShortcut::new('0').virtual_key(), 0x30);
        assert_eq!(MenuShortcut::new('9').virtual_key(), 0x39);
        assert!(MenuShortcut::new('7').is_valid());
    }

    /// 英数字以外は環境ごとに扱いが違うので、仮想キーを持たない。
    #[test]
    fn non_alphanumeric_keys_are_rejected() {
        let s = MenuShortcut::new('/');
        assert!(!s.is_valid());
        assert_eq!(s.virtual_key(), 0);
    }

    #[test]
    fn matches_requires_the_exact_modifiers() {
        let s = MenuShortcut::new('z').shift(true);
        assert!(s.matches("Z", true, true, false));
        assert!(s.matches("z", true, true, false));
        // 主修飾キーが無い、⇧ が足りない、⌥ が余っている。
        assert!(!s.matches("z", false, true, false));
        assert!(!s.matches("z", true, false, false));
        assert!(!s.matches("z", true, true, true));
        // 別のキー。
        assert!(!s.matches("y", true, true, false));
    }

    /// `KeyboardEvent.key` は "Enter" のような名前も来る。1 文字だけを見る。
    #[test]
    fn matches_ignores_named_keys() {
        let s = MenuShortcut::new('s');
        assert!(!s.matches("Enter", true, false, false));
        assert!(!s.matches("", true, false, false));
    }
}
