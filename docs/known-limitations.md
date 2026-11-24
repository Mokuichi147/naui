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
| Where dragged `List` rows will land | The rows open a gap | A line between the rows | A line in the accent color | A line in the `Highlight` system color |
| Scrolling while dragging `List` rows near the edge | No | Yes | No | As the browser does |
| Moving `List` rows with the keyboard | Alt + Shift + Up / Down (`ListView`'s own) | No | No | No |
| What a dropped file carries (`on_file_drop`) | The absolute path | The absolute path | The absolute path (local files only) | A `blob:` URL, no path |
| Looks of `ButtonRole` | Only `Primary` (accent) | `Primary` (accent), `Destructive` (red text, macOS 26 and later) | Both (`suggested-action` / `destructive-action`) | Neither (only `data-naui-role` is written) |
| `AdaptivePanes` switching to one pane | `TwoPaneView` decides, below the breakpoint, with no slack | naui decides, with 8 px of slack | naui decides, with 8 px of slack (slides between the panes) | naui decides, with 8 px of slack |
| `Label` holding links | Selectable as asked | Always selectable | Selectable as asked | Selectable as asked |
| Where `Group`'s heading goes | Above the card | On the frame's top edge | On the frame's top edge | On the frame's top edge (`<legend>`) |
| Images written to the clipboard (`set_image`) | PNG (and a bitmap without transparency for apps that read no PNG), kept after the app quits | PNG and TIFF, kept after the app quits | Offered by the app while it runs (unless a clipboard manager keeps it) | `image/png` only, inside a user action (Firefox 127 and later) |
| Where a `Toast` appears (`ToastPosition`) | Any of the six | Any of the six | Always the bottom center | Any of the six |
| Where a `Toast`'s second and later action buttons go | A row below the text | Next to the first, after the text | Next to the first, after the text | Next to the first, after the text |
| A `Flow` child wider than the flow | Keeps its width and is cut off, unless it has `Fill` on the width | Keeps its width and is cut off, unless it has `Fill` on the width | Narrowed to the flow, down to its minimum | Narrowed to the flow, down to its `min-content` width |
| `Spinner`'s look and default size | A ring, 32 × 32 | Spokes, 32 × 32 | A ring, 16 × 16 (grows with the space up to 64 × 64) | A bar (indeterminate `<progress>`) |

## Common

### Scope

- The supported components are the 52 in the table in the
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
- `Settings::language` is read once at startup; macOS and Linux cannot change the
  app's language while it runs (Pango reads its preferred languages once, and
  AppKit and Core Text the locale). It applies to the whole app: no platform but
  the Web and Windows can take a language per widget. On Windows naui sets it on each
  window's content; a `Dialog` or `Sheet`, which WinUI shows in a separate layer,
  is not given it. On macOS it is listed first by `Ui::preferred_languages`, but
  the native controls' own strings are not switched by it: AppKit chooses them
  itself, and the file dialog stayed in Japanese for `ja-JP`, `en-US` and `zh-CN`
  alike in an app bundle whose development region is Japanese. On Linux it is
  put into `PANGO_LANGUAGE` only when `run` finds no other thread in the process
  (counted from `/proc/self/task`), since changing the environment while another
  thread reads it is undefined behavior. An app that starts threads (an async
  runtime, say) before `run` keeps the OS's glyph choice; naui's own strings still
  follow the language, and setting `PANGO_LANGUAGE` before launch works.
- `Settings::layout_direction` is read once at startup and applies to the whole
  app. AppKit decides the direction when `NSApplication` is created, and Auto
  Layout's leading and trailing follow the app's direction, not a view's (a view
  given `userInterfaceLayoutDirection` alone was not mirrored), so no platform
  switches it per widget or while the app runs. Without `Settings::language`,
  `LayoutDirection::Auto` leaves each toolkit's default, which differs: GTK follows
  the locale (an untranslated app is mirrored when `LANG` is Arabic), while AppKit,
  WinUI and the Web stayed left to right. On macOS no case was found where AppKit
  mirrors an app by itself: an unbundled binary with `AppleLanguages` set to `ar`,
  and an app bundle localized into Arabic launched with `-AppleLanguages (ar)`
  (directly and through `open`), all stayed left to right, and only
  `AppleTextDirection` turned the direction over. So `LayoutDirection::Ltr` has
  not been tried against an app AppKit would mirror on its own; naui then writes
  `AppleTextDirection` and `NSForceRightToLeftWritingDirection` as `NO`. On Windows naui sets
  `FlowDirection` on each window's content and on `Dialog` and `Sheet`; a
  `Popover` (a `Flyout`), the menu bar's menus and context menus take it from the
  element they open at. `PopoverPosition::Left` and `Right` stay on the physical
  sides on every platform: GTK's popover does not swap them, nor do AppKit and the
  Web, while WinUI's `Flyout` would, so naui swaps them back on Windows. The
  playback bar of `Video` is AVKit's on macOS, which keeps it left to right in a
  right-to-left app by itself; naui pins it left to right on the other platforms.
  On Windows the whole window's content is mirrored, naui's own title bar and menu
  rows included, but the system keeps the minimize, maximize and close buttons at
  the right end, so the title and toolbar sit just left of them (the title at the
  right, the toolbar on its left) rather than moving to the left edge as in a
  mirrored Win32 window. On Windows the text of a `TextInput` and `TextArea`
  (typed Latin text and the placeholder) stays at the left in a right-to-left
  layout, while `PasswordInput` and `SearchInput` put theirs at the right; text in
  a right-to-left script has not been tried there.
  Horizontal scroll positions in a right-to-left layout (`Scroll::scroll_to`'s
  `x` and the metrics `Scroll` reports) are left to each toolkit and have not been
  aligned: the Web's `scrollLeft` runs from 0 toward negative values there, while
  AppKit's offsets stay physical.
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
- `Flow` lays its children out in rows only: there is no vertical flow that wraps
  into columns, which would need a height to wrap in. Each line is packed on its
  own, so children are not lined up in columns (use a `Grid` for that), and
  `set_align(Align::Fill)` is the same as `Start`: only `Fill` on a child's width
  widens it to fill its line. On Windows and macOS a child is measured once at its
  natural width, so one wider than the flow is not narrowed (its text does not
  wrap) unless it has `Fill` on the width; Linux narrows it to its minimum and the
  Web to its `min-content` width.
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
- A `MenuItem`'s `Shortcut` is either **the primary modifier key + one
  alphanumeric character** (the primary modifier is ⌘ on macOS and Ctrl on the
  other three platforms) or **a function key from F1 to F24**, on its own or with
  modifiers. Symbol keys and named keys (Delete, the arrow keys and so on) cannot
  be given to a menu item (windows and widgets take the named keys). Some function keys reach the OS or the browser first: macOS assigns
  F11 to Show Desktop by default and laptops may need fn, and browsers may keep F11
  (full screen) or F12 (developer tools) for themselves.
- `List` has one column. For data lists that need columns, use `Table`.
- A `List` with more than 200 rows and a limited height (`Fixed` or `Fill`) builds
  only the range shown on screen. With an `Auto` height every row is built, because
  the list is as tall as all its rows. This leads to the following rules.
  - **All rows are treated as having the same height** (except on macOS, where
    `NSTableView` measures each row as it appears). Rows are given the tallest height
    built so far, so a taller row appearing later moves the scroll position once. If
    heights vary, set it with `set_row_height`.
  - naui does not know whether rows it has not built yet can be chosen, so with
    `set_row_builder` they are assumed to be choosable by default (this can be passed
    with `set_row_selectable`).
  - A control inside a row loses the keyboard focus when its row leaves the range
    built, because the row is discarded. While the row stays in the range it is left
    as it is, so what the builder returned for it only changes with `refresh`.
  - On the Web, such a list is built from `<ul role="listbox">` instead of `<select>`,
    and the same limit on the element height as for `Table` applies. naui handles the
    keys of a `<select>` itself, but typing to jump to a row works only for rows passed
    as text: rows from `set_row_builder` have no text until they are built.
  - Assistive technology is told each row's position among all the rows and how many
    there are, because the native list knows only the rows built. On Linux the values
    are set on every GTK version, but GTK sends them over AT-SPI only from 4.20; with
    an earlier GTK, assistive technology sees just the rows built.
- On Windows, an `Auto` height on a `List` with many rows can make the app stop
  responding when a UI Automation client (a testing tool, and possibly a screen
  reader) walks its rows. Mouse and keyboard use are not affected. The cost grows
  steeply with the rows the `ListView` has built and comes from WinUI's `ListView`,
  which a plain `ListView` without naui shows too. On a development machine, listing
  every element took about 1.3 seconds at 200 rows and 9 seconds at 400. At 5000 rows
  each step to the next row took about 3.6 seconds, and listing every element timed
  out and left the app unresponsive for more than five minutes. With `Fixed` or `Fill`
  only the rows on screen are built, so the same 5000 rows are listed in under 0.1
  seconds: **use one of those for a list that may have more than about 200 rows.**
- On Windows, a window opened right after the keyboard moved through the rows of a
  `List` or `Table` can take seconds to lay out for the first time, and to show a scroll
  and report a focus change. The wait grows with the number of moves and is gone within a
  few seconds; the UI thread is not blocked meanwhile. It comes from WinUI's `ListView`, which a plain `ListView` without naui shows
  too. On a development machine, 20 presses of an arrow key delayed the next window by
  about 2.4 seconds in a `Table` and 0.7 seconds in a plain `ListView`.
- Sorting by pressing a `Table` header only goes as far as notifying "which column,
  in which direction"; **sorting the rows is the app's job**.
- A `Table` with more than 200 rows builds only the range shown on screen (with
  either `set_rows` or `set_row_builder`). This leads to the following rules.
  - **All rows are assumed to have the same height.** If needed, set it with
    `set_row_height`. On Windows rows are given the tallest height built so far, so a
    taller row appearing later moves the scroll position once.
  - naui does not know whether rows it has not built yet can be chosen, so by
    default they are assumed to be choosable (this can be passed with
    `set_row_selectable`).
  - On Linux, keyboard movement goes through the native list, which knows only the
    rows built: Home and End go to the first and last rows built, and the arrow keys
    continue past them as the rows follow. A Shift range selection also covers only
    the rows built. Windows, macOS and the Web cover every row.
  - A control inside a row loses the keyboard focus when its row leaves the range
    built, because the row is rebuilt. On Windows, while the row stays in the range it
    is left as it is, so what the builder returned for it only changes with `refresh`.
  - Columns with widgets in their cells, if no width is specified, are sized only
    by **the header's width and their share of the leftover space** (so columns do
    not move even when content widths differ per row).
  - Assistive technology is told each row's position among all the rows and how many
    there are, with the same limit on GTK versions as for `List`.
  - On the Web, when the number of rows × the row height exceeds the element height
    the browser can handle (about 33 million px in Chromium), the mapping between
    the scroll bar position and rows drifts (around 1 million rows for 30 px rows).
    Selection by index and `set_rows` work even with more than that.
- `Tree`'s items can hold only strings. Items are addressed by paths
  (`&[usize]`), and they cannot be rearranged by dragging. With multiple selection,
  `on_select` still passes only the first selected item (read `Tree::selection`
  for all of them), and selecting a branch never selects its children (there is no
  check-box selection as in WinUI's `TreeView`).
- Only one `Dialog` can be shown at a time, with at most three buttons: Primary,
  Secondary and Cancel. `Sheet` has the same three buttons, and its cancel button
  cannot be disabled (Esc closes it anyway).
- A `Sheet` takes its content's natural size (at least 320 px wide), so `Fill` in
  the content has nothing to fill. It cannot be resized by the user.
- One `Toast` is on screen at a time; the others wait in one queue for the whole
  app (not one per window, as `AdwToastOverlay` keeps), and each comes up in the
  frontmost window when its turn comes. `ToastPriority::High` shows one at once
  instead. Toasts are meant for one or two short actions: the buttons are laid out
  in a single row (on Windows, a row below the text when there are two or more),
  which does not wrap.
- When a `Toast` disappears while one of its action buttons has focus (reached with
  Tab), only Windows puts focus back on the element it came from. naui does nothing
  about it elsewhere: on macOS the removed view's window itself becomes the first
  responder, on the Web focus falls to `<body>` (the button is hidden with
  `display: none`), and on Linux it goes wherever GTK moves it when
  `AdwToastOverlay` removes the toast.
- `Canvas` draws single lines of text only (no wrapping or rich text), gradients
  fill shapes but do not stroke lines, and gradients are linear or radial. A radial
  gradient spreads from its center, with no separate focal point, and there are no
  conic (sweep) gradients. Images are PNG, JPEG
  and GIF (the first frame) only; SVG cannot be drawn. Whether a JPEG's EXIF
  orientation is applied differs by platform. Stretched parts of an image can take
  in the pixels just outside them at their edges. Its size is not decided by its
  content, so specify it with `set_sizing`.
- `Canvas` text starts at the top of its em box everywhere, but where the baseline
  falls inside it comes from each platform's UI font (about 0.8 × the size: 0.81
  for Segoe UI), so the same text can sit up to a pixel or so apart between
  platforms. `Painter` measures widths only, not heights or baselines.
- Gradient stops that differ in both color and opacity blend slightly differently
  between platforms (some interpolate premultiplied colors, some not). Stops of one
  color fading out look the same everywhere.
- `Canvas` pointer notifications carry the modifier keys held at the time of each
  event (`PointerEvent::modifiers`), not those held when the drag began. On macOS
  a Ctrl-click on a `Canvas` arrives as the primary button with `control` set, not
  as a secondary click. Trackpad pinches do not arrive as zooming (on the Web,
  Chromium-based browsers and Firefox deliver them to `on_wheel` as wheel events
  with Ctrl). The amount per wheel notch is normalized to `WHEEL_STEP`, but when a
  browser on the Web passes pixels, the browser's amount is kept.
- `on_file_drop` takes only files from the OS. Dragging between widgets (rows from
  one `List` into another, or onto other widgets), dragging out of the app, and
  dropped text or links are not supported. Drops onto text-input widgets go to the
  field itself and do not reach `on_file_drop`.
- Rows can be rearranged by dragging only within one `List`
  (`List::set_reorderable`), not in `Tree` or `Table`. Rows that cannot be selected
  cannot be dragged, and the line or gap showing where the rows will land differs
  by platform (see [the guide](../crates/naui/src/guide.md#rearranging-list-rows)).
  Only Windows moves rows with the keyboard (Alt + Shift + Up / Down, which
  `ListView` has by itself); on macOS, Linux and the Web they are moved only by
  dragging.

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
- **A window can be made smaller than content placed with `Fill`**, which is then
  cut off. A `Fill` child and a `Track::Fill` column or row never decide the
  window's minimum, and a `Scroll` filling the window is no exception, even in a
  direction it does not scroll. Linux stops at what the rest of the window needs
  (the header bar and content that is not `Fill`); Windows and the Web do not take
  any minimum from the content. On macOS it has not been checked on a real device;
  there the content of a `Scroll` is held to the visible width by a required
  constraint in a direction it does not scroll, so the window may stop at that
  content's width instead. Set a minimum with `Window::set_min_size` or the
  child's `min_width` / `min_height`.

- Shortcuts given with `on_shortcut` (windows and widgets) are matched by naui
  in its own key receiver, not registered as WinUI's `KeyboardAccelerator`, a
  `GtkShortcutController` or AppKit's key equivalents, so that the scope (the
  focus inside the widget) and the order (the focus outward, the window, then the
  menu bar) are the same on all four. In return, **screen readers do not announce
  them**, and they show nowhere on screen; put the key in a tooltip or label
  (`Shortcut::label`) where it matters.
- A shortcut cannot be a symbol key (`/`, `+`, `[` and so on: which key types them
  depends on the keyboard layout) or a character key without the primary modifier.
  With ⇧, a digit arrives as the symbol it types on macOS, Linux and the Web (`!`
  for `1` on a US layout), so a `Shift` + digit shortcut fires only on Windows.
- With a keyboard layout that does not type Latin letters (Russian, Greek and so
  on), a character shortcut such as `Ctrl+S` may not fire on macOS, Linux and the
  Web: naui compares the character the key types, not the physical key. Windows
  compares the virtual key, and the menu bar's shortcuts on macOS and Linux are
  matched by the OS, so those are not affected. This has not been checked on a
  real device.
- Keys pressed in a `Dialog` or `Sheet` do not reach the window's `on_key_down` or
  shortcuts. Keys pressed while an IME is composing reach no shortcut.

### naui-app

- A panic can be caught only with `panic = "unwind"` on a desktop platform. **On the
  Web (wasm aborts on a panic) and with `panic = "abort"`, the handler is told from
  inside the panic hook and the app stops, whatever the handler answers.** A process
  built with `panic = "abort"` ends there. A page on the Web stays as it was, with
  nothing handling its messages (a page cannot quit), and the browser's console shows
  the trap. Callbacks that do not go through the queue, such as a plain `Tasks::after`,
  can still be called there.
- There is no way to tell whether a model is still whole after a panic in `update`.
  That is why the handler decides, and `Decision::GoOn` carries on with the model as
  the panic left it.
- The handler is called from inside the panic hook when a panic is outside the queue,
  and for every panic where nothing can be caught. A panic in the handler there aborts
  the process, and it is better off not touching widgets. A panic in the handler at any
  other time is caught, and the app goes on.
- With several apps on a thread, each handler hears about its own app's queue and
  effects, but a panic outside any queue cannot be tied to an app. It goes to the
  handler set last among those still there, and `Decision::Stop` stops that app.
- The panic hook is the process's. An app that sets a panic hook of its own after the
  handler replaces naui-app's. The hook then sees nothing, but panics caught in the
  queue are still reported, without a location.
- A handler that sends a message about a panic can go round forever when handling it
  panics again (a `render` that always panics). naui-app reports a failed mount and a
  repeated key once until they are put right for this reason, but cannot tell one
  panic from the next.
- `Report::message` is the `Debug` text of the message, cut to fit in 1024 bytes, and
  `render` sees the last message handled before it, not all that led to it. A failure
  from an effect's thread or a panic in a callback that ran later carries no message.

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

### Testing apps

- Not every user action has a method that acts like it (pressing keys for
  `on_key_down`, for example). The native file dialogs themselves cannot be
  answered: `FilePicker::choose` and `FileSaver::choose` stand in for clicking the button
  and choosing in the dialog, without showing it, so a test that clicks the
  button instead shows the real dialog and waits for a person to answer it (on
  macOS `open` does not return until then). They do not check the picker's mode
  or filters.
- `Canvas::send_pointer` and `send_wheel` make no native event: the event goes to
  `on_pointer` or `on_wheel` as given, so an outer scroll view does not scroll on
  `Continue`, and a right-click opens no context menu.
- `Window::request_close` does nothing on the Web, where `on_close_request` is
  never called. On macOS it goes through `performClose:`, as ⌘W does, so AppKit
  plays the alert sound when `on_close_request` keeps the window open. On Windows
  naui asks `on_close_request` and closes the window as `Window::close` does:
  WinUI raises the `AppWindow`'s `Closing` only for the user's own close, and
  naui winds the whole UI down there, which would stop the remaining test cases.
- On Linux, `Toast::click_action` on a toast with one action clicks the button
  inside libadwaita's toast widget, found by its private type name; libadwaita
  takes the toast down a moment later, so `on_action` and `on_dismiss` arrive from
  the event loop. `Window::request_close` on a window that has not been shown
  does nothing there (`on_close_request` is not asked).
- `drop_files` starts from the widget it is called on rather than from a point
  under the pointer, and natively it does not check whether the widget is shown.
  Natively a text field passes the drop on like other widgets, while a real drop
  onto one is taken as text by the field itself and reaches no `on_file_drop`. On Windows the files of a real drop arrive once they have been
  read, a moment later; `drop_files` delivers them at once.
- `drop_files` on the Web dispatches a real `drop` event with an empty file per
  entry, so the files arrive named after the entries, with `blob:` URLs and no
  paths, as dropped files do there. Text fields take drops themselves there, so
  `drop_files` onto one reaches no `on_file_drop`, and the widgets inside a
  `Dialog` or `Sheet` do not pass a drop on to the window, as with real drops.
  `FilePicker::choose` on the Web goes through the file input the same way, except
  in folder mode, where a made-up file cannot stand for a folder: the first entry
  arrives with its name only. `FileSaver::choose` on the Web writes nothing and
  starts no download; `on_save` gets an entry with the name only.
- Inside a test case `Ui::open_url` opens nothing and records the URL for
  `testing::opened_urls` instead. Links the user clicks inside a `Label` are not
  recorded (no method clicks them).
- `Window::send_key` delivers a key only through naui (the `on_key_down`s,
  shortcuts and the menu bar's shortcuts): the focused control does not get it, so
  no text is typed and a control's own keys (a list's arrow keys, a field's Enter)
  do nothing. It starts from the widget that has focus, which `request_focus` moves.
- `enter_text`, `submit` and `search` press no key: `on_key_down` is not called,
  so an `on_key_down` that stops Enter does not stop `submit`, and nothing goes
  through an IME. `enter_text` replaces the whole text at once, so `on_change` is
  called once with the new text rather than once per character, and an
  `EditableComboBox` completes nothing from its suggestions.
- On Windows, `enter_text` does not go through UI Automation: WinUI gives the
  app's own process no Value pattern for a text field, even once the window is
  drawn (UI Automation clients in other processes, such as Narrator, do get one).
  naui writes the text and calls `on_change` itself before `enter_text` returns,
  the same whether or not the window has been drawn. `search` and `respond` click
  through UI Automation's Invoke pattern, so their notifications arrive a moment
  later; wait for their effect with `testing::wait_until`. `search` calls
  `on_search` itself where the Invoke pattern is not offered. A dialog's buttons
  are in its template, which WinUI applies only once the dialog is drawn, so
  where they are missing (a window nothing draws, as on CI runners) `respond`
  closes the dialog with the role itself when its button has not appeared (or,
  after the dialog opens again, been loaded again) within a second or a click on
  it leaves the dialog open. Right after the first window is shown, `Dialog::open` and
  `Sheet::open` wait until WinUI can show them (its `XamlRoot`), for up to 10
  seconds.
- `Dialog::respond` and `Sheet::respond` click a button, so a role with no button,
  or a disabled one, leaves the dialog open; on macOS that leaves `Dialog::open`
  (and the click that opened it) waiting. Give the answer **before** whatever opens
  the dialog: on macOS `Dialog::open` returns only once the dialog is answered. On
  the other platforms the answer arrives a moment after the dialog opens. An
  answer given while it is closed waits for the next `open` that shows it, so one
  given to a dialog that then could not open (no window shown yet, for example)
  answers it later.
- `Link::click` calls `on_click` without opening `href`, unlike a user's click.
- The tests need a test target with `harness = false` (`naui::test_cases!` writes
  its `main`): macOS and Linux touch the UI only from the main thread, and WinUI 3
  starts only once per process. For the same reason **every case shares one
  `Ui`**, so settings a case changes (the theme, for example) carry over to the
  cases after it, and a window a failing case opened stays open.
- A panic inside `on_click` and other native callbacks cannot be caught per case
  (it cannot unwind through AppKit or WinRT). Check results in the case itself.
- On the Web, `testing::resize` compares only the width, and a window cannot be
  wider than the browser's viewport (the page decides the height). Adaptive
  changes arrive through `ResizeObserver`, which runs only while the browser
  renders the page, so in a tab that is not shown (the runner's page opened with
  `NO_HEADLESS=1` behind another tab, for example) waiting for them times out.
- Where an adaptive layout switches is not the same everywhere: a `Sidebar`
  collapses on macOS only below `SIDEBAR_MIN_WIDTH + SIDEBAR_COLLAPSE_CONTENT_WIDTH`
  (it first narrows to its minimum width), and elsewhere below its own width plus
  `SIDEBAR_COLLAPSE_CONTENT_WIDTH`.
- A collapsed `Sidebar` becomes a strip of icons (a navigation rail) only on
  Windows, where an item without an icon shows only the first letters of its
  label, cut off with an ellipsis. AppKit and libadwaita have
  no icon-only state, so macOS, Linux and the Web hide the whole pane. Opening it
  while narrow lays it over the content on Windows, widens the window on macOS
  (outside full screen), and keeps it beside the content on Linux and the Web.

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
- A `Tree` with `SelectionMode::Multiple` is not a `TreeView` but a `ListView` laid
  out as a tree (`TreeView`'s own multiple selection is check boxes that also
  select a branch's descendants). Its rows look like `List` rows, the chevron is a
  borderless button with `TreeViewItem`'s glyphs, and ← / → expand and collapse as
  in `TreeView`. Assistive technology is told each row's level and its position
  among its siblings, but not whether a branch is expanded.
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
  focused control handles itself does not reach them (shortcuts given with
  `on_shortcut` come before the focused control, so they do not have this gap).
  Keys pressed in a popup (a `Dialog` or `Sheet`'s `ContentDialog`, a menu, a
  drop-down list) pass through the window root too, but naui gives them only to
  the widgets they come from (the shortcuts of the widgets in a `Sheet` fire), not
  to the window's `on_key_down` or shortcuts. F10 and Alt combinations, which
  Windows uses for its own menu keys, reach `on_shortcut` (checked with real
  keys); whether they reach the `MenuBar`'s shortcuts has not been checked on a
  real device.

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

- **A `Label` holding links is selectable**, whatever `set_selectable` says (and
  `is_selectable` still answers what was asked), because AppKit reacts to links only
  in a selectable field. Clicking a link gives the label the keyboard focus, and the
  pointer over a link stays an I-beam rather than a pointing hand (the field's cursor
  rect covers the whole label, and the field editor's `linkTextAttributes` names no
  cursor).
- `ButtonRole::Destructive` sets `hasDestructiveAction`, but **only macOS 26 and
  later give it a look** (red text on a faint red bezel, the same as destructive
  buttons in alerts). Before that it looks like an ordinary button.
  `ButtonRole::Primary` draws the accent color like the window's default button,
  but is not given the Return key equivalent, so Return does not activate it.
- `Group` holds one widget; to group several, put them in a `Stack`. It has no
  `set_enabled`: disabling every control inside is up to the app (only the Web's
  `<fieldset disabled>` could do it natively). Windows has no group box, so the
  heading sits above a card instead of on the frame (see
  [Widget implementations](widgets.md#windows-construction)).
- `Toggle` switches **only when the switch is pressed**; pressing the label text
  does not switch it.
- `EditableComboBox` completes the rest when you type characters matching the start
  of an option, but **does not narrow the option list itself**.
- `Table` row height is a fixed value derived from the system font. Rows do not
  grow individually to fit their content.
- `Image` loads remote URLs synchronously, so local files are recommended.
- `Image::set_pixels` hands Windows, macOS and the Web an uncompressed PNG, about
  as large as the pixels, rather than the pixels themselves. Linux takes them as
  they are (`GdkMemoryTexture`).
- On Windows an SVG loaded from a remote URL has no natural size (an `Auto` axis
  gets 0), because `SvgImageSource` does not report one. Give it a size with
  `set_sizing`, or load it with `set_bytes`.
- On Linux, SVG is read by librsvg's gdk-pixbuf loader (`librsvg2-common` on
  Debian and Ubuntu), which desktops install with GTK.
- `Expander` has no expand/collapse animation.
- `Dialog::open` and `PopupMenu::open_at` do not return until closed. While a
  `Dialog` opened from inside a `Tasks` task or a timer is shown, other tasks and
  timers wait until it closes: they run on the main dispatch queue, which does not
  run again until the work that opened the dialog returns. One opened from a
  click or another notification does not hold them up.
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
- `NumberInput` (`GtkSpinButton`) does not let characters other than digits, a sign
  and a decimal point be typed.
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
- `Toast` always appears at the bottom center: `AdwToastOverlay` has no way to
  change where toasts go, so `ToastPosition` is ignored. With two or more actions,
  the buttons are laid out after the text in a custom title rather than as
  `AdwToast`'s own button, which holds only one.
- `AdwToast` always has a close button (×), which the other platforms do not.
  Closing a toast with it calls `on_dismiss`, as a timeout does.
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

- A `Canvas` image made with `Bitmap::from_bytes` is decoded by the browser in the
  background, so it is missing from draws until it has loaded; the surface is
  drawn again then. Images made from pixels are drawn at once.
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
- Images on the clipboard are **`image/png` only**, the one image type the
  Clipboard API requires. `Clipboard::set_image` turns JPEG and GIF files into PNG
  through a `<canvas>` (dropping their metadata), and gets an `Err` in browsers
  without `ClipboardItem` (Firefox before 127). `Clipboard::read_image` may ask the
  user for permission (Firefox shows a "Paste" button to press) and reads `None`
  when it is refused.
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
- `MenuBar` shortcuts, and those given with `on_shortcut`, **accept either Ctrl or
  ⌘ as the primary modifier** (because ⌘ is the primary modifier in browsers on
  macOS). A window's or widget's shortcut that fires also prevents the browser's
  default, so a window's `Ctrl+S` keeps the browser from saving the page; keys the
  browser keeps for itself (such as `Ctrl+W` or `Ctrl+T` in Chrome) never reach
  the page. Keys pressed while the page itself has focus (after a click on an empty
  area, for example) belong to no window and go to every shown window's
  `on_key_down` and shortcuts in turn. When pressed, the browser default
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
- For the same reason, naui-app's `Effect::thread` cannot start on the Web and is
  reported as a failure. Use `Effect::spawn`, or `Effect::channel` with a sending side
  of your own. Without a handler, a failure goes to standard error, which wasm does
  not show anywhere, so an app for the Web should set one
  (`naui_app::run_with_handler`).
- A Web Worker is a separate JavaScript runtime, so even though `Sender` is `Send` as
  a Rust type, it cannot be passed there (bridge with `postMessage` and call `send`
  on the receiving side).
- `Tasks::spawn` advances on the microtask queue. Running a future that never
  yields, or one that keeps waking itself, starves drawing.
- Browsers have no app termination, so `Sender::send` never fails because the UI has
  finished. It does fail once the channel is closed with a `ChannelCloser`.
