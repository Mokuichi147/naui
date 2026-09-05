//! 検索の入力欄 (WinUI 3 のネイティブ `AutoSuggestBox`)。
//!
//! Fluent 2 で検索の欄にあたるのは `AutoSuggestBox` で、虫めがねの印は
//! `QueryIcon`、確定は `QuerySubmitted` が受け持つ。型は [`naui_winui3`]
//! の投影をそのまま使う。
//!
//! 候補の一覧 (`ItemsSource`) は渡さないので、打っても候補は出ない。
//! naui の検索欄は「打鍵の通知」と「確定の通知」だけを持つ、4 環境で
//! そろう部分に合わせてある。

use std::rc::Rc;
use std::sync::Arc;

use naui_core::Result;
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    AutoSuggestBox, AutoSuggestBoxQuerySubmittedEventArgs, AutoSuggestBoxTextChangedEventArgs,
    AutoSuggestionBoxTextChangeReason, Control,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::UIElement;
use windows::Foundation::TypedEventHandler;
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

// 虫めがねの印は WinUI の標準アイコン (Segoe Fluent Icons の Find) を使う。
// 生成ではなく XAML から読み込むのは、`QueryIcon` へ入れる `SymbolIcon` が
// 投影に無いため。文字列で書けば XAML 側の変換が作ってくれる。
const AUTO_SUGGEST_BOX_XAML: &str = r#"<AutoSuggestBox
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    QueryIcon="Find"/>"#;

/// 文字列を 1 つ受け取る通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するので [`UiThreadCell`] に
/// 載せる。呼び出しの間だけクロージャを取り出すため、通知の中から同じ欄を
/// 操作しても二重借用にならない。
#[derive(Clone)]
struct TextHandler(HandlerCell<dyn FnMut(&str)>);

impl TextHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(&str) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, text: &str) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(text);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct SearchInputInner {
    native: AutoSuggestBox,
    on_change: TextHandler,
    on_search: TextHandler,
}

/// 検索の入力欄 (`AutoSuggestBox`)。
///
/// 虫めがねの印と、打ち始めると出る取り消しボタン (✕) は WinUI が出す。
#[derive(Clone)]
pub struct SearchInput(Rc<SearchInputInner>);
impl_widget!(SearchInput, native);

impl SearchInput {
    pub(crate) fn new() -> Result<Self> {
        let native = load_auto_suggest_box()?;
        let this = Self(Rc::new(SearchInputInner {
            native,
            on_change: TextHandler::new(),
            on_search: TextHandler::new(),
        }));
        this.connect()?;
        Ok(this)
    }

    /// WinUI の `TextChanged` / `QuerySubmitted` を Rust のクロージャへつなぐ。
    ///
    /// `TextChanged` は `Text` を書き換えたときにも飛ぶので、`Reason` が
    /// プログラムからの変更なら黙る (macOS / GTK / Web と同じく、通知は
    /// 利用者の操作のときだけ)。
    fn connect(&self) -> Result<()> {
        let target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let changed = TypedEventHandler::<AutoSuggestBox, AutoSuggestBoxTextChangedEventArgs>::new(
            move |_sender, args| {
                let _ = target.try_with_mut(|weak| {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let reason = args
                        .as_ref()
                        .and_then(|args| args.Reason().ok())
                        .unwrap_or(AutoSuggestionBoxTextChangeReason::ProgrammaticChange);
                    if reason == AutoSuggestionBoxTextChangeReason::ProgrammaticChange {
                        return;
                    }
                    let text = inner.native.Text().unwrap_or_default().to_string();
                    inner.on_change.emit(&text);
                });
                Ok(())
            },
        );
        self.0
            .native
            .TextChanged(&changed)
            .map_err(|e| to_error("AutoSuggestBox の変更購読", e))?;

        let target = Arc::new(UiThreadCell::new(Rc::downgrade(&self.0)));
        let submitted =
            TypedEventHandler::<AutoSuggestBox, AutoSuggestBoxQuerySubmittedEventArgs>::new(
                move |_sender, args| {
                    let _ = target.try_with_mut(|weak| {
                        let Some(inner) = weak.upgrade() else {
                            return;
                        };
                        // 確定した文字列は引数で来るが、読めなければ欄から読む。
                        let text = args
                            .as_ref()
                            .and_then(|args| args.QueryText().ok())
                            .or_else(|| inner.native.Text().ok())
                            .unwrap_or_default()
                            .to_string();
                        inner.on_search.emit(&text);
                    });
                    Ok(())
                },
            );
        self.0
            .native
            .QuerySubmitted(&submitted)
            .map_err(|e| to_error("AutoSuggestBox の確定購読", e))?;
        Ok(())
    }

    /// いま入力されている文字列。
    pub fn text(&self) -> String {
        self.0
            .native
            .Text()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// 文字列を置き換える。`on_change` は呼ばれない。
    pub fn set_text(&self, text: &str) {
        let _ = self.0.native.SetText(&HSTRING::from(text));
    }

    pub fn set_placeholder(&self, text: &str) {
        let _ = self.0.native.SetPlaceholderText(&HSTRING::from(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(control) = self.0.native.cast::<Control>() {
            let _ = control.SetIsEnabled(enabled);
        }
    }

    /// 1 文字入力するたびに、その時点の文字列で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// Enter か虫めがねの印で確定したときに、その時点の文字列で呼ばれる。
    pub fn on_search(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_search.set(f);
    }
}

fn load_auto_suggest_box() -> Result<AutoSuggestBox> {
    XamlReader::Load(&HSTRING::from(AUTO_SUGGEST_BOX_XAML))
        .and_then(|element| element.cast::<AutoSuggestBox>())
        .map_err(|e| to_error("AutoSuggestBox の生成", e))
}
