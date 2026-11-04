# naui-app

A Model–View–Update structure for [naui](https://crates.io/crates/naui) apps.

The app's state lives in one model. Callbacks only send typed messages; the
messages are handled one at a time, in order: `update` changes the model, and only
after the borrow of the model ends does `render` set the widgets to match it. A
message sent in the meantime (from `render`, from a nested run loop, or from
another thread) waits in the queue instead of re-entering `update`.

- A message `update` forgets to handle is a compile error (`match` is exhaustive).
- The model needs no `Rc<RefCell<_>>`, so there are no double borrows that compile
  but panic.
- `update` is plain Rust and the queue runs without a window, so both can be
  tested without one.

The widgets are built once with naui's API and `render` updates them with
setters. There is no virtual widget tree and no diffing; naui's setters do
nothing when given the value already shown.

Screens, panels and rows can be split into components, each with its own model,
messages, `update` and widgets. A child's messages are wrapped into its parent's
and go through the same queue; `Mounted` keeps one child's widgets, and `Children`
mounts and unmounts children as a collection in the model changes.

`update` does not start work: it returns an `Effect` asking for a task, work on
another thread, a timer or a failure to report, which starts after `render`. Results
come back as messages. An effect belongs to the model that asked for it: dropping a
child's `Scoped` wrapper cancels what it started, and effects under the same key in
one model replace each other (a search overtaken by further typing).

See [`examples/todo`](https://github.com/mokuichi147/naui/tree/main/examples/todo)
for a complete app,
[`examples/checklist`](https://github.com/mokuichi147/naui/tree/main/examples/checklist)
for one split into components, and
[`examples/search`](https://github.com/mokuichi147/naui/tree/main/examples/search)
for one with effects.

It is `#![forbid(unsafe_code)]`.
