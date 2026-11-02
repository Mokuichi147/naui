# Known limitations

This page collects what naui cannot do and what behaves differently per platform.
Which controls are used on each platform, how they are assembled, and why they took
that shape are covered in
[Widget implementations](widgets.md#construction-per-platform).

## Platform differences at a glance

| Item | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| Weights of `Grid`'s `Track::Fill` | Works | Works (with one exception, see [macOS](#macos)) | Works | Works |
| Stacking in the same `Grid` cell | Possible | Only the last placed one remains | Possible | Possible |
| Dragging `Table` column widths | Not possible | Possible | Not possible | Not possible |
| `EditableComboBox` narrowing options by typed text | No | No (only prefix completion) | No | The browser narrows |
| Where `Toolbar` goes | Right of the title | Title bar (the title text is hidden) | Left side of the header bar | Start of the window |
| Where `MenuBar` goes | Below the title bar | The app's menu | Below the header bar | Start of the window |
| `Dialog::open` and `PopupMenu::open_at` | Return immediately | Do not return until closed | Return immediately | Return immediately |
| Where `Sheet`'s buttons go | The dialog's button row (primary on the left) | Bottom right (primary on the right), secondary at the bottom left | The header bar: cancel at the start, primary at the end | Bottom right (primary on the right) |
| Window position (`position` / `set_position`) | Possible | Possible | Not possible | Not possible |
| Restoring a saved window (`set_placement`) | Size, position and state | Size, position and state | Size and state | Size only (fullscreen needs a user action) |
| Maximizing and minimizing | Possible | Possible | Possible (minimizing cannot be detected on Wayland) | Not possible |
| Full screen | Possible | Possible | Possible | Only from within a user action |
| `Sender::send` from `std::thread::spawn` | Possible | Possible | Possible | Not possible (no threads) |
| What a dropped file carries (`on_file_drop`) | The absolute path | The absolute path | The absolute path (local files only) | A `blob:` URL, no path |
| Looks of `ButtonRole` | Only `Primary` (accent) | `Primary` (accent), `Destructive` (red text, macOS 26 and later) | Both (`suggested-action` / `destructive-action`) | Neither (only `data-naui-role` is written) |
| `AdaptivePanes` switching to one pane | `TwoPaneView` decides, below the breakpoint, with no slack | naui decides, with 8 px of slack | naui decides, with 8 px of slack (slides between the panes) | naui decides, with 8 px of slack |
| `Spinner`'s look and default size | A ring, 32 × 32 | Spokes, 32 × 32 | A ring, 16 × 16 (grows with the space up to 64 × 64) | A bar (indeterminate `<progress>`) |

## Common

### Scope

- The supported components are the 49 in the table in the
  [README](../README.md#widgets).
- There is no absolute positioning. Layout uses `Stack`, `Grid` and `Spacer`.
- Supported media formats depend on each OS, browser, and the GStreamer setup on
  Linux.
- The few strings naui shows itself (the default macOS main menu, the `Sidebar`
  toggle on the Web and Linux, the back button of `AdaptivePanes` (and the names of
  its panes on Linux), the accessible names of `Breadcrumbs` and
  `Pagination` on the Web, the "OK" of `DialogButtons::ok()`) come in English and
  Japanese only. Other languages get English. The language is chosen once when the
  app starts, from the OS's preferred languages (`NSLocale.preferredLanguages`,
  `GetUserPreferredUILanguages`, `navigator.languages`, and on Linux the
  `LANGUAGE` / `LC_ALL` / `LC_MESSAGES` / `LANG` environment variables, read the
  way gettext reads them for GTK's own strings, so `LANGUAGE` is ignored under the
  C locale), so changing it while the app runs takes effect on the next launch.
  This matches the strings the native controls carry themselves, which also stay
  in the language they started with.
- `Ui::on_preferred_languages_change` is never called on Linux, which decides the
  languages from environment variables at startup. Elsewhere it is called only
  when the list itself changes: a change of region or format alone does not call
  it. Whether a change reaches a running app at all depends on the OS: on
  Windows reordering the language list in Settings is told right away, but a new
  display language (which naui's own strings follow) takes effect after signing
  in again, and macOS may reopen the app instead of telling it.

### Widgets

- `SplitView` has only two panes and one divider. To split into three or more,
  nest them. There is no setting to collapse a pane (hide it at width 0).
- `AdaptivePanes` has two panes, a list and its detail, and the list's width is
  fixed: the user cannot drag the boundary (`TwoPaneView` and
  `AdwNavigationSplitView` have no divider to drag). For three panes, attach a
  `Sidebar` to the window or nest another `AdaptivePanes` in the detail. There is
  no notification when it switches between one pane and two (read
  `is_collapsed`), and the one-pane state never stacks the panes vertically
  (`TwoPaneView`'s `Tall` mode is not used). Only Linux has ways of going back
  besides the back button (Escape, Alt+←, the mouse's back button, a swipe).
- `Toolbar` is attached to a window and cannot be placed anywhere in the layout
  (because `NSToolbar` attaches to an `NSWindow`). Icons can only be chosen from
  the 20 kinds of `ToolbarIcon`; arbitrary images cannot be used.
- Like `Toolbar`, `MenuBar` is attached to a window and cannot be placed anywhere
  in the layout. Items can open submenus and show check marks, but **there are no
  radio items** (use check items and turn the others off with
  `set_item_checked`) and no icons in items. `PopupMenu` items show no shortcuts.
- `MenuShortcut` is either **the primary modifier key + one alphanumeric
  character** (the primary modifier is ⌘ on macOS and Ctrl on the other three
  platforms) or **a function key from F1 to F24**, on its own or with modifiers.
  Symbol keys and named keys (Delete, the arrow keys and so on) cannot be
  specified. Some function keys reach the OS or the browser first: macOS assigns
  F11 to Show Desktop by default and laptops may need fn, and browsers may keep F11
  (full screen) or F12 (developer tools) for themselves.
- `List` has one column. For data lists that need columns, use `Table`. `List`
  also lacks `Table`'s row windowing for large numbers of rows (building only the
  range shown on screen), so `Table` is also better suited for lists of more than
  tens of thousands of rows.
- Sorting by pressing a `Table` header only goes as far as notifying "which column,
  in which direction"; **sorting the rows is the app's job**.
- A `Table` with more than 200 rows builds only the range shown on screen (with
  either `set_rows` or `set_row_builder`). This leads to the following rules.
  - **All rows are assumed to have the same height.** If needed, set it with
    `set_row_height`.
  - naui does not know whether rows it has not built yet can be chosen, so by
    default they are assumed to be choosable (this can be passed with
    `set_row_selectable`).
  - Columns with widgets in their cells, if no width is specified, are sized only
    by **the header's width and their share of the leftover space** (so columns do
    not move even when content widths differ per row).
  - On the Web, when the number of rows × the row height exceeds the element height
    the browser can handle (about 33 million px in Chromium), the mapping between
    the scroll bar position and rows drifts (around 1 million rows for 30 px rows).
    Selection by index and `set_rows` work even with more than that.
- `Tree` has single selection, and its items can hold only strings. Items are
  addressed by paths (`&[usize]`), and they cannot be rearranged by dragging.
- Only one `Dialog` can be shown at a time, with at most three buttons: Primary,
  Secondary and Cancel. `Sheet` has the same three buttons, and its cancel button
  cannot be disabled (Esc closes it anyway).
- A `Sheet` takes its content's natural size (at least 320 px wide), so `Fill` in
  the content has nothing to fill. It cannot be resized by the user.
- Only one `Toast` is shown at a time too, and a newly shown one replaces the
  previous one (`AdwToastOverlay`'s queueing is aligned to this behavior). There is
  one action button, and the position is fixed at the bottom center.
- `Canvas`'s vocabulary is only fills, strokes and single lines of text; there is no
  image drawing, clipping, transforms (rotation or scaling), gradients, or text
  width measurement. Its size is not decided by its content, so specify it with
  `set_sizing`.
- `Canvas` pointer notifications distinguish buttons but not modifier keys (⇧,
  Ctrl, and so on). Trackpad pinches do not arrive as zooming (on the Web,
  Chromium-based browsers and Firefox deliver them to `on_wheel` as wheel events
  with Ctrl). The amount per wheel notch is normalized to `WHEEL_STEP`, but when a
  browser on the Web passes pixels, the browser's amount is kept.
- `on_file_drop` takes only files from the OS. Dragging between widgets (to
  rearrange `List` or `Tree` rows), dragging out of the app, and dropped text or
  links are not supported. Drops onto text-input widgets go to the field itself
  and do not reach `on_file_drop`.

### Window

- There is no notification when a window's position changes (because the position
  itself cannot be obtained on Linux and the Web). To save the position, read
  `Window::placement` before closing (for example inside `on_close_request`).
- On Windows and macOS, where positions are supported, `position` /
  `set_position` while maximized, minimized or full screen read and write the
  position for when the window returns to the normal state. A requested position
  takes effect when it returns to the normal state. `set_size` does the same with
  the size on Windows, macOS and Linux.
- `placement` / `set_placement` restore only the size and state on Linux, and only
  the size on the Web (the size requested with `set_size`, not the browser's;
  fullscreen is refused outside a user action). naui does not store the placement
  itself: the app writes its text form to its own settings.

### Other threads and async

- When handing values over from other threads, order is preserved **only within
  the same channel**. With several channels, the delivery order across channels is
  not defined.
- Channels are unbounded. Sending many values in a short time **does not drop a
  single value**, but because multiple notifications are processed together before
  the next draw, **intermediate states may not be visible and only the last state may
  be drawn**. Throttle high-frequency progress or telemetry on the app side.
- The `Ok` returned by `Sender::send` means "the value was accepted and its delivery
  to the UI was scheduled", not a guarantee that it was delivered. If the screen
  closes right after scheduling, it does not run.
- `Tasks::spawn` accepts only futures that return no value. Write the result to
  widgets directly inside the future. Dropping the returned `Task` does not stop the
  task (call `Task::cancel` to stop it).
- A panic inside a receiving closure or future only stops that piece on the spot;
  the app does not unwind. Unwinding across the WinRT / GLib / libdispatch
  boundaries is not possible, so all four platforms behave this way (the panic
  message goes to standard error as usual). With `panic = "abort"` it cannot be
  caught.

## Windows

### Layout

- When several children along a `Stack`'s main axis have `Fill`, they come out the
  same size (`Grid`'s `Star` tracks share out the whole leftover area). Linux and
  the Web instead give each its content size plus an equal share of what is left,
  and macOS does not define how the space is split.
- A `Grid`'s `Fill` track gets its weighted share even when that is narrower than
  its content, which is then cut off (a `Fill` child does not claim its content's
  minimum, so it never pushes the window wider). The Web does the same
  (`min-width: 0` under `fr`). macOS and Linux keep a `Fill` track at what its
  content needs while there is room and share the rest by weight. Give the child
  a `min_width` to get that everywhere.

### Widgets

- `ButtonRole::Destructive` **looks like an ordinary button**. WinUI has a style
  only for the primary button (`AccentButtonStyle`), not for destructive actions.
- Over a `SplitView` divider, no divider-specific cursor (⇔) is shown.
- `NumberInput` is limited to **10 significant digits**. `NumberBox` rounds the
  value to 10 significant digits before building its display, so passing
  `set_decimals` a number of digits that would require more than 10 significant
  digits is still cut off. Numbers are right-aligned (Linux and the Web align them
  left). Since the step buttons and clear button sit beside it, give it more width
  than a single-line input (naui's Gallery uses 200 px).
- `NumberInput` and `EditableComboBox` pick up per-keystroke notifications from
  the input field in the `NumberBox` / `ComboBox` template. If the standard
  template is replaced and the input field cannot be found, they notify only on
  confirmation (for `EditableComboBox`, choosing an option and Enter).
- `SearchInput` shows no suggestion list.
- `DatePicker`'s calendar is fixed to the Gregorian calendar. The order and
  notation of year / month / day follow the system locale. For `set_range` of
  `DatePicker` and `TimePicker`, the WinUI controls only support a range of years
  (`TimePicker` supports no range at all), so naui clamps out-of-range values the
  user picks to the ends.
- Branches set to `TreeItem::enabled(false)` in a `Tree` can still be expanded and
  collapsed.
- `Video` / `Audio` show no standard playback bar. This avoids crashes in some
  Windows App SDK environments (for the same reason, `Tabs` does not use
  `TabView`).
- `Canvas` text uses Segoe UI Variable (Segoe UI on Windows 10) drawn by
  DirectWrite, not XAML's `TextBlock`, so its lines can sit a pixel or so apart
  from a `Label` of the same size. A surface wider or taller in physical pixels
  than the GPU's texture limit (16384, or 2048 to 8192 on older GPUs) is drawn at
  a lower resolution and stretched, so it looks softer.
- Two-finger scrolling on a precision touchpad over a `Canvas` inside a `Scroll`
  does not reach `on_wheel`; the outer `Scroll` moves instead. `ScrollViewer`
  receives touchpad gestures itself, and they do not become wheel notifications
  (mouse wheels do arrive).
- `Dialog`, `Sheet` and `Toast` cannot be shown before `window.show()`.
- `Sheet` keeps the width `ContentDialog` allows, between 320 and 548 px
  (`ContentDialogMinWidth` / `ContentDialogMaxWidth`), so wider content is squeezed.
- `MenuBar` shortcuts are picked up from the window's `KeyDown`, so a key that the
  focused control handles itself does not reach them. Whether F10 and Alt
  combinations, which Windows uses for its own menu keys, arrive has not been
  checked on a real device.

### Window

- `Window::position` **includes the invisible border Windows adds around windows
  (the resize grips)**. It points a few pixels above and to the left of the visible
  frame's top-left corner, but passing the value read as is to `set_position`
  returns the window to the same place.
- The position is converted to logical pixels with this window's display scale, so
  when crossing screens with different DPI, the value is counted with this window's
  scale. `placement` / `set_placement` count it with the primary screen's scale
  instead, so a saved position comes back to the same place on any screen; on a
  screen whose scale differs from the primary one, it differs from `position`.
  Restoring across screens with different DPI has not been checked on a real
  device.
- `on_file_drop` is called a moment after the drop, once
  `GetStorageItemsAsync` has read the files. Files without a path (such as the
  inside of a zip shown by Explorer) are left out. Drops onto the title bar go to
  the system, not to the window.

### Other threads and async

- `Sender::send` fails after naui's UI runtime has ended (because the
  `DispatcherQueue` no longer accepts work).

## macOS

### Layout

- `Grid`'s `Fill` tracks split the room by weight through the `Fill` children
  (without a maximum) in them: each wishes for its track's share, and a `Fill`
  track without such a child takes what is left. **When two or more `Fill`
  tracks hold no such child, how the room is split between them is left to
  `NSGridView`**, which has no weights.
- **Children cannot be stacked in the same `Grid` cell.** The one placed later
  removes the earlier one (because `NSGridCell` can hold only one view).
- **Removing a merged cell (a child with a span) from a `Grid` leaves the merge
  itself in place**, because `NSGridView` has no API to unmerge. naui treats the
  merged range **as one cell**, so a one-cell child placed in its spot has the same
  position and size as in a normal cell. There are two differences: a `Fill` child
  extends over the merged range, and placing a child at another position within the
  range removes the previous child because it is the same cell.
- The height of `Grid`'s `Auto` rows, the share of `Fill` tracks, and the widths
  of `Table` columns without a specified width are calculated by naui (an
  **exception** to "no layout calculation on the Rust side"), because `NSGridView`
  and `NSTableView` do not decide where leftover space goes. Auto Layout makes the
  final decision on positions and sizes
  ([construction](widgets.md#macos-construction)).

### Widgets

- `ButtonRole::Destructive` sets `hasDestructiveAction`, but **only macOS 26 and
  later give it a look** (red text on a faint red bezel, the same as destructive
  buttons in alerts). Before that it looks like an ordinary button.
  `ButtonRole::Primary` draws the accent color like the window's default button,
  but is not given the Return key equivalent, so Return does not activate it.
- `Toggle` switches **only when the switch is pressed**; pressing the label text
  does not switch it.
- `EditableComboBox` completes the rest when you type characters matching the start
  of an option, but **does not narrow the option list itself**.
- `Table` row height is a fixed value derived from the system font. Rows do not
  grow individually to fit their content.
- `Image` loads remote URLs synchronously, so local files are recommended.
- `Expander` has no expand/collapse animation.
- `Dialog::open` and `PopupMenu::open_at` do not return until closed.
- A `Toast` appears in the frontmost window (or the most recently created window
  when none has been decided yet).
- Attaching a `Toolbar` hides the window's title text (the macOS convention). The
  `set_title` value is kept, and removing the toolbar with `clear_toolbar` brings
  the display back. Separators are fixed-width gaps (`NSToolbarSpaceItem`), with no
  line drawn. Rearranging by the user ("Customize Toolbar") is not possible.
- `MenuBar` attaches to the app, not the window (`NSApplication.mainMenu`).
  **naui always keeps the app-name menu and the "Edit" menu**, so the headings the
  app passes are placed between those two. Since ⌘C / ⌘V are delivered as key
  equivalents of the main menu, removing them would break pasting into text input.
  To fully control the order, take out the `NSMenu` with
  `menu_bar.backend().native_menu()`.
- AppKit's own strings (such as the default button of an `NSAlert`) follow the
  localizations of the app's bundle, not the OS language. An app run without a
  bundle, or with a bundle that declares no localizations, gets them in English.

### Window

- `WindowState::Maximized` is `zoom:` (the same as pressing the green button while
  holding Option). Pressing the green button as is enters full screen
  (`Fullscreen`). `isZoomed` is decided by whether the frame matches the screen's
  work area, so it may return `Maximized` even when the user has manually enlarged
  the window to fill the screen.
- Entering and leaving full screen completes after an animation, so `state` right
  after `set_state` is still the previous state. A state requested during the
  transition is applied after the animation ends.
- A position requested with `set_position` while maximized takes effect after
  passing once through the original place on the way back to the original size.
  When the user has manually enlarged the window to fill the screen (when it became
  `isZoomed` without going through `zoom:`), there is no known place to return to,
  so the visible frame is read and written as is.

## Linux

### Layout

- `SplitView` panes cannot be made smaller than the minimum size their content
  reports (because `GtkPaned` does not move the divider past a child's minimum).

### Widgets

- Themes are applied to the whole app, not per window.
- `TextStyle::Title` and `TextStyle::Subtitle` of `Label::set_style` have the same
  size and differ only in weight (`Heading` and `Body` have the same relationship).
  This is because libadwaita gives `.title-2` and `.title-3` the same size, and
  naui does not decide sizes itself.
- `Toggle` switches **only when the switch is pressed**; pressing the label text
  does not switch it.
- `EditableComboBox` does not narrow options by typed text (pressing the arrow shows
  all options).
- `Spinner` is `AdwSpinner`, which arrived in libadwaita 1.6. With libadwaita 1.5 it
  falls back to `GtkSpinner`, which has the same default size and 64 × 64 drawing
  limit but the older look.
- In `DatePicker`, **choosing a day does not close the popover** (because
  `GtkCalendar` does not distinguish "a day was pressed" from "the month was
  changed"). The date shown on the button follows the locale's format.
- `ColorPicker` does not handle opacity.
- In a `Sheet`, **Enter in a text field does not press the primary button**: a
  `GtkText` activates the dialog's default widget only with `activates-default`,
  which naui's text fields leave off. Clicking it or Enter with the focus elsewhere
  in the dialog does.
- With `List` set to `SelectionMode::Multiple`, selection with Ctrl / Shift is left
  to GTK4, so **row clicks (`ListRow::on_activate`) arrive on a double press**.
- `Fit::None` corresponds to GTK4's `SCALE_DOWN`, so it behaves as "do not enlarge"
  rather than "original size".
- `Toast` durations are in **seconds**. Values under one second become one second
  (rounding to 0 would turn it into "never disappear").
- `MenuBar` goes **below** the header bar. Shortcuts work without opening the menu,
  and when there are several windows, only the items of the focused window are
  invoked.

### Window

- **Window positions can be neither read nor set.** GTK4 has no API for window
  positions. On Wayland there is no mechanism at all for an app to know its
  window's position (the compositor decides placement), and GTK4 removed it on X11
  as well to match. `Window::position` always returns `None`, and
  `Window::set_position` does nothing.
- **Minimizing cannot be detected on Wayland.** xdg-shell has no mechanism to report
  the minimized state, so minimizing does not make `state` become
  `WindowState::Minimized`, and whether requesting `Normal` restores it depends on
  the compositor. On X11 both work.
- **Minimizing requested before showing is applied after showing.** The window
  manager does not respond to a request made before the window is shown, so it
  appears on screen for a moment.
- Maximizing, minimizing and full screen are requests to the compositor (window
  manager), so `state` changes after the compositor responds. On X11 without a
  window manager (Xvfb and so on), nobody responds to requests made after showing,
  and the state does not change (maximizing and full screen requested before
  showing do take effect when the window appears).
- `on_file_drop` passes only files with a local path. Files offered only as URIs
  (remote locations that GVFS does not mount) are left out.

## Web

### Widgets

- `ButtonRole` **does not change the look**. Browsers have no look for primary or
  destructive buttons, so naui only writes the role as the `data-naui-role`
  attribute (`"primary"` / `"destructive"`). An app that wants a look can select
  it in its own CSS, such as `button[data-naui-role="primary"]`.
- `Window` is not an OS window; it is represented by an element directly under
  `<body>` and `document.title`.
- `FilePicker::open` and `FileSaver::open` must be called inside a user-action
  event.
- In browsers without `showSaveFilePicker` (Firefox / Safari), `FileSaver` becomes an
  `<a download>` download, and no destination confirmation may appear.
  `FileEntry::path` is `None` either way.
- The input UIs of `DatePicker`, `TimePicker` and `ColorPicker` (calendars,
  spinners, palettes and so on) are shown by the browser, so their look and
  operation differ per browser. `TimePicker` handles values up to minutes even when
  the browser returns seconds. `ColorPicker` notifies only on confirmation
  (`change`).
- In `EditableComboBox`, the browser narrows options by typed text. It cannot be
  turned off, and how it narrows (prefix or substring match), whether an arrow is
  shown, and how the list opens differ per browser.
- The DOM has no spinner, so `Spinner` is an indeterminate `<progress>` and **is
  drawn as a bar** (its look and width follow the browser). Stopping it hides it
  with `visibility: hidden`, keeping its space as on the other platforms.
- Only Safari 17.4 and later support `Toggle`'s `switch` attribute, so Chrome and
  Firefox show it as a check box (value handling, notifications and screen reader
  announcements are the same).
- `Expander::set_enabled(false)` only removes the heading from mouse interaction and
  the tab order so it cannot be pressed (because `<details>` has no `disabled`).
  `aria-disabled` is set, but it is treated differently from a natively disabled
  control.
- Rows of arbitrary content in `List` (`ListRow`) put buttons and input fields
  inside `role="option"`, so **they deviate from ARIA's rule against placing
  interactive content inside an option**. Tab reaches the controls inside rows, and
  their names and roles are announced, but in a screen reader's browse mode the row
  may be read as a single item and the controls inside may not appear.
- `Tree`'s `TreeItem::detail` fits on one line in the form `label — detail` to keep
  row heights uniform.
- For success, warning and danger in `Label::set_color`, browsers have no
  corresponding system colors, so naui decides the colors with `light-dark()`.
  Also, **Chromium-based browsers return gray for `AccentColor` instead of the OS
  accent color**, so `TextColor::Accent` becomes the OS color only in Firefox and
  Safari.
- Browsers have no standard icon set, so naui carries SVGs for `Toolbar` icons (the
  only place where OS assets are not used).
- `MenuBar` shortcuts **accept either Ctrl or ⌘ as the primary modifier** (because
  ⌘ is the primary modifier in browsers on macOS). When pressed, the browser default
  (such as ⌘S or F5's reload) is prevented. The menu bar of a hidden window does not react to keys,
  and when a page has several windows, only the menu bar of the window the key
  bubbled up to receives it.
- In `MenuBar` and `PopupMenu`, submenus open on click or hover and close with
  Escape or a click outside, but **the arrow keys do not move between items**.
  Browsers draw no check marks or submenu arrows for menu roles, so naui puts `✓`
  and `▸` as text beside the label (hidden from screen readers, which read
  `aria-checked` and `aria-haspopup`).
- `Popover` is placed with CSS anchor positioning. In a browser without it (Firefox
  before 147, Safari before 26, Chromium before 129), it is shown in the middle of
  the window instead. **WebKit (Safari 26) chooses the side as if the document had
  not scrolled**, so a popover over a scrolled page can appear on the wrong side.
  naui's windows do not scroll the document itself (`Scroll` scrolls inside an
  element), so this only matters for pages that put a window below other content.
- `Popover::on_close` arrives with the browser's `toggle` event, a task after the
  popover has closed (`is_open` already returns `false` before that).
- Due to browser restrictions, autoplay of media may be refused.

### Window

- The only thing a page can control is the Fullscreen API, so **the only window
  state is full screen**. `Window::position` is always `None`, `set_position` does
  nothing, requesting `WindowState::Maximized` / `Minimized` only leaves full
  screen, and `set_resizable` only remembers the value.
- Full screen is applied to the whole page (`document.documentElement`), so if the
  page has several windows, all of them go into full screen. Browsers only allow
  full screen from within a user action (a click or key press), so
  `set_state(WindowState::Fullscreen)` at startup or from a timer is refused, and
  the state does not change. Safari on iPhone has no Fullscreen API.
- `on_file_drop` receives contents, not paths. Each file's `source` is a `blob:`
  URL that stays valid until the next drop onto the same widget or window, or
  until that widget or window is dropped. A dropped folder comes with only its
  name. Once any `on_file_drop` is set, file drops that nothing takes are refused
  for the whole page, so the browser does not replace the page with the dropped
  file. Drops onto text fields are among them: they reach neither `on_file_drop`
  nor the field.

### Other threads and async

- **wasm has no threads.** `std::thread::spawn` cannot be used, so put the side
  that sends to a `Sender` inside a future run with `Tasks::spawn`. The `Tasks` API
  itself is the same on all four platforms, so no `cfg` branching is needed, but
  **code that includes `std::thread::spawn` does not run on the Web as is**.
- A Web Worker is a separate JavaScript runtime, so even though `Sender` is `Send` as
  a Rust type, it cannot be passed there (bridge with `postMessage` and call `send`
  on the receiving side).
- `Tasks::spawn` advances on the microtask queue. Running a future that never
  yields, or one that keeps waking itself, starves drawing.
- Browsers have no app termination, so `Sender::send` never fails because the
  channel is closed.
