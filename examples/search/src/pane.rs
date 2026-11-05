//! One search: a field, what it found, and the work in between.

use std::time::Duration;

use naui::{Length, Orientation, Sizing, Ui};
use naui_app::{Component, Dispatch, Effect, Program};

/// How long typing has to pause before a search starts.
const PAUSE: Duration = Duration::from_millis(300);

/// Under this key, the wait and the search it leads to replace each other, so each key
/// press overtakes whichever of them is under way.
const SEARCH: &str = "search";

const WORDS: &[&str] = &[
    "anchor", "apple", "bridge", "candle", "canyon", "cedar", "delta", "ember", "falcon", "forest",
    "garden", "harbor", "island", "juniper", "lantern", "meadow", "mountain", "orchard", "pebble",
    "river", "saddle", "valley", "willow",
];

/// The slow part, which runs on another thread: a file, a database or a server would
/// go here.
fn search(query: &str) -> Result<Vec<String>, String> {
    std::thread::sleep(Duration::from_millis(500));
    if !query.chars().all(char::is_alphabetic) {
        return Err(format!(
            "cannot search for \"{query}\": only letters will do"
        ));
    }
    let query = query.to_lowercase();
    Ok(WORDS
        .iter()
        .filter(|word| word.contains(&query))
        .map(|word| word.to_string())
        .collect())
}

#[derive(Debug, PartialEq)]
pub enum State {
    Empty,
    Waiting,
    Searching,
    Found(Vec<String>),
    Failed,
}

pub struct Pane {
    pub id: u64,
    pub query: String,
    pub state: State,
}

#[derive(Debug)]
pub enum Message {
    QueryChanged(String),
    /// Typing paused.
    Search,
    Results(Result<Vec<String>, String>),
    /// The parent acts on it: it removes the pane, which cancels what the pane started.
    Close,
}

pub struct View {
    root: naui::Stack,
    field: naui::SearchInput,
    status: naui::Label,
}

impl Pane {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            query: String::new(),
            state: State::Empty,
        }
    }
}

impl Program for Pane {
    type Message = Message;
    type View = View;

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::QueryChanged(query) => {
                self.query = query;
                if self.query.is_empty() {
                    self.state = State::Empty;
                    return Effect::cancel(SEARCH);
                }
                self.state = State::Waiting;
                Effect::after(PAUSE, Message::Search).key(SEARCH)
            }
            Message::Search => {
                self.state = State::Searching;
                let query = self.query.clone();
                Effect::thread(move || Message::Results(search(&query))).key(SEARCH)
            }
            Message::Results(Ok(words)) => {
                self.state = State::Found(words);
                Effect::none()
            }
            // The model records the failure, and the app's report takes it from here.
            Message::Results(Err(error)) => {
                self.state = State::Failed;
                Effect::fail(error)
            }
            Message::Close => Effect::none(),
        }
    }

    fn render(&self, view: &View) {
        view.field.set_text(&self.query);
        view.status.set_text(&match &self.state {
            State::Empty => "Type a word".to_owned(),
            State::Waiting => "…".to_owned(),
            State::Searching => "Searching…".to_owned(),
            State::Found(words) if words.is_empty() => "Nothing found".to_owned(),
            State::Found(words) => words.join(", "),
            State::Failed => "Letters only".to_owned(),
        });
    }
}

impl Component for Pane {
    fn mount(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
        let root = ui.stack(Orientation::Horizontal)?;
        root.set_spacing(8.0);
        root.set_sizing(Sizing::fill_width());
        let field = ui.search_input()?;
        // A set width, so that the field stays put as the results get longer.
        field.set_sizing(Sizing::default().width(Length::Fixed(180.0)));
        field.on_change(dispatch.with(|text: &str| Message::QueryChanged(text.to_owned())));
        let status = ui.label("")?;
        status.set_sizing(Sizing::fill_width());
        let close = ui.button("Close")?;
        close.on_click(dispatch.to(|| Message::Close));
        root.append(&field);
        root.append(&status);
        root.append(&close);
        Ok(View {
            root,
            field,
            status,
        })
    }

    fn root(view: &View) -> &dyn naui::Widget {
        &view.root
    }
}
