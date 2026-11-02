# Widget implementations

This page summarizes which control each widget becomes on each platform.

## Implementations per platform

Legend:

| Symbol | Meaning |
| --- | --- |
| ✅ | Uses the platform's standard control as is |
| 🟡 | Implemented by combining standard controls |
| 🔴 | Reproduced with other elements, because there is no corresponding concept |

### Windows and layout

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Window` | ✅ `Microsoft.UI.Xaml.Window` | ✅ `NSWindow` | ✅ `AdwApplicationWindow` | 🔴 `<div>` + `document.title` |
| `Stack` | 🟡 `Grid` (a track per child) | ✅ `NSStackView` | ✅ `GtkBox` | 🟡 `<div>` + CSS Flexbox |
| `Grid` | ✅ `Grid` | ✅ `NSGridView` | 🔴 naui's own container, laid out like `GtkGrid` | 🟡 `<div>` + CSS Grid |
| `Scroll` | ✅ `ScrollViewer` | ✅ `NSScrollView` | ✅ `GtkScrolledWindow` | 🟡 `<div>` + `overflow` |
| `Spacer` | 🟡 An empty `Grid` | 🟡 An empty `NSView` | 🟡 An empty `GtkBox` | 🟡 `<div>` + `flex-grow` |
| `Expander` | ✅ `Expander` | 🟡 `NSButton` (disclosure) + `NSStackView` | ✅ `GtkExpander` | ✅ `<details>` + `<summary>` |
| `SplitView` | 🔴 `Grid` + a divider `Grid` | ✅ `NSSplitView` | ✅ `GtkPaned` | 🔴 `<div>` + `<div role="separator">` |
| `AdaptivePanes` | ✅ `TwoPaneView` + a back `Button` | 🔴 `NSView` + `NSBox` (separator) + a back `NSButton` | ✅ `AdwNavigationSplitView` + a back `GtkButton` | 🔴 `<div>` + a back `<button>` |
| `Toolbar` | ✅ `CommandBar` + `AppBarButton` | ✅ `NSToolbar` + `NSToolbarItem` | 🟡 `AdwHeaderBar` + `GtkButton` | 🟡 `<div role="toolbar">` + `<button>` |
| `MenuBar` | 🟡 A row of `Button` + `MenuFlyout` | ✅ `NSMenu` (`NSApplication.mainMenu`) | ✅ `GtkPopoverMenuBar` + `GMenu` | 🟡 `<div role="menubar">` + `<div role="menu">` |
| `Sidebar` | 🟡 `NavigationView` (Auto) + a divider `Grid` | ✅ `NSSplitViewController` (sidebar item) + `NSTableView` (source list) | 🟡 `GtkPaned` + `GtkListBox` (`.navigation-sidebar`) | 🔴 `<aside>` + `<nav>` + `<div role="separator">` |

### Basics and input

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Label` | ✅ `TextBlock` | ✅ `NSTextField` | ✅ `GtkLabel` | ✅ `<span>` |
| `Button` | ✅ `Button` | ✅ `NSButton` | ✅ `GtkButton` | ✅ `<button>` |
| `Checkbox` | ✅ `CheckBox` | ✅ `NSButton` | ✅ `GtkCheckButton` | 🟡 `<input type="checkbox">` + `<label>` |
| `Toggle` | ✅ `ToggleSwitch` | 🟡 `NSSwitch` + `NSTextField` | 🟡 `GtkSwitch` + `GtkLabel` | 🟡 `<input type="checkbox" switch>` + `<label>` |
| `TextInput` | ✅ `TextBox` | ✅ `NSTextField` | ✅ `GtkEntry` | ✅ `<input type="text">` |
| `TextArea` | ✅ `TextBox` | 🟡 `NSTextView` + `NSScrollView` | 🟡 `GtkTextView` + `GtkScrolledWindow` | ✅ `<textarea>` |
| `PasswordInput` | ✅ `PasswordBox` | ✅ `NSSecureTextField` | ✅ `GtkPasswordEntry` | ✅ `<input type="password">` |
| `SearchInput` | ✅ `AutoSuggestBox` | ✅ `NSSearchField` | ✅ `GtkSearchEntry` | ✅ `<input type="search">` |
| `NumberInput` | ✅ `NumberBox` | 🟡 `NSTextField` + `NSStepper` | ✅ `GtkSpinButton` | ✅ `<input type="number">` |
| `Slider` | ✅ `Slider` | ✅ `NSSlider` | ✅ `GtkScale` | ✅ `<input type="range">` |
| `ProgressBar` | 🟡 `Grid` + `Border` | ✅ `NSProgressIndicator` | ✅ `GtkProgressBar` | ✅ `<progress>` |
| `Spinner` | ✅ `ProgressRing` | ✅ `NSProgressIndicator` (spinning) | ✅ `AdwSpinner` (`GtkSpinner` before libadwaita 1.6) | 🔴 `<progress>` (indeterminate) |

### Choosing data

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `ComboBox` | ✅ `ComboBox` | ✅ `NSPopUpButton` | ✅ `GtkDropDown` | ✅ `<select>` |
| `EditableComboBox` | ✅ `ComboBox` (`IsEditable`) | ✅ `NSComboBox` | 🟡 `GtkEntry` + `GtkMenuButton` + `GtkListBox` | ✅ `<input list>` + `<datalist>` |
| `RadioGroup` | 🟡 `StackPanel` + `RadioButton` | 🟡 `NSStackView` + `NSButton` (radio type) | 🟡 `GtkBox` + grouped `GtkCheckButton`s | 🟡 `<div role="radiogroup">` + `<input type="radio">` |
| `DatePicker` | ✅ `DatePicker` / `TimePicker` | ✅ `NSDatePicker` | 🟡 `GtkMenuButton` + `GtkCalendar` + `GtkSpinButton` | ✅ `<input type="date">` / `"time"` / `"datetime-local"` |
| `TimePicker` | ✅ `TimePicker` | ✅ `NSDatePicker` (hours and minutes only) | 🟡 `GtkSpinButton`s for hours and minutes | ✅ `<input type="time">` |
| `ColorPicker` | 🟡 `Button` + `Flyout` + `ColorPicker` | ✅ `NSColorWell` | ✅ `GtkColorDialogButton` | ✅ `<input type="color">` |
| `List` | ✅ `ListView` | ✅ `NSTableView` + `NSScrollView` | 🟡 `GtkListBox` + `GtkScrolledWindow` | ✅ `<select size>` / 🟡 `<ul role="listbox">` |
| `Table` | 🟡 `Grid` + `ListView` (rows are `Grid`s) | ✅ `NSTableView` + `NSTableHeaderView` | 🟡 `GtkListBox` + `GtkSizeGroup` | 🟡 `<table role="grid">` |
| `Tree` | ✅ `TreeView` | ✅ `NSOutlineView` + `NSScrollView` | 🟡 `GtkListBox` + disclosure buttons | 🟡 `<ul role="tree">` |

### Files and media

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `FilePicker` | 🟡 `Button` + `IFileOpenDialog` | 🟡 `NSButton` + `NSOpenPanel` | 🟡 `GtkButton` + `GtkFileDialog` | 🟡 `<button>` + `<input type="file">` |
| `FileSaver` | 🟡 `Button` + `IFileSaveDialog` | 🟡 `NSButton` + `NSSavePanel` | 🟡 `GtkButton` + `GtkFileDialog` (save) | 🔴 `<button>` + `showSaveFilePicker` / `<a download>` |
| `Image` | ✅ `Image` | ✅ `NSImageView` | ✅ `GtkPicture` | ✅ `<img>` |
| `Video` | ✅ `MediaPlayerElement` | ✅ `AVPlayerView` | 🟡 `GtkPicture` + `GtkMediaControls` | ✅ `<video>` |
| `Audio` | ✅ `MediaPlayerElement` | 🟡 `AVPlayerView` | 🟡 `GtkMediaControls` + `GtkMediaFile` | ✅ `<audio>` |

### Overlays

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `PopupMenu` | ✅ `MenuFlyout` | ✅ `NSMenu` | ✅ `GtkPopoverMenu` + `GMenu` | 🟡 `<div role="menu">` |
| `Popover` | ✅ `Flyout` | ✅ `NSPopover` | ✅ `GtkPopover` | ✅ `popover` attribute + CSS anchor positioning |
| `Dialog` | ✅ `ContentDialog` | 🟡 `NSAlert` + `accessoryView` | ✅ `AdwAlertDialog` | 🟡 `<dialog>` + `showModal()` |
| `Sheet` | ✅ `ContentDialog` | 🟡 An `NSWindow` shown as a sheet + `NSButton`s | 🟡 `AdwDialog` + `AdwHeaderBar` | 🟡 `<dialog>` + `showModal()` |
| `Toast` | 🟡 An overlaid `InfoBar` | 🔴 An overlaid `NSVisualEffectView` | ✅ `AdwToast` + `AdwToastOverlay` | 🔴 `<div role="status">` |

### Drawing

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Canvas` | 🔴 `Grid` + `CompositionDrawingSurface` (Direct2D) | ✅ `NSView` + `drawRect:` | ✅ `GtkDrawingArea` | ✅ `<canvas>` |

### Navigation

| naui | Windows (WinUI 3) | macOS (AppKit) | Linux (GTK4) | Web (DOM) |
| --- | --- | --- | --- | --- |
| `Tabs` | 🟡 `Grid` + `ToggleButton` | ✅ `NSTabView` | ✅ `GtkNotebook` | 🟡 `role="tablist"` + `<button>` |
| `Navbar` | 🟡 `TextBlock` + `ToggleButton` | 🟡 `NSTextField` + `NSSegmentedControl` | 🟡 `GtkLabel` + `GtkToggleButton` | 🟡 `<nav>` + `<strong>` + `<button>` |
| `Dock` | 🟡 A row of `ToggleButton`s | ✅ `NSSegmentedControl` | 🟡 A row of `GtkToggleButton`s | 🟡 `<nav>` + `<button>` |
| `Menu` | 🟡 A column of `NavigationViewItem`s | 🟡 A column of `NSButton`s | 🟡 A column of `GtkToggleButton`s | 🟡 `<nav><ul><li><button>` |
| `Breadcrumbs` | 🟡 `HyperlinkButton` + separators | ✅ `NSPathControl` | 🟡 `GtkLabel` links + separators | 🟡 `<nav><ol><li><a>` |
| `Pagination` | 🟡 `Button` + `ToggleButton` | 🟡 `NSButton` + `NSSegmentedControl` | 🟡 `GtkButton` + `GtkToggleButton` | 🟡 `<nav>` + `<button>` |
| `Link` | ✅ `HyperlinkButton` | 🟡 `NSButton` + `NSWorkspace` | ✅ `GtkLinkButton` | ✅ `<a>` |

## Construction per platform

How the 🟡 / 🔴 entries in the tables above, and the standard controls naui
modifies, are assembled, and why. Differences seen from the user's side are
collected in [Known limitations](known-limitations.md).

### Windows construction

- **`Stack`**: A `Grid` with one row (column) per child, not a `StackPanel`.
  `StackPanel` never hands out leftover space, so `Fill` and `Spacer` along the
  main axis would do nothing. A child with `Fill` on the main axis gets a `Star`
  track, the others `Auto`, and the spacing is `RowSpacing` / `ColumnSpacing`. A
  hidden child is given no track of its own and shares the one before it:
  `RowSpacing` falls between tracks, so an empty track would still leave a gap on
  each side. The tracks are redone when a child is added, removed, shown or hidden,
  or given a new `Sizing`.
- **`MenuBar`**: Rather than WinUI 3's `MenuBar`, it is a row of `Button`s with
  their background removed, each holding a `MenuFlyout`. This is because
  `MenuBar` / `MenuBarItem` / `KeyboardAccelerator` are not in `naui-winui3`'s
  projection (the projection is a committed generated file, and adding types means
  regenerating all of it). In exchange, naui watches `KeyDown` events that bubble up
  to the window root and invokes shortcuts itself (function keys and items inside
  submenus too). Submenus are `MenuFlyoutSubItem`s and check items
  `ToggleMenuFlyoutItem`s, the same as in `PopupMenu`.
- **`Toolbar`**: A `CommandBar`, with `AppBarButton` items and `AppBarSeparator`
  separators. It is placed **to the right of the title** (the same position as
  macOS's `NSToolbar` and Linux's `AdwHeaderBar`). Passing an element to
  `SetTitleBar` would make window dragging start even over the interactive controls
  inside it, so the drag region is split with `AppWindowTitleBar::SetDragRectangles`
  into **two parts: the title text side and the free space to the right of the
  toolbar**. Dragging does not start over buttons, and the rest of the title bar
  can be grabbed anywhere. The rectangles are in physical pixels, so they are
  recalculated whenever the window width, the number of items or the display scale
  changes. Icons are Segoe Fluent Icons shown with `FontIcon`, and each glyph is 40
  wide (`AppBarButton`'s default of 68 is the width for a layout with the label
  below the glyph). Labels are hidden by setting `DefaultLabelPosition` to
  `Collapsed`, so only glyphs are lined up as on the other three platforms. The
  hidden labels are routed to `AutomationProperties.Name` and
  `ToolTipService.ToolTip`, so they appear in screen reader announcements and
  tooltips. When width runs out, `CommandBar` itself moves items to the overflow
  menu, so items are never truncated into being unpressable.
- **`SplitView`**: The Windows App SDK has no control with a movable divider
  (`Microsoft.UI.Xaml.Controls.SplitView` is a navigation pane that opens and
  closes, and `GridSplitter` belongs to the Community Toolkit), so a divider is
  placed in the middle of a `Grid` with three columns (rows). The divider color is
  taken from `ControlStrokeColorDefaultBrush`, so it follows the theme. Only the
  1 px boundary is painted (`BorderThickness`), and the rest is a grab area with
  `Background="Transparent"` (unlike `null`, `Transparent` keeps hit testing). A
  divider-specific cursor (⇔) cannot be shown because
  `UIElement.ProtectedCursor`, which changes the cursor shape, is accessible only
  from derived classes. Changes in pane size are picked up with `LayoutUpdated`.
- **`AdaptivePanes`**: `TwoPaneView` itself, which decides by its own width:
  `MinWideModeWidth` is the breakpoint, `Pane1Length` the list's width, and
  `PanePriority` the pane shown alone. `TallModeConfiguration` is `SinglePane`, so
  a narrow, tall view never stacks the panes. It draws no back button, so a
  `Button` with the `Back` symbol sits above the detail while only the detail is
  shown, following `ModeChanged` (and `SizeChanged`, since a view first laid out
  narrow raises no `ModeChanged`). It has no slack at the breakpoint, unlike the
  other platforms. When the width hides a pane, WinUI has already moved focus to
  the next tab stop by `ModeChanged`, so naui remembers the pane that last raised
  `GotFocus` and moves focus into the pane shown.
- **`Label`**: The `TextBlock` is created from XAML with
  `TextTrimming="CharacterEllipsis"` (wrapping is switched with `TextWrapping`).
- **`Toggle`**: The label is put into both `OnContent` and `OffContent`, so the
  text read does not change between on and off (WinUI's default is text that
  switches between "On" and "Off").
- **`NumberInput`**: A `NumberBox`. Step buttons are not shown by default, so
  `SpinButtonPlacementMode` is set to `Inline` to place them to the right of the
  field, and the range, step and decimal places are also written to `Minimum` /
  `Maximum` / `SmallChange` / `NumberFormatter`, so that the up/down keys, the
  wheel and the display agree. At the ends the step buttons disable themselves, and
  PageUp / PageDown move by 10 times the step.
  - Per-keystroke notifications are picked up from `TextChanged` of the input field
    in the template (`InputBox`), because `NumberBox` decides its value only on
    confirmation (Enter, leaving the field, the step buttons, the up/down keys, the
    wheel).
  - So **only naui holds the value while typing**, and `NumberBox`'s value stays
    stale until confirmation. Both the value unreadable text reverts to and the base
    for stepping are the value `NumberBox` holds, so **naui passes it the value
    received so far as soon as the display becomes unreadable**. Otherwise, after
    typing `12` and then `12x`, both the revert and stepping would start from the
    old value (pressing the up arrow on `12x` would give 2 instead of 13). Passing
    the value makes `NumberBox` rebuild its display, so the in-progress display
    (just `-`, or `12x`) and the selection are written back. When the display is
    readable, `NumberBox` reads the display first for both confirmation and
    stepping, so there is no need to pass it.
  - The in-progress display is read with the same `NumberFormatter` (which is also
    an `INumberParser`) that `NumberBox` uses for confirmation. Decimal points and
    digit grouping change with regional settings, so if the reader differed between
    typing and confirmation, notifications would drift.
  - The limit of 10 significant digits exists because `NumberBox` internally holds
    a `SignificantDigits(10)` rounder that cannot be removed even by replacing the
    `NumberFormatter`. On confirmation `NumberBox` reads that display back, so the
    value is also aligned to 10 digits, and value and display never stay out of
    sync.
  - Numbers are right-aligned because macOS's `NSTextField` and Windows up to 0.3.0
    right-aligned them (`NumberBox`'s default is left-aligned). `NumberBox`'s own
    `TextAlignment` is not yet in the Windows App SDK the projection comes from, so
    it is written directly to the template's input field.
- **`SearchInput`**: An `AutoSuggestBox`. The magnifying glass is created by passing
  `QueryIcon="Find"` in XAML (because `SymbolIcon` is not in the projection), and
  confirmation (`on_search`) is `QuerySubmitted`. No suggestion list
  (`ItemsSource`) is passed. `TextChanged` also fires when `Text` is rewritten, so
  it stays silent when `Reason` is a programmatic change (`set_text` does not
  notify).
- **`EditableComboBox`**: A `ComboBox` (`IsEditable`). Per-keystroke notifications
  are picked up from `TextChanged` of the input field in the template
  (`EditableText`), because `ComboBox` itself does not expose text changes.
  `IsTextSearchEnabled` only moves the selection to a matching option and does not
  narrow the list. `AutoSuggestBox`, which shows suggestions from typed text, is
  used by `SearchInput`, but it cannot "open and see all" options. `EditableComboBox`
  is centered on being able to open the list and choose, so it stays a `ComboBox`,
  following Fluent conventions.
- **`DatePicker`**: With `DatePickerMode::DateTime`, a `DatePicker` and a
  `TimePicker` are placed side by side in a `StackPanel`. `set_range` is passed to
  WinUI as a range of years (`MinYear` / `MaxYear`), and boundaries of month, day
  and time are clamped by naui. The standard template makes only the month column
  wide and left-aligned to fit long English month names, so the three columns are
  changed to equal widths and centered, and the minimum width is tightened.
- **`TimePicker`**: The same WinUI `TimePicker` as in `DatePicker`. It has no lower
  or upper bound, so `set_range` is clamped by naui.
- **`ColorPicker`**: WinUI 3's `ColorPicker` is a large surface with a spectrum and
  sliders stacked vertically, so it is put in a `Button`'s `Flyout`, and the button
  shows a swatch of the chosen color (`Border` + `SolidColorBrush`). Only the
  assembly is written in XAML and passed to `XamlReader` (because `Flyout` is not in
  the projection).
- **`List`**: A `ListView`. The row appearance (rounded corners, a subtle fill, the
  accent-colored indicator at the left edge) comes from the standard template, so
  naui adds only a frame `Border` (the same color and corner radius as `Table` and
  `Tree`). `ListView` handles scrolling itself.
- **`Table`**: WinUI 3 has no `DataGrid` (it belongs to the Community Toolkit), so
  the table is built with `ListView` rows made of `Grid`s, and the header is put in
  a separate `Grid` with the same column definitions to align widths. Only the row
  padding is written with the same value as the header instead of `ListView`'s
  default, to align column positions. Sortable headers are `Button`s with background
  and border removed (because WinUI has no control for column headers).
- **`Tree`**: A `TreeView`. Items are mapped to `TreeViewNode`s and passed as a
  tree, so indentation, the disclosure chevrons, keyboard operation and the
  selection appearance all come from WinUI. Only the frame is a `Border` with the
  same color and corner radius as `List`. WinUI has nothing equivalent to
  `TreeItem::enabled` (disabling a `TreeViewItem` also prevents a branch from
  expanding), so the text is dimmed and the row content is covered with an overlay
  that absorbs presses. The overlay covers only the area right of the disclosure
  chevron, so unselectable branches can still be expanded and collapsed.
  `TreeView` handles scrolling itself (its inner list lays out only as much as the
  given height, so delegating to an outer `ScrollViewer` as `List` does would cut it
  off instead of growing).
- **`PopupMenu`**: A `MenuFlyout`. Positioning, shadow, rounded corners, light
  dismiss, keyboard operation (arrows and Esc) and wrapping at screen edges all come
  from WinUI. Right-click is delegated to `UIElement.ContextFlyout`, so long press
  and the context key open the same menu. Submenus are `MenuFlyoutSubItem`s and
  check items `ToggleMenuFlyoutItem`s, which flip their own check mark.
- **`Popover`**: A `Flyout` with the content widget as its `Content`, shown with
  `ShowAt` against the widget's element. The presenter, light dismiss and moving to
  another side when there is no room come from WinUI; naui sets only `Placement`.
  `Placement` takes effect the next time it is shown, so changing the side while open
  shows it again at the same widget.
- **`Sheet`**: The same `ContentDialog` as `Dialog`, with the body text left empty
  and the content widget as the only thing in its content area. The roles map onto
  its three buttons as in `Dialog`, and `set_button_enabled` onto
  `IsPrimaryButtonEnabled` / `IsSecondaryButtonEnabled`. An empty title sets
  `Title` to null, which leaves the title area out (for `Dialog` too).
- **`Toast`**: It looks like an `InfoBar`. However, `InfoBar` is meant to be a band
  laid out inside a page, so it is moved to the bottom center and overlaid on the
  content layer. The action button goes in `ActionButton`, so WinUI decides whether
  it is placed to the right of or below the text. No close button (×) is shown
  (`IsClosable` is turned off). It disappears in three ways — timeout, the action
  button and `dismiss` — because the other three platforms have no × either.
- **`Canvas`**: a `SpriteVisual` showing a `CompositionDrawingSurface` is placed
  over a `Grid` with `ElementCompositionPreview::SetElementChildVisual`, and the
  `on_draw` commands are replayed onto it with Direct2D (text with DirectWrite).
  The surface is sized in physical pixels (the logical size times
  `RasterizationScale`), so one surface pixel is one screen pixel; a change of
  scale alone is caught with `XamlRoot.Changed`. Dashes are converted to **units
  where the stroke width is 1**, as Direct2D requires. Centered and right-aligned
  text is moved by the width DirectWrite measures. When the GPU is reset, the
  devices are made again and every surface is redrawn.
- **`Window` position and state**: `position` is `AppWindow.Position` in physical
  pixels divided by this window's `RasterizationScale`. While maximized or
  minimized, `rcNormalPosition` (the frame when restored) of
  `GetWindowPlacement` / `SetWindowPlacement` is read and written. Full screen is
  made by swapping `AppWindow`'s presenter for `FullScreenPresenter`; the position
  before entering is remembered and returned, and a requested position is applied
  on leaving full screen. On leaving, the original `OverlappedPresenter` is
  reattached (to keep the minimum size and `set_resizable` settings). For
  `placement`, the content size last seen in the normal state is remembered, and
  `set_size` while not normal is applied once `AppWindow.Changed` reports the normal
  state again. A minimized window's saved state comes from `WPF_RESTORETOMAXIMIZED`,
  and `set_placement` checks the saved position against the monitors' work areas
  (`EnumDisplayMonitors`).

### macOS construction

- **`Grid`**: The height of `Auto` rows and the share received by `Fill` children
  are calculated by naui. `NSGridView` decides row and column sizes from content,
  but **where leftover space goes is not defined**, and with hugging priorities
  alone the space ends up on the `Auto` side. So `Auto` rows are given the
  `fittingSize` of their cell content as the row height, and `Fill` children get a
  weak preference to grow up to "the grid size − the width the other columns need"
  (fixed-width columns are estimated by their specified width, the rest by their
  cell content's `fittingSize`). `NSGridView` has no weights either, so when two or
  more `Fill` tracks share the room, that preference is replaced: naui decides
  which tracks take a share and which stay at what they need (naui-core's
  `distribute_fill`, the rule of CSS's `fr`), and each `Fill` child wishes for its
  cell's share written as a proportion of the grid (`width = k × grid + c`). Two
  children each wishing for "everything the others do not need" cannot both be
  satisfied, and the solver would settle it by shrinking the grid and the window;
  proportions hold together at any size, so resizing keeps the weights by itself.
  They stay below a window keeping its size (priority 490), so they never resize a
  window. **Auto Layout decides the positions and sizes themselves**; naui does not
  set frames. Rows and columns that cross a merge cannot
  be removed because AppKit throws an exception when you try.
- **`SplitView`**: `NSSplitView` itself. However, **naui replaces only how the
  panes are sized** (`splitView:resizeSubviewsWithOldSize:`). `NSSplitView`'s
  default distributes by **proportion** when its size changes, so as is, widening
  the window would widen the sidebar too. Because naui sets the panes' frames
  directly, only the pane views keep `translatesAutoresizingMaskIntoConstraints` on
  (Auto Layout arranges the content within that frame).
- **`AdaptivePanes`**: AppKit has no container that shows one pane in place of
  two (`NSSplitViewController` only collapses a sidebar), so naui lays out two
  views with Auto Layout and decides from the frame
  (`NSViewFrameDidChangeNotification`), switching on the next turn of the main
  queue. Side by side, an `NSBox` separator (the line of a thin `NSSplitView`
  divider) sits between them. One at a time, both panes span the whole width and
  the other one is hidden, so the minimum width and the content of the hidden pane
  stay as they were. The back button is an `NSButton` with the
  `chevron.backward` symbol.
- **`Expander`**: AppKit has no collapsible control, so it is built from a
  disclosure `NSButton` (the chevron is an SF Symbol) and an `NSStackView`.
- **`Toolbar`**: Since items are identified by index, "Customize Toolbar"
  (rearranging by the user) is turned off. Separators follow macOS conventions with
  `NSToolbarSpaceItem` (a fixed-width gap). The title text is hidden because, if
  shown, the title would occupy the start and push the items to the right edge.
- **`Label`**: With `set_wrap(true)`, naui watches
  `NSViewFrameDidChangeNotification` and makes `preferredMaxLayoutWidth` follow the
  frame width, because `NSTextField` can report its height only once the wrapping
  width is known.
- **`Toggle`**: `NSSwitch` has no text, so the label is an `NSTextField` placed
  beside it.
- **`NumberInput`**: AppKit has no numeric control, so it is built from an
  `NSTextField` and an `NSStepper` side by side (the same shape as numeric fields in
  System Settings).
- **`EditableComboBox`**: An `NSComboBox`. `completes`, which completes partially
  typed text from the options, is turned on (typing through leaves text not among
  the options as is). The list is not narrowed because AppKit has no mechanism for
  narrowing it.
- **`DatePicker`**: `NSDatePicker` itself. Clicking the field opens an overlaid
  calendar (`presentsCalendarOverlay`), and it can also be edited with the keyboard
  and the stepper. Both look and operation are the same as AppKit's default
  (`textFieldAndStepper`, the same as SwiftUI's `DatePicker`).
- **`TimePicker`**: AppKit has no time-only control, so it is an `NSDatePicker`
  showing only hours and minutes (the AppKit convention for time input). The date
  part is not shown and is fixed to 1970-01-01, and the lower and upper bounds of
  `set_range` are also passed to the `NSDatePicker` itself.
- **`ColorPicker`**: `NSColorWell` itself. Pressing it opens the system color panel.
  The panel also returns catalog colors (such as `systemBlue`), so they are
  converted to sRGB before the components are read. `set_enabled(false)` also cuts
  the connection to the panel.
- **`Table`**: The row height is fixed because letting AppKit compute it
  (`usesAutomaticRowHeights`) makes the constraints AppKit sets on rows stay active
  while pointing at cells removed with a previous column when columns are added or
  removed, which breaks the table. naui calculates the allocation for columns
  without a specified width because AppKit's automatic column resizing cannot
  distribute all leftover space when there are fixed-width columns, leaving the
  right side of the table empty.
- **`Popover`**: `NSPopover` with `NSPopoverBehaviorTransient` (closes on a click
  outside or Esc). The content is placed with a 12 pt margin in the view of its
  `contentViewController`, and the popover's size follows that view's constraints,
  so content that grows while shown (an opened `Expander`) grows the popover.
  Content that cannot decide its own width (a lone text field) gets 260 pt, the same
  as `Dialog`. `preferredEdge` is read in the anchor view's coordinates, so top and
  bottom are swapped for views that are not flipped.
- **`Sheet`**: A titled `NSWindow` shown with `beginSheet:completionHandler:` on the
  key window (or the main window, or the first window on screen), so a sheet opened
  from a sheet stacks on it. A sheet shows no title bar, so the title is a bold
  label at the top, the same look as `NSAlert`'s heading. Below it are the content
  (in the same container as `Popover`'s, which gives content without a width
  260 pt) and a row of `NSButton`s in an `NSStackView`: the secondary action in
  the leading gravity area, cancel and the primary action in the trailing one. The
  default button (primary, or cancel when there is none) gets the Return key
  equivalent, which makes AppKit draw it as the window's default button, and
  cancel otherwise gets Esc. Esc that no button takes reaches the window as
  `cancelOperation:`, which the sheet's window class turns into a cancel. That
  class also tracks focus like naui's other windows, so `on_focus_change` works in
  the content. The window's size follows the content's constraints through its
  `contentViewController`, with a minimum width of 320 pt (`SHEET_MIN_WIDTH`).
- **`Toast`**: AppKit has no toast control, so it is built by overlaying an
  `NSVisualEffectView` (the same material as `NSPopover`) on the window content.
- **`Window` position and state**: While minimized, `frame` stays the original
  frame, so it is read and written as is. For maximizing, the frame before zooming
  is remembered in `windowShouldZoom:toFrame:`, and the window is moved to the
  requested position after AppKit has finished restoring the original frame. For
  full screen, the position before entering is remembered and returned, and after
  leaving, the window is moved to the requested position or the position before
  entering (because macOS 15's AppKit sometimes forgets moves made with
  `set_position` and returns to the frame from when the window was shown). The
  content size for `placement` comes from the same frames, and `set_size` while
  maximized or in full screen is applied together with the position.
  `set_placement` checks the saved position against `NSScreen`'s `visibleFrame`.

### Linux construction

- **`Grid`**: Not a `GtkGrid` but naui's own container (`NauiGrid`). `GtkGrid`
  gives each column its natural width and splits what is left **evenly** among the
  growing columns, so `Track::Fill` weights could not work, and two `Fill` columns
  differed by their content. The container measures the children as `GtkGrid` does
  (lines without a shown child take no room or spacing, a spanning child spreads
  what it lacks over the lines it crosses, over `Fill` lines by their weights,
  heights are measured for the widths the columns get, columns mirror in
  right-to-left text) and splits the room with
  naui-core's `allocate_tracks`: `Fill` tracks share the leftover by weight, never
  below what their content needs, and without a `Fill` track the leftover goes to
  the lines whose children grow, as before. A `Fill` child reports a minimum of 0
  so that it never pushes the window wider, so the grid asks `SizeBin` separately
  for the size below which the content is cut off (`content_minimum`). While the
  room allows, a `Fill` track is held at that size and the others share the rest;
  when it does not, every track shrinks towards its minimum the way a GTK box
  does. The grid's own minimum leaves that size out. GTK4 still places each child inside its
  cell (margins, `halign` / `valign`).
- **`SplitView`**: A `GtkPaned`. The panes' minimum sizes are held by a container
  wrapped around each pane, so they do not conflict with the `size_request` the app
  specifies with `Sizing`.
- **`AdaptivePanes`**: An `AdwNavigationSplitView`, whose sidebar page holds the
  list and whose content page holds the detail. libadwaita leaves collapsing to an
  `AdwBreakpoint`, which takes away the minimum size of the window (or
  `AdwBreakpointBin`) holding it, so naui compares the width after each paint and
  sets `collapsed` on the next main loop iteration. The sidebar's smallest and
  largest widths are both the list's width, in pixels (`sidebar-width-unit` is set
  where libadwaita 1.7 or later has it). The back button is a flat `GtkButton` with
  `go-previous-symbolic`, since `AdwNavigationSplitView` shows one only in an
  `AdwHeaderBar` inside the page. Going back by libadwaita's own means turns
  `show-content` off, which naui follows.
- **Wrapping content**: Since "the height is decided after the width", the
  containers naui wraps content in inherit `GtkSizeRequestMode` from the content.
  `GtkWidget`'s default is `GTK_SIZE_REQUEST_CONSTANT_SIZE`, and with that the
  parent asks for the height without giving a width, allocating only one unwrapped
  line's worth of space.
- **`Toolbar`**: Items are placed on the left side of the header bar, following
  GNOME conventions.
- **`MenuBar`**: A `GtkPopoverMenuBar`, placed below the header bar. GNOME
  recommends hamburger menus over menu bars, but naui offers the same API on all
  four platforms, so it uses the menu bar GTK4 provides as is. Items are represented
  by a `GMenu` model paired with `GSimpleAction`s (a check item is an action with a
  boolean state, and a submenu has a `submenu-action` that decides whether it
  opens), and shortcuts are registered as
  `GtkApplication` accelerators. There is one accelerator table for the whole app,
  so they are registered only while attached to a window and unregistered when
  removed with `clear_menu_bar`. Action namespaces are separate per menu bar, so
  several windows using the same shortcut do not overwrite each other.
- **`Label`**: `set_style` only applies libadwaita style classes (such as
  `.title-1`); the theme decides size and weight. By default (no wrapping),
  `PangoEllipsizeMode::End` is set. **Adding an ellipsis also lowers `GtkLabel`'s
  minimum width**, so putting it in a narrow container no longer pushes the whole
  container wider.
- **`Toggle`**: `GtkSwitch` has no text, so the label is a `GtkLabel` placed beside
  it in a `GtkBox`.
- **`EditableComboBox`**: GTK4 has no editable combo box, so it is built from a
  `GtkEntry` and a `GtkMenuButton` that shows the options, placed in a `.linked`
  `GtkBox`. `GtkComboBoxText` with an entry and `GtkEntryCompletion` were deprecated
  in GTK 4.10 without a replacement.
- **`DatePicker`**: GTK4 has no single date-picking control, so it is built from a
  `GtkCalendar` in a `GtkMenuButton`'s popover and `GtkSpinButton`s for hours and
  minutes.
- **`TimePicker`**: GTK4 has no single time-picking control either, so
  `GtkSpinButton`s for hours and minutes are placed on either side of `:` (the same
  shape as the GNOME Clocks app). Spin buttons have no notion of a time range, so
  `set_range` is clamped by naui, and the clamped result is written back to the
  display.
- **`ColorPicker`**: A `GtkColorDialogButton`. Opacity is not handled, so
  `with-alpha` of the `GtkColorDialog` it opens is turned off.
- **`Popover`**: A `GtkPopover`. A GTK4 popover is a child of the widget it points
  at, so it is attached to the widget passed to `open` and detached again after it
  closes (one main loop iteration later, because detaching during `closed` collides
  with GTK's own cleanup). While attached, the widget is held by naui: a widget
  finalized first, for example when its window closes, would leave the popover
  pointing at freed memory. It is attached to the `SizeBin` wrapping the widget
  rather than to the native widget: a popover is laid out only when its parent
  calls `gtk_popover_present` from its `size_allocate`, which `GtkScrolledWindow`
  does not do (a popover on a `Scroll` collapsed to 0×0 once its content grew).
- **`Sheet`**: An `AdwDialog` holding an `AdwToolbarView`, laid out the way the
  GNOME guidelines describe: an `AdwHeaderBar` showing the title, with cancel at its
  start and the primary action (`.suggested-action`, the dialog's default widget)
  at its end, the secondary action beside it, and the content below with an 18 px
  margin. The header bar's close button is shown only when there is no cancel
  button. `follows-content-size` is on, so the dialog grows with its content, and
  the toolbar view asks for at least 320 px (`SHEET_MIN_WIDTH`). As with `Dialog`,
  the `AdwDialog` is built on each `open`; the content moves into the new one, and
  is let go once the old one has finished closing.
- **`List`**: A `GtkListBox` in a `GtkScrolledWindow`. Row clicks
  (`ListRow::on_activate`) are received through `GtkListBox`'s `row-activated`
  (`GtkListBoxRow`'s `activate` is the path for keyboard Enter / Space only). With
  `SelectionMode::Multiple`, single-click activation is turned off so that GTK4 reads
  the Ctrl / Shift attached to clicks.
- **`Table`**: Instead of `GtkColumnView` (which requires a `GtkListItemFactory` and
  a `GListModel`), it is built from `GtkListBox` rows laid out horizontally, with
  column widths aligned by a `GtkSizeGroup` per column. Sortable headers are `flat`
  `GtkButton`s, and the direction is shown by an arrow added to the header text.
- **`Tree`**: Instead of `GtkTreeExpander` (only for `GtkListView`), it is built
  from `GtkListBox` rows and disclosure buttons.
- **`Window` state**: On X11, the window manager sometimes does not restore a window
  with `unminimize` alone, so restoring from minimized also asks for it to be brought
  to the front with `present`. Minimizing requested before showing is applied after
  the first state arrives from the compositor. GTK4 keeps the size from before
  maximizing, full screen or tiling as the window's default size, so `placement`
  converts it back to the content size (subtracting the window's CSS frame measured
  in the normal state, which can differ while maximized).

### Web construction

- **`Label`**: **`<span>` wraps by default**, so naui sets `white-space: nowrap` and
  `text-overflow: ellipsis` to match the other three platforms' default (one line +
  ellipsis). To wrap, use `set_wrap(true)`. Browsers have no heading levels (type
  ramp), so only the sizes of `set_style` are matched to the other three platforms
  by naui with relative CSS values (`em`). Because they are relative, they follow
  the base font size decided by the browser and the user.
- **`SplitView`**: HTML has no element that splits panes with a movable divider
  (`resize` only shows a grip in the element's corner), so naui assembles one. The
  divider is a `<div role="separator">`, and the cursor is left to `col-resize` /
  `row-resize` (naui's CSS does not draw a grip). Only a 1 px line at the boundary is
  visible, and the rest is a transparent grab area. The line color uses the system
  color `GrayText`. `ButtonBorder` and `CanvasText` become **the exact opposite of
  the background** in some browsers (white in dark mode, black in light mode), too
  strong for a separator, while `GrayText` falls between the background and text
  colors in either scheme. It has `tabindex="0"`, so it can also be moved with the
  keyboard (10 px per arrow key press).
- **`AdaptivePanes`**: HTML has no such element, and a CSS container query cannot
  tell the page which pane it shows, so naui watches the width with the same
  `ResizeObserver` as `Stack::set_breakpoint` and switches before the page is
  painted. The pane not shown gets `hidden` and `display: none`. Side by side, a
  1 px line in `GrayText` separates the panes (the same line as `SplitView`), and
  the back button is a `<button>` with an arrow in front of its text.
- **`MenuBar`**: Browsers have no OS application menu, so it is assembled from
  elements with WAI-ARIA roles (`role="menubar"` / `"menu"` / `"menuitem"`). naui
  watches `keydown` on `document` to match shortcuts, and also adds the `Ctrl+S`
  label at the right edge of items. The subscription on `document` is held only
  while attached to a window. Check items are `role="menuitemcheckbox"` with a `✓`
  in front, and submenus are nested `role="menu"` boxes beside their heading,
  opening on click or hover; `PopupMenu` builds its items the same way.
- **`EditableComboBox`**: `<input list>` and `<datalist>`. naui builds neither the
  visuals nor the narrowing.
- **`DatePicker`**: Uses `datetime-local` so as not to bring in time zones.
- **`List`**: Using `ListItem::detail` or `ListRow` (rows of arbitrary content)
  switches from `<select>` to an implementation using `<ul role="listbox">`. With
  text-only rows it stays `<select size>`, because the content model of `<option>`
  is text only and can hold neither elements nor line breaks. **Having left
  `<select>`, naui adds selection, focus and keyboard operation (arrows, Home / End,
  Space) as it does for `Table`.** The current row is indicated with
  `aria-activedescendant`, and pressing a row itself moves focus to the list
  (`<ul tabindex="0">`) so keyboard use can continue. Buttons and input fields
  placed inside rows own their events, so clicks and keys that reach them are not
  turned into list operations (so that Space on a check box does not turn into row
  selection). Frame and selection colors remain the browser's system colors
  (`Field` / `SelectedItem` / `Highlight` / `GrayText`). This form is the only way to
  show rows of arbitrary content, so both operating the row itself
  (`ListRow::on_activate`) and the controls inside it are kept, and either path
  works.
- **`Table`**: `<table role="grid">`. `<table>` has no mechanism for selecting rows,
  so naui adds clicks (⌘ / Ctrl / Shift) and keyboard operation (arrows, Home / End,
  Space). Column widths are decided by `<col>` in a `<colgroup>` and
  `table-layout: fixed`. Sortable headers are `<button>`s inside `<th>`, and the
  direction is shown with `aria-sort` and an arrow.
- **`Dialog` and `Sheet`**: The same `<dialog>`; a sheet leaves the body text
  empty and has a minimum width of 320 px (`SHEET_MIN_WIDTH`, but no wider than the
  viewport allows). Enter in a text field (an `<input>` other than check boxes,
  radio buttons, sliders and the like) presses the default button, the primary one
  or else cancel, unless the button is disabled or a widget already handled the
  key. Enter and Esc that belong to an IME conversion are passed to the IME: those
  with `isComposing`, and those with `keyCode` 229, which is how Safari sends the
  Enter that confirms a conversion (the same check as `TextInput::on_submit`). A
  `<form>` would give the browser's implicit submission instead, but that
  presses the first submit button in the form, which would be a `<button>` in the
  content. The primary button carries `data-naui-role="primary"`, the same mark as
  `Button::set_role`.
- **`Dialog` and `PopupMenu`**: For Esc, **naui prevents the default action of
  `keydown` and closes them itself**, rather than relying on `<dialog>`'s `cancel`
  event. When the window is full screen, Safari first uses Esc to exit full screen
  unless the page prevents the default action, so closing a dialog would take two
  presses. Esc during composition (`isComposing`) is for cancelling input, so it is
  passed to the IME as is.
- **`Popover`**: A `<div popover="auto">` under `<body>`, shown with
  `showPopover()`; the top layer, light dismiss and the look (`Canvas` /
  `CanvasText` and a border) are the browser's. When the widget is inside a
  `<dialog>` or another popover, the popover moves into that element on opening: a
  modal dialog makes everything outside it inert, and the top layer does not
  change that, and the browser treats popovers as nested only when one is inside
  the other in the DOM (otherwise opening the inner one closes the outer). It goes
  back to `<body>` once closed (a closing popover moves the closed ones inside it
  out on `beforetoggle`), since WebKit stops rendering when a modal dialog or a
  popover holding an element with `position-try-fallbacks` closes. Hiding a window or
  dialog does not hide a popover in the top layer, so `Window::close` and the
  dialog's closing close the popovers opened from widgets inside them (notifying
  `on_close`, as a click outside does), and Esc in a dialog closes the topmost
  such popover before the dialog. It is placed with CSS anchor
  positioning: the widget gets an `anchor-name` unique to the popover (added to a
  comma-separated list, since several popovers can point at the same widget), and
  the popover gets `position-anchor`, `position-area` for the side and
  `position-try-fallbacks` (`flip-block` / `flip-inline`) for the opposite side.
  `inset: auto` replaces the user agent's `inset: 0; margin: auto`, so it lines up
  with the middle of the widget. These are set only when `CSS.supports` reports
  `position-area`; otherwise they would undo the centering and push the popover
  off screen. Newer parts such as `position-visibility` still
  differ between engines and are not used.
- **`Toast`**: Pages have no standard element for transient notifications, so a
  `<div role="status">` is shown at the bottom center with `position: fixed` (the
  `Notification` API goes outside the page and is a different thing).

## Main data types

- `WindowState` / `CloseResponse`: window state (normal, maximized, minimized, full screen) and the response to a close request
- `WindowPlacement`: a window's normal size, position and state, for saving and restoring it (with a one-line text form)
- `Sizing` / `Length` / `Track` / `GridCell`: layout and size
- `Key` / `KeyEvent` / `Modifiers` / `EventResponse`: key input notifications, and whether to pass the key on
- `NavItem`: navigation items
- `ListItem` / `ListRow` / `SelectionMode`: simple list items, rows of arbitrary content, single and multiple selection
- `TableColumn` / `TableRow` / `SortOrder`: table columns, rows and sort direction
- `TreeItem`: tree items (nesting, expansion, whether they can be chosen)
- `DateTime` / `DatePickerMode`: values of date and time, and what a date picker lets the user pick
- `Time`: values of hours and minutes only (the values exchanged by time pickers)
- `NumberSpec`: lower bound, upper bound, step and decimal places of number input
- `Color`: colors as 8-bit sRGB (the values exchanged by color pickers)
- `FileFilter` / `FilePickerMode` / `FileEntry`: choosing and saving files
- `Fit` / `PlaybackState`: media display and playback state
- `Painter` / `Path` / `Point` / `Rect` / `PointerEvent` / `PointerButton`: drawing commands for drawing surfaces, their coordinates, and pointer and wheel notifications
- `PopupItem`: pop-up menu items
- `ToolbarItem` / `ToolbarIcon`: toolbar items and icons
- `MenuSpec` / `MenuItem` / `MenuShortcut`: menu bar headings, items and shortcuts
- `SidebarSection` / `SidebarItem`: sidebar groups (headings) and items (text and icons)
- `DialogButtons` / `DialogResponse`: dialog and sheet buttons and responses
- `ToastSpec`: toast text, action button and time until it disappears

`WeakWidget`, the weak reference used to touch a widget itself from inside its
notifications, is created with each widget's `downgrade()` (see
[the guide](../crates/naui/src/guide.md#removing-widgets-later)).

Detailed API documentation can be viewed by running the following command in the
repository.

```sh
cargo doc --open -p naui
```

## Access to native objects

When the common API is not enough, backend-specific objects can be obtained.
`backend()` on a naui type returns the backend type it wraps, so call its
`native_*` methods. The backend crates are exposed as `naui::macos` /
`naui::windows` / `naui::gtk` / `naui::web`.

```rust,ignore
// macOS
use naui::macos::Widget as _;
let view = button.backend().native_view();

// Web
use naui::web::Widget as _;
let element = button.backend().native_element();
```

This code depends on the target platform, so split it with `cfg` as needed.
