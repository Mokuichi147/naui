//! キーボードの入力 (`on_key_down`) で受け渡す値。
//!
//! 文字入力のウィジェットと `Window` は、押されたキーを [`KeyEvent`] で
//! 通知し、アプリは [`EventResponse`] で「ここで処理した」かどうかを返す。
//! `Handled` を返すと、そのキーはネイティブのコントロール (改行の挿入など)
//! にもウィンドウにも渡らない。
//!
//! **IME で変換している間のキーは届かない。** 変換を確定する Enter などは
//! IME が受け取るので、`Enter` で送信するような処理を書いても変換の確定で
//! 送ってしまうことはない。

/// 押されたキー。
///
/// 文字のキーは [`Key::Character`] で、英字は**小文字にそろえる**
/// ([`MenuShortcut`](crate::MenuShortcut) と同じ)。⇧ を押していたかは
/// [`Modifiers::shift`] で見る。記号のキーで ⇧ を押したときにどの文字に
/// なるかは環境 (キー配列) によって違う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Key {
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    /// ファンクションキー。`F(1)` が F1。
    F(u8),
    /// 文字を打つキー。英字は小文字。
    Character(char),
    /// 上のどれにも当たらないキー (修飾キーそのもの・メディアキーなど)。
    Other,
}

impl Key {
    /// 文字のキーを作る。英字は小文字にそろえ、空白は [`Key::Space`] にする。
    pub fn character(c: char) -> Self {
        match c {
            ' ' => Self::Space,
            '\r' | '\n' => Self::Enter,
            '\t' => Self::Tab,
            c if c.is_control() => Self::Other,
            c => Self::Character(c.to_lowercase().next().unwrap_or(c)),
        }
    }

    /// Web の `KeyboardEvent.key` の名前から作る。
    ///
    /// ```
    /// # use naui_core::Key;
    /// assert_eq!(Key::from_web_key("Enter"), Key::Enter);
    /// assert_eq!(Key::from_web_key("A"), Key::Character('a'));
    /// assert_eq!(Key::from_web_key("F5"), Key::F(5));
    /// assert_eq!(Key::from_web_key("Shift"), Key::Other);
    /// ```
    pub fn from_web_key(key: &str) -> Self {
        match key {
            "Enter" => Self::Enter,
            "Escape" | "Esc" => Self::Escape,
            "Tab" => Self::Tab,
            "Backspace" => Self::Backspace,
            "Delete" | "Del" => Self::Delete,
            " " | "Spacebar" => Self::Space,
            "ArrowUp" | "Up" => Self::ArrowUp,
            "ArrowDown" | "Down" => Self::ArrowDown,
            "ArrowLeft" | "Left" => Self::ArrowLeft,
            "ArrowRight" | "Right" => Self::ArrowRight,
            "Home" => Self::Home,
            "End" => Self::End,
            "PageUp" => Self::PageUp,
            "PageDown" => Self::PageDown,
            _ => {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Self::character(c),
                    _ => function_key(key).unwrap_or(Self::Other),
                }
            }
        }
    }
}

/// `"F1"`〜`"F24"` をファンクションキーとして読む。
fn function_key(name: &str) -> Option<Key> {
    let number: u8 = name.strip_prefix('F')?.parse().ok()?;
    (1..=24).contains(&number).then_some(Key::F(number))
}

/// 押していた修飾キー。
///
/// `meta` は macOS の ⌘、Windows の Windows キー、Linux の Super。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Modifiers {
    /// 主修飾キー (macOS は ⌘、Windows・Linux は Ctrl) を押していたか。
    ///
    /// Web は OS を確かめずに Ctrl と ⌘ のどちらでもよいとする
    /// (メニューバーのショートカットと同じ扱い)。
    pub fn primary(&self) -> bool {
        if cfg!(target_arch = "wasm32") {
            self.control || self.meta
        } else if cfg!(target_os = "macos") {
            self.meta
        } else {
            self.control
        }
    }

    /// どの修飾キーも押していないか。
    pub fn is_empty(&self) -> bool {
        !(self.shift || self.control || self.alt || self.meta)
    }
}

/// `on_key_down` に届くキー 1 回分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeyEvent {
    pub key: Key,
    pub modifiers: Modifiers,
    /// 押しっぱなしによる繰り返しか。
    pub repeat: bool,
}

impl KeyEvent {
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        Self {
            key,
            modifiers,
            repeat: false,
        }
    }

    /// 押しっぱなしによる繰り返しかどうかを指定する。
    pub fn repeat(mut self, repeat: bool) -> Self {
        self.repeat = repeat;
        self
    }
}

/// `on_key_down` からの返事。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventResponse {
    /// ここで処理した。ネイティブのコントロールにもウィンドウにも渡さない。
    Handled,
    /// 処理していない。いつもどおりネイティブのコントロールへ渡す。
    #[default]
    Continue,
}

/// `on_key_down` の通知先を 1 つ持つ。各バックエンドで共有する。
///
/// 呼んでいる間はクロージャを取り出しておくので、通知の中から同じ
/// ウィジェットを操作したり `on_key_down` で差し替えたりしてもよい。
/// 入れ子で届いたキー (通知の中で別のキーを起こしたときなど) は
/// `Continue` として扱う。
#[derive(Default)]
pub struct KeyHandler(std::cell::RefCell<crate::Slot<KeyCallback>>);

type KeyCallback = dyn FnMut(&KeyEvent) -> EventResponse;

impl KeyHandler {
    pub fn set(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    /// 通知先を持っているか。
    pub fn is_set(&self) -> bool {
        self.0.try_borrow().map_or(true, |slot| slot.is_some())
    }

    pub fn emit(&self, event: &KeyEvent) -> EventResponse {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return EventResponse::Continue;
        };
        let response = f(event);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn characters_are_lowercased() {
        assert_eq!(Key::character('S'), Key::Character('s'));
        assert_eq!(Key::character('あ'), Key::Character('あ'));
        assert_eq!(Key::character(' '), Key::Space);
        assert_eq!(Key::character('\r'), Key::Enter);
        assert_eq!(Key::character('\u{1b}'), Key::Other);
    }

    #[test]
    fn web_names_map_to_keys() {
        assert_eq!(Key::from_web_key("ArrowLeft"), Key::ArrowLeft);
        assert_eq!(Key::from_web_key(" "), Key::Space);
        assert_eq!(Key::from_web_key("F12"), Key::F(12));
        assert_eq!(Key::from_web_key("F99"), Key::Other);
        assert_eq!(Key::from_web_key("Process"), Key::Other);
    }

    #[test]
    fn handler_can_be_replaced_while_running() {
        use std::rc::Rc;
        let handler = Rc::new(KeyHandler::default());
        assert_eq!(
            handler.emit(&KeyEvent::new(Key::Enter, Modifiers::default())),
            EventResponse::Continue,
            "通知先が無ければ Continue"
        );
        handler.set({
            let handler = handler.clone();
            move |_| {
                handler.set(|_| EventResponse::Continue);
                EventResponse::Handled
            }
        });
        let enter = KeyEvent::new(Key::Enter, Modifiers::default());
        assert_eq!(handler.emit(&enter), EventResponse::Handled);
        assert_eq!(
            handler.emit(&enter),
            EventResponse::Continue,
            "差し替えた方が残る"
        );
    }
}
