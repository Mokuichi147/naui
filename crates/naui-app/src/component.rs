//! Components: parts of an app with their own model, messages, `update` and widgets.

use std::any::Any;
use std::borrow::Borrow;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::hash::Hash;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::rc::{Rc, Weak};

use naui::{Ui, Widget};

use crate::error::report;
use crate::{Dispatch, Program, ReportKind, Running};

/// A part of an app (a screen, a panel, a row) with its own model, messages,
/// `update` and widgets.
///
/// The type implementing it is the component's model, and [`Program`] gives it its
/// messages, `update` and `render`. The model lives in the parent's model, as a
/// field or in a collection, so the parent's `update` stays plain Rust that can be
/// tested without a window, children included. The widgets live in the parent's
/// view, in a [`Mounted`] or in [`Children`].
///
/// A component goes through:
///
/// 1. **`mount`**: builds its widgets once and wires their callbacks to the
///    [`Dispatch`] it receives. What that dispatch sends is wrapped into the
///    parent's message (`Message::Editor(editor::Message::Saved)`), so it goes
///    through the same queue and reaches the parent's `update`.
/// 2. **`update`**: the parent's `update` hands the child's messages on to it, and
///    its effects back up
///    (`Message::Editor(message) => self.editor.update(message).map(Message::Editor)`).
///    It can act on some of them itself first, which is how a child tells its parent
///    something. A child that starts work and can go away before it ends is kept in a
///    [`Scoped`](crate::Scoped), which cancels that work when dropped.
/// 3. **`render`**: called with the child's model through [`Mounted::render`] or
///    [`Children::sync`], from the parent's `render`.
/// 4. **`unmount`**: when the parent takes it down, in this order: its dispatch is
///    disconnected (what its widgets send from then on, including notifications
///    raised by removing them, is dropped), [`unmount`](Self::unmount) is called
///    with the widgets still in place, the parent removes the root widget from its
///    container, and the widgets are dropped.
///
/// The parent places the root widget and removes it: a component does not know
/// which container it is in, and naui's containers remove children by position or
/// cell rather than by widget. [`Mounted::unmount`] takes the removal as an
/// argument, and [`Children::sync`] asks for it through [`Change::Remove`], so the
/// two always happen together and in this order.
///
/// A component nested in another's view is taken down with it: dropping a [`Mounted`]
/// or [`Children`] takes what it holds down too, so a parent has nothing to do for
/// its children. They go when the parent's widgets are dropped, so a nested child's
/// [`unmount`](Self::unmount) runs after the parent's, once the parent's root widget
/// is out of its container. Its dispatch is already disconnected by then, along with
/// the parent's. Its root stays in the parent's containers, which go away with it.
///
/// ```no_run
/// use naui::{Orientation, Ui};
/// use naui_app::{Component, Dispatch, Effect, Program};
///
/// #[derive(Default)]
/// struct Search {
///     query: String,
/// }
///
/// enum SearchMessage {
///     Changed(String),
///     Submit,
/// }
///
/// struct SearchView {
///     root: naui::Stack,
///     field: naui::SearchInput,
///     go: naui::Button,
/// }
///
/// impl Program for Search {
///     type Message = SearchMessage;
///     type View = SearchView;
///
///     fn update(&mut self, message: SearchMessage) -> Effect<SearchMessage> {
///         match message {
///             SearchMessage::Changed(text) => self.query = text,
///             // The parent acts on it (see below).
///             SearchMessage::Submit => {}
///         }
///         Effect::none()
///     }
///
///     fn render(&self, view: &SearchView) {
///         view.field.set_text(&self.query);
///         view.go.set_enabled(!self.query.is_empty());
///     }
/// }
///
/// impl Component for Search {
///     fn mount(ui: &Ui, dispatch: &Dispatch<SearchMessage>) -> naui::Result<SearchView> {
///         let root = ui.stack(Orientation::Horizontal)?;
///         let field = ui.search_input()?;
///         field.on_change(dispatch.with(|text: &str| SearchMessage::Changed(text.to_owned())));
///         let go = ui.button("Search")?;
///         go.on_click(dispatch.to(|| SearchMessage::Submit));
///         root.append(&field);
///         root.append(&go);
///         Ok(SearchView { root, field, go })
///     }
///
///     fn root(view: &SearchView) -> &dyn naui::Widget {
///         &view.root
///     }
/// }
///
/// // In the parent:
/// enum Message {
///     Search(SearchMessage),
/// }
///
/// struct App {
///     search: Search,
///     results: Vec<String>,
/// }
///
/// impl App {
///     fn update(&mut self, message: Message) -> Effect<Message> {
///         match message {
///             Message::Search(SearchMessage::Submit) => {
///                 self.results = vec![format!("Results for {}", self.search.query)];
///                 Effect::none()
///             }
///             Message::Search(message) => self.search.update(message).map(Message::Search),
///         }
///     }
/// }
/// ```
pub trait Component: Program {
    /// Builds the widgets once, wiring their callbacks to `dispatch`.
    ///
    /// It does not see the model: `render` runs right after and sets the widgets
    /// from it, as for the app's own view.
    fn mount(ui: &Ui, dispatch: &Dispatch<Self::Message>) -> naui::Result<Self::View>;

    /// The widget the parent places in a container. Usually the outermost container
    /// built in `mount`.
    fn root(view: &Self::View) -> &dyn Widget;

    /// Cleans up before the root widget is removed from its container: undoes what
    /// `mount` or `render` did outside its own widgets (items it added to the
    /// window's toolbar, a popover it opened, media it started).
    ///
    /// The dispatch is already disconnected, so what this sends is dropped. The
    /// model is not given: by the time the parent takes a child down, its `update`
    /// has usually removed the model already. Does nothing by default.
    ///
    /// It runs once, however the component is taken down, except in two cases where
    /// the widgets should not be touched: when the app has already ended (the OS
    /// takes back what is left), and while a panic is unwinding. A component that has
    /// to save something when the app ends should do that from a message instead.
    ///
    /// If it panics, the root widget is still removed, and the panic carries on from
    /// there.
    fn unmount(view: &Self::View) {
        let _ = view;
    }
}

/// One mounted component: its widgets, and the link that keeps its [`Dispatch`]
/// connected.
///
/// Keep it in the parent's view. For a component that is always there, mount it
/// in `build` and place its [`root`](Self::root):
///
/// ```no_run
/// # use naui::{Orientation, Ui};
/// # use naui_app::{Component, Dispatch, Effect, Mounted, Program};
/// # struct Search;
/// # impl Program for Search {
/// #     type Message = ();
/// #     type View = naui::Stack;
/// #     fn update(&mut self, _: ()) -> Effect<()> { Effect::none() }
/// #     fn render(&self, _: &naui::Stack) {}
/// # }
/// # impl Component for Search {
/// #     fn mount(ui: &Ui, _: &Dispatch<()>) -> naui::Result<naui::Stack> { ui.stack(Orientation::Vertical) }
/// #     fn root(view: &naui::Stack) -> &dyn naui::Widget { view }
/// # }
/// # enum Message { Search(()) }
/// # fn build(ui: &Ui, dispatch: &Dispatch<Message>, stack: &naui::Stack) -> naui::Result<()> {
/// let search = Mounted::<Search>::new(ui, dispatch, Message::Search)?;
/// stack.append(search.root());
/// // In the parent's `render`: search.render(&self.search);
/// # Ok(())
/// # }
/// ```
///
/// To switch what a single-child container shows (one screen for another), mount
/// the new one, [`unmount`](Self::unmount) the old one with a removal that does
/// nothing, and then replace the child with `set_child`: the old one's
/// [`Component::unmount`] runs while its widgets are still shown.
///
/// Dropping it takes the component down in the same way, except that nothing removes
/// the root widget from its container. That is what a parent's own removal does for
/// the children in its view. When the root has to leave a container that stays, call
/// [`unmount`](Self::unmount).
pub struct Mounted<C: Program> {
    view: C::View,
    /// While this is alive, the component's dispatch sends. Taken when it is taken
    /// down, so that happens once.
    link: RefCell<Option<Rc<()>>>,
    dispatch: Dispatch<C::Message>,
    unmount: fn(&C::View),
}

impl<C: Component> Mounted<C> {
    /// Mounts `C`: builds its widgets with [`Component::mount`], giving it a dispatch
    /// that wraps its messages with `wrap` into the parent's.
    ///
    /// It is not rendered yet: call [`render`](Self::render) from the parent's
    /// `render`, which runs right after `build` too.
    pub fn new<M: 'static>(
        ui: &Ui,
        dispatch: &Dispatch<M>,
        wrap: impl Fn(C::Message) -> M + 'static,
    ) -> naui::Result<Self> {
        Self::build(
            dispatch,
            wrap,
            |dispatch| C::mount(ui, dispatch),
            C::unmount,
        )
    }

    /// The widget to place in a container.
    pub fn root(&self) -> &dyn Widget {
        C::root(&self.view)
    }

    /// Takes the component down: disconnects its dispatch, calls
    /// [`Component::unmount`], calls `remove` with the root widget to take it out
    /// of its container, and drops the widgets.
    pub fn unmount(self, remove: impl FnOnce(&dyn Widget)) {
        self.unmount_with(|view| remove(C::root(view)));
    }
}

impl<C: Program> Mounted<C> {
    /// Builds the view with `mount`, which receives the component's dispatch.
    pub(crate) fn build<M: 'static, E>(
        dispatch: &Dispatch<M>,
        wrap: impl Fn(C::Message) -> M + 'static,
        mount: impl FnOnce(&Dispatch<C::Message>) -> Result<C::View, E>,
        unmount: fn(&C::View),
    ) -> Result<Self, E> {
        let link = Rc::new(());
        let dispatch = dispatch.wrapped(wrap, Some(Rc::downgrade(&link)));
        Self::with_link(link, dispatch, mount, unmount)
    }

    /// Builds the view with `mount`, given a dispatch that sends while `link` is alive.
    fn with_link<E>(
        link: Rc<()>,
        dispatch: Dispatch<C::Message>,
        mount: impl FnOnce(&Dispatch<C::Message>) -> Result<C::View, E>,
        unmount: fn(&C::View),
    ) -> Result<Self, E> {
        let view = mount(&dispatch)?;
        Ok(Self {
            view,
            link: RefCell::new(Some(link)),
            dispatch,
            unmount,
        })
    }

    /// The widgets built by `mount`.
    pub fn view(&self) -> &C::View {
        &self.view
    }

    /// Sets the widgets to match `model`, with the component's `render`.
    pub fn render(&self, model: &C) {
        model.render(&self.view);
    }

    /// Takes the component down, with `remove` taking its root out of its container.
    /// The widgets are dropped on the way out.
    pub(crate) fn unmount_with(self, remove: impl FnOnce(&C::View)) {
        let cleaned_up = self.clean_up();
        remove(&self.view);
        if let Err(panic) = cleaned_up {
            resume_unwind(panic);
        }
    }

    /// Disconnects the dispatch and calls `unmount`, once. A panic in `unmount` is
    /// handed back, so that the caller can still remove the root widget.
    fn clean_up(&self) -> std::thread::Result<()> {
        let Some(link) = self.link.borrow_mut().take() else {
            return Ok(());
        };
        drop(link);
        // アプリが終わったあとはウィジェットに触らない。巻き戻しの途中で後片付けが
        // また panic すると abort になるので、そのときも呼ばない。
        if self.dispatch.mailbox.app_is_there() && !std::thread::panicking() {
            catch_unwind(AssertUnwindSafe(|| (self.unmount)(&self.view)))
        } else {
            Ok(())
        }
    }

    fn is_taken_down(&self) -> bool {
        self.link.borrow().is_none()
    }
}

impl<C: Program> Drop for Mounted<C> {
    fn drop(&mut self) {
        if let Err(panic) = self.clean_up() {
            resume_unwind(panic);
        }
    }
}

/// What [`Children::sync`] asks the parent to do with its container.
///
/// The positions count the children of this [`Children`] only, in the order of the
/// models given, as the container holds them at the moment of the call. Apply each
/// change as it comes: the next one's position assumes this one was applied.
#[derive(Clone, Copy)]
pub enum Change<'a> {
    /// Insert `root` at `index`: a child just mounted, or one moved there.
    Insert { index: usize, root: &'a dyn Widget },
    /// Remove the child at `index`: one being unmounted (its
    /// [`Component::unmount`] has run), or one about to be inserted elsewhere.
    Remove { index: usize, root: &'a dyn Widget },
}

/// A [`Change`], with the view instead of the root widget.
pub(crate) enum Step<'a, V> {
    Insert { index: usize, view: &'a V },
    Remove { index: usize, view: &'a V },
}

/// A mounted child, under its key.
struct Entry<K, C: Program> {
    key: K,
    mounted: Rc<Mounted<C>>,
}

/// Makes the dispatch of the child under a key, sending while the link is alive.
type MakeDispatch<K, M> = Box<dyn Fn(&K, Weak<()>) -> Dispatch<M>>;

/// The lifecycle part of [`Children`], which does not need a window.
pub(crate) struct Keyed<K, C: Program> {
    entries: RefCell<Vec<Entry<K, C>>>,
    dispatch: MakeDispatch<K, C::Message>,
    unmount: fn(&C::View),
    /// Whether `sync_with` is running. A call made meanwhile does nothing.
    syncing: Cell<bool>,
    /// Whether `place` panicked, so that the container may not match `entries`.
    /// `entries` still lists everything the container may hold.
    broken: Cell<bool>,
    /// The keys whose failed mount has been reported. Each `render` tries them again,
    /// and telling every time would make a handler that sends a message go round forever.
    failed: RefCell<HashSet<K>>,
    /// Whether the repeated keys of the last call were reported, for the same reason.
    repeat_reported: Cell<bool>,
}

/// Runs `f`, keeping a panic for later, so that the other children still get their
/// turn. Without unwinding (`panic = "abort"`, wasm) the app stops here.
pub(crate) fn attempt<R>(
    panicked: &mut Option<Box<dyn Any + Send>>,
    f: impl FnOnce() -> R,
) -> Option<R> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(panic) => {
            panicked.get_or_insert(panic);
            None
        }
    }
}

/// Marks a longest increasing subsequence of `values`, which are all different.
fn longest_increasing(values: &[usize]) -> Vec<bool> {
    // tails[n] は、長さ n + 1 の増加列のうち末尾が最も小さいものの末尾の位置。
    let mut tails: Vec<usize> = Vec::new();
    let mut previous: Vec<Option<usize>> = vec![None; values.len()];
    for (index, value) in values.iter().enumerate() {
        let length = tails.partition_point(|&tail| values[tail] < *value);
        if length > 0 {
            previous[index] = Some(tails[length - 1]);
        }
        if length == tails.len() {
            tails.push(index);
        } else {
            tails[length] = index;
        }
    }
    let mut marked = vec![false; values.len()];
    let mut at = tails.last().copied();
    while let Some(index) = at {
        marked[index] = true;
        at = previous[index];
    }
    marked
}

impl<K: Clone + Eq + Hash + 'static, C: Program> Keyed<K, C> {
    pub(crate) fn new<M: 'static>(
        dispatch: &Dispatch<M>,
        wrap: impl Fn(K, C::Message) -> M + 'static,
        unmount: fn(&C::View),
    ) -> Self {
        let parent = dispatch.clone();
        let wrap = Rc::new(wrap);
        Self {
            entries: RefCell::new(Vec::new()),
            dispatch: Box::new(move |key, link| {
                let key = key.clone();
                let wrap = wrap.clone();
                parent.wrapped(move |message| wrap(key.clone(), message), Some(link))
            }),
            unmount,
            syncing: Cell::new(false),
            broken: Cell::new(false),
            failed: RefCell::new(HashSet::new()),
            repeat_reported: Cell::new(false),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    pub(crate) fn get(&self, key: &K) -> Option<Rc<Mounted<C>>> {
        self.entries
            .borrow()
            .iter()
            .find(|entry| entry.key == *key)
            .map(|entry| entry.mounted.clone())
    }

    // 外のコード (mount / unmount / render / place) を呼ぶ間は entries を借りない。
    // 呼ばれた先から len や get を呼べるようにするため。entries を変えるのはここだけ
    // で、入れ子の呼び出しは何もしないので、借り直しても位置はずれない。
    pub(crate) fn sync_with<'a, E: Display>(
        &self,
        models: impl IntoIterator<Item = (K, &'a C)>,
        mut mount: impl FnMut(&Dispatch<C::Message>) -> Result<C::View, E>,
        mut place: impl FnMut(Step<'_, C::View>),
    ) where
        C: 'a,
    {
        if self.syncing.replace(true) {
            return;
        }
        let _syncing = Running(&self.syncing);

        let mut wanted_keys = HashSet::new();
        let mut repeated = false;
        let wanted: Vec<(K, &C)> = models
            .into_iter()
            .filter(|(key, _)| {
                let new = wanted_keys.insert(key.clone());
                repeated |= !new;
                new
            })
            .collect();
        if repeated && !self.repeat_reported.get() {
            report(
                ReportKind::RepeatedKey,
                "Children::sync got the same key twice; ignoring the later ones",
            );
        }
        self.repeat_reported.set(repeated);
        self.failed
            .borrow_mut()
            .retain(|key| wanted_keys.contains(key));

        let mut apply = |step: Step<'_, C::View>| {
            self.broken.set(true);
            place(step);
            self.broken.set(false);
        };
        let mut panicked = None;

        // 前回 place が panic したなら、容器に何が残っているか分からない。entries は
        // 容器に残りうるものをすべて含むので、全部外して (範囲外は何も起きない)
        // 入れ直せば、entries と容器がそろう。
        if self.broken.get() {
            let all: Vec<_> = self
                .entries
                .borrow()
                .iter()
                .map(|entry| entry.mounted.clone())
                .collect();
            for (index, mounted) in all.iter().enumerate().rev() {
                apply(Step::Remove {
                    index,
                    view: mounted.view(),
                });
            }
            for (index, mounted) in all.iter().enumerate() {
                apply(Step::Insert {
                    index,
                    view: mounted.view(),
                });
            }
        }

        // 1. なくなったものを外す。位置は外した時点の並びで数える。place が panic
        // したものは entries に残す。後片付けは済んでいるので、次に来ても同じ子として
        // 使わず、外し直す。
        let mut index = 0;
        loop {
            let (key_is_wanted, mounted) = match self.entries.borrow().get(index) {
                Some(entry) => (wanted_keys.contains(&entry.key), entry.mounted.clone()),
                None => break,
            };
            if key_is_wanted && !mounted.is_taken_down() {
                index += 1;
                continue;
            }
            if let Err(panic) = mounted.clean_up() {
                panicked.get_or_insert(panic);
            }
            apply(Step::Remove {
                index,
                view: mounted.view(),
            });
            self.entries.borrow_mut().remove(index);
        }

        // 2. 求める並びへそろえる。順番がすでに合っている最長の並びは動かさず、残りだけ
        // を動かす。動いた子はキーボードのフォーカスを失うので、巻き添えを減らすため。
        let unchanged = {
            let entries = self.entries.borrow();
            entries.len() == wanted.len()
                && entries
                    .iter()
                    .zip(&wanted)
                    .all(|(entry, (key, _))| entry.key == *key)
        };
        let mut placed: Vec<&C> = Vec::with_capacity(wanted.len());
        if unchanged {
            placed.extend(wanted.iter().map(|(_, model)| *model));
        } else {
            let (staying, moving) = {
                let order: HashMap<&K, usize> = wanted
                    .iter()
                    .enumerate()
                    .map(|(position, (key, _))| (key, position))
                    .collect();
                let entries = self.entries.borrow();
                let positions: Vec<usize> = entries.iter().map(|entry| order[&entry.key]).collect();
                let mut staying = HashSet::new();
                let mut moving = HashSet::new();
                for (entry, stays) in entries.iter().zip(longest_increasing(&positions)) {
                    if stays { &mut staying } else { &mut moving }.insert(entry.key.clone());
                }
                (staying, moving)
            };
            // 求める順に、ひとつ前の子のすぐ後ろへ置いていく。next はその位置で、前の子
            // と、まだ動かしていない子のあいだを指す。作れなかった子は飛ばす。
            let mut next = 0;
            for (key, model) in &wanted {
                if staying.contains(key) {
                    next += self.entries.borrow()[next..]
                        .iter()
                        .position(|entry| entry.key == *key)
                        .expect("a staying key is after the ones placed")
                        + 1;
                } else if moving.contains(key) {
                    // 作り直さないので、中身はそのまま残る。
                    let (from, mounted) = {
                        let entries = self.entries.borrow();
                        let from = entries
                            .iter()
                            .position(|entry| entry.key == *key)
                            .expect("a moving key is still listed");
                        (from, entries[from].mounted.clone())
                    };
                    apply(Step::Remove {
                        index: from,
                        view: mounted.view(),
                    });
                    let index = if from < next { next - 1 } else { next };
                    {
                        let mut entries = self.entries.borrow_mut();
                        let entry = entries.remove(from);
                        entries.insert(index, entry);
                    }
                    apply(Step::Insert {
                        index,
                        view: mounted.view(),
                    });
                    next = index + 1;
                } else {
                    let link = Rc::new(());
                    let dispatch = (self.dispatch)(key, Rc::downgrade(&link));
                    let mounted = attempt(&mut panicked, || {
                        Mounted::with_link(link, dispatch, &mut mount, self.unmount)
                    });
                    let mounted = match mounted {
                        Some(Ok(mounted)) => {
                            self.failed.borrow_mut().remove(key);
                            Rc::new(mounted)
                        }
                        Some(Err(error)) => {
                            // この子だけ外したまま続け、ほかの子は置いて描く。
                            // 次の呼び出しでまた作ってみる。
                            if self.failed.borrow_mut().insert(key.clone()) {
                                report(
                                    ReportKind::Mount,
                                    &format!("could not mount a child: {error}"),
                                );
                            }
                            continue;
                        }
                        None => continue,
                    };
                    self.entries.borrow_mut().insert(
                        next,
                        Entry {
                            key: key.clone(),
                            mounted: mounted.clone(),
                        },
                    );
                    apply(Step::Insert {
                        index: next,
                        view: mounted.view(),
                    });
                    next += 1;
                }
                placed.push(model);
            }
        }

        // 3. それぞれを描く。
        let all: Vec<_> = self
            .entries
            .borrow()
            .iter()
            .map(|entry| entry.mounted.clone())
            .collect();
        debug_assert_eq!(all.len(), placed.len());
        for (mounted, model) in all.iter().zip(placed) {
            attempt(&mut panicked, || mounted.render(model));
        }

        if let Some(panic) = panicked {
            resume_unwind(panic);
        }
    }
}

/// A changing set of mounted components of one type, each under a key: rows,
/// documents in tabs, items of a form.
///
/// The models live in the parent's model, in a `Vec` or the like, each with a key
/// that identifies it (an id, not a position). From the parent's `render`,
/// [`sync`](Self::sync) brings the mounted children in line with them: it mounts
/// the ones that are new, unmounts the ones that are gone, keeps the order, and
/// renders each one with its model. What to do with the container, it asks through
/// [`Change`]s.
///
/// ```no_run
/// # use naui::{Orientation, Ui};
/// # use naui_app::{Change, Children, Component, Dispatch, Effect, Program};
/// # struct Row { id: u64 }
/// # impl Program for Row {
/// #     type Message = ();
/// #     type View = naui::Stack;
/// #     fn update(&mut self, _: ()) -> Effect<()> { Effect::none() }
/// #     fn render(&self, _: &naui::Stack) {}
/// # }
/// # impl Component for Row {
/// #     fn mount(ui: &Ui, _: &Dispatch<()>) -> naui::Result<naui::Stack> { ui.stack(Orientation::Vertical) }
/// #     fn root(view: &naui::Stack) -> &dyn naui::Widget { view }
/// # }
/// # struct View { rows: Children<u64, Row>, list: naui::Stack }
/// # struct Model { rows: Vec<Row> }
/// # impl Model {
/// fn render(&self, view: &View) {
///     let rows = self.rows.iter().map(|row| (row.id, row));
///     view.rows.sync(rows, |change| match change {
///         Change::Insert { index, root } => view.list.insert(index, root),
///         Change::Remove { index, .. } => view.list.remove(index),
///     });
/// }
/// # }
/// ```
///
/// The container can hold other widgets too, as long as the positions are offset
/// by them. A message a child sent before it was unmounted can still be waiting in
/// the queue, so the parent's `update` has to expect a key that is no longer there.
///
/// Dropping it takes every child down, as dropping a [`Mounted`] does.
///
/// It is meant for a modest number of children: the rows of a form, the documents in
/// tabs. Each [`sync`](Self::sync) renders every child, and a reordering takes time
/// proportional to the number of children for each one that moves, on top of moving
/// it in the native container. For thousands of rows use [`naui::List`] or
/// [`naui::Table`], which build only the rows on screen.
pub struct Children<K, C: Program> {
    keyed: Keyed<K, C>,
    ui: Ui,
}

impl<K: Clone + Eq + Hash + 'static, C: Component> Children<K, C> {
    /// No children yet. Each child mounted later gets a dispatch that wraps its
    /// messages with `wrap`, together with its key, into the parent's: for example
    /// `Message::Row` for a variant `Row(u64, row::Message)`.
    pub fn new<M: 'static>(
        ui: &Ui,
        dispatch: &Dispatch<M>,
        wrap: impl Fn(K, C::Message) -> M + 'static,
    ) -> Self {
        Self {
            keyed: Keyed::new(dispatch, wrap, C::unmount),
            ui: ui.clone(),
        }
    }

    /// Brings the mounted children in line with `models`, given in the order they
    /// are shown, each with its key (each model as it is, or in a
    /// [`Scoped`](crate::Scoped)):
    ///
    /// 1. A child whose key is no longer there is unmounted (as
    ///    [`Mounted::unmount`] does) and [`Change::Remove`] takes it out.
    /// 2. A new key is mounted and [`Change::Insert`] places it. When the order has
    ///    changed, the longest run of children already in order stays where it is,
    ///    and each of the others is taken out and inserted again, keeping its widgets
    ///    (they lose the keyboard focus, as any moved widget does). Moving one row
    ///    from the top to the bottom moves only that row.
    /// 3. Every child is rendered with its model.
    ///
    /// When a key appears more than once, the later ones are ignored and that is
    /// reported, since it means the ids are not unique. If a mount fails, that child is
    /// left out and the rest are still placed and rendered, so the other children do not
    /// stay stale. That is reported too, and the next call tries to mount the missing
    /// child again. It returns nothing, since `render` has no caller to hand a failure
    /// to: what is reported goes to the
    /// [handler](crate::App::set_error_handler), once until it is put right, or without
    /// one to standard error.
    ///
    /// The code it calls can read [`len`](Self::len) and [`get`](Self::get), which
    /// show the children as they are at that moment. A `sync` of the same `Children`
    /// made from there does nothing: the one running decides.
    ///
    /// A panic in a child's `mount`, `unmount` or `render` does not stop the others.
    /// It carries on once the rest are done. A panic in `place` stops there, since
    /// the container is then in an unknown state. The next call puts it right by
    /// asking to remove every child, from the last, and to insert them again. That
    /// relies on the container holding only these children and ignoring positions
    /// out of range, as [`naui::Stack`] does.
    pub fn sync<'a, T: Borrow<C> + 'a>(
        &self,
        models: impl IntoIterator<Item = (K, &'a T)>,
        mut place: impl FnMut(Change<'_>),
    ) where
        C: 'a,
    {
        self.keyed.sync_with(
            models.into_iter().map(|(key, model)| (key, model.borrow())),
            |dispatch| C::mount(&self.ui, dispatch),
            |step| {
                place(match step {
                    Step::Insert { index, view } => Change::Insert {
                        index,
                        root: C::root(view),
                    },
                    Step::Remove { index, view } => Change::Remove {
                        index,
                        root: C::root(view),
                    },
                })
            },
        );
    }

    /// The child under `key`, if it is mounted.
    ///
    /// Holding it across a [`sync`](Self::sync) is harmless: a child unmounted
    /// meanwhile is still taken down at that point, and only its widgets stay until
    /// this is dropped.
    pub fn get(&self, key: &K) -> Option<Rc<Mounted<C>>> {
        self.keyed.get(key)
    }

    /// How many children are mounted.
    pub fn len(&self) -> usize {
        self.keyed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
