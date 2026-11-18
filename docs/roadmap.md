# Roadmap

This lists features commonly expected of a GUI toolkit that naui (as of v0.17.0)
does not have yet, with priorities.

The native counterparts written for each item are estimates. When work starts,
they are checked again on all four platforms (including OS and toolkit versions).

## How versions are decided

Items are placed in versions by these points, the first weighing the most:

1. **Whether all four platforms have a native counterpart.** This decides whether
   it fits naui's approach, so it weighs the most. For something missing on one
   platform, the decision is whether documenting the difference in
   [Known limitations](known-limitations.md) is enough.
2. **Whether ordinary apps run into trouble right away without it.**
3. **How widely other toolkits provide it.**
4. **How heavy the implementation is, and whether it forms the basis for other
   items.** An item that others build on comes in an earlier version.

Within each version, items are listed in the order they are to be done. Items
implemented but not yet released are checked, and they are removed from this page
once a release includes them. A version is released with what is done; an item
not finished in time moves to the next version. [v1.0.0](#v100) lists conditions
rather than features.

Items marked *(adaptive)* belong to the [adaptive layout](#adaptive-layout) track,
and items marked *(drawing)* to
[drawing in separate crates](#drawing-in-separate-crates). Tracks whose items have
all been released are removed, as items are.

## Updating this page

The roadmap is updated after each release, in these steps:

1. **Remove what was released.** Checked items of the released version are
   removed, along with its section. Unchecked ones move to the next version, and
   track sections that refer to them are updated.
2. **Put every item in a version.** None is left waiting without one. An item
   whose shape is still open (what to build, on which platforms) goes into a
   version with the open questions written in it, and they are settled when its
   work starts. An item that does not fit the approach moves to
   [Not planned](#not-planned) with the reason. Ideas that come up between
   updates are placed the same way at the next update.
3. **Rebalance the versions.** Each version keeps around five items, and a new
   version section is added at the end when the later items need room.
4. **Check the rest of the page.** The version in the opening sentence, the
   track sections, and [v1.0.0](#v100) are brought in line with the versions.

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

What remains is `Flow` and the narrow navigation rail (v0.19.0), which do not
depend on the released `Stack::set_breakpoint`, `AdaptivePanes` and `Sidebar`
collapsing, and the display shape (v0.22.0), which comes with the first mobile
target. The composition swap and the bottom bar were investigated against those
and moved to Not planned.

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

1. **`NativeHost`** (v0.20.0), placing an external view on all four platforms.
2. **`naui-wgpu`** (v0.21.0), connecting a GPU surface through it and checking
   basic 2D and 3D drawing and resizing.

`naui-charts` (v0.19.0) draws on `Canvas`, so it does not wait for these. What it
waited on in `Canvas`, clipping and text width measurement, was released in
v0.17.0.

## v0.18.0

Gaps that ordinary apps run into right away and that all four platforms have
native counterparts for.

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
- [ ] **Typing and answering dialogs in tests.** Since v0.17.0, `click`,
      `select`, `activate`, `pick` and `drag_to` work on all four platforms with
      `naui::testing`, but typing into text fields so that `on_change` is called,
      and answering a `Dialog` or `Sheet` with its buttons, have no method that
      acts like them (see [Known limitations](known-limitations.md#testing-apps)).
      Check first how each platform can deliver them as a user's action would,
      and whether a nested run loop, such as macOS's modal `Dialog::open`, can be
      answered from a test.
- [x] **Multiple selection in `Tree`** (`set_selection_mode` with
      `SelectionMode`, as `List` has). Added `set_selection_mode`,
      `selection_mode`, `selection`, `set_selection` and `select_many`; `selected`
      and `on_select` keep passing one path (the first selected item). As in
      `List`, items are picked with ⌘ / Ctrl and Shift, and **selecting a branch
      does not select its children**: that is how `NSOutlineView` and `GtkListBox`
      behave, while WinUI's `TreeView` offers only check boxes that also select a
      branch's descendants (and its parent once all children are). So on Windows a
      tree with multiple selection is laid out in a `ListView` with `Extended`
      selection, with `TreeViewItem`'s chevrons and indentation; the Web's tree
      sets `aria-multiselectable`. Selected items inside a collapsed branch stay
      selected while hidden on every platform (macOS used to drop them with single
      selection too), and the next choice keeps them or drops them by the same
      rule as rows off screen in a long `List`.
- [ ] **Modifier keys in `Canvas`'s pointer notifications.** `PointerEvent`
      tells buttons apart but not ⇧, Ctrl, ⌥ / Alt or ⌘, so a drawing app cannot
      tell a click from a ⇧-click. `KeyEvent` already carries them as
      `Modifiers`, and `PointerEvent` is `#[non_exhaustive]`, so a field can be
      added without breaking apps. Every platform's pointer event carries them
      (`NSEvent`'s `modifierFlags`, `GdkModifierType` from the event, WinUI's
      `KeyModifiers` on `PointerRoutedEventArgs`, `shiftKey` and the others on DOM
      events), so it is light.

## v0.19.0

The rest of the testing actions, the separate crate that `Canvas` is now ready
for, and adaptive items that need investigating first but are not heavy.

- [ ] **The rest of the actions for testing apps:** a `Toast`'s action button,
      the window's close button (`on_close_request`), pointer events on a
      `Canvas`, and choosing and dropping files, delivered the way v0.18.0
      settles for typing and dialogs.
- [ ] *(drawing)* **`naui-charts`.** Line, bar and scatter charts in a separate
      crate drawing on `Canvas`. What it waited on, clipping so that plot areas
      do not draw outside their bounds and text width measurement for laying out
      axis labels, was released in v0.17.0, and Windows's `Canvas` keeps up with
      charts of many points since v0.15.0.
- [ ] *(adaptive)* **`Flow`** (a container that wraps its children onto new lines).
      `GtkFlowBox` and CSS `flex-wrap` exist, but WinUI only has
      `VariableSizedWrapGrid` with fixed-size cells (`WrapPanel` is in the
      Community Toolkit), and AppKit has nothing besides `NSCollectionView`'s flow
      layout. Which platforms need naui to lay it out itself is investigated first.
- [ ] *(adaptive)* **Narrow navigation rail.** Windows is covered by `Sidebar`'s
      automatic collapsing (`LeftCompact`). Whether the same collapsing covers
      macOS, GTK and the Web is checked first; if it does, nothing new is needed.

## v0.20.0

Heavier items, `NativeHost`, which the drawing crates and a WebView build on, and
the always-visible calendar.

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
- [ ] **Make dragging `Table` column widths work on Windows and Linux too.** Linux
      has the native feature in `GtkColumnView`, while Windows uses a custom table,
      so it is heavy.
- [ ] **Placing widgets in `Tree` items.** Items hold only strings today, while
      `List` and `Table` can hold widgets.
- [ ] **Always-visible calendar widget.** WinUI's `CalendarView`, `GtkCalendar`
      and `NSDatePicker` (calendar style) exist. The Web has no always-visible
      control, only the popup of `<input type="date">`, so it would be composed
      from buttons there (🟡), as `Toast` is where no DOM element exists.

## v0.21.0

Items that build on `NativeHost`, and the items that depend on how an app is
distributed.

- [ ] *(drawing)* **`naui-wgpu`.** A separate crate that connects a wgpu surface
      through `NativeHost` on all four platforms, and checks basic 2D and 3D
      drawing, resizing and scale-factor changes. It waits for `NativeHost`,
      including the Linux approach.
- [ ] **WebView, a code editor, Markdown rendering.** Markdown can use `RichText`
      (added in v0.16.0) for inline parts and `Stack` for blocks, so it fits a
      separate crate. A WebView builds on `NativeHost` (`WKWebView`, WebKitGTK,
      WebView2, `<iframe>`), and a code editor is a separate crate too. None of
      them needs the core.
- [ ] **Single instance, file associations, URL schemes.** GTK's `GtkApplication`
      already gives single-instance activation and an `open` signal for files.
      macOS needs `application:openURLs:` and a bundle `Info.plist`, WinUI uses
      `AppInstance` key redirection with MSIX registration, and the Web has no
      counterpart. What each distribution method (`cargo run`, `.app`, MSIX,
      Flatpak) provides has to be decided first.
- [ ] **Desktop notifications.** macOS needs a bundle, Linux a `.desktop` file, and
      Windows registration, so the conditions for each distribution method need to
      be documented.

## v0.22.0

The heaviest items: printing, and a first backend beyond the four platforms.

- [ ] **Printing.** All four have a print pipeline (`NSPrintOperation`,
      `GtkPrintOperation`, WinUI's `PrintDocument` through window interop, and
      CSS for `window.print()`). None can print a naui widget tree directly, so
      an app would print what `Canvas` draws or a separate document; which of
      the two naui offers is decided first.
- [ ] **iOS, Android, or an embedded device.** naui has no mobile backend, so
      each would need a new one, and the native-control approach would have to
      fit touch and phone navigation. The obstacle is checking: the iOS Simulator
      runs only on macOS, and these targets cannot be checked in CI at present.
      Which target comes first, and how it is verified, is decided first.
- [ ] *(adaptive)* **Display shape: safe areas, folds and hinges**, with the
      first mobile target. Only the Web exposes it today, through the Viewport
      Segments API, available only in Chrome and Edge from version 138, marked
      experimental by MDN, and not checkable in CI.

## v1.0.0

Unlike the versions above, v1.0.0 is not a set of features. From it on, naui
does not break its public API without a major version, so it lists the
conditions for making that promise. They are a draft, settled before v1.0.0 is
planned, and the versions above may change to meet them.

- **Items that change existing API come first.** Items on this page that would
  change what is public rather than add to it are done before v1.0.0. More than
  one action on a toast changes `Toast::action` and `on_action`, which assume one
  button. Widgets in `Tree` items change `TreeItem`, whose fields are public, so
  a new field breaks apps that build it with a struct literal. Items that only
  add, such as multiple selection in `Tree` and modifier keys on `PointerEvent`,
  can also come after.
- **Public types are reviewed for growth.** Each public struct and enum is
  checked for whether it may gain fields or variants later. Those that may get
  `#[non_exhaustive]`, or private fields with constructors, as `PointerEvent`
  and `ToastSpec` have.
- **Decided before v1.0.0: what the promise covers.** `backend()` and the
  `native_*` functions in `naui::macos`, `naui::gtk`, `naui::windows` and
  `naui::web` return the native toolkits' types (`gtk::Entry` and the like), so
  they change when naui moves to a new version of those crates. Whether they
  fall under the promise, or are documented as outside it, is decided.
- **Decided before v1.0.0: the MSRV policy.** naui needs Rust 1.88 today, and CI
  builds with it on Windows. When it may be raised, and in which kind of
  version, is decided and written down.
- **Decided before v1.0.0: which crates go to 1.0 together.** `naui` re-exports
  `naui-core`'s types, so the two move together. Whether `naui-app`, first
  released in v0.15.0, goes to 1.0 at the same time or stays at 0.x for a while
  is decided.
- **The four platforms stay checked.** CI builds and tests all four, and every
  difference found between them is in [Known limitations](known-limitations.md)
  or fixed.

## Not planned

Things that do not fit naui's approach.

- **Free per-widget colors or corner radii, animations, custom-drawn widgets, GPU
  rendering or game loops in naui itself.** These go against the approach of not
  drawing widgets ourselves. GPU drawing is left to separate crates through
  `NativeHost` (see [drawing in separate crates](#drawing-in-separate-crates)).
- **Declarative UI syntax, a virtual widget tree with diffing, hot reload.** If
  built at all, they would be separate crates on top, not part of the core. The
  application layer, `naui-app` (see [Structuring large apps](large-apps.md)),
  is not one of these: it builds the tree with the existing API and updates it
  with setters.
- **Swapping whole compositions by size** (a different widget tree per size
  class). It would rebuild the tree from a size callback, which the adaptive
  approach rules out. The cases it would cover are handled by `Stack::set_breakpoint`
  and `AdaptivePanes`.
- **A bottom navigation bar.** It is a phone pattern, and naui has no mobile
  backend (see the mobile entry in v0.22.0).
- **Font choosers.** WinUI has no font picker (only Win32's `ChooseFont`), and the
  Web can list fonts only through Local Font Access, which is available in Chrome
  and Edge from version 103 and not in Firefox or Safari. There is no common
  behavior to build on.
- **Tray icons.** The Web has no counterpart, and Linux needs DBus
  StatusNotifierItem. An app that needs one can add it as native code in its own
  crate.
