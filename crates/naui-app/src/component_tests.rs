//! A component's lifecycle, without a window: messages wrapped into the parent's,
//! the order of `unmount`, and how `Children` mounts, moves and unmounts.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::convert::Infallible;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::{Rc, Weak};

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
    /// Which of `render` and `unmount` panics.
    explode: Cell<Option<&'static str>>,
    /// Called from `render` and `unmount`, to read the `Keyed` holding this row.
    during: RefCell<Option<Box<dyn Fn()>>>,
    log: Log,
}

impl RowView {
    /// What `render` and `unmount` share: calls `during`, and panics if told to.
    fn step(&self, name: &'static str) {
        if let Some(during) = &*self.during.borrow() {
            during();
        }
        if self.explode.get() == Some(name) {
            panic!("{name} panicked (intended)");
        }
    }
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
        view.step("render");
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
            explode: Cell::new(None),
            during: RefCell::new(None),
            log: log.clone(),
        })
    }
}

/// A row's `unmount`. Its dispatch is already disconnected, so what it sends is
/// dropped.
fn unmount_row(view: &RowView) {
    assert!(!view.dispatch.is_connected());
    view.dispatch.send(RowMsg::Rename("from unmount".into()));
    view.log.borrow_mut().push(format!("unmount {}", view.name));
    view.step("unmount");
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
        let mounted =
            Mounted::<Row>::build(dispatch, ParentMsg::Row, mount_row(log, "row"), unmount_row)?;
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
    mounted.unmount_with(|view| view.log.borrow_mut().push("remove root".into()));
    // 後片付けは 1 回だけ (外したあとの破棄でもう一度呼ばない)。
    assert_eq!(*log.borrow(), ["unmount row", "remove root", "drop row"]);
    assert!(!dispatch.is_connected());
    dispatch.send(RowMsg::Rename("late".into()));
    assert!(app.model().got.is_empty(), "外したあとの送信は捨てられる");
    assert!(app.dispatch().is_connected(), "親の送り口はつながったまま");
}

#[test]
fn dropping_a_mounted_child_takes_it_down() {
    let log = Log::default();
    let app = parent(&log);
    let dispatch = child_dispatch(&app);
    log.borrow_mut().clear();
    drop(app.view().borrow_mut().take());
    assert_eq!(*log.borrow(), ["unmount row", "drop row"]);
    assert!(!dispatch.is_connected());
}

#[test]
fn the_app_ending_does_not_call_unmount() {
    let log = Log::default();
    let app = parent(&log);
    log.borrow_mut().clear();
    drop(app);
    assert_eq!(
        *log.borrow(),
        ["drop row"],
        "終了後はウィジェットに触らない"
    );
}

#[test]
fn unwinding_does_not_call_unmount() {
    let log = Log::default();
    let app = parent(&log);
    let mounted = app.view().borrow_mut().take().expect("mounted");
    log.borrow_mut().clear();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _mounted = mounted;
        panic!("unwinding (intended)");
    }));
    assert!(outcome.is_err());
    assert_eq!(*log.borrow(), ["drop row"]);
}

/// A component whose view holds other components, one always there and some under
/// keys.
struct Outer;

enum OuterMsg {
    Inner(#[allow(dead_code)] RowMsg),
    Row(#[allow(dead_code)] u32, #[allow(dead_code)] RowMsg),
}

struct OuterView {
    inner: Mounted<Row>,
    rows: Keyed<u32, Row>,
    log: Log,
}

impl Program for Outer {
    type Message = OuterMsg;
    type View = OuterView;

    fn update(&mut self, _: OuterMsg) {}

    fn render(&self, _: &OuterView) {}
}

fn unmount_outer(view: &OuterView) {
    view.log.borrow_mut().push("unmount outer".into());
}

/// Mounts an [`Outer`] under the app, with an inner row and two keyed rows.
fn mount_outer(app: &App<Parent>, log: &Log) -> Mounted<Outer> {
    let outer = Mounted::<Outer>::build(
        &app.dispatch(),
        |_| ParentMsg::Row(RowMsg::Rename("from outer".into())),
        |dispatch| {
            let inner = Mounted::<Row>::build(
                dispatch,
                OuterMsg::Inner,
                mount_row(log, "inner"),
                unmount_row,
            )?;
            Ok::<_, Infallible>(OuterView {
                inner,
                rows: Keyed::new(dispatch, OuterMsg::Row, unmount_row),
                log: log.clone(),
            })
        },
        unmount_outer,
    )
    .expect("mount");
    let rows = [row(1, "a", log), row(2, "b", log)];
    let mut names = ["a", "b"].into_iter();
    outer
        .view()
        .rows
        .sync_with(
            rows.iter().map(|row| (row.id, row)),
            |dispatch| mount_row(log, names.next().expect("a name"))(dispatch),
            |_| {},
        )
        .expect("sync");
    outer
}

#[test]
fn unmounting_a_parent_takes_the_children_in_its_view_down() {
    let log = Log::default();
    let app = parent(&log);
    let outer = mount_outer(&app, &log);
    let inner = outer.view().inner.view().dispatch.clone();
    let row = outer
        .view()
        .rows
        .get(&1)
        .expect("row 1")
        .view()
        .dispatch
        .clone();
    log.borrow_mut().clear();
    outer.unmount_with(|view| view.log.borrow_mut().push("remove outer".into()));
    assert_eq!(
        *log.borrow(),
        [
            "unmount outer",
            "remove outer",
            "unmount inner",
            "drop inner",
            "unmount a",
            "drop a",
            "unmount b",
            "drop b"
        ]
    );
    assert!(!inner.is_connected());
    assert!(!row.is_connected());
    assert!(app.dispatch().is_connected());
}

#[test]
fn a_parents_dispatch_being_cut_cuts_the_childrens() {
    let log = Log::default();
    let app = parent(&log);
    let outer = mount_outer(&app, &log);
    let inner = outer.view().inner.view().dispatch.clone();
    assert!(inner.is_connected());
    // 親の後片付けの間に、子の送り口からも親へ届かない。
    let check = inner.clone();
    outer.unmount_with(move |_| assert!(!check.is_connected()));
    inner.send(RowMsg::Rename("late".into()));
    assert!(app.model().got.is_empty());
}

#[test]
fn the_app_ending_does_not_call_a_nested_childs_unmount() {
    let log = Log::default();
    let app = parent(&log);
    let outer = mount_outer(&app, &log);
    drop(app);
    log.borrow_mut().clear();
    drop(outer);
    assert_eq!(*log.borrow(), ["drop inner", "drop a", "drop b"]);
}

#[test]
fn a_failing_mount_returns_its_error_and_leaves_nothing_connected() {
    let log = Log::default();
    let app = parent(&log);
    let mut kept = None;
    let result = Mounted::<Row>::build(
        &app.dispatch(),
        ParentMsg::Row,
        |dispatch| {
            kept = Some(dispatch.clone());
            Err::<RowView, _>("no widgets")
        },
        unmount_row,
    );
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
    keyed: Rc<Keyed<u32, Row>>,
    container: Container,
    /// How many views have been mounted, to name them apart.
    mounted: usize,
}

impl Fixture {
    fn new() -> Self {
        let log = Log::default();
        let app = parent(&log);
        let keyed = Rc::new(Keyed::new(&app.dispatch(), ParentMsg::Keyed, unmount_row));
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
                |step| container.apply(step),
            )
            .expect("sync");
    }

    fn take_log(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.borrow_mut())
    }

    fn dispatch(&self, id: u32) -> Dispatch<RowMsg> {
        self.row(id).view().dispatch.clone()
    }

    fn row(&self, id: u32) -> Rc<Mounted<Row>> {
        self.keyed.get(&id).expect("mounted")
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
    assert_eq!(*fixture.row(2).view().shown.borrow(), "b");
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
    assert_eq!(*fixture.row(1).view().shown.borrow(), "a");
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

impl Fixture {
    /// Syncs like `sync`, except that mounting the keys in `failing` fails.
    fn sync_failing(&mut self, rows: &[Row], failing: &[u32]) -> Result<(), String> {
        let log = self.log.clone();
        let mounted = &mut self.mounted;
        let container = &mut self.container;
        // mount はキーを受け取らないので、作る順に並べたキーから引く。
        let mut new_keys = rows
            .iter()
            .map(|row| row.id)
            .filter(|id| self.keyed.get(id).is_none())
            .collect::<Vec<_>>()
            .into_iter();
        self.keyed.sync_with(
            rows.iter().map(|row| (row.id, row)),
            |dispatch| {
                let key = new_keys.next().expect("a key for each mount");
                if failing.contains(&key) {
                    return Err(format!("no widgets for {key}"));
                }
                *mounted += 1;
                mount_row(&log, format!("v{mounted}"))(dispatch).map_err(|never| match never {})
            },
            |step| container.apply(step),
        )
    }
}

#[test]
fn a_failing_mount_leaves_that_child_out_and_places_the_rest() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    let rows = [row(1, "a", &log), row(2, "b", &log), row(3, "c", &log)];
    let result = fixture.sync_failing(&rows, &[2]);
    // 作れなかった子だけ外れ、その後ろの子は置かれて描かれる。
    assert_eq!(result, Err("no widgets for 2".to_owned()));
    assert_eq!(fixture.container.names, ["v1", "v2"]);
    assert_eq!(
        fixture.take_log(),
        [
            "mount v1",
            "insert 0 v1",
            "mount v2",
            "insert 1 v2",
            "render 1 a",
            "render 3 c"
        ]
    );
    // 次の呼び出しで、外れた子をまた作ってみる。
    fixture.sync(&rows);
    assert_eq!(fixture.container.names, ["v1", "v3", "v2"]);
    assert_eq!(fixture.keyed.len(), 3);
}

#[test]
fn a_failing_mount_does_not_leave_the_other_children_stale() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log)]);
    fixture.take_log();
    // 既存の子のモデルが変わったのと同じ回に、新しい子を作れなかった。
    let result = fixture.sync_failing(&[row(1, "a2", &log), row(2, "b", &log)], &[2]);
    assert!(result.is_err());
    assert_eq!(fixture.take_log(), ["render 1 a2"]);
    assert_eq!(*fixture.row(1).view().shown.borrow(), "a2");
}

#[test]
fn every_failing_mount_is_tried_and_the_first_error_returned() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    let rows = [row(1, "a", &log), row(2, "b", &log), row(3, "c", &log)];
    let result = fixture.sync_failing(&rows, &[1, 3]);
    assert_eq!(result, Err("no widgets for 1".to_owned()));
    assert_eq!(fixture.container.names, ["v1"]);
    assert_eq!(*fixture.row(2).view().shown.borrow(), "b");
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

/// Runs `f`, returning what it panicked with, if anything.
fn panic_of(f: impl FnOnce()) -> Option<String> {
    catch_unwind(AssertUnwindSafe(f))
        .err()
        .map(
            |panic: Box<dyn Any + Send>| match panic.downcast::<String>() {
                Ok(text) => *text,
                Err(panic) => (*panic.downcast::<&str>().expect("a text")).to_owned(),
            },
        )
}

/// Reads `keyed` as outside code would: its length, and whether a key is there.
fn read(keyed: &Keyed<u32, Row>, key: u32) -> (usize, bool) {
    (keyed.len(), keyed.get(&key).is_some())
}

#[test]
fn children_can_be_read_from_place_and_mount() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    let keyed = fixture.keyed.clone();
    let rows = [row(1, "a", &log), row(2, "b", &log)];
    let seen = RefCell::new(Vec::new());
    keyed
        .sync_with(
            rows.iter().map(|row| (row.id, row)),
            |dispatch| {
                seen.borrow_mut().push(("mount", read(&keyed, 1)));
                mount_row(&log, "v")(dispatch)
            },
            |step| {
                seen.borrow_mut().push(("place", read(&keyed, 1)));
                fixture.container.apply(step);
            },
        )
        .expect("sync");
    assert_eq!(
        *seen.borrow(),
        [
            ("mount", (0, false)),
            ("place", (1, true)),
            ("mount", (1, true)),
            ("place", (2, true))
        ]
    );
}

#[test]
fn children_can_be_read_from_render_and_unmount() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(2, "b", &log)]);
    let seen = Rc::new(RefCell::new(Vec::new()));
    for id in [1, 2] {
        let seen = seen.clone();
        let keyed: Weak<Keyed<u32, Row>> = Rc::downgrade(&fixture.keyed);
        *fixture.row(id).view().during.borrow_mut() = Some(Box::new(move || {
            seen.borrow_mut()
                .push(keyed.upgrade().expect("keyed").len());
        }));
    }
    fixture.sync(&[row(1, "a", &log)]);
    // 2 の後片付けはまだ 2 件の間に、1 の描画は外れて 1 件になってから。
    assert_eq!(*seen.borrow(), [2, 1]);
}

#[test]
fn a_sync_from_inside_a_sync_does_nothing() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    let keyed = fixture.keyed.clone();
    let rows = [row(1, "a", &log), row(2, "b", &log)];
    let other = [row(9, "other", &log)];
    let nested = Cell::new(0);
    keyed
        .sync_with(
            rows.iter().map(|row| (row.id, row)),
            |dispatch| mount_row(&log, format!("w{}", keyed.len()))(dispatch),
            |step| {
                fixture.container.apply(step);
                let result = keyed.sync_with(
                    other.iter().map(|row| (row.id, row)),
                    |_| Err("the nested sync must not mount"),
                    |_| panic!("the nested sync must not place"),
                );
                assert_eq!(result, Ok(()));
                nested.set(nested.get() + 1);
            },
        )
        .expect("sync");
    assert_eq!(nested.get(), 2);
    assert_eq!(keyed.len(), 2);
    assert!(keyed.get(&9).is_none());
    // 終われば、また同期できる。
    fixture.sync(&[row(1, "a", &log)]);
    assert_eq!(keyed.len(), 1);
}

#[test]
fn a_child_held_across_a_sync_is_taken_down_on_time() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log)]);
    let held = fixture.row(1);
    fixture.take_log();
    fixture.sync(&[]);
    // 持っていても後片付けと取り外しはその場で行い、破棄だけが手放すまで待つ。
    assert_eq!(fixture.take_log(), ["unmount v1", "remove 0 v1"]);
    assert!(!held.view().dispatch.is_connected());
    drop(held);
    assert_eq!(fixture.take_log(), ["drop v1"]);
}

#[test]
fn a_panicking_unmount_still_removes_the_root() {
    let log = Log::default();
    let app = parent(&log);
    let mounted = app.view().borrow_mut().take().expect("mounted");
    mounted.view().explode.set(Some("unmount"));
    log.borrow_mut().clear();
    let panic = panic_of(|| {
        mounted.unmount_with(|view| view.log.borrow_mut().push("remove root".into()));
    });
    assert_eq!(panic.as_deref(), Some("unmount panicked (intended)"));
    assert_eq!(*log.borrow(), ["unmount row", "remove root", "drop row"]);
}

#[test]
fn a_panicking_unmount_does_not_stop_the_other_children() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(2, "b", &log), row(3, "c", &log)]);
    fixture.row(1).view().explode.set(Some("unmount"));
    fixture.take_log();
    let panic = panic_of(|| fixture.sync(&[row(3, "c2", &log)]));
    assert_eq!(panic.as_deref(), Some("unmount panicked (intended)"));
    // panic した子も外れ、残りの後片付けと描画も済んでから panic が続く。
    assert_eq!(
        fixture.take_log(),
        [
            "unmount v1",
            "remove 0 v1",
            "drop v1",
            "unmount v2",
            "remove 0 v2",
            "drop v2",
            "render 3 c2"
        ]
    );
    assert_eq!(fixture.container.names, ["v3"]);
    assert_eq!(fixture.keyed.len(), 1);
}

#[test]
fn a_panicking_render_does_not_stop_the_other_children() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(2, "b", &log)]);
    fixture.row(1).view().explode.set(Some("render"));
    let panic = panic_of(|| fixture.sync(&[row(1, "a2", &log), row(2, "b2", &log)]));
    assert_eq!(panic.as_deref(), Some("render panicked (intended)"));
    assert_eq!(*fixture.row(2).view().shown.borrow(), "b2");
}

#[test]
fn a_panicking_mount_does_not_stop_the_other_children() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    let keyed = fixture.keyed.clone();
    let rows = [row(1, "a", &log), row(2, "b", &log)];
    let mut count = 0;
    let container = &mut fixture.container;
    let panic = panic_of(|| {
        let _ = keyed.sync_with(
            rows.iter().map(|row| (row.id, row)),
            |dispatch| {
                count += 1;
                if count == 1 {
                    panic!("mount panicked (intended)");
                }
                mount_row(&log, "w")(dispatch)
            },
            |step| container.apply(step),
        );
    });
    assert_eq!(panic.as_deref(), Some("mount panicked (intended)"));
    assert_eq!(fixture.container.names, ["w"]);
    // 次の呼び出しで、外れた子をまた作ってみる。
    fixture.sync(&rows);
    assert_eq!(fixture.container.names, ["v1", "w"]);
}

/// A container that behaves as `Stack` does: a position out of range does nothing
/// for a removal, and appends for an insertion.
#[derive(Default)]
struct LikeStack(Vec<String>);

impl LikeStack {
    fn apply(&mut self, step: Step<'_, RowView>) {
        match step {
            Step::Insert { index, view } => {
                let index = index.min(self.0.len());
                self.0.insert(index, view.name.clone());
            }
            Step::Remove { index, .. } => {
                if index < self.0.len() {
                    self.0.remove(index);
                }
            }
        }
    }
}

/// Names the views in the order they are mounted.
fn mount_numbered<'a>(
    log: &'a Log,
    count: &'a Cell<u32>,
) -> impl FnMut(&Dispatch<RowMsg>) -> Result<RowView, Infallible> + 'a {
    move |dispatch| {
        count.set(count.get() + 1);
        mount_row(log, format!("v{}", count.get()))(dispatch)
    }
}

#[test]
fn a_panic_in_place_is_put_right_by_the_next_sync() {
    // 1 回の同期の中のすべての呼び出しで、容器へ反映する前と後の両方で panic させる。
    for applied_first in [false, true] {
        for fail_at in 0.. {
            let log = Log::default();
            let app = parent(&log);
            let keyed = Keyed::<u32, Row>::new(&app.dispatch(), ParentMsg::Keyed, unmount_row);
            let stack = RefCell::new(LikeStack::default());
            let count = Cell::new(0);
            let names = |ids: &[u32]| -> Vec<String> {
                ids.iter()
                    .map(|id| keyed.get(id).expect("mounted").view().name.clone())
                    .collect()
            };

            let before = [1, 2, 3, 4].map(|id| row(id, "before", &log));
            keyed
                .sync_with(
                    before.iter().map(|row| (row.id, row)),
                    mount_numbered(&log, &count),
                    |step| stack.borrow_mut().apply(step),
                )
                .expect("sync");
            let kept = names(&[3, 1]);
            let gone = names(&[2, 4]);

            // 外す (2, 4)、動かす (3)、作る (5, 6) がすべて入る同期。
            let after = [5, 3, 1, 6].map(|id| row(id, "after", &log));
            let calls = Cell::new(0);
            let panicked = panic_of(|| {
                keyed
                    .sync_with(
                        after.iter().map(|row| (row.id, row)),
                        mount_numbered(&log, &count),
                        |step| {
                            let call = calls.get();
                            calls.set(call + 1);
                            if call != fail_at {
                                stack.borrow_mut().apply(step);
                            } else {
                                if applied_first {
                                    stack.borrow_mut().apply(step);
                                }
                                panic!("place panicked (intended)");
                            }
                        },
                    )
                    .expect("sync");
            });
            let Some(panicked) = panicked else {
                assert!(fail_at > 0, "少なくとも 1 回は place を呼ぶ");
                break;
            };
            assert_eq!(panicked, "place panicked (intended)");

            keyed
                .sync_with(
                    after.iter().map(|row| (row.id, row)),
                    mount_numbered(&log, &count),
                    |step| stack.borrow_mut().apply(step),
                )
                .expect("sync");

            let context = format!("applied_first={applied_first} fail_at={fail_at}");
            assert_eq!(stack.borrow().0, names(&[5, 3, 1, 6]), "{context}");
            assert_eq!(keyed.len(), 4, "{context}");
            assert_eq!(names(&[3, 1]), kept, "残る子は作り直さない: {context}");
            let log = log.borrow();
            for name in &gone {
                let line = format!("unmount {name}");
                let times = log.iter().filter(|entry| **entry == line).count();
                assert_eq!(times, 1, "{name} の後片付けは 1 回: {context}");
            }
        }
    }
}

#[test]
fn a_key_coming_back_after_a_panic_in_place_is_mounted_again() {
    let log = Log::default();
    let app = parent(&log);
    let keyed = Keyed::<u32, Row>::new(&app.dispatch(), ParentMsg::Keyed, unmount_row);
    let stack = RefCell::new(LikeStack::default());
    let count = Cell::new(0);
    let rows = [row(1, "a", &log)];
    let sync = |rows: &[Row]| {
        keyed
            .sync_with(
                rows.iter().map(|row| (row.id, row)),
                mount_numbered(&log, &count),
                |step| stack.borrow_mut().apply(step),
            )
            .expect("sync");
    };
    sync(&rows);
    let panic = panic_of(|| {
        let _ = keyed.sync_with(std::iter::empty(), mount_numbered(&log, &count), |_| {
            panic!("place panicked (intended)")
        });
    });
    assert!(panic.is_some());
    // 後片付けの済んだ子を同じキーで使い回すと、送り口が切れたままになる。
    sync(&rows);
    let mounted = keyed.get(&1).expect("mounted");
    assert_eq!(mounted.view().name, "v2");
    assert!(mounted.view().dispatch.is_connected());
    assert_eq!(stack.borrow().0, ["v2"]);
}

#[test]
fn moving_the_first_child_to_the_end_moves_only_that_child() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(2, "b", &log), row(3, "c", &log)]);
    fixture.take_log();
    fixture.sync(&[row(2, "b", &log), row(3, "c", &log), row(1, "a", &log)]);
    assert_eq!(
        fixture.take_log(),
        [
            "remove 0 v1",
            "insert 2 v1",
            "render 2 b",
            "render 3 c",
            "render 1 a"
        ]
    );
    assert_eq!(fixture.container.names, ["v2", "v3", "v1"]);
}

/// Every order of `0..n`.
fn permutations(n: u32) -> Vec<Vec<u32>> {
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut all = Vec::new();
    for shorter in permutations(n - 1) {
        for at in 0..=shorter.len() {
            let mut order = shorter.clone();
            order.insert(at, n - 1);
            all.push(order);
        }
    }
    all
}

/// The length of a longest increasing subsequence, the slow and obvious way.
fn longest_run(values: &[u32]) -> usize {
    let mut lengths: Vec<usize> = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let before = (0..index)
            .filter(|&earlier| values[earlier] < *value)
            .map(|earlier| lengths[earlier])
            .max();
        lengths.push(before.unwrap_or(0) + 1);
    }
    lengths.into_iter().max().unwrap_or(0)
}

#[test]
fn every_reordering_ends_in_order_with_as_few_moves_as_possible() {
    for n in 0..=6 {
        for order in permutations(n) {
            let log = Log::default();
            let app = parent(&log);
            let keyed = Keyed::<u32, Row>::new(&app.dispatch(), ParentMsg::Keyed, unmount_row);
            let stack = RefCell::new(LikeStack::default());
            let count = Cell::new(0);
            let before: Vec<Row> = (0..n).map(|id| row(id, "before", &log)).collect();
            keyed
                .sync_with(
                    before.iter().map(|row| (row.id, row)),
                    mount_numbered(&log, &count),
                    |step| stack.borrow_mut().apply(step),
                )
                .expect("sync");
            let names = |ids: &[u32]| -> Vec<String> {
                ids.iter()
                    .map(|id| keyed.get(id).expect("mounted").view().name.clone())
                    .collect()
            };
            let mounted = count.get();
            let after: Vec<Row> = order.iter().map(|&id| row(id, "after", &log)).collect();
            let changes = Cell::new(0);
            keyed
                .sync_with(
                    after.iter().map(|row| (row.id, row)),
                    mount_numbered(&log, &count),
                    |step| {
                        changes.set(changes.get() + 1);
                        stack.borrow_mut().apply(step);
                    },
                )
                .expect("sync");
            assert_eq!(stack.borrow().0, names(&order), "{order:?}");
            assert_eq!(count.get(), mounted, "作り直さない: {order:?}");
            let moved = order.len() - longest_run(&order);
            assert_eq!(changes.get(), 2 * moved, "{order:?}");
            // 描くのは、それぞれ自分のモデルで。
            for model in &after {
                let shown = keyed.get(&model.id).expect("mounted");
                assert_eq!(*shown.view().shown.borrow(), "after", "{order:?}");
            }
        }
    }
}

#[test]
fn moves_mounts_and_a_failing_mount_together_keep_each_child_with_its_model() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    fixture.sync(&[row(1, "a", &log), row(2, "b", &log), row(3, "c", &log)]);
    let rows = [
        row(3, "c2", &log),
        row(9, "fails", &log),
        row(4, "d", &log),
        row(1, "a2", &log),
    ];
    let result = fixture.sync_failing(&rows, &[9]);
    assert_eq!(result, Err("no widgets for 9".to_owned()));
    assert_eq!(fixture.container.names.len(), 3);
    assert_eq!(fixture.container.names[0], fixture.row(3).view().name);
    assert_eq!(fixture.container.names[1], fixture.row(4).view().name);
    assert_eq!(fixture.container.names[2], fixture.row(1).view().name);
    assert_eq!(*fixture.row(3).view().shown.borrow(), "c2");
    assert_eq!(*fixture.row(4).view().shown.borrow(), "d");
    assert_eq!(*fixture.row(1).view().shown.borrow(), "a2");
    // 次の呼び出しで、外れた子だけを足す。
    fixture.sync(&rows);
    assert_eq!(fixture.container.names.len(), 4);
    assert_eq!(fixture.container.names[1], fixture.row(9).view().name);
}
