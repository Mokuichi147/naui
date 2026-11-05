//! Text that mixes bold, colors and links within a line.

use crate::{EventResponse, Slot, TextColor};

/// A stretch of text with one look. A [`RichText`] is a sequence of these.
///
/// ```
/// # use naui_core::{TextColor, TextRun};
/// let run = TextRun::new("failed").bold().color(TextColor::Danger);
/// assert!(run.is_bold());
/// assert_eq!(run.text(), "failed");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TextRun {
    text: String,
    bold: bool,
    color: TextColor,
    link: Option<String>,
}

impl TextRun {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// Makes the run bold.
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// The color role of the run. [`TextColor::Default`] takes the label's color
    /// (`Label::set_color`).
    ///
    /// **A link keeps the platform's link color**, so the role has no effect on a
    /// run that is also a [`link`](Self::link).
    pub fn color(mut self, color: TextColor) -> Self {
        if self.link.is_none() {
            self.color = color;
        }
        self
    }

    /// Makes the run a link to `href`.
    ///
    /// Clicking it opens `href` with the default handler, as with `Link`, unless
    /// `Label::on_link` answers [`EventResponse::Handled`]. An empty `href` opens
    /// nothing and only notifies `on_link`.
    pub fn link(mut self, href: impl Into<String>) -> Self {
        // 効かない役割を残すと、見た目が同じ値どうしが等しくならない。
        self.color = TextColor::Default;
        self.link = Some(href.into());
        self
    }

    /// The text of the run.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_bold(&self) -> bool {
        self.bold
    }

    /// The color role. Always [`TextColor::Default`] for a link.
    pub fn text_color(&self) -> TextColor {
        self.color
    }

    /// Where the run links to. `None` if it is not a link.
    pub fn href(&self) -> Option<&str> {
        self.link.as_deref()
    }

    /// Whether two runs look and behave the same, ignoring their text.
    fn same_look(&self, other: &Self) -> bool {
        self.bold == other.bold && self.color == other.color && self.link == other.link
    }
}

/// Text that mixes bold, colors and links within a line. Shown with
/// `Label::set_rich_text`.
///
/// | | macOS | Windows | Linux | Web |
/// | --- | --- | --- | --- | --- |
/// | Container | `NSAttributedString` | `TextBlock.Inlines` | Pango markup | `<span>` |
/// | Bold | `NSFontDescriptor`'s bold trait | `Run.FontWeight` | `<b>` | `<b>` |
/// | Color | `NSForegroundColorAttributeName` | `Run.Foreground` | `<span foreground>` | `<span style="color">` |
/// | Link | `NSLinkAttributeName` | `Hyperlink` | `<a href>` | `<a href>` |
///
/// Two values built differently are equal when they show the same thing: empty runs
/// are dropped and neighbouring runs with the same look are joined. That equality is
/// what lets `set_rich_text` do nothing when given the content already shown.
///
/// ```
/// # use naui_core::{RichText, TextColor};
/// let text = RichText::new()
///     .text("Build ")
///     .colored("failed", TextColor::Danger)
///     .text(": see the ")
///     .link("log", "https://example.com/log");
/// assert_eq!(text.plain_text(), "Build failed: see the log");
/// assert_eq!(text.runs().len(), 4);
/// assert_eq!(RichText::new().bold("ab"), RichText::new().bold("a").bold("b"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct RichText {
    runs: Vec<TextRun>,
}

impl RichText {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends text with the label's own look.
    pub fn text(self, text: impl Into<String>) -> Self {
        self.run(TextRun::new(text))
    }

    /// Appends bold text.
    pub fn bold(self, text: impl Into<String>) -> Self {
        self.run(TextRun::new(text).bold())
    }

    /// Appends text in a color role.
    pub fn colored(self, text: impl Into<String>, color: TextColor) -> Self {
        self.run(TextRun::new(text).color(color))
    }

    /// Appends a link. See [`TextRun::link`].
    pub fn link(self, text: impl Into<String>, href: impl Into<String>) -> Self {
        self.run(TextRun::new(text).link(href))
    }

    /// Appends a run, for combinations such as bold and colored.
    pub fn run(mut self, run: TextRun) -> Self {
        if run.text.is_empty() {
            return self;
        }
        match self.runs.last_mut() {
            Some(last) if last.same_look(&run) => last.text.push_str(&run.text),
            _ => self.runs.push(run),
        }
        self
    }

    pub fn runs(&self) -> &[TextRun] {
        &self.runs
    }

    /// The text without its formatting. What `Label::text` returns.
    pub fn plain_text(&self) -> String {
        self.runs.iter().map(TextRun::text).collect()
    }

    /// Whether nothing in it differs from text passed to `Label::set_text`.
    pub fn is_plain(&self) -> bool {
        self.runs
            .iter()
            .all(|run| run.same_look(&TextRun::default()))
    }

    /// Whether it contains a link.
    pub fn has_links(&self) -> bool {
        self.runs.iter().any(|run| run.link.is_some())
    }
}

/// Holds one handler for `Label::on_link`. Shared by all backends.
///
/// The closure is taken out while it is called, so you may set the label's content
/// or replace the handler from inside the notification. A click that arrives
/// re-entrantly is treated as [`EventResponse::Continue`].
#[derive(Default)]
pub struct LinkHandler(std::cell::RefCell<Slot<LinkCallback>>);

type LinkCallback = dyn FnMut(&str) -> EventResponse;

impl LinkHandler {
    pub fn set(&self, f: impl FnMut(&str) -> EventResponse + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub fn emit(&self, href: &str) -> EventResponse {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return EventResponse::Continue;
        };
        let response = f(href);
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
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn built_differently_but_shown_the_same_is_equal() {
        // 空の断片と、同じ見た目の隣どうしは、値の比較に影響しない。
        // `set_rich_text` はこの比較で「表示中と同じ」を見分ける。
        assert_eq!(
            RichText::new().text("ab").bold("cd"),
            RichText::new()
                .text("a")
                .text("")
                .text("b")
                .bold("c")
                .bold("d"),
        );
        assert_ne!(RichText::new().bold("ab"), RichText::new().text("ab"));
        assert_ne!(
            RichText::new().link("a", "https://a.example"),
            RichText::new().link("a", "https://b.example"),
        );
    }

    #[test]
    fn neighbouring_links_stay_apart_unless_they_share_the_href() {
        let same = RichText::new()
            .link("a", "https://a.example")
            .link("b", "https://a.example");
        assert_eq!(same.runs().len(), 1);
        let different = RichText::new()
            .link("a", "https://a.example")
            .link("b", "https://b.example");
        assert_eq!(different.runs().len(), 2);
    }

    #[test]
    fn a_link_ignores_its_color_role() {
        let run = TextRun::new("a").color(TextColor::Danger).link("x:y");
        assert_eq!(run.text_color(), TextColor::Default);
        assert_eq!(
            TextRun::new("a")
                .link("x:y")
                .color(TextColor::Danger)
                .text_color(),
            TextColor::Default
        );
        // 効かない役割の違いで別の内容と見なさない。
        assert_eq!(
            RichText::new().run(run),
            RichText::new().run(TextRun::new("a").link("x:y"))
        );
    }

    #[test]
    fn plain_text_drops_the_formatting() {
        let text = RichText::new()
            .bold("a")
            .colored("b", TextColor::Accent)
            .link("c", "x:y");
        assert_eq!(text.plain_text(), "abc");
        assert!(!text.is_plain());
        assert!(text.has_links());
        assert!(RichText::new().text("abc").is_plain());
        assert!(RichText::new().is_plain());
        assert!(!RichText::new().text("abc").has_links());
    }

    #[test]
    fn the_handler_may_be_replaced_from_inside_the_notification() {
        let handler = Rc::new(LinkHandler::default());
        assert_eq!(handler.emit("x:y"), EventResponse::Continue);

        let replaced = Rc::new(Cell::new(false));
        {
            let inner = handler.clone();
            let replaced = replaced.clone();
            handler.set(move |_href| {
                let replaced = replaced.clone();
                inner.set(move |_href| {
                    replaced.set(true);
                    EventResponse::Continue
                });
                EventResponse::Handled
            });
        }
        assert_eq!(handler.emit("x:y"), EventResponse::Handled);
        assert_eq!(handler.emit("x:y"), EventResponse::Continue);
        assert!(replaced.get());
    }
}
