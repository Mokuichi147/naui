//! Search fields (WinUI 3's native `AutoSuggestBox`).
//!
//! In Fluent 2, the search field is the `AutoSuggestBox`; the magnifying glass is
//! `QueryIcon`, and confirmation is handled by `QuerySubmitted`. The types come from
//! the [`naui_winui3`] projection as is.
//!
//! No suggestion list (`ItemsSource`) is given, so typing shows no suggestions.
//! naui's search field has only "keystroke notifications" and "confirmation
//! notifications", matching what all four platforms share.

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
use crate::ui_thread::{TextHandler, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

// 虫めがねの印は WinUI の標準アイコン (Segoe Fluent Icons の Find) を使う。
// 生成ではなく XAML から読み込むのは、`QueryIcon` へ入れる `SymbolIcon` が
// 投影に無いため。文字列で書けば XAML 側の変換が作ってくれる。
const AUTO_SUGGEST_BOX_XAML: &str = r#"<AutoSuggestBox
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    QueryIcon="Find"/>"#;

struct SearchInputInner {
    native: AutoSuggestBox,
    on_change: TextHandler,
    on_search: TextHandler,
    /// The `on_focus_change` handler.
    focus: crate::focus::Focus,
}

/// A search field (`AutoSuggestBox`).
///
/// The magnifying glass and the clear button (✕) that appears once you start typing
/// are shown by WinUI.
#[derive(Clone)]
pub struct SearchInput(Rc<SearchInputInner>);
crate::keys::impl_key_down!(SearchInput);
crate::focus::impl_focus_change!(SearchInput);
impl_widget!(SearchInput, native);

impl SearchInput {
    pub(crate) fn new() -> Result<Self> {
        let native = load_auto_suggest_box()?;
        let this = Self(Rc::new(SearchInputInner {
            native,
            on_change: TextHandler::new(),
            on_search: TextHandler::new(),
            focus: Default::default(),
        }));
        this.connect()?;
        Ok(this)
    }

    /// Connects WinUI's `TextChanged` / `QuerySubmitted` to the Rust closures.
    ///
    /// `TextChanged` also fires when `Text` is rewritten, so it stays silent when the
    /// `Reason` is a change from code (as on macOS / GTK / Web, notifications are only
    /// for user actions).
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
            .map_err(|e| to_error("subscribe to AutoSuggestBox text changes", e))?;

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
            .map_err(|e| to_error("subscribe to AutoSuggestBox query submission", e))?;
        Ok(())
    }

    /// The text currently entered.
    pub fn text(&self) -> String {
        self.0
            .native
            .Text()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// Replaces the text. `on_change` is not called.
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

    /// Called with the current text every time a character is typed.
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change.set(f);
    }

    /// Called with the current text when the user confirms with Enter or the magnifying
    /// glass.
    pub fn on_search(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_search.set(f);
    }
}

fn load_auto_suggest_box() -> Result<AutoSuggestBox> {
    XamlReader::Load(&HSTRING::from(AUTO_SUGGEST_BOX_XAML))
        .and_then(|element| element.cast::<AutoSuggestBox>())
        .map_err(|e| to_error("create the AutoSuggestBox", e))
}
