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

/// A row's `unmount`. Its dispatch is already disconnected, so what it sends is
/// dropped.
fn unmount_row(view: &RowView) {
    assert!(!view.dispatch.is_connected());
    view.dispatch.send(RowMsg::Rename("from unmount".into()));
    view.log.borrow_mut().push(format!("unmount {}", view.name));
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
    let row = outer.view().rows.view(&1).expect("row 1").dispatch.clone();
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
    keyed: Keyed<u32, Row>,
    container: Container,
    /// How many views have been mounted, to name them apart.
    mounted: usize,
}

impl Fixture {
    fn new() -> Self {
        let log = Log::default();
        let app = parent(&log);
        let keyed = Keyed::new(&app.dispatch(), ParentMsg::Keyed, unmount_row);
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
            .filter(|id| self.keyed.view(id).is_none())
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
    assert_eq!(*fixture.keyed.view(&1).expect("row 1").shown.borrow(), "a2");
}

#[test]
fn every_failing_mount_is_tried_and_the_first_error_returned() {
    let mut fixture = Fixture::new();
    let log = fixture.log.clone();
    let rows = [row(1, "a", &log), row(2, "b", &log), row(3, "c", &log)];
    let result = fixture.sync_failing(&rows, &[1, 3]);
    assert_eq!(result, Err("no widgets for 1".to_owned()));
    assert_eq!(fixture.container.names, ["v1"]);
    assert_eq!(*fixture.keyed.view(&2).expect("row 2").shown.borrow(), "b");
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
