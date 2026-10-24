# Roadmap

This lists features commonly expected of a GUI toolkit that naui (as of v0.11.1)
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

Items marked *(adaptive)* belong to the [adaptive layout](#adaptive-layout) track.
That track has its own order and proceeds alongside the other items, rather than
waiting until every P1 item is done.

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

Each step builds on the ones before it.

1. **Contract for changes after adding to a container** (P0). The base for moving
   children around safely.
2. **`Stack::set_orientation`** (P0).
3. **Main-axis `Fill` and `Spacer` in Windows's `Stack`** (P0).
4. **Automatic collapsing of `Sidebar`** (P1). Uses only the native mechanisms, so
   it does not wait for step 5.
5. **Size-based arrangements** (P1).
6. **`AdaptivePanes`** (P1).

`Flow` (P2) does not depend on these and can start at any time. The items in P3 are
decided after investigating.

## P0: Do now

Small, painful to lack, and with counterparts on all four platforms, or the
foundation the adaptive layout track builds on.

- [x] **`on_submit` for input fields** (`TextInput`, `PasswordInput`,
      `NumberInput`). Confirming with Enter is the first thing any form needs. The
      rule from `SearchInput`, "Enter during IME composition is not counted", can be
      reused as is.
- [x] **Notifications for focus entering and leaving** (`on_focus_change`). Used
      for input validation and saving when leaving a field.
- [x] **Controlling window state.** Position, maximizing, minimizing, full
      screen, whether the window can be resized, and reading that state. The Web
      only gets the Fullscreen API, and the rest is documented as unsupported.
      Added as `Window::set_state` / `state` / `on_state_change` (`WindowState`),
      `set_position` / `position`, and `set_resizable` / `is_resizable`. On Linux,
      GTK4 has no position API, so `position` is `None`.
- [x] **Right-click and double-click on `List`.** Combined with
      `PopupMenu::open_at`, right-click gives per-row context menus.
      (`List::on_context_menu` and `List::on_row_activate`; activation also arrives
      with Enter.)
- [x] **The wheel on `Canvas`, and distinguishing mouse buttons.** Just additions
      to `PointerEvent`, forming the basis for zooming and panning.
- [ ] *(adaptive)* **A contract for changes after adding to a container.** Settings
      made after a child is added (`set_sizing`, `set_spacing`, placement within a
      `Grid` cell) take effect, and a child moved to another container gets the
      constraints that depend on its new parent. Today macOS requires these to be
      set before adding. Also check that focus, scroll position and IME composition
      survive the move, and fix or document what does not.
- [ ] *(adaptive)* **`Stack::set_orientation`.** Switching between vertical and
      horizontal after creation. `NSStackView.orientation`, `GtkOrientable`,
      `Orientation` on WinUI panels, CSS `flex-direction`.
- [ ] *(adaptive)* **Make main-axis `Fill` and `Spacer` work in Windows's
      `Stack`.** `StackPanel` does not distribute leftover space, so the `Stack` is
      rebuilt on `Grid`. This removes the first row of the
      [platform differences](known-limitations.md#platform-differences-at-a-glance).

## P1: Do next

Medium weight and frequently requested.

- [ ] *(adaptive)* **Automatic collapsing of `Sidebar`** when the window narrows.
      `NavigationView`'s `PaneDisplayMode::Auto`, `NSSplitViewItem`'s
      `canCollapseFromWindowResize`, `AdwBreakpoint` (with `AdwOverlaySplitView`),
      CSS container queries.
- [ ] *(adaptive)* **Size-based arrangements.** Declaring in advance which layout
      to use above and below a width threshold. naui-core computes the thresholds,
      and the switching is left to `TwoPaneView`, `AdwBreakpointBin` and CSS
      container queries. Only macOS has no counterpart, so naui decides there. This
      also adds a `Size` type for thresholds (margins reuse `Padding`).
- [ ] *(adaptive)* **`AdaptivePanes`.** Showing list and detail side by side when
      wide, and one at a time when narrow. In the one-pane state a way back to the
      list is needed. `TwoPaneView`, `AdwNavigationSplitView`, `NSSplitView`, CSS
      container queries. The native ones are mostly two-pane, so how far three
      panes can go is checked when work starts.

- [ ] **Nested menus, menu items with check marks, and function-key shortcuts**
      (`MenuBar` and `PopupMenu`). All four platforms have them natively; for now
      this is a limitation on naui's side only.
- [ ] **Receiving file drops** (from the OS onto widgets and windows).
      `registerForDraggedTypes`, `GtkDropTarget`, `AllowDrop`, the DOM's `drop`.
- [ ] **Popovers** (attached to a widget, showing arbitrary content). `NSPopover`,
      `GtkPopover`, `Flyout`, HTML's `popover` attribute. Showing one beside a
      widget on the Web needs CSS anchor positioning, so the decision waits until
      browser support has been checked.
- [ ] **Dialogs with arbitrary content.** `ContentDialog`, `AdwDialog`, sheets,
      `<dialog>`. Much of the construction can be shared with popovers.
- [ ] **More image sources** (bytes in memory, SVG, pixel buffers). For now only
      paths and URLs. The Web can use Blob URLs.
- [ ] **Rich text** (mixing bold, colors and links within a line).
      `NSAttributedString`, Pango markup, `TextBlock.Inlines`, `<span>`.
- [ ] **Row windowing for large `List`s.** Extend `Table`'s mechanism
      (`row_window`) to `List`.
- [ ] **Spinning indicators** (Spinner). `NSProgressIndicator`, `AdwSpinner`,
      `ProgressRing`. The DOM has no counterpart, so the Web shows an indeterminate
      `<progress>` and the difference is documented.
- [ ] **Separators and framed groups.** `NSBox`, `GtkSeparator` / `GtkFrame`.
      WinUI has no standalone separator, so what to substitute is investigated
      first.
- [ ] **Button roles** (primary buttons, destructive actions). macOS's
      `hasDestructiveAction`, GTK's `suggested-action` / `destructive-action`.
      WinUI has only the primary button (`AccentButtonStyle`), and the Web has no
      counterpart, so the differences are documented.

## P2: Do if there is time

Heavy, or supported on a limited set of platforms.

- [ ] **Publish the testing facilities across platforms.** Gather the existing
      testing APIs (`click`, `activate`, and so on) for app developers. Also
      provide a way to resize windows in tests, so apps can check their adaptive
      layouts.
- [ ] *(adaptive)* **`Flow`** (a container that wraps its children onto new lines).
      `GtkFlowBox` and CSS `flex-wrap` exist, but WinUI only has
      `VariableSizedWrapGrid` with fixed-size cells (`WrapPanel` is in the
      Community Toolkit), and AppKit has nothing besides `NSCollectionView`'s flow
      layout. Which platforms need naui to lay it out itself is investigated first.
- [ ] **Broaden `Canvas`'s vocabulary** (images, clipping, rotation and scaling,
      gradients). Text width measurement is hard on WinUI, so it is postponed.
- [ ] **Make dragging `Table` column widths work on Windows and Linux too.** Linux
      has the native feature in `GtkColumnView`, while Windows uses a custom table,
      so it is heavy.
- [ ] **Multiple selection in `Tree`, and placing widgets in items.**
- [ ] **Make `Track::Fill` weights work in `Grid` on macOS and Linux.**
      `NSGridView` and `GtkGrid` have no weights, so naui distributes the space
      itself.
- [ ] **Desktop notifications.** macOS needs a bundle, Linux a `.desktop` file, and
      Windows registration, so the conditions for each distribution method need to
      be documented.
- [ ] **Images on the clipboard.**
- [ ] **Saving and restoring window position and size.** The P0 window state
      control it builds on is in, so this builds on top of it.
- [ ] **Relax the limits on dialogs and toasts** (number of buttons, toast queueing,
      position).

## P3: Decide after investigating

Not yet clear whether they fit the approach, or whether they are needed.

- [ ] **Tray icons.** GTK4 has none and needs DBus StatusNotifierItem. WinUI has
      none either, so it would be built with Win32, and the Web has no counterpart.
- [ ] **Drag and drop between widgets** (rearranging `List` or `Tree`).
- [ ] **Printing.**
- [ ] **Single instance, file associations, URL schemes.** Strongly tied to
      distribution and packaging.
- [ ] **Font choosers, and an always-visible calendar widget.**
- [ ] **Layout for right-to-left languages, and a translation mechanism.** First
      check how much the native toolkits fix automatically.
- [ ] **WebView, a code editor, Markdown rendering, charts.** These seem better
      suited to separate crates than to the core.
- [ ] *(adaptive)* **Switching navigation by size** (`Sidebar`, a narrow rail,
      a bottom bar). The rail is covered on Windows by `Sidebar`'s automatic
      collapsing (`LeftCompact`). The bottom bar is a phone pattern, so it waits
      until there is a reason to have it on the desktop.
- [ ] *(adaptive)* **Display shape: safe areas, folds and hinges.** Only the Web
      exposes this (the Viewport Segments API), and it cannot be checked in CI, so
      for now this is limited to investigating how the information could be
      obtained.
- [ ] *(adaptive)* **Swapping whole compositions by size** (a different widget tree
      per size class). First check whether size-based arrangements and
      `AdaptivePanes` leave real cases uncovered.

## Not planned

Things that do not fit naui's approach.

- **Free per-widget colors or corner radii, animations, custom-drawn widgets, GPU
  integration.** These go against the approach of not drawing widgets ourselves.
- **Declarative UI syntax, hot reload.** If built at all, they would be separate
  crates on top, not part of the core.
- **iOS, Android, embedded devices.** There are no mobile backends, and they
  cannot be checked in CI either, so they are on hold.
