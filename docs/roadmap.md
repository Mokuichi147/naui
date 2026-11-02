# Roadmap

This lists features commonly expected of a GUI toolkit that naui (as of v0.14.0)
does not have yet, with priorities.

The native counterparts written for each item are estimates. When work starts,
they are checked again on all four platforms (including OS and toolkit versions).

## How priorities are decided

1. **Whether all four platforms have a native counterpart.** This decides whether
   it fits naui's approach, so it weighs the most. For something missing on one
   platform, the decision is whether documenting the difference in
   [Known limitations](known-limitations.md) is enough.
2. **Whether ordinary apps run into trouble right away without it.**
3. **How widely other toolkits provide it.**
4. **How heavy the implementation is, and whether it forms the basis for other
   items.**

Within each priority, items are listed in the order they are to be done. Items
implemented but not yet released are checked, and they are removed from this page
once a release includes them.

Items marked *(adaptive)* belong to the [adaptive layout](#adaptive-layout) track.
That track has its own order and proceeds alongside the other items, rather than
waiting until every P1 item is done. Items marked *(i18n)* belong to
[internationalization](#internationalization), items marked *(app)* to the
[application layer](#application-layer), and items marked *(drawing)* to
[drawing in separate crates](#drawing-in-separate-crates).

## Adaptive layout

Layouts that rearrange themselves when the window or a container changes size:
switching a `Stack` between vertical and horizontal, collapsing a `Sidebar`, showing
one or two panes.

### Approach

- **Let the native toolkit decide.** Where a platform has a mechanism that switches
  layout by size (`NavigationView`'s `PaneDisplayMode::Auto`, `AdwBreakpoint`, CSS
  container queries), naui uses it. Only where none exists does naui decide itself.
- **No public size-change callback for rebuilding the tree.** Rearranging widgets
  from a Rust callback that watches sizes causes back-and-forth relayout, conflicts
  with GTK4's rule against changing widgets during size allocation, and re-enters
  AppKit's layout pass. Where naui has to decide internally, the change is deferred
  to the next run loop iteration, and thresholds get some hysteresis.
  `Window::on_resize` and the `on_resize` of `Sidebar` and `SplitView` stay as they
  are, for saving sizes and similar uses.
- **Mobile-only patterns are on hold.** Bottom navigation bars, safe areas, folds
  and hinges matter mainly on phones, and naui has no mobile backend. Only the Web
  could provide the information, and it cannot be checked in CI.

### Order

Each step builds on the ones before it. Steps 1 to 3 (main-axis `Fill` and
`Spacer` in Windows's `Stack`, the contract for changes after adding to a
container, and `Stack::set_orientation`) were released in v0.12.0, step 4
(automatic collapsing of `Sidebar`) in v0.13.0, and step 5 (size-based
arrangements, `Stack::set_breakpoint`) in v0.14.0.

6. **`AdaptivePanes`** (P0).

`Flow` (P2) does not depend on these and can start at any time. The items in P3 are
decided after investigating.

## Internationalization

Making apps that show text in the user's language and follow its writing
direction.

### Approach

- **naui handles only what needs the native side.** That is the OS's preferred
  languages and their changes, the writing direction, and naui's own strings.
  Translating the app's strings is left to existing libraries (Fluent, gettext,
  ICU4X for dates and numbers), so apps that do not translate gain no dependencies.
- **No binding mechanism in the core.** Updating widget strings when the language
  changes is done by the app holding widgets with `downgrade()` and calling
  `set_text`, as shown in [Translating apps](translating.md). This keeps the core free of a reactive
  system, in line with keeping declarative UI out of it.
- **Native strings follow the OS language.** Strings the native controls carry
  themselves (file dialogs, date pickers, text field context menus) use the OS's UI
  language, and most platforms cannot switch it per app while running. Switching
  languages inside a running app therefore leaves those in the OS language, so the
  default is to follow the OS language, and the differences are documented.

## Application layer

An optional crate on top of naui's widget API (tentatively `naui-app`) that gives
large apps a structure for their state, events and lifecycles: Model–View–Update,
split into components, with asynchronous work managed as effects. Apps that do not
use it are unaffected, and naui's own API stays as it is.

The aim is to manage state, events and lifecycles safely, not to automate building
the UI. In return, apps get:

- **Checks at compile time.** Each part of the app has a typed model and message,
  so a message `update` forgets to handle is a compile error (`match` has to be
  exhaustive).
- **Fewer panics at run time.** Events cannot re-enter `update`, so the model is
  free of the double borrows of `Rc<RefCell<_>>` that compile but panic.
- **Testable logic.** `update` is plain Rust, so it can be tested without a window.
- **Division of work.** Screens and features become components that can be written
  and tested on their own.

### Approach

- **Build the tree once and update it with setters.** Widgets are created once, as
  today, and `render` updates them with `set_text`, `set_enabled`, `append` and the
  like. There is no virtual widget tree, no diffing and no automatic detection of
  changed properties: `render` calls the setters it needs every time. This keeps
  declarative UI and reactive systems out of naui (see
  [Not planned](#not-planned)), and the backends need no changes.
- **Messages go one way, through a queue.** Callbacks only send a typed message
  (a click, a changed input, a finished load). Messages are handled one at a time,
  in the order they were sent: `update` changes the model and may request
  effects, the mutable borrow of the model ends, and only then does `render`
  touch widgets and the effects start. A message sent in the meantime, from
  `render`, from a nested run loop such as macOS's modal `Dialog::open`, or from
  another thread, waits in the queue instead of re-entering `update`.
- **Panics are contained and reported, not prevented.** Indexing out of bounds or
  an explicit `panic!` can still happen. Today naui stops a panic at the callback
  boundary (no backend can unwind across the C / WinRT boundary), and only the
  panic hook's message on stderr shows it happened. naui-app catches panics per
  message, and hands them, together with the message that caused them, to one
  error handler that also receives the errors `update` and effects return, so they
  can be logged or shown to the user.
- **Components hold widgets rather than being widgets.** `naui::Widget` is sealed,
  so a component owns its model, its message type and its widget handles, and gives
  its parent a root widget to place in a container. Parent and child talk only
  through typed messages.
- **Asynchronous work builds on `Tasks`.** An effect runs a future on the UI thread
  with `Tasks::spawn`, or hands blocking work to another thread and brings the
  result back through `Tasks::channel`; either way the result arrives as a
  message. naui-app does not depend on an async runtime, so I/O that needs one
  (tokio and the like) runs in the app's own runtime and comes back the same way.

### Order

Each step builds on the ones before it, and comes with tests of its own: the queue's
order and its guarantee against re-entry, a component's lifecycle, the
cancellation of effects.

1. **Message queue and the split of `update` and `render`** (P0).
2. **Components** (P1).
3. **Effects and lifecycles** (P1).
4. **Unified error handling** (P1).
5. **A guide for structuring large apps** (P2).

## Drawing in separate crates

Widening what apps can draw (charts, GPU-rendered 2D and 3D, viewers, CAD, games)
while naui stays a light toolkit of native UI. naui is not to become a rendering or
game engine: it gains one widget that hosts a native view made elsewhere, and the
drawing is done by separate crates.

### Approach

- **naui adds only `NativeHost`.** It places a native view created by another
  crate among ordinary widgets. naui handles its layout, size and scale-factor
  changes, and its lifecycle (when it is attached and released), and has no part
  in GPU setup, drawing or game loops.
- **`Widget` stays sealed.** Extension goes through `NativeHost`, not through
  widget types implemented outside naui.
- **No GPU dependencies in naui.** wgpu and the like are dependencies of the
  separate crates only.
- **`Canvas` stays for simple 2D.** Static shapes and charts keep using `Canvas`,
  which draws with each platform's own 2D API.
- **What the separate crates cover.** These are sketches, not commitments:

  | Crate | Role | Needs from naui |
  | --- | --- | --- |
  | `naui-charts` | Line, bar and scatter charts | Nothing new (draws on `Canvas`) |
  | `naui-wgpu` | Fast 2D and 3D drawing with wgpu | `NativeHost` |
  | A scene crate | 3D models, cameras, lights | Nothing beyond `naui-wgpu` |
  | Game crates | ECS, physics, audio, game loop | Nothing beyond `naui-wgpu` |

  The last two are left to whoever needs them and are not on this roadmap.

### Order

1. **`NativeHost`** (P2), placing an external view on all four platforms.
2. **`naui-wgpu`** (P3), connecting a GPU surface through it and checking basic 2D
   and 3D drawing and resizing.

`naui-charts` (P3) draws on `Canvas`, so it does not wait for these.

## P0: Do now

Gaps that ordinary apps run into right away and that all four platforms have native
counterparts for, plus the next steps of the adaptive layout,
internationalization and application layer tracks.

- [x] *(adaptive)* **`AdaptivePanes`.** A list and its detail, side by side when
      wide and one at a time when narrow, with a back button above the detail in
      the one-pane state. Windows uses `TwoPaneView`, which decides by itself, and
      Linux `AdwNavigationSplitView` (which also goes back by Escape, Alt+← and a
      swipe). Its `AdwBreakpoint` would take away the window's minimum size, so on
      Linux, as on macOS and the Web, which have no such control, naui decides
      with naui-core's `PanesArrangement`, sharing the slack of
      `StackArrangement`. Both native controls are two-pane, so three panes are
      a `Sidebar` beside the panes or another `AdaptivePanes` in the detail,
      each deciding by its own width.
- [ ] *(i18n)* **Telling the native side the app's language.** The language
      affects which glyphs are chosen for CJK text and how screen readers
      pronounce it (`lang` on the Web, `FrameworkElement.Language` on WinUI).
      Without it, an app showing Japanese on an OS set to another language can
      get Chinese glyph forms. Which platforms can take it per window or per widget is
      investigated first.
- [ ] **Separators and framed groups.** Settings screens, and now the content of
      `Popover` and `Sheet`, need a way to divide and group controls. `NSBox`,
      `GtkSeparator` / `GtkFrame`, `<hr>` / `<fieldset>`. WinUI has no standalone
      separator, so what to substitute is investigated first.
- [ ] **More image sources** (bytes in memory, SVG, pixel buffers). For now only
      paths and URLs, so an app cannot show an icon embedded with `include_bytes!`
      without writing it to a file. The Web can use Blob URLs.
- [x] **Make `Track::Fill` weights work in `Grid` on macOS and Linux.** naui-core
      now splits the room with the rule of CSS's `fr` (by weight, never below what a
      track's content needs, the rest split again among the others), and both
      backends use it. Linux replaces `GtkGrid` with naui's own container, since
      `GtkGrid` gave each column its natural width plus an even share, so `Fill`
      columns of the same weight also differed by their content. macOS keeps
      `NSGridView` and has each `Fill` child wish for its cell's share as a
      proportion of the grid's size; when two or more
      `Fill` tracks hold no `Fill` child, their split is still left to
      `NSGridView`. A `Fill` track beside one with a capped child now gets its
      share instead of everything but the cap, as on Windows and the Web.
- [ ] *(app)* **Message queue and the split of `update` and `render`.** Create the
      crate with the model, message, `update` and `render`, and the queue that runs
      them as described in [Application layer](#application-layer). Since `render` sets every
      value each time, first check on each platform that a setter given the value
      already shown does nothing: `set_text` on a focused text field must not move
      the caret or break IME composition. Where it does something, naui's setter is
      made to skip it, which also helps apps that do not use naui-app.

## P1: Do next

Medium weight and frequently requested, or a smaller gap between platforms.

- [ ] **Row windowing for large `List`s.** Extend `Table`'s mechanism
      (`row_window` in naui-core) to `List`.
- [x] **Make Windows's `Canvas` keep up with large drawings.** Windows now draws
      the commands with Direct2D (text with DirectWrite) onto a Composition drawing
      surface, the way the other three platforms rasterize, instead of turning them
      into one XAML string of `Path` and `TextBlock` elements for `XamlReader`.
      Measured with 5000 short lines, a redraw went from about 590 ms to under one
      frame. Of those 590 ms, about 200 went to parsing, 80 to replacing the tree,
      and most of the rest to rendering the 5000 elements. Because the per-element
      cost remained, the two cheaper candidates were not taken. Reusing the
      previous elements helps only when few commands change. Merging commands of
      the same style into one `Path` changes the result where translucent shapes
      overlap or windings cancel, and does nothing for drawings whose colors vary.
      DirectWrite text measurement is now available to broaden `Canvas`'s
      vocabulary.
- [ ] **Rich text** (mixing bold, colors and links within a line).
      `NSAttributedString`, Pango markup, `TextBlock.Inlines`, `<span>`.
- [ ] *(app)* **Components.** A component has its own model, messages, `update`
      and widgets, and goes through `mount` (building its widgets), `update`,
      `render` and `unmount` (cleaning up). A child's messages are turned into the
      parent's. How `unmount` pairs with removing the root widget from its
      container is decided when work starts.
- [ ] *(app)* **Effects and lifecycles.** `Effect<Message>` gathers asynchronous
      work: tasks (`Tasks::spawn`), work on other threads (`Tasks::channel`) and
      timers (`Tasks::after`). A component's tasks and timers are cancelled on
      `unmount` (dropping a `Task` or `Timer` does not stop it), results of effects
      superseded by newer ones are discarded (a search overtaken by further
      typing), and failures are reported.
- [ ] *(app)* **Unified error handling.** One handler, set by the app, receives the
      panics caught per message and the errors from `update` and effects, with the
      message involved. Whether naui itself should also report panics in ordinary
      callbacks to a handler, rather than only through the panic hook, is decided
      then.

## P2: Do if there is time

Heavy, or supported on a limited set of platforms.

- [ ] **Publish the testing facilities across platforms.** Gather the existing
      testing APIs (`click`, `activate`, and so on) for app developers. Also
      provide a way to resize windows in tests, so apps can check their adaptive
      layouts.
- [ ] **Relax the limits on toasts** (queueing, more than one action, position).
      Dialogs needing more than three buttons can now put the extra controls in a
      `Sheet`'s content, so only toasts remain. Queueing has a native counterpart
      only in `AdwToastOverlay`, so the others would be built in naui.
- [ ] **Images on the clipboard.**
- [ ] **Desktop notifications.** macOS needs a bundle, Linux a `.desktop` file, and
      Windows registration, so the conditions for each distribution method need to
      be documented.
- [ ] *(adaptive)* **`Flow`** (a container that wraps its children onto new lines).
      `GtkFlowBox` and CSS `flex-wrap` exist, but WinUI only has
      `VariableSizedWrapGrid` with fixed-size cells (`WrapPanel` is in the
      Community Toolkit), and AppKit has nothing besides `NSCollectionView`'s flow
      layout. Which platforms need naui to lay it out itself is investigated first.
- [ ] **Broaden `Canvas`'s vocabulary** (images, clipping, rotation and scaling,
      gradients). Text width measurement on Windows can use the DirectWrite that
      `Canvas` now draws with. Image drawing builds on more image sources (P0).
- [ ] **Multiple selection in `Tree`, and placing widgets in items.**
- [ ] **Make dragging `Table` column widths work on Windows and Linux too.** Linux
      has the native feature in `GtkColumnView`, while Windows uses a custom table,
      so it is heavy.
- [ ] *(drawing)* **`NativeHost`.** A widget that places a native view created by
      another crate: an `NSView` on macOS, a `GtkWidget` on Linux, a XAML element
      on Windows, a DOM element on the Web. The view is platform-specific, so how
      the crate receives the native object (and whether `raw-window-handle` fits,
      which has no form for GTK widgets or WinUI's `SwapChainPanel`) is decided
      first. Linux needs the most checking: GTK4 has no native child windows, so a
      GPU surface cannot simply be placed, and the candidates are `GtkGLArea` with
      wgpu's GL backend, or rendering offscreen and handing frames over as
      textures (`GdkDmabufTexture` from GTK 4.14). Windows needs `SwapChainPanel`
      in the projection, and the Web a `<canvas>` for WebGPU or WebGL.
- [ ] *(app)* **A guide for structuring large apps.** Like
      [Translating apps](translating.md), with an example under `examples/`: how to
      split an app into crates by feature (an app crate, a core crate with the
      models and `update` that does not depend on naui, a crate per screen, and
      services) so that Cargo reuses the build of crates that did not change. A
      change to a crate's public API still rebuilds the crates that depend on it,
      so where to draw the boundaries is part of the guide.

- [ ] *(i18n)* **Writing direction** (`LayoutDirection` with `Auto`, `Ltr` and
      `Rtl`). `userInterfaceLayoutDirection`, `gtk_widget_set_default_direction`,
      `FlowDirection`, the `dir` attribute. First check how much each native
      toolkit mirrors automatically, and which widgets must not be mirrored
      (`Canvas`, number inputs, media controls).

## P3: Decide after investigating

Not yet clear whether they fit the approach, or whether they are needed.

- [ ] **Drag and drop between widgets** (rearranging `List` or `Tree`). Builds on
      receiving file drops.
- [ ] **Single instance, file associations, URL schemes.** Strongly tied to
      distribution and packaging.
- [ ] **Tray icons.** GTK4 has none and needs DBus StatusNotifierItem. WinUI has
      none either, so it would be built with Win32, and the Web has no counterpart.
- [ ] **Printing.**
- [ ] **Font choosers, and an always-visible calendar widget.**
- [ ] **WebView, a code editor, Markdown rendering.** These seem better suited to
      separate crates than to the core. A WebView (`WKWebView`, WebKitGTK,
      WebView2, `<iframe>`) could be one built on `NativeHost`.
- [ ] *(drawing)* **`naui-charts`.** Line, bar and scatter charts in a separate
      crate drawing on `Canvas`, so it can start before `NativeHost`. Windows's
      `Canvas` now keeps up with charts of many points (P1).
- [ ] *(drawing)* **`naui-wgpu`.** A separate crate that connects a wgpu surface
      through `NativeHost` on all four platforms, and checks basic 2D and 3D
      drawing, resizing and scale-factor changes.
- [ ] *(adaptive)* **Swapping whole compositions by size** (a different widget tree
      per size class). First check whether size-based arrangements and
      `AdaptivePanes` leave real cases uncovered.
- [ ] *(adaptive)* **Switching navigation by size** (`Sidebar`, a narrow rail,
      a bottom bar). The rail is covered on Windows by `Sidebar`'s automatic
      collapsing (`LeftCompact`). The bottom bar is a phone pattern, so it waits
      until there is a reason to have it on the desktop.
- [ ] *(adaptive)* **Display shape: safe areas, folds and hinges.** Only the Web
      exposes this (the Viewport Segments API), and it cannot be checked in CI, so
      for now this is limited to investigating how the information could be
      obtained.

## Not planned

Things that do not fit naui's approach.

- **Free per-widget colors or corner radii, animations, custom-drawn widgets, GPU
  rendering or game loops in naui itself.** These go against the approach of not
  drawing widgets ourselves. GPU drawing is left to separate crates through
  `NativeHost` (see [drawing in separate crates](#drawing-in-separate-crates)).
- **Declarative UI syntax, a virtual widget tree with diffing, hot reload.** If
  built at all, they would be separate crates on top, not part of the core. The
  [application layer](#application-layer) is not one of these: it builds the tree
  with the existing API and updates it with setters.
- **iOS, Android, embedded devices.** There are no mobile backends, and they
  cannot be checked in CI either, so they are on hold.
