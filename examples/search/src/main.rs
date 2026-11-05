//! Searches that come and go, with naui-app's effects.
//!
//! - Each [`pane::Pane`] searches on another thread once typing pauses. The wait and
//!   the search are effects under one key, so further typing replaces them: the timer
//!   is cancelled, and the result of a search already running is discarded.
//! - The panes are kept in [`Scoped`] wrappers, so closing one (removing it from the
//!   model) cancels what it started, with nothing else to do.
//! - The app's own clock is an effect too, started by a message sent from `build`.
//! - A failure reaches the one error handler, which shows it through a message. A panic
//!   stops the app, since the model may be half changed.

mod pane;

use std::time::Duration;

use naui::{Orientation, Padding, Settings, Sizing, Ui};
use naui_app::{Change, Children, Decision, Dispatch, Effect, Program, Report, ReportKind, Scoped};
use pane::Pane;

#[derive(Default)]
struct Search {
    panes: Vec<Scoped<Pane>>,
    next_id: u64,
    seconds: u64,
    /// The last failure reported.
    problem: String,
}

#[derive(Debug)]
enum Message {
    Started,
    Tick,
    AddPane,
    Pane(u64, pane::Message),
    Reported(String),
}

struct View {
    panes: Children<u64, Pane>,
    /// Holds only the panes' widgets, so a `Change`'s position is a position in it.
    list: naui::Stack,
    clock: naui::Label,
    problem: naui::Label,
}

impl Search {
    fn add_pane(&mut self) {
        self.panes.push(Scoped::new(Pane::new(self.next_id)));
        self.next_id += 1;
    }
}

impl Program for Search {
    type Message = Message;
    type View = View;

    fn update(&mut self, message: Message) -> Effect<Message> {
        match message {
            Message::Started => {
                self.add_pane();
                Effect::every(Duration::from_secs(1), || Message::Tick)
            }
            Message::Tick => {
                self.seconds += 1;
                Effect::none()
            }
            Message::AddPane => {
                self.add_pane();
                Effect::none()
            }
            Message::Reported(problem) => {
                self.problem = problem;
                Effect::none()
            }
            Message::Pane(id, pane::Message::Close) => {
                self.panes.retain(|pane| pane.id != id);
                Effect::none()
            }
            // A pane closed a moment ago can still have a message in the queue, so a
            // missing id is not an error.
            Message::Pane(id, message) => match self.panes.iter_mut().find(|pane| pane.id == id) {
                Some(pane) => pane
                    .update(message)
                    .map(move |message| Message::Pane(id, message)),
                None => Effect::none(),
            },
        }
    }

    fn render(&self, view: &View) {
        let panes = self.panes.iter().map(|pane| (pane.id, pane));
        view.panes.sync(panes, |change| match change {
            Change::Insert { index, root } => view.list.insert(index, root),
            Change::Remove { index, .. } => view.list.remove(index),
        });
        view.clock
            .set_text(&format!("Running for {} s", self.seconds));
        view.problem.set_text(&self.problem);
    }
}

fn build(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
    let window = ui.window("Search", 560.0, 320.0)?;
    let root = ui.stack(Orientation::Vertical)?;
    root.set_spacing(8.0);
    root.set_padding(Padding::all(16.0));

    let list = ui.stack(Orientation::Vertical)?;
    list.set_spacing(8.0);
    list.set_sizing(Sizing::fill_width());
    let scroll = ui.scroll()?;
    scroll.set_sizing(Sizing::fill());
    scroll.set_child(&list);

    let footer = ui.stack(Orientation::Horizontal)?;
    footer.set_sizing(Sizing::fill_width());
    let clock = ui.label("")?;
    let problem = ui.label("")?;
    let add = ui.button("Add a search")?;
    add.on_click(dispatch.to(|| Message::AddPane));
    footer.append(&clock);
    footer.append(&ui.spacer()?);
    footer.append(&add);

    root.append(&scroll);
    root.append(&problem);
    root.append(&footer);
    window.set_child(&root);
    window.show();

    // What is to start with the app starts as any other effect: from a message.
    dispatch.send(Message::Started);
    Ok(View {
        panes: Children::new(ui, dispatch, Message::Pane),
        list,
        clock,
        problem,
    })
}

/// What goes wrong anywhere in the app ends up here.
fn on_error(report: &Report, dispatch: &Dispatch<Message>) -> Decision {
    eprintln!("{report}");
    if report.kind() == ReportKind::Panic {
        return Decision::Stop;
    }
    dispatch.send(Message::Reported(report.text().to_owned()));
    Decision::GoOn
}

fn main() -> naui::Result<()> {
    naui_app::run_with_handler(Settings::new("search"), Search::default(), build, on_error)
}

/// `update` and the effects it returns are plain values: no window is needed.
#[cfg(test)]
mod tests {
    use super::*;
    use pane::{Message as PaneMessage, State};

    fn typed(search: &mut Search, id: u64, text: &str) -> Effect<Message> {
        search.update(Message::Pane(
            id,
            PaneMessage::QueryChanged(text.to_owned()),
        ))
    }

    fn started() -> Search {
        let mut search = Search::default();
        let clock = search.update(Message::Started);
        assert!(!clock.is_none());
        search
    }

    #[test]
    fn typing_waits_under_the_key_the_search_runs_under() {
        let mut search = started();
        let waiting = typed(&mut search, 0, "river");
        assert_eq!(waiting.keys(), ["search"]);
        assert_eq!(search.panes[0].state, State::Waiting);
        let searching = search.update(Message::Pane(0, PaneMessage::Search));
        assert_eq!(searching.keys(), ["search"]);
        assert_eq!(search.panes[0].state, State::Searching);
    }

    #[test]
    fn clearing_the_field_cancels_the_search() {
        let mut search = started();
        let _ = typed(&mut search, 0, "river");
        let cleared = typed(&mut search, 0, "");
        assert_eq!(cleared.keys(), ["search"]);
        assert_eq!(search.panes[0].state, State::Empty);
    }

    #[test]
    fn a_failed_search_is_recorded_and_reported() {
        let mut search = started();
        let failed = search.update(Message::Pane(0, PaneMessage::Results(Err("no".into()))));
        assert_eq!(failed.failures(), ["no"]);
        assert_eq!(search.panes[0].state, State::Failed);
    }

    #[test]
    fn a_reported_failure_is_kept_to_be_shown() {
        let mut search = started();
        assert!(search.update(Message::Reported("no".to_owned())).is_none());
        assert_eq!(search.problem, "no");
    }

    #[test]
    fn a_message_for_a_closed_pane_is_ignored() {
        let mut search = started();
        let closed = search.update(Message::Pane(0, PaneMessage::Close));
        assert!(closed.is_none());
        assert!(search.panes.is_empty());
        assert!(search
            .update(Message::Pane(0, PaneMessage::Results(Ok(Vec::new()))))
            .is_none());
    }
}
