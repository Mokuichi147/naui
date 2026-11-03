//! A component's lifecycle, without a window: messages wrapped into the parent's,
//! the order of `unmount`, and how `Children` mounts, moves and unmounts.

use std::cell::RefCell;
use std::convert::Infallible;
use std::rc::Rc;

use super::component::{Keyed, Mounted, Step};
use super::{App, Dispatch, Program};

/// What happened, in order.
type Log = Rc<RefCell<Vec<String>>>;

/// A child: a row with an id and a text.
struct Row {
    id: u32,
    text: String,
    log: Log,
}

#[derive(Debug, Clone, PartialEq)]
enum RowMsg {
    Rename(String),
}

/// The row's "widgets": its dispatch (as callbacks hold it) and what render wrote.
struct RowView {
    name: String,
    dispatch: Dispatch<RowMsg>,
    shown: RefCell<String>,
    /// Messages the next `render` sends, once (as a notification raised by a setter
    /// would).
    from_render: RefCell<Vec<RowMsg>>,
    log: Log,
}

impl Drop for RowView {
    fn drop(&mut self) {
        self.log.borrow_mut().push(format!("drop {}", self.name));
    }
}

impl Program for Row {
    type Message = RowMsg;
    type View = RowView;

    fn update(&mut self, message: RowMsg) {
        match message {
            RowMsg::Rename(text) => self.text = text,
        }
    }

    fn render(&self, view: &RowView) {
        self.log
            .borrow_mut()
            .push(format!("render {} {}", self.id, self.text));
        *view.shown.borrow_mut() = self.text.clone();
        for message in view.from_render.take() {
            view.dispatch.send(message);
        }
    }
}

fn row(id: u32, text: &str, log: &Log) -> Row {
    Row {
        id,
        text: text.to_owned(),
        log: log.clone(),
    }
}

/// Builds a row's view, as `mount` would.
fn mount_row(
    log: &Log,
    name: impl Into<String>,
) -> impl FnOnce(&Dispatch<RowMsg>) -> Result<RowView, Infallible> + '_ {
    let name = name.into();
    move |dispatch| {
        log.borrow_mut().push(format!("mount {name}"));
        Ok(RowView {
            name,
            dispatch: dispatch.clone(),
            shown: RefCell::new(String::new()),
            from_render: RefCell::new(Vec::new()),
            log: log.clone(),
        })
    }
}

/// A parent that holds one row and forwards its messages to it.
struct Parent {
    row: Row,
    /// Every message the parent received, in order.
    got: Vec<ParentMsg>,
    log: Log,
}

#[derive(Debug, Clone, PartialEq)]
enum ParentMsg {
    Row(RowMsg),
    Keyed(u32, RowMsg),
}

impl Program for Parent {
    type Message = ParentMsg;
    type View = RefCell<Option<Mounted<Row>>>;

    fn update(&mut self, message: ParentMsg) {
        self.log.borrow_mut().push(format!("parent {message:?}"));
        self.got.push(message.clone());
        if let ParentMsg::Row(message) = message {
            self.row.update(message);
        }
    }

    fn render(&self, view: &Self::View) {
        if let Some(mounted) = &*view.borrow() {
            mounted.render(&self.row);
        }
    }
}

fn parent(log: &Log) -> App<Parent> {
    let model = Parent {
        row: row(1, "first", log),
        got: Vec::new(),
        log: log.clone(),
    };
    App::new(model, |dispatch| {
        let mounted = Mounted::<Row>::build(dispatch, ParentMsg::Row, mount_row(log, "row"))?;
        Ok::<_, Infallible>(RefCell::new(Some(mounted)))
    })
    .expect("build")
}

fn child_dispatch(app: &App<Parent>) -> Dispatch<RowMsg> {
    app.view()
        .borrow()
        .as_ref()
        .expect("mounted")
        .view()
        .dispatch
        .clone()
}

#[test]
fn a_mounted_child_is_rendered_from_the_parents_render() {
    let log = Log::default();
    let _app = parent(&log);
    assert_eq!(*log.borrow(), ["mount row", "render 1 first"]);
}

#[test]
fn a_childs_message_reaches_the_parents_update_wrapped() {
    let log = Log::default();
    let app = parent(&log);
    log.borrow_mut().clear();
    child_dispatch(&app).send(RowMsg::Rename("renamed".into()));
    // 子の送り口から送ったものは、親のメッセージに包まれて親の update へ届き、
    // 親が子の update へ渡す。描くのは親の render から。
    assert_eq!(
        *log.borrow(),
        ["parent Row(Rename(\"renamed\"))", "render 1 renamed"]
    );
    assert_eq!(app.model().row.text, "renamed");
}

#[test]
fn a_wrapped_dispatch_stays_connected_while_the_app_is_there() {
    let log = Log::default();
    let app = parent(&log);
    let wrapped: Dispatch<RowMsg> = app.dispatch().wrap(ParentMsg::Row);
    assert!(wrapped.is_connected());
    wrapped.send(RowMsg::Rename("again".into()));
    assert_eq!(app.model().row.text, "again");
    drop(app);
    assert!(!wrapped.is_connected());
    wrapped.send(RowMsg::Rename("ignored".into()));
}

#[test]
fn unmount_disconnects_then_cleans_up_then_removes_then_drops() {
    let log = Log::default();
    let app = parent(&log);
    let dispatch = child_dispatch(&app);
    log.borrow_mut().clear();
    let mounted = app.view().borrow_mut().take().expect("mounted");
    mounted.unmount_with(
        |view| {
            // 送り口はもう切れていて、ここから送っても親へ届かない。
            assert!(!view.dispatch.is_connected());
            view.dispatch.send(RowMsg::Rename("from unmount".into()));
            view.log.borrow_mut().push("unmount".into());
        },
        |view| view.log.borrow_mut().push("remove root".into()),
    );
    assert_eq!(*log.borrow(), ["unmount", "remove root", "drop row"]);
    assert!(!dispatch.is_connected());
    dispatch.send(RowMsg::Rename("late".into()));
    assert!(app.model().got.is_empty(), "外したあとの送信は捨てられる");
    assert!(app.dispatch().is_connected(), "親の送り口はつながったまま");
}

#[test]
fn dropping_a_mounted_child_disconnects_it_without_unmount() {
    let log = Log::default();
    let app = parent(&log);
    let dispatch = child_dispatch(&app);
    log.borrow_mut().clear();
    drop(app.view().borrow_mut().take());
    assert_eq!(*log.borrow(), ["drop row"]);
    assert!(!dispatch.is_connected());
}

#[test]
fn a_failing_mount_returns_its_error_and_leaves_nothing_connected() {
    let log = Log::default();
    let app = parent(&log);
    let mut kept = None;
    let result = Mounted::<Row>::build(&app.dispatch(), ParentMsg::Row, |dispatch| {
        kept = Some(dispatch.clone());
        Err::<RowView, _>("no widgets")
    });
    assert_eq!(result.err().map(|_| ()), Some(()));
    assert!(!kept.expect("dispatch").is_connected());
}

/// Records the changes `sync_with` asks for, as a container would apply them.
#[derive(Default)]
struct Container {
    /// The names of the views placed, in order.
    names: Vec<String>,
    log: Log,
}

impl Container {
    fn apply(&mut self, step: Step<'_, RowView>) {
        match step {
            Step::Insert { index, view } => {
                self.log
                    .borrow_mut()
                    .push(format!("insert {index} {}", view.name));
                self.names.insert(index, view.name.clone());
            }
            Step::Remove { index, view } => {
                self.log
                    .borrow_mut()
                    .push(format!("remove {index} {}", view.name));
                assert_eq!(self.names.remove(index), view.name, "外す位置が合っている");
            }
        }
    }
}

struct Fixture {
    log: Log,
    app: App<Parent>,
    keyed: Keyed<u32, Row>,
    container: Container,
    /// How many views have been mounted, to name them apart.
    mounted: usize,
}

impl Fixture {
    fn new() -> Self {
        let log = Log::default();
        let app = parent(&log);
        let keyed = Keyed::new(&app.dispatch(), ParentMsg::Keyed);
        log.borrow_mut().clear();
        Self {
            container: Container {
                names: Vec::new(),
                log: log.clone(),
            },
            log,
            app,
            keyed,
            mounted: 0,
        }
    }

    fn sync(&mut self, rows: &[Row]) {
        let log = self.log.clone();
        let mounted = &mut self.mounted;
        let container = &mut self.container;
        self.keyed
            .sync_with(
                rows.iter().map(|row| (row.id, row)),
                |dispatch| {
                    *mounted += 1;
                    let name = format!("v{mounted}");
                    mount_row(&log, name)(dispatch)
                },
                |view| view.log.borrow_mut().push(format!("unmount {}", view.name)),
                |step| container.apply(step),
            )
            .expect("sync");
    }

    fn take_log(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.borrow_mut())
    }

    fn dispatch(&self, id: u32) -> Dispatch<RowMsg> {
        self.keyed.view(&id).expect("mounted").dispatch.clone()
    }
}

#[test]
fn sync_mounts_new_children_in_order_and_renders_each() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(2, "b", &log)]);
    assert_eq!(
        fixture.take_log(),
        [
            "mount v1",
            "insert 0 v1",
            "mount v2",
            "insert 1 v2",
            "render 1 a",
            "render 2 b"
        ]
    );
    assert_eq!(fixture.container.names, ["v1", "v2"]);
    assert_eq!(fixture.keyed.len(), 2);
    assert_eq!(*fixture.keyed.view(&2).expect("row 2").shown.borrow(), "b");
}

#[test]
fn sync_keeps_children_whose_keys_stay_and_only_renders_them() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(2, "b", &log)]);
    fixture.take_log();
    fixture.sync(&[row(1, "a2", &log), row(2, "b", &log), row(3, "c", &log)]);
    assert_eq!(
        fixture.take_log(),
        [
            "mount v3",
            "insert 2 v3",
            "render 1 a2",
            "render 2 b",
            "render 3 c"
        ]
    );
}

#[test]
fn sync_unmounts_children_that_are_gone() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(2, "b", &log), row(3, "c", &log)]);
    let removed = fixture.dispatch(2);
    fixture.take_log();
    fixture.sync(&[row(1, "a", &log), row(3, "c", &log)]);
    // 外す前に後片付けが走り、そのあと容器から外して、最後に捨てる。
    assert_eq!(
        fixture.take_log(),
        [
            "unmount v2",
            "remove 1 v2",
            "drop v2",
            "render 1 a",
            "render 3 c"
        ]
    );
    assert_eq!(fixture.container.names, ["v1", "v3"]);
    assert!(!removed.is_connected());
    removed.send(RowMsg::Rename("late".into()));
    assert!(fixture.app.model().got.is_empty());
}

#[test]
fn sync_moves_children_without_mounting_them_again() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(2, "b", &log), row(3, "c", &log)]);
    fixture.take_log();
    fixture.sync(&[row(3, "c", &log), row(1, "a", &log), row(2, "b", &log)]);
    assert_eq!(
        fixture.take_log(),
        [
            "remove 2 v3",
            "insert 0 v3",
            "render 3 c",
            "render 1 a",
            "render 2 b"
        ]
    );
    assert_eq!(fixture.container.names, ["v3", "v1", "v2"]);
}

#[test]
fn sync_handles_removal_moves_and_mounts_together() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(2, "b", &log), row(3, "c", &log)]);
    fixture.take_log();
    fixture.sync(&[row(4, "d", &log), row(3, "c", &log), row(1, "a", &log)]);
    assert_eq!(fixture.container.names, ["v4", "v3", "v1"]);
    let log = fixture.take_log();
    assert_eq!(log[..3], ["unmount v2", "remove 1 v2", "drop v2"]);
}

#[test]
fn sync_ignores_a_repeated_key() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(1, "again", &log)]);
    assert_eq!(fixture.container.names, ["v1"]);
    assert_eq!(*fixture.keyed.view(&1).expect("row 1").shown.borrow(), "a");
}

#[test]
fn a_keyed_childs_message_carries_its_key() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(2, "b", &log)]);
    fixture.dispatch(2).send(RowMsg::Rename("x".into()));
    assert_eq!(
        fixture.app.model().got,
        [ParentMsg::Keyed(2, RowMsg::Rename("x".into()))]
    );
}

#[test]
fn sync_stops_at_a_failing_mount_and_carries_on_next_time() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    let rows = [row(1, "a", &log), row(2, "b", &log), row(3, "c", &log)];
    let container = &mut fixture.container;
    let mut count = 0;
    let result = fixture.keyed.sync_with(
        rows.iter().map(|row| (row.id, row)),
        |dispatch| {
            count += 1;
            if count == 2 {
                return Err("no widgets");
            }
            mount_row(&log, format!("w{count}"))(dispatch).map_err(|never| match never {})
        },
        |_| {},
        |step| container.apply(step),
    );
    assert_eq!(result.err(), Some("no widgets"));
    assert_eq!(fixture.container.names, ["w1"]);
    fixture.sync(&rows);
    assert_eq!(fixture.container.names, ["w1", "v1", "v2"]);
    assert_eq!(fixture.keyed.len(), 3);
}

#[test]
fn sync_with_nothing_unmounts_every_child() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(2, "b", &log)]);
    fixture.take_log();
    fixture.sync(&[]);
    assert_eq!(
        fixture.take_log(),
        [
            "unmount v1",
            "remove 0 v1",
            "drop v1",
            "unmount v2",
            "remove 0 v2",
            "drop v2"
        ]
    );
    assert!(fixture.container.names.is_empty());
    assert_eq!(fixture.keyed.len(), 0);
}

#[test]
fn a_child_sending_from_its_render_waits_its_turn() {
    let log = Log::default();
    let app = parent(&log);
    app.view()
        .borrow()
        .as_ref()
        .expect("mounted")
        .view()
        .from_render
        .borrow_mut()
        .push(RowMsg::Rename("from render".into()));
    log.borrow_mut().clear();
    child_dispatch(&app).send(RowMsg::Rename("n".into()));
    // 子の render の中で送られたものは、render が終わってから親の update へ渡る。
    assert_eq!(
        *log.borrow(),
        [
            "parent Row(Rename(\"n\"))",
            "render 1 n",
            "parent Row(Rename(\"from render\"))",
            "render 1 from render"
        ]
    );
}
