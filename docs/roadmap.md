# Roadmap

This lists features commonly expected of a GUI toolkit that naui (as of v0.12.0)
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
container, and `Stack::set_orientation`) were released in v0.12.0.

4. **Automatic collapsing of `Sidebar`** (P0). Uses only the native mechanisms, so
   it does not wait for step 5.
5. **Size-based arrangements** (P1).
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

- [x] **Receiving file drops** (from the OS onto widgets and windows). Opening a
      file by dropping it is expected of any app that edits files, and all four
      platforms have it natively. `registerForDraggedTypes`, `GtkDropTarget`,
      `AllowDrop`, the DOM's `drop`. Every widget and `Window` have
      `on_file_drop`, and the files arrive as the same `FileEntry`s `FilePicker`
      returns: absolute paths natively, `blob:` URLs on the Web.
- [x] *(adaptive)* **Automatic collapsing of `Sidebar`** when the window narrows.
      On by default (`Sidebar::set_auto_collapse`): it collapses when the content
      beside it would get narrower than `SIDEBAR_COLLAPSE_CONTENT_WIDTH` (360),
      expands again when the window widens, and notifies through `on_collapse`.
      `NSSplitViewItem`'s `canCollapseFromWindowResize` and `NavigationView`'s
      `PaneDisplayMode::Auto` decide on macOS and Windows. Linux and the Web decide
      in naui (`SidebarAutoCollapse`): an `AdwBreakpoint` takes away the minimum
      size of the window (or `AdwBreakpointBin`) holding it, and a CSS container
      query cannot tell the page when it applies.
- [x] **Saving and restoring window position and size.** `Window::placement`
      returns the normal-state size and position with the state as a
      `WindowPlacement` (one line of text with `to_string` / `parse`), and
      `Window::set_placement` puts it back, skipping a position on no connected
      screen. `set_size` while maximized or in fullscreen now decides the size for
      returning, like `set_position`. Linux restores only the size and state, and
      the Web only the size. Storing it is left to the app.
- [x] *(i18n)* **Write naui's error messages in English.** `Error` displays as
      "failed to {context}: {detail}", with the context as a verb phrase
      ("create the Button"). The details, the warnings printed to stderr, the
      panic messages and the errors of the Windows App SDK build script are
      English too. These are messages for developers, so they are not translated
      (like `#[deprecated]` notes).
- [x] *(i18n)* **The OS's preferred languages, and notification when they
      change.** `Ui::preferred_languages` returns them as BCP 47 tags, and
      `Ui::on_preferred_languages_change` is called only when the list actually
      changes: on `NSCurrentLocaleDidChangeNotification` (macOS), `WM_SETTINGCHANGE`
      with `"intl"` (Windows) and `languagechange` (Web). Linux reads them from
      environment variables at startup (`g_get_language_names`, `ja_JP.UTF-8`
      becoming `ja-JP`), so it is never called there. naui's own strings keep the
      language chosen at startup, like the native controls' own strings, so one
      screen does not mix two languages.

## P1: Do next

Medium weight and frequently requested, or a smaller gap between platforms.

- [ ] **Popovers** (attached to a widget, showing arbitrary content). `NSPopover`,
      `GtkPopover`, `Flyout`, HTML's `popover` attribute. Showing one beside a
      widget on the Web needs CSS anchor positioning, so the decision waits until
      browser support has been checked.
- [ ] **Dialogs with arbitrary content.** `ContentDialog`, `AdwDialog`, sheets,
      `<dialog>`. Much of the construction can be shared with popovers.
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
- [ ] **Button roles** (primary buttons, destructive actions). macOS's
      `hasDestructiveAction`, GTK's `suggested-action` / `destructive-action`.
      WinUI has only the primary button (`AccentButtonStyle`), and the Web has no
      counterpart, so the differences are documented.
- [ ] **Separators and framed groups.** `NSBox`, `GtkSeparator` / `GtkFrame`.
      WinUI has no standalone separator, so what to substitute is investigated
      first.
- [ ] **More image sources** (bytes in memory, SVG, pixel buffers). For now only
      paths and URLs. The Web can use Blob URLs.
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
- [ ] **Rich text** (mixing bold, colors and links within a line).
      `NSAttributedString`, Pango markup, `TextBlock.Inlines`, `<span>`.

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
- [ ] *(i18n)* **Telling the native side the app's language.** The language
      affects which glyphs are chosen for CJK text and how screen readers
      pronounce it (`lang` on the Web, `FrameworkElement.Language` on WinUI).
      Which platforms can take it per window or per widget is investigated first.
- [ ] *(i18n)* **A guide for translating apps.** Show how to combine Fluent with
      naui (loading translations, re-setting strings via `downgrade()` when the
      language changes). Whether an optional `naui-i18n` crate is needed is
      decided after writing it.

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
