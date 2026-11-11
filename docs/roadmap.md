# Roadmap

This lists features commonly expected of a GUI toolkit that naui (as of v0.16.0)
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

All six steps have been released, each building on the ones before it. Steps 1
to 3 (main-axis `Fill` and `Spacer` in Windows's `Stack`, the contract for changes
after adding to a container, and `Stack::set_orientation`) were released in
v0.12.0, step 4 (automatic collapsing of `Sidebar`) in v0.13.0, step 5
(size-based arrangements, `Stack::set_breakpoint`) in v0.14.0, and step 6
(`AdaptivePanes`) in v0.15.0.

What remains is `Flow` (P2), which does not depend on these steps. The P3 items
were investigated against `Stack::set_breakpoint`, `AdaptivePanes` and `Sidebar`:
the composition swap and the bottom bar moved to Not planned, and the rail is
still to be checked on the other platforms.

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

An optional crate on top of naui's widget API (`naui-app`) that gives
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
- **Testable logic.** `update` is plain Rust, so it can be tested without a window,
  children included, and the effects it returns are values that tests can check.
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
  panic hook's message on stderr shows it happened. naui-app puts a panic hook of
  its own in front of the previous one, so every panic on the UI thread reaches one
  error handler set by the app, together with the message being handled when it
  happens inside the queue. Where unwinding is available (the desktop platforms
  with the default `panic = "unwind"`), naui-app also catches the panic per message
  and per `render`. Catching it does not mean the app can safely go on: the model may
  be left half changed, and naui-app cannot tell. So the handler decides whether the
  queue goes on or the app stops; without a handler it goes on, as naui's callbacks
  do today. On the Web, wasm aborts on a panic, so the handler is told and the app
  stops there.
  The same handler receives the failures `update` and effects report, so they can
  be logged or shown to the user.
- **Components hold widgets rather than being widgets.** `naui::Widget` is sealed,
  so a component is not a widget: it has its own model, message type and widget
  handles, and gives its parent a root widget to place in a container. Its model
  lives in the parent's model and its widgets in the parent's view. That keeps the
  parent's `update`, children included, plain Rust that runs without a window, and
  lets the parent read a child's state instead of keeping a copy of it. The
  child's messages are wrapped into the parent's and go through the same queue; the
  parent's `update` hands them on, acting on some of them itself first, which is
  how a child tells its parent something.
- **Effects belong to the model that requested them.** `update` returns an
  `Effect`: nothing, a task, work on another thread, a timer, or a failure to
  report. It does nothing until the app runs it after `render`. A parent hands a
  child's effect on with `.map(Message::Editor)`, as it wraps the child's
  messages. The effects a child's `update` requests belong to that child's model:
  the parent keeps each child model in a wrapper that records them, and dropping
  the wrapper (the parent's `update` removing the child) cancels them. Tying them
  to the model rather than to `unmount` means a task never outlives the state its
  result is for, and needs nothing from the view: the widgets follow in the next
  `render`, which unmounts them.
- **Asynchronous work builds on `Tasks`.** An effect runs a future on the UI thread
  with `Tasks::spawn`, or hands blocking work to another thread and brings the
  result back through `Tasks::channel`; either way the result arrives as a
  message. naui-app does not depend on an async runtime, so I/O that needs one
  (tokio and the like) runs in the app's own runtime and comes back the same way.

### Order

Each step builds on the ones before it, and comes with tests of its own: the queue's
order and its guarantee against re-entry, a component's lifecycle, the
cancellation of effects. Step 1 (the message queue and the split of `update` and
`render`: `Program`, `App`, `Dispatch` and `examples/todo`) was released in
v0.15.0, and steps 2 to 4 (components, effects and lifecycles, unified error
handling, with `examples/checklist` and `examples/search`) in v0.16.0. What remains
is the guide below.

5. **A guide for structuring large apps** (P0, for v0.17.0).

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
counterparts for, plus the next steps of the tracks, so that they are complete in
v0.17.0.

- [x] *(i18n)* **Writing direction** (`Settings::layout_direction` with
      `LayoutDirection::Auto`, `Ltr` and `Rtl`, and `Ui::layout_direction`).
      `NSApp.userInterfaceLayoutDirection` (fixed through the arguments domain
      before `NSApplication` starts), `gtk_widget_set_default_direction`,
      `FlowDirection` on each window's content, and `dir` on `<html>`. The native
      toolkits mirror their own controls; naui pins `Canvas`, pictures, the number
      in `NumberInput` and the playback bar left to right where the toolkit would
      mirror them, and `Padding` is mirrored with the layout. `Auto` follows
      `Settings::language`, and without one keeps each toolkit's default.
- [ ] **Broaden `Canvas`'s vocabulary** (images, clipping, rotation and scaling,
      gradients). What it waited on was released in v0.15.0: text width
      measurement on Windows can use the DirectWrite that `Canvas` now draws with,
      and image drawing can build on the image sources `Image::set_bytes` and
      `set_pixels` read.
- [ ] **Images on the clipboard.** Copying and pasting images: `NSPasteboard` on
      macOS, `GdkTexture` on GTK, `DataPackage.SetBitmap` on WinUI, and the
      Clipboard API's `image/png` on the Web, which depends on the browser's
      support for image types. The images to copy come from `Image::set_bytes`
      and `set_pixels`, which were released in v0.15.0.
- [ ] **Publish the testing facilities across platforms.** Gather the existing
      testing APIs (`click`, `activate`, and so on) for app developers. Also
      provide a way to resize windows in tests, so apps can check their adaptive
      layouts.
- [ ] *(app)* **A guide for structuring large apps.** Like
      [Translating apps](translating.md), with an example under `examples/`: how to
      split an app into crates by feature (an app crate, a core crate with the
      models and `update` that does not depend on naui, a crate per screen, and
      services) so that Cargo reuses the build of crates that did not change. A
      change to a crate's public API still rebuilds the crates that depend on it,
      so where to draw the boundaries is part of the guide. It is the last step of
      the application layer track, after steps 1 to 4 were released.

## P1: Do next

Medium weight and frequently requested, or a smaller gap between platforms.

- [ ] **Relax the limits on toasts** (queueing, more than one action, position).
      Dialogs needing more than three buttons can now put the extra controls in a
      `Sheet`'s content, so only toasts remain. Queueing has a native counterpart
      only in `AdwToastOverlay`, so the others would be built in naui.
- [ ] **Reordering rows in a `List` by drag and drop.** `NSTableView` drag and
      drop, WinUI `ListView` (`CanReorderItems`), GTK drag sources and drop targets
      on a `GtkListBox`, and HTML5 drag and drop all support it. It builds on the
      file drops that are already in place. Still open: the shape of the payload
      passed between widgets within one app. `Tree` on GTK is built from
      `GtkListBox`, so dragging in `Tree` is left out.

## P2: Do if there is time

Heavy, or supported on a limited set of platforms.

- [ ] **Desktop notifications.** macOS needs a bundle, Linux a `.desktop` file, and
      Windows registration, so the conditions for each distribution method need to
      be documented.
- [ ] *(adaptive)* **`Flow`** (a container that wraps its children onto new lines).
      `GtkFlowBox` and CSS `flex-wrap` exist, but WinUI only has
      `VariableSizedWrapGrid` with fixed-size cells (`WrapPanel` is in the
      Community Toolkit), and AppKit has nothing besides `NSCollectionView`'s flow
      layout. Which platforms need naui to lay it out itself is investigated first.
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
## P3: Decide after investigating

Each entry records what the four platforms offer and what is still open. The
native API facts come from the platforms' documentation, and are checked again
when work starts. Items that did not fit the approach have moved to
[Not planned](#not-planned).

- [ ] **Single instance, file associations, URL schemes.** GTK's `GtkApplication`
      already gives single-instance activation and an `open` signal for files.
      macOS needs `application:openURLs:` and a bundle `Info.plist`, WinUI uses
      `AppInstance` key redirection with MSIX registration, and the Web has no
      counterpart. What each distribution method (`cargo run`, `.app`, MSIX,
      Flatpak) provides has to be decided first.
- [ ] **Printing.** All four have a print pipeline (`NSPrintOperation`,
      `GtkPrintOperation`, WinUI's `PrintDocument` through window interop, and
      CSS for `window.print()`). None can print a naui widget tree directly, so
      an app would print what `Canvas` draws or a separate document. Heavy, so it
      stays here.
- [ ] **Always-visible calendar widget.** WinUI's `CalendarView`, `GtkCalendar`
      and `NSDatePicker` (calendar style) exist. The Web has no always-visible
      control, only the popup of `<input type="date">`, so it would be composed
      from buttons there (🟡). It waits until an app needs it.
- [ ] **WebView, a code editor, Markdown rendering.** Markdown can use `RichText`
      (added in v0.16.0) for inline parts and `Stack` for blocks, so it fits a
      separate crate. A WebView builds on `NativeHost` (`WKWebView`, WebKitGTK,
      WebView2, `<iframe>`), and a code editor is a separate crate too. None of
      them needs the core.
- [ ] *(drawing)* **`naui-charts`.** Line, bar and scatter charts in a separate
      crate drawing on `Canvas`. Plot areas must not draw outside their bounds, so
      it waits for `Canvas`'s clipping (P1). Windows's `Canvas` keeps up with
      charts of many points since v0.15.0.
- [ ] *(drawing)* **`naui-wgpu`.** A separate crate that connects a wgpu surface
      through `NativeHost` on all four platforms, and checks basic 2D and 3D
      drawing, resizing and scale-factor changes. It waits for `NativeHost`,
      including the Linux approach.
- [ ] *(adaptive)* **Narrow navigation rail.** Windows is covered by `Sidebar`'s
      automatic collapsing (`LeftCompact`). Still to check: whether the same
      collapsing covers macOS, GTK and the Web.
- [ ] *(adaptive)* **Display shape: safe areas, folds and hinges.** Only the Web
      exposes this, through the Viewport Segments API. It is available only in
      Chrome and Edge from version 138, MDN marks it experimental, and it cannot
      be checked in CI. It stays on hold until a foldable or mobile target is
      decided.
- [ ] **iOS, Android, and embedded devices.** naui has no mobile backend, so each
      would need a new one, and the native-control approach would have to fit touch
      and phone navigation (the display-shape and bottom-bar items depend on it).
      The obstacle is checking: the iOS Simulator runs only on macOS, and these
      targets cannot be checked in CI at present. Decide which target comes first,
      and how it would be verified, before any backend work.

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
- **Swapping whole compositions by size** (a different widget tree per size
  class). It would rebuild the tree from a size callback, which the adaptive
  approach rules out. The cases it would cover are handled by `Stack::set_breakpoint`
  and `AdaptivePanes`.
- **A bottom navigation bar.** It is a phone pattern, and naui has no mobile
  backend (see the mobile entry in P3).
- **Font choosers.** WinUI has no font picker (only Win32's `ChooseFont`), and the
  Web can list fonts only through Local Font Access, which is available in Chrome
  and Edge from version 103 and not in Firefox or Safari. There is no common
  behavior to build on.
- **Tray icons.** The Web has no counterpart, and Linux needs DBus
  StatusNotifierItem. An app that needs one can add it as native code in its own
  crate.
