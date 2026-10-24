# Roadmap

This lists features commonly expected of a GUI toolkit that naui (as of v0.10.0)
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

Adaptive layout work (`Stack::set_orientation`, moving Windows's `Stack` to `Grid`,
automatic collapsing of `Sidebar`, and so on) is being pursued separately, so it is
not included here.

## P0: Do now

Small, painful to lack, and with counterparts on all four platforms.

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

## P1: Do next

Medium weight and frequently requested.

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
      testing APIs (`click`, `activate`, and so on) for app developers.
- [ ] **Broaden `Canvas`'s vocabulary** (images, clipping, rotation and scaling,
      gradients). Text width measurement is hard on WinUI, so it is postponed.
- [ ] **Make dragging `Table` column widths work on Windows and Linux too.** Linux
      has the native feature in `GtkColumnView`, while Windows uses a custom table,
      so it is heavy.
- [ ] **Multiple selection in `Tree`, and placing widgets in items.**
- [ ] **Make `Track::Fill` weights work in `Grid` on Windows and Linux.**
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

## Not planned

Things that do not fit naui's approach.

- **Free per-widget colors or corner radii, animations, custom-drawn widgets, GPU
  integration.** These go against the approach of not drawing widgets ourselves.
- **Declarative UI syntax, hot reload.** If built at all, they would be separate
  crates on top, not part of the core.
- **iOS, Android, embedded devices.** There are no mobile backends, and they
  cannot be checked in CI either, so they are on hold.
