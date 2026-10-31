# Roadmap

This lists features commonly expected of a GUI toolkit that naui (as of v0.13.0)
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
[internationalization](#internationalization).

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
container, and `Stack::set_orientation`) were released in v0.12.0, and step 4
(automatic collapsing of `Sidebar`) in v0.13.0.

5. **Size-based arrangements** (P0).
6. **`AdaptivePanes`** (P1).

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
  changes is done by the app (or an optional crate) holding widgets with
  `downgrade()` and calling `set_text`. This keeps the core free of a reactive
  system, in line with keeping declarative UI out of it.
- **Native strings follow the OS language.** Strings the native controls carry
  themselves (file dialogs, date pickers, text field context menus) use the OS's UI
  language, and most platforms cannot switch it per app while running. Switching
  languages inside a running app therefore leaves those in the OS language, so the
  default is to follow the OS language, and the differences are documented.

## P0: Do now

Gaps that ordinary apps run into right away and that all four platforms have native
counterparts for, plus the next steps of the adaptive layout and
internationalization tracks.

- [ ] **Popovers** (attached to a widget, showing arbitrary content). Settings
      panels and pickers next to a button are common, and for now an app has to
      fall back on a modal dialog. `NSPopover`, `GtkPopover`, `Flyout`, HTML's
      `popover` attribute. Placing it beside the widget on the Web uses CSS
      anchor positioning, which every major engine has supported since Firefox 147
      (January 2026), so it no longer has to wait. Newer parts such as
      `position-visibility` still differ between engines, so only the basic
      placement is used.
- [ ] **Dialogs with arbitrary content.** `ContentDialog`, `AdwDialog`, sheets,
      `<dialog>`. Much of the construction can be shared with popovers. The
      existing `Dialog` stays as the short form for a message and buttons.
- [ ] **Button roles** (primary buttons, destructive actions). Small, and dialogs
      with arbitrary content need it to mark their buttons. macOS's
      `hasDestructiveAction`, GTK's `suggested-action` / `destructive-action`.
      WinUI has only the primary button (`AccentButtonStyle`), and the Web has no
      counterpart, so the differences are documented.
- [x] *(adaptive)* **Size-based arrangements.** `Stack::set_breakpoint` declares
      the width (`Breakpoint`) from which a stack lays its children out in the
      other orientation; below it the stack keeps its own (`Stack::orientation`),
      and `Stack::current_orientation` returns the one in use. The switch is the
      same as `set_orientation`, so focus and IME composition survive it, and going
      back takes 8 pixels less. naui decides on all four platforms with
      naui-core's `StackArrangement`: the native mechanisms did not fit when
      checked. `TwoPaneView` holds only two panes and `AdaptiveTrigger` follows the
      window, `AdwBreakpointBin` takes away the minimum size of what it holds (as
      with `Sidebar`), and a CSS container query needs a wrapping element whose
      width no longer comes from its content (an `Auto` grid column shrinks it to
      nothing) while the children's direction-dependent styles are inline. macOS,
      Linux and Windows switch on the next run loop iteration after layout, and the
      Web inside a `ResizeObserver` before paint. Thresholds are widths only, so no
      `Size` type was added.
- [ ] *(i18n)* **A guide for translating apps.** With `Ui::preferred_languages`
      and `Ui::on_preferred_languages_change` in place, show how to combine Fluent
      with naui: choosing a translation from the preferred languages, loading it,
      and re-setting strings via `downgrade()` when the language changes. Whether
      an optional `naui-i18n` crate is needed is decided after writing it.

## P1: Do next

Medium weight and frequently requested, or a smaller gap between platforms.

- [ ] **Row windowing for large `List`s.** Extend `Table`'s mechanism
      (`row_window` in naui-core) to `List`.
- [ ] **Make `Track::Fill` weights work in `Grid` on macOS and Linux.** Today they
      are distributed evenly there. `NSGridView` and `GtkGrid` have no weights, so
      naui distributes the space itself.
- [ ] **Make Windows's `Canvas` keep up with large drawings.** On every redraw,
      all commands become one XAML string that `XamlReader` parses, and the result
      replaces the whole surface, so drawings with thousands of commands are slow.
      Candidates, investigated in this order: reusing the previous elements when
      only some commands change (may need `PathGeometry` in the projection),
      merging consecutive commands with the same style into one `Path`, and
      drawing with Direct2D onto a Composition drawing surface, the way the other
      three platforms rasterize. The last would also bring DirectWrite text
      measurement, which broadening `Canvas`'s vocabulary waits on.
- [ ] **Separators and framed groups.** `NSBox`, `GtkSeparator` / `GtkFrame`.
      WinUI has no standalone separator, so what to substitute is investigated
      first.
- [ ] **More image sources** (bytes in memory, SVG, pixel buffers). For now only
      paths and URLs. The Web can use Blob URLs.
- [ ] *(adaptive)* **`AdaptivePanes`.** Showing list and detail side by side when
      wide, and one at a time when narrow. In the one-pane state a way back to the
      list is needed. `TwoPaneView`, `AdwNavigationSplitView`, `NSSplitView`, CSS
      container queries. The native ones are mostly two-pane, so how far three
      panes can go is checked when work starts.
- [ ] **Rich text** (mixing bold, colors and links within a line).
      `NSAttributedString`, Pango markup, `TextBlock.Inlines`, `<span>`.
- [ ] *(i18n)* **Telling the native side the app's language.** The language
      affects which glyphs are chosen for CJK text and how screen readers
      pronounce it (`lang` on the Web, `FrameworkElement.Language` on WinUI).
      Without it, an app showing Japanese on an OS set to another language can
      get Chinese glyph forms. Which platforms can take it per window or per widget is
      investigated first.

## P2: Do if there is time

Heavy, or supported on a limited set of platforms.

- [ ] **Publish the testing facilities across platforms.** Gather the existing
      testing APIs (`click`, `activate`, and so on) for app developers. Also
      provide a way to resize windows in tests, so apps can check their adaptive
      layouts.
- [ ] **Relax the limits on dialogs and toasts** (number of buttons, toast queueing,
      position). Decided after dialogs with arbitrary content, which may cover part
      of it.
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
      gradients). Text width measurement on Windows waits for the `Canvas` drawing
      item in P1.
- [ ] **Multiple selection in `Tree`, and placing widgets in items.**
- [ ] **Make dragging `Table` column widths work on Windows and Linux too.** Linux
      has the native feature in `GtkColumnView`, while Windows uses a custom table,
      so it is heavy.

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
- [ ] **WebView, a code editor, Markdown rendering, charts.** These seem better
      suited to separate crates than to the core.
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
  integration.** These go against the approach of not drawing widgets ourselves.
- **Declarative UI syntax, hot reload.** If built at all, they would be separate
  crates on top, not part of the core.
- **iOS, Android, embedded devices.** There are no mobile backends, and they
  cannot be checked in CI either, so they are on hold.
