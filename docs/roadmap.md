# Roadmap

This lists features commonly expected of a GUI toolkit that naui (as of v0.18.0)
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
collapsing, and the display shape (v0.24.0), which comes with the first mobile
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
  widget types implemented outside naui: `Widget` hands out the backend's own
  widget, whose trait has methods for naui's internal use, so opening it would
  put them under the API promise. A type made outside naui is passed to
  containers through a second trait that only names the naui widget inside it
  (v0.20.0).
- **No GPU dependencies in naui.** wgpu and the like are dependencies of the
  separate crates only.
- **`Canvas` stays for simple 2D.** Static shapes and charts keep using `Canvas`,
  which draws with each platform's own 2D API.
- **What the separate crates cover.** These are sketches, not commitments:

  | Crate | Role | Needs from naui |
  | --- | --- | --- |
  | `naui-charts` | Line, bar and scatter charts | Nothing new (draws on `Canvas`) |
  | An extension widget crate | Tags, badges and other widgets no platform has | `NativeHost`, and passing its types to containers |
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
v0.17.0. The extension widget crate (v0.22.0) builds its widgets from native
pieces through `NativeHost`, so it comes after it.

## v0.19.0

The rest of the testing actions, the separate crate that `Canvas` is now ready
for along with the colors it needs in a dark appearance, and adaptive items that
need investigating first but are not heavy.

- [ ] **The rest of the actions for testing apps:** a `Toast`'s action button,
      the window's close button (`on_close_request`), pointer events on a
      `Canvas`, and choosing and dropping files, delivered the way typing and
      answering dialogs are since v0.18.0. Whether a test can also read what the
      app handed to the OS (the URL given to `Ui::open_url`, say) without it
      really opening is decided with them.
- [ ] **Colors that follow light and dark in `Canvas`.** Widgets follow the
      appearance by themselves, but `Canvas` draws with fixed `Color`s and
      nothing tells the app when the appearance changes, so a drawing made for
      light stays that way in dark. All four can tell: AppKit's
      `viewDidChangeEffectiveAppearance`, `AdwStyleManager`'s `dark` property,
      WinUI's `ActualThemeChanged` and the `prefers-color-scheme` media query.
      Which shape fits is decided first: `Painter` taking the roles of
      `TextColor` (and the accent color) so that the platform resolves them at
      each draw, a notification with the resolved light or dark, or both. The
      charts below need it for axes and labels, so it comes before them.
- [ ] *(drawing)* **`naui-charts`.** Line, bar and scatter charts in a separate
      crate drawing on `Canvas`. What it waited on, clipping so that plot areas
      do not draw outside their bounds and text width measurement for laying out
      axis labels, was released in v0.17.0, and Windows's `Canvas` keeps up with
      charts of many points since v0.15.0.
- [x] *(adaptive)* **`Flow`** (a container that wraps its children onto new lines).
      `GtkFlowBox` and CSS `flex-wrap` exist, but WinUI only has
      `VariableSizedWrapGrid` with fixed-size cells (`WrapPanel` is in the
      Community Toolkit), and AppKit has nothing besides `NSCollectionView`'s flow
      layout. Only the Web wraps natively (`flex-wrap`): `GtkFlowBox` lines its
      children up in columns and wraps each in a focusable, selectable child, and
      `AdwWrapBox` needs libadwaita 1.7. So macOS, Linux and Windows break the
      lines with naui-core's `FlowLayout`, inside the toolkit's own layout pass
      (`NSView::layout`, a `GtkWidget`'s height-for-width measuring, a derived
      `Panel`'s `MeasureOverride` / `ArrangeOverride`), with no callback through
      the app.
- [ ] *(adaptive)* **Narrow navigation rail.** Windows is covered by `Sidebar`'s
      automatic collapsing (`LeftCompact`). Whether the same collapsing covers
      macOS, GTK and the Web is checked first; if it does, nothing new is needed.

## v0.20.0

Heavier items, `NativeHost`, which the drawing crates, a WebView and the
extension widgets build on, passing types made outside naui to containers,
shortcuts that are not in a menu, and the always-visible calendar.

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
- [ ] **Passing a type made outside naui to containers.** `Stack::append` and
      the like take `&dyn Widget`, which is sealed, so a crate's own type (a
      composition of naui widgets, or one wrapping a `NativeHost`) has to hand
      out the widget inside it at every call (`tag.root()`), as `naui-app`'s
      `Mounted::root` does. A public trait with one method returning that
      widget, taken by the containers, lets `stack.append(&tag)` be written
      while `Widget` stays sealed. Decided first: whether the containers take
      the new trait in place of `&dyn Widget` (every `Widget` would implement
      it) or beside it, and that the common methods (`set_sizing`,
      `set_visible`) reach the inner widget.
- [ ] **Keyboard shortcuts outside the menu bar.** A shortcut is given today
      only to a `MenuBar` item (`MenuShortcut`); anything else is matched by
      hand in `on_key_down`, whose order relative to menu shortcuts differs by
      platform. All four have a counterpart: WinUI's `KeyboardAccelerator` on
      any element, `GtkShortcutController`, AppKit's key equivalents
      (`performKeyEquivalent:`), and `keydown` on the Web
      (`KeyboardAccelerator` is not in naui's WinUI projection yet; the menu
      bar's shortcuts are matched from keys that reach the window's root).
      Decided first: whether
      a shortcut belongs to a window or to a widget and its descendants (the
      scope `GtkShortcutController` and `KeyboardAccelerator` both have), and
      whether it reuses `MenuShortcut`. Naming commands, binding them from a
      keymap the user can edit, and choosing by context are an app's own layer
      on top (they fit `naui-app` or a separate crate), not the core.
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

Items that build on `NativeHost`, cursor shapes for `Canvas`, and the items that
depend on how an app is distributed.

- [ ] *(drawing)* **`naui-wgpu`.** A separate crate that connects a wgpu surface
      through `NativeHost` on all four platforms, and checks basic 2D and 3D
      drawing, resizing and scale-factor changes. It waits for `NativeHost`,
      including the Linux approach.
- [ ] **WebView, a code editor, Markdown rendering.** Markdown can use `RichText`
      (added in v0.16.0) for inline parts and `Stack` for blocks, so it fits a
      separate crate. A WebView builds on `NativeHost` (`WKWebView`, WebKitGTK,
      WebView2, `<iframe>`), and a code editor is a separate crate too. None of
      them needs the core.
- [ ] **Pointer cursor shapes on `Canvas`.** A drawing that can be dragged or
      resized cannot show it: the cursor over a `Canvas` stays the arrow.
      `NSCursor` with cursor rects, `gtk_widget_set_cursor` and CSS `cursor`
      take a shape per view. WinUI has only `UIElement.ProtectedCursor`, which
      is protected, so whether naui's `Canvas` element can set it through the
      projection is checked first, along with which shapes all four name
      (WinUI's `InputSystemCursorShape` is the shortest list). Other widgets
      keep the cursors their controls choose.
- [ ] **Single instance, file associations, URL schemes.** GTK's `GtkApplication`
      already gives single-instance activation and an `open` signal for files.
      macOS needs `application:openURLs:` and a bundle `Info.plist`, WinUI uses
      `AppInstance` key redirection with MSIX registration, and the Web has no
      counterpart. What each distribution method (`cargo run`, `.app`, MSIX,
      Flatpak) provides has to be decided first. Being reopened while running
      (a click on the Dock icon), the list of recent documents, and the Dock
      menu and Windows's jump list hang on the same registration and are
      looked at with it; the Web has none of them.
- [ ] **Desktop notifications.** macOS needs a bundle, Linux a `.desktop` file, and
      Windows registration, so the conditions for each distribution method need to
      be documented.

## v0.22.0

Printing and dragging between widgets, the heaviest of the features, what
windows tell and take about activation and focus, and the extension crate for
widgets no platform has.

- [ ] **Dragging between widgets and out of the app.** A drag today stays
      inside one `List` (reordering, v0.18.0) or comes in as files
      (`on_file_drop`). Dragging a row to another `List` or a `Tree`, onto any
      widget, and out to other apps as files or text use the same native
      machinery (`NSDraggingSource`, `GtkDragSource` and `GtkDropTarget`,
      WinUI's `CanDragItems` and `DragStarting`, HTML drag and drop), and the
      mark a drag already carries for its list is the shape they grow from.
      Decided first: what a drag carries (rows of a known widget, or data by
      type as a clipboard holds), and which widgets take drops.
- [ ] **Printing.** All four have a print pipeline (`NSPrintOperation`,
      `GtkPrintOperation`, WinUI's `PrintDocument` through window interop, and
      CSS for `window.print()`). None can print a naui widget tree directly, so
      an app would print what `Canvas` draws or a separate document; which of
      the two naui offers is decided first.
- [ ] **Telling when a window becomes active or inactive.** For pausing work
      while the app is in the background and refreshing on return. AppKit's
      `windowDidBecomeKey:` and `windowDidResignKey:`, `GtkWindow`'s
      `is-active`, WinUI's `Window.Activated`, and `focus` and `blur` on the
      Web's `window`. It joins `on_state_change`, which tells minimizing but
      not being covered or switched away from.
- [ ] **Moving focus to the next widget, and the Tab order.** `request_focus`
      names one widget; nothing moves focus forward or back from where it is,
      and the Tab order is the order the widgets were added in. AppKit has
      `selectNextKeyView:` and `nextKeyView`, WinUI `FocusManager.TryMoveFocus`
      and `TabIndex`, GTK `child_focus`, and the Web `tabindex`. GTK 4 dropped
      the focus chain and the Web has no call that moves focus to the next
      element, so what all four can do is checked first: moving may be offered
      without setting the order, or the item moves to
      [Not planned](#not-planned) with "add the widgets in Tab order".

- [ ] *(drawing)* **An extension crate for widgets no platform has.** Tags,
      badges, avatars and the like are common in current UIs but are not
      controls of any of the four toolkits, so naui itself does not carry
      them. A separate crate offers them on top of naui. It builds each from
      the platform's own pieces through `NativeHost` (v0.20.0): an element
      styled with CSS on the Web, a label with a CSS class on GTK, a `Border`
      around a `TextBlock` with theme resources on WinUI, a layer-backed view
      on AppKit. The text is then a real text element, so the platform's
      font, light and dark, and assistive technology come with it, which a
      `Canvas` drawing would have to be given by hand; `Canvas` is the
      fallback where a platform has no piece to build from. The colors and
      corner radii naui does not set per widget are set here with each
      platform's API. Decided first: which widgets come first (tags and
      badges are the candidates), and how the crate depends on the native
      crates: on Windows it needs naui's own WinUI projection (`naui-winui3`),
      so its versions move with naui's.

## v0.23.0

Small pieces of OS integration. Several are missing on one or two platforms, so
each says what is checked before it is built.

- [ ] **Keeping the system awake** while a task runs (playback, a long export).
      `NSProcessInfo`'s `beginActivityWithOptions:`, `gtk_application_inhibit`,
      Win32's `SetThreadExecutionState` (WinUI has none of its own), and the
      Web's Screen Wake Lock, which holds only while the page is visible and
      keeps the screen on rather than the system. Whether one request with a
      reason covers both "keep the display on" and "do not sleep" is decided
      first.
- [ ] **Showing a file in the file manager, and opening one with its default
      app.** `Ui::open_url` is documented for `https:` and `mailto:`; whether
      a `file:` URL already opens through it on every platform is checked
      first, which would leave only the file manager to add. `NSWorkspace`
      (`activateFileViewerSelectingURLs:`, `openURL:`), `GtkFileLauncher`
      (`open_containing_folder`, GTK 4.10) and Windows's
      `SHOpenFolderAndSelectItems` and `Launcher` exist. The Web has neither
      (a page cannot reach the file manager), so it would return an error
      there and be listed in [Known limitations](known-limitations.md).
- [ ] **Reading the "reduce motion" setting.** naui animates nothing itself,
      but an app that animates a `Canvas` with a `Timer` should stop when the
      user asks for less motion. `NSWorkspace`'s
      `accessibilityDisplayShouldReduceMotion`, GTK's `gtk-enable-animations`,
      Windows's `UISettings.AnimationsEnabled` and the `prefers-reduced-motion`
      media query. Whether it is read once or also told when it changes, and
      whether it sits with the appearance from v0.19.0, is decided first.
- [ ] **Asking for the user's attention** when a background window finishes
      something. `NSApplication`'s `requestUserAttention:` (the Dock icon
      bounces) and Win32's `FlashWindowEx` exist. GTK 4 dropped the urgency
      hint and the Web has nothing but the tab's title, so whether desktop
      notifications (v0.21.0) cover the need is checked first; if they do,
      this moves to [Not planned](#not-planned).
- [ ] **Telling when the system sleeps and wakes, and its power state** (for
      dropping connections before sleep and refreshing after, or doing less
      work on battery). `NSWorkspace`'s `willSleep` and `didWake`
      notifications and `NSProcessInfo`'s low power mode and thermal state,
      and Windows's `PowerManager` and suspend and resume notifications. Linux
      has it only over DBus (logind's `PrepareForSleep`, UPower), not in GTK,
      and the Web tells only page visibility. Which of the three (sleep and
      wake, low power, thermal state) enough platforms share is checked
      first.

## v0.24.0

A first backend beyond the four platforms.

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
  change what is public rather than add to it are done before v1.0.0. Widgets in
  `Tree` items change `TreeItem`, whose fields are public, so a new field breaks
  apps that build it with a struct literal (more than one action on a toast,
  which changed `Toast::action` and `on_action`, was done in v0.18.0).
  Shortcuts outside the menu bar may rename or move `MenuShortcut`, and colors
  by role in `Canvas` may add a variant to `Paint`, which is not
  `#[non_exhaustive]`.
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
  with setters. Shared state objects that widgets observe are left to that
  layer too.
- **Swapping whole compositions by size** (a different widget tree per size
  class). It would rebuild the tree from a size callback, which the adaptive
  approach rules out. The cases it would cover are handled by `Stack::set_breakpoint`
  and `AdaptivePanes`.
- **A bottom navigation bar.** It is a phone pattern, and naui has no mobile
  backend (see the mobile entry in v0.24.0).
- **Font choosers.** WinUI has no font picker (only Win32's `ChooseFont`), and the
  Web can list fonts only through Local Font Access, which is available in Chrome
  and Edge from version 103 and not in Firefox or Safari. There is no common
  behavior to build on.
- **Widgets no platform has, in naui itself** (tags, badges, avatars, skeleton
  placeholders, ratings). naui composes a widget only where most platforms
  have the control and one lacks it (as `Toast` on the Web). The others are
  left to an extension crate (see v0.22.0).
- **Window chrome that only some platforms have:** always on top, transparent
  or blurred backgrounds, custom title bars, native window tabs, the
  document-edited mark. Each exists on one or two platforms, and an app that
  needs one reaches it through the window's native object.
- **Listing displays and placing windows on them.** Linux and the Web do not
  give a window's position (see the window section of
  [Known limitations](known-limitations.md)), so there is nothing common to
  build on beyond `WindowPlacement`.
- **Storing credentials** (the keychain, Credential Manager, Secret Service).
  The Web has no counterpart, and it involves no UI: crates such as `keyring`
  work beside naui as they are.
- **Tray icons.** The Web has no counterpart, and Linux needs DBus
  StatusNotifierItem. An app that needs one can add it as native code in its own
  crate.
