//! Components: parts of an app with their own model, messages, `update` and widgets.

use std::cell::{Ref, RefCell};
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::{Rc, Weak};

use naui::{Ui, Widget};

use crate::{Dispatch, Program};

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
/// 2. **`update`**: the parent's `update` hands the child's messages on to it
///    (`Message::Editor(message) => self.editor.update(message)`). It can act on
///    some of them itself first, which is how a child tells its parent something.
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
/// ```no_run
/// use naui::{Orientation, Ui};
/// use naui_app::{Component, Dispatch, Program};
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
///     fn update(&mut self, message: SearchMessage) {
///         match message {
///             SearchMessage::Changed(text) => self.query = text,
///             // The parent acts on it (see below).
///             SearchMessage::Submit => {}
///         }
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
///     fn update(&mut self, message: Message) {
///         match message {
///             Message::Search(SearchMessage::Submit) => {
///                 self.results = vec![format!("Results for {}", self.search.query)];
///             }
///             Message::Search(message) => self.search.update(message),
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
/// # use naui_app::{Component, Dispatch, Mounted, Program};
/// # struct Search;
/// # impl Program for Search {
/// #     type Message = ();
/// #     type View = naui::Stack;
/// #     fn update(&mut self, _: ()) {}
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
/// Dropping it without `unmount` disconnects its dispatch too, but does not call
/// [`Component::unmount`] (the widgets may already be gone, as when the app ends).
pub struct Mounted<C: Program> {
    view: C::View,
    /// While this is alive, the component's dispatch sends.
    link: Rc<()>,
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
        Self::build(dispatch, wrap, |dispatch| C::mount(ui, dispatch))
    }

    /// The widget to place in a container.
    pub fn root(&self) -> &dyn Widget {
        C::root(&self.view)
    }

    /// Takes the component down: disconnects its dispatch, calls
    /// [`Component::unmount`], calls `remove` with the root widget to take it out
    /// of its container, and drops the widgets.
    pub fn unmount(self, remove: impl FnOnce(&dyn Widget)) {
        self.unmount_with(C::unmount, |view| remove(C::root(view)));
    }
}

impl<C: Program> Mounted<C> {
    /// Builds the view with `mount`, which receives the component's dispatch.
    pub(crate) fn build<M: 'static, E>(
        dispatch: &Dispatch<M>,
        wrap: impl Fn(C::Message) -> M + 'static,
        mount: impl FnOnce(&Dispatch<C::Message>) -> Result<C::View, E>,
    ) -> Result<Self, E> {
        let link = Rc::new(());
        let view = mount(&dispatch.wrapped(wrap, Some(Rc::downgrade(&link))))?;
        Ok(Self { view, link })
    }

    /// The widgets built by `mount`.
    pub fn view(&self) -> &C::View {
        &self.view
    }

    /// Sets the widgets to match `model`, with the component's `render`.
    pub fn render(&self, model: &C) {
        model.render(&self.view);
    }

    pub(crate) fn unmount_with(
        self,
        unmount: impl FnOnce(&C::View),
        remove: impl FnOnce(&C::View),
    ) {
        let Self { view, link } = self;
        drop(link);
        unmount(&view);
        remove(&view);
        drop(view);
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
struct Entry<K, V> {
    key: K,
    view: V,
    link: Rc<()>,
}

/// Makes the dispatch of the child under a key, sending while the link is alive.
type MakeDispatch<K, M> = Box<dyn Fn(&K, Weak<()>) -> Dispatch<M>>;

/// The lifecycle part of [`Children`], which does not need a window.
pub(crate) struct Keyed<K, C: Program> {
    entries: RefCell<Vec<Entry<K, C::View>>>,
    dispatch: MakeDispatch<K, C::Message>,
}

impl<K: Clone + Eq + Hash + 'static, C: Program> Keyed<K, C> {
    pub(crate) fn new<M: 'static>(
        dispatch: &Dispatch<M>,
        wrap: impl Fn(K, C::Message) -> M + 'static,
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
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    pub(crate) fn view(&self, key: &K) -> Option<Ref<'_, C::View>> {
        Ref::filter_map(self.entries.borrow(), |entries| {
            entries
                .iter()
                .find(|entry| entry.key == *key)
                .map(|entry| &entry.view)
        })
        .ok()
    }

    pub(crate) fn sync_with<'a, E>(
        &self,
        models: impl IntoIterator<Item = (K, &'a C)>,
        mut mount: impl FnMut(&Dispatch<C::Message>) -> Result<C::View, E>,
        unmount: impl Fn(&C::View),
        mut place: impl FnMut(Step<'_, C::View>),
    ) -> Result<(), E>
    where
        C: 'a,
    {
        let mut wanted_keys = HashSet::new();
        let wanted: Vec<(K, &C)> = models
            .into_iter()
            .filter(|(key, _)| wanted_keys.insert(key.clone()))
            .collect();
        let mut entries = self.entries.borrow_mut();

        // 1. なくなったものを外す。位置は外した時点の並びで数える。
        let mut index = 0;
        while index < entries.len() {
            if wanted_keys.contains(&entries[index].key) {
                index += 1;
                continue;
            }
            let Entry { view, link, .. } = entries.remove(index);
            drop(link);
            unmount(&view);
            place(Step::Remove { index, view: &view });
        }

        // 2. 先頭から順に、求める並びへそろえる。index より前はそろっている。
        let mut present: HashSet<K> = entries.iter().map(|entry| entry.key.clone()).collect();
        for (index, (key, _)) in wanted.iter().enumerate() {
            if entries.get(index).is_some_and(|entry| entry.key == *key) {
                continue;
            }
            if present.contains(key) {
                // 後ろにあるものを動かす。作り直さないので、中身はそのまま残る。
                let from = index
                    + entries[index..]
                        .iter()
                        .position(|entry| entry.key == *key)
                        .expect("a present key is after the matched ones");
                let entry = entries.remove(from);
                place(Step::Remove {
                    index: from,
                    view: &entry.view,
                });
                place(Step::Insert {
                    index,
                    view: &entry.view,
                });
                entries.insert(index, entry);
            } else {
                let link = Rc::new(());
                let view = mount(&(self.dispatch)(key, Rc::downgrade(&link)))?;
                place(Step::Insert { index, view: &view });
                entries.insert(
                    index,
                    Entry {
                        key: key.clone(),
                        view,
                        link,
                    },
                );
                present.insert(key.clone());
            }
        }
        debug_assert_eq!(entries.len(), wanted.len());

        // 3. それぞれを描く。
        for (entry, (_, model)) in entries.iter().zip(&wanted) {
            model.render(&entry.view);
        }
        Ok(())
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
/// # use naui_app::{Change, Children, Component, Dispatch, Program};
/// # struct Row { id: u64 }
/// # impl Program for Row {
/// #     type Message = ();
/// #     type View = naui::Stack;
/// #     fn update(&mut self, _: ()) {}
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
///     let synced = view.rows.sync(rows, |change| match change {
///         Change::Insert { index, root } => view.list.insert(index, root),
///         Change::Remove { index, .. } => view.list.remove(index),
///     });
///     if let Err(error) = synced {
///         eprintln!("could not show a row: {error}");
///     }
/// }
/// # }
/// ```
///
/// The container can hold other widgets too, as long as the positions are offset
/// by them. A message a child sent before it was unmounted can still be waiting in
/// the queue, so the parent's `update` has to expect a key that is no longer there.
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
            keyed: Keyed::new(dispatch, wrap),
            ui: ui.clone(),
        }
    }

    /// Brings the mounted children in line with `models`, given in the order they
    /// are shown, each with its key:
    ///
    /// 1. A child whose key is no longer there is unmounted (as
    ///    [`Mounted::unmount`] does) and [`Change::Remove`] takes it out.
    /// 2. A new key is mounted and [`Change::Insert`] places it. A key that moved
    ///    is taken out and inserted again, keeping its widgets (they lose the
    ///    keyboard focus, as any moved widget does).
    /// 3. Every child is rendered with its model.
    ///
    /// When a key appears more than once, the later ones are ignored. If a mount
    /// fails, it stops there and returns the error, leaving what it did so far in
    /// place; the next call carries on from there.
    pub fn sync<'a>(
        &self,
        models: impl IntoIterator<Item = (K, &'a C)>,
        mut place: impl FnMut(Change<'_>),
    ) -> naui::Result<()>
    where
        C: 'a,
    {
        self.keyed.sync_with(
            models,
            |dispatch| C::mount(&self.ui, dispatch),
            C::unmount,
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
        )
    }

    /// The widgets of the child under `key`, if it is mounted. Do not hold it across
    /// a [`sync`](Self::sync).
    pub fn view(&self, key: &K) -> Option<Ref<'_, C::View>> {
        self.keyed.view(key)
    }

    /// How many children are mounted.
    pub fn len(&self) -> usize {
        self.keyed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
