# Guide

A continuation of the quick start on the [top page](crate). Usage examples for
every widget are in
[`examples/gallery`](https://github.com/mokuichi147/naui/tree/main/examples/gallery).
Per-platform limitations are collected in the
[known limitations](https://github.com/mokuichi147/naui/blob/main/docs/known-limitations.md).

## Creating widgets later

Like widget handles, [`Ui`] **can be cloned**. Cloning only adds a reference to
the same contents, so moving a clone into a callback lets you create widgets even
after `build` has finished. Screens whose number of elements is decided at run
time, such as adding rows to a list or adding input fields, are written this way.

```no_run
# use naui::{ListRow, Orientation, Result, Ui};
# use std::cell::RefCell;
# use std::rc::Rc;
# fn build(ui: &Ui) -> Result<()> {
let list = ui.list()?;
// Rows accumulate, so the app keeps the sequence.
let rows: Rc<RefCell<Vec<ListRow>>> = Rc::new(RefCell::new(Vec::new()));

let add = ui.button("Add row")?;
add.on_click({
    let ui = ui.clone(); // the Ui used inside the callback
    let list = list.clone();
    let rows = rows.clone();
    move || {
        // The APIs that create widgets return a Result, so handle it here.
        let (Ok(content), Ok(label)) = (
            ui.stack(Orientation::Horizontal),
            ui.label("New row"),
        ) else {
            return;
        };
        content.append(&label);
        rows.borrow_mut().push(ListRow::new(&content));
        list.set_rows(&rows.borrow());
    }
});
# Ok(())
# }
```

`Ui` is UI-thread only and not `Send`, so it **cannot be passed to other
threads**. Widgets can be created only on the same UI thread as `build`: inside
widget notifications, [`Tasks`] receivers and spawned futures (see
"[Other threads and async](#other-threads-and-async)").

## Removing widgets later

Each container also has ways to reduce what has been placed.

| What to change | API to use |
| --- | --- |
| Children of a `Stack` | [`insert`](Stack::insert) / [`remove`](Stack::remove) / [`clear`](Stack::clear) |
| Cells of a `Grid` | [`replace`](Grid::replace) / [`remove`](Grid::remove) / [`clear`](Grid::clear) |
| Tabs of `Tabs` | [`remove_tab`](Tabs::remove_tab) / [`clear`](Tabs::clear) |
| Lists, tables, trees, navigation | Replace everything with `set_items` / `set_rows` |
| Single-content widgets (`Scroll` / `Expander` / `SplitView` / `Window`) | Replace with `set_child` and the like |
| Things attached to a window (`Toolbar` / `MenuBar` / `Sidebar`) | [`clear_toolbar`](Window::clear_toolbar) / [`clear_menu_bar`](Window::clear_menu_bar) / [`clear_sidebar`](Window::clear_sidebar) |

```no_run
# use naui::{GridCell, Orientation, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let stack = ui.stack(Orientation::Vertical)?;
stack.append(&ui.label("Line 1")?);
stack.insert(0, &ui.label("Heading")?); // insert at the start
stack.remove(1);                        // remove by position
stack.clear();                          // remove everything

let form = ui.grid()?;
form.attach(&ui.label("Name")?, GridCell::new(0, 0));
form.remove(GridCell::new(0, 0));       // remove by cell
# Ok(())
# }
```

`Stack` and `Tabs` **refer by position (index)**, and `Grid` **refers by cell**
(the span is ignored). Pointing at an out-of-range position or an empty cell does
nothing. When [`Tabs::remove_tab`] removes the selected tab, the tab at the same
position (or the last tab if there is none) is selected, and **this move is not
notified** (the same rule as [`Tabs::set_selected`]).

[`Grid::replace`] replaces **only that cell**. Children in other cells remain.

On macOS only, **removing a merged cell (a child with a span) leaves the merge in
place**. `NSGridView` has no API to unmerge, and rows and columns crossing a merge
cannot be removed either (AppKit throws an exception if you try). naui **treats
that range as one cell**, so a one-cell child placed there gets the same position
and size as an ordinary cell. Only two things differ:

- A `Fill` child stretches across the whole merged range rather than one cell
- Placing into another position within the range is the same cell, so the
  previous child is removed

Also, on macOS **children cannot be stacked in the same cell** (an `NSGridCell`
can hold only one content view, so what is placed later removes what was there).
The other three platforms allow stacking.

Removed widgets are freed once all their handles are dropped. However, **if a
widget's notification closure (`on_change`, `on_focus_change` and so on) captures
a `clone()` of the widget's own handle, it forms a cycle and is never freed.**
To touch the widget itself inside its notification, capture a weak reference
([`WeakWidget`]) made with `downgrade()` and `upgrade()` it when you use it. After
the widget is freed, `None` is returned. Every widget has `downgrade()`, in the
same form as [`Window::downgrade`].

```no_run
# use naui::{Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let counter = ui.button("0")?;
let weak = counter.downgrade();
let mut count = 0;
counter.on_click(move || {
    count += 1;
    if let Some(counter) = weak.upgrade() {
        counter.set_text(&count.to_string());
    }
});
# Ok(())
# }
```

When widgets capture each other in their notifications (A's notification captures
B, and B's captures A), that is also a cycle, so make one of them a weak
reference.

## Changing and moving widgets after adding

Settings can be changed after a widget is in a container, in any order. The result
is the same as making them before adding it:

- [`set_sizing`](Button::set_sizing) on the child, including `Fill` on the cross
  axis and how it sits in a `Grid` cell
- [`set_spacing`](Stack::set_spacing), [`set_padding`](Stack::set_padding) and
  [`set_align`](Stack::set_align) on the container
- [`set_column_track`](Grid::set_column_track) and
  [`set_row_track`](Grid::set_row_track) on a `Grid`

To **move a widget to another container, remove it from the one it is in first**,
then add it. It then behaves as if it had been made for the new container: nothing
the previous one decided (a `Grid` column's width, a `Stack`'s alignment) carries
over. Adding a widget that is still in another container is not supported: GTK and
WinUI refuse it, while macOS and the Web move it but the previous container still
counts it as a child.

```no_run
# use naui::{Orientation, Result, Sizing, Ui};
# fn build(ui: &Ui) -> Result<()> {
let left = ui.stack(Orientation::Vertical)?;
let right = ui.stack(Orientation::Vertical)?;
let field = ui.text_input("")?;
left.append(&field);
field.set_sizing(Sizing::fill_width()); // fine after adding, too

left.remove(0);        // take it out first
right.append(&field);  // then add it to the new container
field.request_focus(); // moving takes the focus away
# Ok(())
# }
```

[`Stack::insert`] and [`Stack::remove`] leave the other children where they are, so
a field being typed into keeps its focus and the text being composed with an IME.

[`Stack::set_orientation`] switches a stack between vertical and horizontal the same
way: the children stay where they are, and the result is the same as a stack made
in that direction. [`Sizing`] refers to the width and height, not to the main and
cross axes, so a child with `Fill` on the width that stretched across a vertical
stack takes the leftover width once the stack is horizontal. Spacing, padding and
[`set_align`](Stack::set_align) carry over, the alignment now applying to the new
cross axis.

```no_run
# use naui::{Orientation, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let actions = ui.stack(Orientation::Horizontal)?;
actions.append(&ui.button("Save")?);
actions.append(&ui.button("Cancel")?);
actions.set_orientation(Orientation::Vertical); // one above the other from now on
# Ok(())
# }
```

The moved widget itself keeps its contents (text, values, choices) and the scroll
position of the [`Scroll`]s inside it (checked on macOS, Linux and the Web; not yet
on Windows). **It loses the keyboard focus**, and text being composed with an IME
ends with it (on macOS it is committed into the field). Call `request_focus` after
adding it again if it should keep the focus.

## Themes

The default, [`Theme::System`], follows the OS or browser setting. Use
[`Settings::theme`] to pin it at startup, and [`Ui::set_theme`] to switch at run
time.

```no_run
use naui::{Settings, Theme};

fn main() -> naui::Result<()> {
    let settings = Settings::new("counter").theme(Theme::Dark);
    naui::run(settings, |ui| {
        ui.set_theme(Theme::System)?; // go back to the OS setting at run time
        Ok(())
    })
}
```

## Languages

[`Ui::preferred_languages`] returns the OS's preferred languages as BCP 47 tags,
most preferred first (`"ja-JP"`, `"en-US"`), as the basis for choosing which
translation to show. Translating is left to libraries such as Fluent or gettext.
[`Ui::on_preferred_languages_change`] is called when the list changes on macOS,
Windows and the Web; Linux decides the languages from environment variables at
startup, so it is never called there. Updating the strings already on screen is up
to the app. How to combine this with Fluent, from choosing a translation to
re-setting every string, is in
[Translating apps](https://github.com/mokuichi147/naui/blob/main/docs/translating.md)
and [`examples/i18n`](https://github.com/mokuichi147/naui/tree/main/examples/i18n).

```no_run
# use naui::{Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let greeting = |tags: &[String]| match tags.first().map(String::as_str) {
    Some(tag) if tag.starts_with("ja") => "こんにちは",
    _ => "Hello",
};
let label = ui.label(greeting(&ui.preferred_languages()))?;
let weak = label.downgrade();
ui.on_preferred_languages_change(move |tags| {
    if let Some(label) = weak.upgrade() {
        label.set_text(greeting(tags));
    }
});
# Ok(())
# }
```

The few strings naui shows itself (the default macOS main menu, the `Sidebar`
toggle and so on) keep the language chosen at startup, like the strings the native
controls carry themselves (file dialogs, date pickers, text field context menus).

## Layout and size

Layout uses these six:

| API | Purpose |
| --- | --- |
| [`Stack`] | Lays out children vertically or horizontally |
| [`Grid`] | Places children by row and column |
| [`Scroll`] | Scrolls content that overflows |
| [`Spacer`] | Takes the parent's leftover space |
| [`Expander`] | Shows its content only when the heading is clicked |
| [`SplitView`] | Divides two panes with a movable divider |

Every widget's size can be specified with [`Sizing`]. [`Length`] has three kinds:
`Auto`, which fits the content; `Fixed`, a fixed value; and `Fill`, which grows
into the leftover area.

```no_run
# use naui::{GridCell, Length, Result, ScrollPolicy, Sizing, Track, Ui};
# fn build(ui: &Ui) -> Result<()> {
let form = ui.grid()?;
form.set_spacing(12.0, 8.0);
form.set_column_track(0, Track::Fixed(96.0)); // the label column has a fixed width
form.set_column_track(1, Track::FILL);        // the input column takes the rest
form.attach(&ui.label("Name")?, GridCell::new(0, 0));

let field = ui.text_input("")?;
field.set_sizing(Sizing::fill_width());
form.attach(&field, GridCell::new(1, 0));

let scroll = ui.scroll()?;
scroll.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
scroll.set_child(&form);
scroll.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(160.0)));
# Ok(())
# }
```

[`Length::Fill`] means "take the leftover space" on the main axis and "stretch to
fill the parent" on the cross axis. A [`Spacer`] is `Fill` in both directions, so
in a stack it pushes the children after it to the far end. How several `Fill`
children in one stack split the space differs a little per platform.

In a `Grid` cell, a child that fits its content (`Auto`) sits at the **start** of
the cell horizontally and at the **center** vertically, on every backend. A
[`Track::Fill`] column or row only widens the cell; **the child itself stretches
only when it asks for `Fill`**, which is why the example above gives the text field
`Sizing::fill_width()`.

Adding a maximum ([`Sizing::max_width`] / [`Sizing::max_height`]) to `Fill` makes
the maximum double as "the size you want under normal conditions". **When the
parent can hand out space it grows up to the maximum, and when there is not
enough it becomes smaller than the maximum.** Use this form to size the display
area of widgets whose natural size cannot be relied on (a video before loading,
for example). In a `Grid`, [`Track::Fill`] rows and columns still take the
leftover space around such a child, and once it stops at its maximum it sits where
an `Auto` child would: at the start of the cell horizontally and in the middle
vertically.

An [`Image`] asks for its pixel size on an `Auto` axis. On a `Fill` axis it asks
for nothing and fits into the leftover space, so a large image inside a vertical
`Scroll` does not push the content out to the image's height. A [`Video`] does the
same, so it gets shorter with the window instead of pushing its controls out of
view.

The content of a [`Scroll`] is **never smaller than the visible area**: it
matches the visible area in a direction that does not scroll, and grows past it in
one that does. So while the content fits, a `Fill` child inside takes the leftover
space, as it would outside a `Scroll`.

An `Auto` height on a [`List`] follows all its rows. To scroll many rows within a
limited area, specify `Fixed` or `Fill`. [`Tree`], [`Table`], `Scroll`, `SplitView`
and [`TextArea`] do not decide their height from their content, so you normally
specify it with `set_sizing`.

### Switching the arrangement by width

A [`Stack`] can declare in advance the width from which it lays its children out in
the other orientation, with [`Stack::set_breakpoint`]. While the stack is at least
[`Breakpoint::min_width`] wide (its padding included) the children follow
[`Breakpoint::orientation`], and below that the stack's own orientation. The switch
is the same as [`Stack::set_orientation`]: the children stay where they are, and a
field being typed into keeps its focus. Going back takes a few pixels less than the
threshold, so a width right at the boundary does not flip it back and forth.

```no_run
# use naui::{Breakpoint, Orientation, Result, Sizing, Ui};
# fn build(ui: &Ui) -> Result<()> {
let fields = ui.stack(Orientation::Vertical)?;
fields.set_sizing(Sizing::fill_width()); // follow the width the parent offers
fields.set_spacing(8.0);
fields.append(&ui.text_input("")?);
fields.append(&ui.text_input("")?);
// Side by side from 560 wide, one above the other below that.
fields.set_breakpoint(Some(Breakpoint::new(560.0, Orientation::Horizontal)));
# Ok(())
# }
```

The width that counts is the one the stack gets, so give it `Fill` on the width
(or put it in a vertical stack with [`Align::Fill`]): a stack that fits its content
is only as wide as its children. [`Stack::orientation`] keeps returning the
stack's own orientation, and [`Stack::current_orientation`] the one in use now.
There is no notification when it switches; the arrangement is declared, not
rebuilt from a callback.

None of the four toolkits switches a stack by its own width without changing how
it is sized (`AdwBreakpointBin` and CSS container queries stop the container from
taking its size from its content, and WinUI's `TwoPaneView` holds only two panes),
so naui decides on every platform. It compares the width once layout is done and
switches on the next main loop iteration on macOS, Linux and Windows, and before
the page is painted on the Web (`ResizeObserver`). Before the first layout, and
while the stack is hidden, it keeps its own orientation.

## Scroll position

[`Scroll`] returns its current position and sizes from
[`metrics`](Scroll::metrics), and can be scrolled with
[`scroll_to`](Scroll::scroll_to) / [`scroll_to_end`](Scroll::scroll_to_end). The
unit is logical pixels, and the destination is clamped to the scrollable range.

```no_run
# use naui::{Orientation, Result, ScrollMetrics, Ui};
# fn build(ui: &Ui) -> Result<()> {
let log = ui.stack(Orientation::Vertical)?;
let scroll = ui.scroll()?;
scroll.set_child(&log);

// Keep following the end after appending only if the user was near the end.
let following = scroll.metrics().distance_to_end() < 120.0;
log.append(&ui.label("New line")?);
if following {
    scroll.scroll_to_end(); // fine to call right after appending
}

scroll.on_scroll(|m: ScrollMetrics| println!("{} / {}", m.y, m.max_y()));
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| Position and sizes | `VerticalOffset` / `ViewportHeight` / `ExtentHeight` | The `NSClipView`'s bounds and the content's frame | `GtkAdjustment` | `scrollTop` / `clientHeight` / `scrollHeight` |
| Scroll | `ChangeView` (no animation) | `scrollToPoint:` | `GtkAdjustment::set_value` | `scrollTo` |
| Notification | `ViewChanged` | `NSViewBoundsDidChangeNotification` | `value-changed` | The `scroll` event |

naui does not decide things like "follow along if within N px of the end"; the
app does that from [`ScrollMetrics`].

**Calling `scroll_to_end` right after changing the contents reaches the end of the
changed contents.** macOS, Windows and the Web finish layout before scrolling.
GTK4 has no way to lay out synchronously, so the destination is remembered until
the next layout and scrolled to again when the sizes change. That is why **on GTK
only, `metrics` right after changing the contents still has the previous sizes**,
catching up on the next frame.

**A destination requested while there is no size yet**, such as before being
placed in a window or inside a hidden tab, **takes effect once the size is
decided**. Calling `scroll_to_end` before showing makes it appear with the end
visible.

[`on_scroll`](Scroll::on_scroll) is called **only when the position changes**, not
for changes in size alone. Unlike other widgets' `set_*`, **it is also called for
moves made by `scroll_to`**. Every platform has moves that cannot be told apart
from user actions, such as clamping when the content shrinks or keyboard
movement, so who moved it is not distinguished. On Windows and the Web the native
notification arrives later, so it is called after `scroll_to` returns. You can
also call `scroll_to` inside the notification to snap to a row boundary; that
move arrives afresh after the notification returns.

On the Web, sizes are rounded to integers, so a difference of less than 1 px from
the end counts as being at the end (`distance_to_end` becomes 0 even when zoomed
in).

## Visibility, focus and tooltips

Every widget has these four:

```no_run
# use naui::{Orientation, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let name = ui.text_input("")?;
let error = ui.label("Please enter a name")?;
error.set_visible(false);           // hide (inside a Stack it takes no space either)
let _shown: bool = error.is_visible();

let save = ui.button("Save")?;
save.set_tooltip(Some("Save the changes")); // None removes it
save.on_click({
    let name = name.clone();
    let error = error.clone();
    move || {
        if name.text().is_empty() {
            error.set_visible(true);
            name.request_focus(); // back to the input field
        }
    }
});
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `set_visible` | `Visibility` (`Collapsed`) | `hidden` | `gtk_widget_set_visible` | `display: none` |
| `request_focus` | `UIElement.Focus` | `makeFirstResponder:` | `gtk_widget_grab_focus` | `focus()` |
| `set_tooltip` | `ToolTipService.ToolTip` | `toolTip` | `tooltip-text` | The `title` attribute |
| `set_accessible_label` | `AutomationProperties.Name` | `accessibilityLabel` | `GTK_ACCESSIBLE_PROPERTY_LABEL` | `aria-label` |

[`set_accessible_label`](Button::set_accessible_label) is the name reported to
screen readers; set it when there is no visible text, as with icon-only buttons.
Even on buttons with text, the given name takes precedence over the visible text
(GTK4 buttons set the `LABELLED_BY` relation to take their name from the inner
label, so it is removed while a name is set).

**Hidden widgets take no space inside a `Stack`.** In a `Grid` the cell, row and
column remain, and on some platforms the spacing (`set_spacing`) remains too.
[`is_visible`](Label::is_visible) returns the value set with `set_visible`, and
stays `true` even when the widget is not visible for reasons such as a hidden
parent or being inside a collapsed `Expander`.

[`request_focus`](TextInput::request_focus) returns `true` if focus moved. **If
the widget itself cannot take focus (containers such as `Stack`), focus moves to
the first thing inside that can.** It is `false` before being placed in a window,
while hidden, or while disabled. Labels do not take focus. Whether buttons and the
like take focus follows the OS setting (on macOS, the "Keyboard navigation"
setting changes whether buttons take focus).

On the Web, so that it beats the inline `display` naui writes per widget, a
single rule applying `display: none !important` to the `data-naui-hidden`
attribute is added to the document.

## Keyboard

Text-input widgets (`TextInput` / `TextArea` / `PasswordInput` / `SearchInput` /
`NumberInput` / `EditableComboBox`) and [`Window`] report pressed keys through
`on_key_down`. Returning [`EventResponse::Handled`] keeps the key from reaching
the field (inserting a newline or confirming) and the window.

```no_run
# use naui::{EventResponse, Key, KeyEvent, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let message = ui.text_area("")?;
message.on_key_down({
    // Capture itself weakly (capturing a clone() creates a cycle that is never freed).
    let message = message.downgrade();
    move |event: &KeyEvent| {
        let Some(message) = message.upgrade() else {
            return EventResponse::Continue;
        };
        // Enter sends, Shift+Enter inserts a newline.
        if event.key == Key::Enter && !event.modifiers.shift {
            println!("Send: {}", message.text());
            message.set_text("");
            EventResponse::Handled
        } else {
            EventResponse::Continue
        }
    }
});
# Ok(())
# }
```

**Keys pressed while an IME is composing are not delivered.** The IME receives
the Enter that confirms the composition, so the example above never sends when
the user merely confirms a conversion. (Per-platform ways of telling, such as
`keyCode` 229, are absorbed inside naui.) The key that **starts** composition
may itself be delivered on macOS and Linux (at that point it is not composing
yet). Returning `Continue` for character keys lets composition start.

Keys are delivered **to the focused widget, then the window**, and keys for
which the widget returned `Handled` do not reach the window. The window's
[`on_key_down`](Window::on_key_down) also receives keys while something other
than a text input (a button, for example) has focus.

Modifier keys (⇧, Ctrl, Alt, ⌘ / Windows key) are not delivered when pressed on
their own. Check whether they were held together with another key through
[`KeyEvent::modifiers`].

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| Receiver | `PreviewKeyDown` on the window root | A local `NSEvent` monitor | The window's `GtkEventControllerKey` (capture) | The element's `keydown` (capture) and `document` |
| Detecting composition | `VK_PROCESSKEY` and `TextCompositionStarted` / `Ended` | `hasMarkedText` | `preedit-changed` | `isComposing` and `keyCode` 229 |

Letters in [`Key::Character`] are normalized to lowercase (the same as
[`MenuShortcut`]). Check [`Modifiers::shift`] for whether ⇧ was held. Which
character a symbol key gives with ⇧ depends on the platform and keyboard layout.
[`Modifiers::primary`] returns whether the primary modifier (⌘ on macOS, Ctrl
elsewhere) was held.

**The order of `on_key_down` relative to menu bar shortcuts differs by
platform** (on macOS it arrives before the shortcut; on other platforms it may
come after). Do not assign the same key to both a shortcut and `on_key_down`.

## Focus entering and leaving

Text-input widgets (the six with `on_key_down`) report focus entering (`true`)
and leaving (`false`) through `on_focus_change`. Use it for validation or saving
when the user leaves a field.

```no_run
# use naui::{Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let mail = ui.text_input("")?;
// Capture mail itself weakly (capturing a clone() means it is never freed).
let weak = mail.downgrade();
mail.on_focus_change(move |focused| {
    let Some(mail) = weak.upgrade() else { return };
    if !focused && !mail.text().contains('@') {
        println!("Please check the email address");
    }
});
# Ok(())
# }
```

**Do not capture a `clone()` of the field's own handle in its notification
closure.** The field holds the notification and the notification holds the
field, forming a cycle, so it is not freed even after it is removed from the
screen and released (the same goes for other widgets' notifications). As above,
create a weak reference ([`WeakWidget`]) with `downgrade()`, capture that, and
`upgrade()` it inside the notification. After the widget is freed, `None` is
returned.

It is called **only when focus moves within the window**. When the window itself
becomes inactive (switching to another app, or on the Web switching tabs or
browser windows), focus within the window does not change, so it is not called.
Moving from part to part within a widget (between a number input's field and its
step buttons, for example) does not call it either. If the widget focus moves to
also has a handler, the `false` for the one left arrives first.

If focus is already inside the widget when its handler is set, entering is not
reported (the `false` for leaving it does arrive). Whether focus is placed on the
first field when a window is shown is up to the platform (macOS, Linux and
Windows may do so; the Web does not), so setting the handler before showing means
the `true` arrives on some platforms and not on others.

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| What is watched | `GotFocus` on the window root | The `NSWindow`'s `makeFirstResponder:` | The `GtkWindow`'s `focus-widget` | `focusin` / `focusout` on `document` |

## Busy indicators and selectable labels

For work with no visible end (waiting for a network response, for example), set a
[`ProgressBar`] to [`set_indeterminate(true)`](ProgressBar::set_indeterminate) to
show it with motion. Turning it off returns to showing the value set with
[`set_value`](ProgressBar::set_value).

Where something small loads (beside a button or a status text, or in place of
content that is not there yet), use a [`Spinner`]. It spins from creation, and
[`set_spinning(false)`](Spinner::set_spinning) stops it. A stopped spinner is not
drawn but keeps its space, so the widgets around it do not move; use
[`set_visible(false)`](Spinner::set_visible) to give the space up as well. The DOM has
no spinner, so the Web shows an indeterminate `<progress>`, which browsers draw as a
bar.

Setting [`Label::set_selectable`] to `true` lets the text be selected and copied
(it cannot be edited). Use it for text readers will want to copy, such as chat
messages or error details. The default is not selectable on all four platforms
(only on the Web is the browser default "selectable", so naui aligns it with
`user-select: none`).

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| Indeterminate progress | Moves the foreground band back and forth (※) | `indeterminate` of `NSProgressIndicator` | `gtk_progress_bar_pulse` at a fixed interval | `<progress>` without a `value` attribute |
| Spinner | `ProgressRing` (`IsActive`) | `NSProgressIndicator` in the spinning style | `AdwSpinner` (`GtkSpinner` before libadwaita 1.6) | `<progress>` without a `value` attribute |
| Stopped spinner | `IsActive` off | `stopAnimation` with `displayedWhenStopped` off | `child-visible` off (`spinning` off for `GtkSpinner`) | `visibility: hidden` |
| Selectable labels | `IsTextSelectionEnabled` | `selectable` | `gtk_label_set_selectable` | `user-select` |

※ WinUI's `ProgressBar` crashes when its template is applied in an unpackaged
launch, so naui uses a band built from the same theme resources.

## Confirming before a window closes, and window size

[`Window::on_close_request`] is called when the user tries to close the window.
Returning [`CloseResponse::KeepOpen`] keeps it open. It is not called for
[`Window::close`] from code.

```no_run
# use std::cell::Cell;
# use std::rc::Rc;
# use naui::{CloseResponse, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let window = ui.window("Editor", 640.0, 480.0)?;
let dirty = Rc::new(Cell::new(true));
window.on_close_request(move || {
    if dirty.get() {
        // Show a dialog asking whether to save here, then `close` based on the answer.
        CloseResponse::KeepOpen
    } else {
        CloseResponse::Close
    }
});
window.on_resize(|(width, _height)| println!("width {width}"));
let (_width, _height) = window.size();
# Ok(())
# }
```

[`Window::set_min_size`] sets the minimum size the user can shrink the window to
(on Windows it takes effect with Windows App SDK runtime 1.7 or later).

On all four platforms, a window's size counts **the content size, excluding the
title bar, menu bar, toolbar and window frame** (with a sidebar attached, its
width is included). The width and height of `ui.window(title, width, height)`,
[`Window::set_size`], [`Window::size`] and the minimum of
[`Window::set_min_size`] all mean the same thing, so the `size` of
`ui.window(.., 900.0, 640.0)` is 900×640 on every platform (for the Web, see the
note below). [`Window::on_resize`] is called with the new size every time the
size changes (including when it is changed with `set_size`).

Right after `set_size`, `size` may still return the previous size on some
platforms (on Windows and Linux it changes after layout). Receive the changed
size through `on_resize`. Before the window is shown, `size` returns the size
given to `ui.window` or `set_size`.

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| Confirmation before closing | `AppWindow.Closing` (`Cancel`) | `windowShouldClose:` | `close-request` | (never called) |
| Size changes | `SizeChanged` of the content layer | `windowDidResize:` | Compares the content size after painting (`after-paint`) | `ResizeObserver` |
| Changing the size | `AppWindow.Resize` (adds the difference from the outer frame) | `setContentSize` (adds the difference from `contentLayoutRect`) | `set_default_size` (adds the header bar and menu bar heights and the window's CSS frame) | `max-width` of the window element (the height follows the browser) |

On every platform the native API takes the size of the whole window (or of a
`contentView` extending under the title bar), so naui measures the current
difference and adds it. On Windows and Linux, right after showing, while the
content is not laid out yet, the heights of the menu bar and the like can only be
estimated, so it refits once layout finishes. On Linux, a `set_size` right after
attaching or detaching a strip is also checked after relayout, and on Wayland, if
the compositor overwrites the size requested right after showing, it is requested
again (both for up to 1 second after showing or `set_size`). Windows's
`AppWindow` takes physical pixels, so `RasterizationScale` is applied too. macOS's
AppKit changes the content size when a toolbar or sidebar is attached or detached
(attaching a toolbar grows the content by the title bar's height), so naui
restores the previous size and changes the window frame instead. However, for
sizes that do not fit the screen, AppKit shrinks the frame on show to fit the
screen (excluding the menu bar and Dock), so `size` becomes smaller by that much.

**The Web cannot fully decide the size.** The size of the browser window itself
cannot be changed, and it plays the part of the window the user resizes. So the
height follows the browser's visible area, and the width is treated as `max-width`
(shrinking on narrow screens). The requested height is not used; `set_min_size`
still applies as `min-width` / `min-height`, and the page scrolls when the browser
is smaller.

**On the Web, the confirmation before closing is never called.** There is no user
action for closing a window inside the page, and no way to show a custom
confirmation when the tab closes.

## Window position and state

[`Window::set_state`] maximizes, minimizes or makes the window fullscreen, and
[`Window::state`] reads the current state as a [`WindowState`].
[`Window::set_position`] and [`Window::position`] are the position of the
window's top-left corner, and [`Window::set_resizable`] and
[`Window::is_resizable`] are whether the user can resize the window by dragging
its edges.

```no_run
# use naui::{Result, Ui, WindowState};
# fn build(ui: &Ui) -> Result<()> {
let window = ui.window("Viewer", 800.0, 600.0)?;
window.set_position(80.0, 60.0); // does nothing on Linux and the Web
window.set_state(WindowState::Maximized); // maximized when shown
window.on_state_change(|state| println!("{state:?}"));
window.show();

// Toggle fullscreen from a button (the Web allows it only inside a user action).
let toggle = ui.button("Fullscreen")?;
let weak = window.downgrade();
toggle.on_click(move || {
    if let Some(window) = weak.upgrade() {
        let next = if window.state() == WindowState::Fullscreen {
            WindowState::Normal
        } else {
            WindowState::Fullscreen
        };
        window.set_state(next);
    }
});
# Ok(())
# }
```

The position is **in logical pixels, counted right and down from the top left of
the primary screen**, and refers to the top left of the window frame including
the title bar. It may be set before the window is on screen. After the position
is set with [`set_position`](Window::set_position), macOS's
[`set_size`](Window::set_size) no longer recenters the window either.

While maximized, minimized or in fullscreen, [`Window::position`] returns **the
position for returning to the normal state**. [`Window::set_position`] leaves the
state as is and decides that return position (the visible frame does not move).
[`Window::set_size`] does the same with the size on Windows, macOS and Linux. To
save all of these when the app quits, use [`Window::placement`] (see [Saving and
restoring the window](#saving-and-restoring-the-window)).

Only one [`WindowState`] is returned. States that can overlap on some platforms
(minimized while maximized, for example) resolve to the one the user sees
(minimized → fullscreen → maximized, in that order). Requesting
[`WindowState::Normal`] undoes minimize, fullscreen and maximize alike. When
moving from fullscreen to another state, it is applied after leaving fullscreen
has finished.

State changes are often asynchronous (macOS fullscreen happens after an
animation, Linux after the compositor responds, the Web after the browser
switches). `state` right after `set_state` may still return the previous state,
so receive the change through [`Window::on_state_change`]. It is also called when
the state changes through user actions (title bar buttons, leaving fullscreen
with Esc and so on). `set_state` before the window is on screen is remembered and
applied in [`Window::show`] (until then, `state` returns the requested state; on
Linux it does so until the compositor's first state arrives after showing, too).

There is no notification for position changes, because Linux and the Web cannot
obtain the position at all. To save the position, read [`Window::placement`]
before closing (inside [`Window::on_close_request`], for example).

What can be done differs by platform.

| | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| Position | ○ | ○ | × (always `None`) | × (always `None`) |
| Maximize / minimize | ○ | ○ | ○ (minimization cannot be detected on Wayland) | × |
| Fullscreen | ○ | ○ | ○ | ○ (only from inside a user action) |
| Resizable | ○ | ○ | ○ | × (the value is just remembered) |

The APIs used on each platform are as follows.

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| Position | `AppWindow.Position` / `Move` (physical pixels divided by `RasterizationScale`) | `frame` / `setFrameTopLeftPoint:` (flipped by subtracting from the primary screen's height) | (none; always `None`) | (none; always `None`) |
| Maximize | `OverlappedPresenter.Maximize` | `zoom:` | `gtk_window_maximize` | (none) |
| Minimize | `OverlappedPresenter.Minimize` | `miniaturize:` | `gtk_window_minimize` (restored with `gtk_window_unminimize` and `present`) | (none) |
| Fullscreen | `AppWindow.SetPresenterByKind(FullScreen)` | `toggleFullScreen:` | `gtk_window_fullscreen` | `documentElement.requestFullscreen()` |
| Reading the state | The kind of `AppWindow.Presenter` and `OverlappedPresenter.State` | `isMiniaturized` / `isZoomed` and fullscreen notifications | `GdkToplevelState` | `document.fullscreenElement` |
| State changes | `AppWindow.Changed` | `windowDidMiniaturize:` / `windowDidEnterFullScreen:` and so on | `notify::state` of `GdkToplevel` | `fullscreenchange` |
| Resizable | `OverlappedPresenter.IsResizable` (together with `IsMaximizable`) | `Resizable` in the styleMask | `gtk_window_set_resizable` | (none; the value is just remembered) |

**Linux cannot handle window positions.** GTK4 has no API to read or move a
window's position. On Wayland there is no mechanism at all for an app to know its
window's position (the compositor decides placement), and GTK4 dropped it on X11
too to match. [`Window::position`] always returns `None`, and
[`Window::set_position`] does nothing. Also, **Wayland has no mechanism for
knowing about or restoring from minimization**, so minimizing does not make
`state` [`WindowState::Minimized`], and whether requesting `Normal` restores it is
up to the compositor (both work on X11). A minimize requested before showing is
ignored by window managers when requested before the window is shown, so it is
applied after showing (the window appears on screen for a moment).

**The Web has only fullscreen.** The only thing a page can operate is the
Fullscreen API, so the position is `None`, maximize and minimize do nothing (they
only leave fullscreen if in it), and `set_resizable` just remembers the value.
Fullscreen applies to **the whole page** (`document.documentElement`), so with
several windows they all go into the fullscreen view. Browsers allow fullscreen
only from inside a user action (a click or key), so call it from a button's
`on_click` or similar (calls at startup or from a timer are refused and the state
does not change). iPhone Safari has no Fullscreen API.

**Windows positions include an invisible border.** The `AppWindow`'s outer frame
includes the resize handles Windows adds around the window (an invisible border a
few pixels wide), so it is a little up and left of the visible frame's top left.
Passing the position you read straight back to `set_position` returns it to the
same place.

Even a window that cannot be resized can still be resized with
[`Window::set_size`]. The maximize button stops working on all three platforms
(macOS cannot zoom, Linux removes the button, and Windows also turns off
`IsMaximizable`).

## Saving and restoring the window

[`Window::placement`] returns the window's size, position and state as a
[`WindowPlacement`], to save when the app quits. [`Window::set_placement`] puts
them back; call it before [`Window::show`] so the window appears where it was.
`WindowPlacement` turns into one line of text with `to_string()` and back with
[`WindowPlacement::parse`], so it can go into whatever settings file the app
already has (naui does not decide where settings are stored).

```no_run
# use naui::{CloseResponse, Result, Ui, WindowPlacement};
# fn load_setting(_: &str) -> Option<String> { None }
# fn save_setting(_: &str, _: &str) {}
# fn build(ui: &Ui) -> Result<()> {
let window = ui.window("Editor", 800.0, 600.0)?;
if let Some(placement) = load_setting("window").and_then(|text| WindowPlacement::parse(&text)) {
    window.set_placement(&placement);
}
let weak = window.downgrade();
window.on_close_request(move || {
    if let Some(window) = weak.upgrade() {
        // "size=800x600 position=80,60 state=maximized"
        save_setting("window", &window.placement().to_string());
    }
    CloseResponse::Close
});
window.show();
# Ok(())
# }
```

The size and position are **those of the normal state**. A window closed while
maximized or in fullscreen reports the frame it returns to, together with the
state, so the next start shows it maximized again and un-maximizing it returns it
to where the user had it. A minimized window reports the state it returns to
(maximized if it was maximized), never [`WindowState::Minimized`], so the app
does not start minimized.

`set_placement` applies the size, then the position, then the state. The
position is skipped if the window's top edge would not be on any screen connected
now (the screen it was on has been disconnected, for example), and the window is
then placed as if no position had been saved.

| | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| Size | ○ | ○ | ○ | The requested size (the browser decides the actual size) |
| Position | ○ | ○ | × (always `None`) | × (always `None`) |
| State | ○ | ○ | ○ (minimization cannot be detected on Wayland) | Fullscreen only, refused at startup |
| Normal frame while maximized | The content size last seen in the normal state | The frame before `zoom:` / before entering fullscreen | The window's default size (GTK4 keeps the size from before maximizing there) | (none) |

**Linux and the Web restore only the size and state.** On Linux the compositor
decides where windows go, and on the Web pages have no position on the screen. On
the Web, [`Window::placement`] returns the size requested with `ui.window` or
`set_size`, not the measured one: the browser window decides the actual size, and
passing a narrow browser's width back would keep the page narrow (it becomes the
`max-width`). Browsers also refuse fullscreen outside a user action, so a saved
fullscreen does not apply at startup.

## Collapsible sections

Put things you want hidden normally and shown only when the heading is clicked in
an [`Expander`]. It holds one child, so to show several things, put in a
container such as a `Stack`.

```no_run
# use naui::{Orientation, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let details = ui.expander("Advanced settings")?;
let body = ui.stack(Orientation::Vertical)?;
body.append(&ui.checkbox("Create a backup")?);
details.set_child(&body);
details.set_expanded(true); // expand without notifying
details.on_toggle(|expanded| println!("expanded: {expanded}"));
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `Expander` | `Expander` | `NSButton` (disclosure) + `NSStackView` | `GtkExpander` | `<details>` + `<summary>` |

Windows uses WinUI 3's native `Expander`. AppKit has no single Expander, so macOS
combines a standard `NSButton` and an `NSStackView`.

While collapsed, the content leaves the layout (it takes no space). It starts
collapsed, and [`set_expanded`](Expander::set_expanded) is an operation from code,
so it does not call `on_toggle` (the same rule as [`Checkbox::set_checked`]).

## Splitting panes

To split the screen in two and let the user move the boundary, use
[`SplitView`]. There are two panes, start (left or top) and end (right or bottom),
and the divider position expresses **the size of the start side** in logical
pixels.

```no_run
# use naui::{Orientation, Result, Sizing, Ui};
# fn build(ui: &Ui) -> Result<()> {
let split = ui.split_view(Orientation::Horizontal)?; // panes side by side
split.set_start(&ui.tree()?);      // sidebar
split.set_end(&ui.text_area("")?); // body
split.set_position(220.0);         // place the divider without notifying
split.set_min_sizes(120.0, 200.0); // cannot be made narrower than this
split.on_resize(|position| println!("the sidebar is {position} wide"));
split.set_sizing(Sizing::fill()); // not decided by the content's height
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `SplitView` | `Grid` + a divider `Grid` | `NSSplitView` | `GtkPaned` | `<div>` + `<div role="separator">` |

**The end side takes the leftover space.** When the window grows, the start side
keeps its specified size, so it works as is for combinations like a sidebar and a
body. To do the opposite, put the narrower one on the end side.

Windows and the Web have no corresponding control, so naui builds one, but there
too **only a 1 px line drawn at the boundary is visible**, and the rest is a
transparent grab area (6 px). A filled band would look less like a separator and
more like a component of its own, standing out from its surroundings. Colors come
from theme resources and CSS system colors, and the cursor shape is left to CSS's
`col-resize` / `row-resize`.

The position right after creation is [`DEFAULT_SPLIT_POSITION`] (200 px), the
same on all four platforms. [`set_position`](SplitView::set_position) does not
notify, and [`drag_to`](SplitView::drag_to) calls
[`on_resize`](SplitView::on_resize) as if the user had moved it (the same rule as
`set_selected` and `select` of `ComboBox`).

Both defaults of [`set_min_sizes`](SplitView::set_min_sizes) are 0, but **the
content controls themselves may not shrink any further**. In that case the
minimum decided by the platform wins. When the screen is too narrow to satisfy
both minimums, the start side takes precedence, and once it widens it returns to
the specified position.

As with `Scroll`, **the height is not determined by the content's size**, so
specify the size with [`set_sizing`](SplitView::set_sizing). There are two panes;
to split into three or more, nest `SplitView`s.

## Label wrapping

[`Label`] **does not wrap by default**. Whatever does not fit on one line is cut
off at the end with an ellipsis (…). To show long text, specify
[`set_wrap(true)`](Label::set_wrap).

```no_run
# use naui::{Result, Sizing, Ui};
# fn build(ui: &Ui) -> Result<()> {
let note = ui.label("A long description that should wrap in narrow places")?;
note.set_wrap(true);
note.set_sizing(Sizing::fill_width()); // the parent decides the wrapping width
# Ok(())
# }
```

**The parent decides the wrapping width**, so inside a `Stack` also give it a
width with [`set_sizing`](Label::set_sizing). Without a decided width, every
platform keeps demanding the width of one line.

| Platform | Not wrapping | Wrapping |
| --- | --- | --- |
| Windows | `TextWrapping="NoWrap"` + `TextTrimming="CharacterEllipsis"` | `TextWrapping="Wrap"` |
| macOS | `usesSingleLineMode` + `ByTruncatingTail` | `ByWordWrapping` + `preferredMaxLayoutWidth` |
| Linux | `PangoEllipsizeMode::End` | `gtk_label_set_wrap` |
| Web | `white-space: nowrap` + `text-overflow: ellipsis` | `white-space: normal` |

A `<span>` wraps by default, so **on the Web only, naui uses CSS to match the
other three platforms** (the same treatment as the cells of `Table` and `Tree`).

## Label typography

A [`Label`]'s size and color can be changed with
[`set_style`](Label::set_style) and [`set_color`](Label::set_color). What you
pass is **a level and a role**, not point sizes or color values. naui maps them
onto the heading levels and semantic colors the platform has as standard (the
same rule as [`ToolbarIcon`]).

```no_run
# use naui::{Result, TextColor, TextStyle, Ui};
# fn build(ui: &Ui) -> Result<()> {
let title = ui.label("Settings")?;
title.set_style(TextStyle::Title);

let note = ui.label("There are unsaved changes")?;
note.set_style(TextStyle::Caption);
note.set_color(TextColor::Warning);
# Ok(())
# }
```

The OS decides the actual point sizes and colors, so **they follow larger-text
settings, the accent color and light / dark switching as is**. There are six
levels in [`TextStyle`] and six roles in [`TextColor`], and the mapping of each is
tabulated in that type's documentation.

| Platform | Levels map to | Colors map to |
| --- | --- | --- |
| macOS | `NSFontTextStyle` (`preferredFontForTextStyle:`) | `NSColor`'s semantic colors |
| Windows | The type ramp's `Style`s | `{ThemeResource}` brushes |
| Linux | libadwaita style classes | Style classes as well |
| Web | Relative `em` values decided by naui | CSS system colors (naui decides only three) |

**Only on the Web do browsers lack heading levels as such**, so, as with
wrapping, naui uses CSS to match the other three platforms. For colors,
`Secondary` and `Accent` can use CSS system colors (`GrayText` / `AccentColor`),
but there are no system colors for success, warning and danger, so naui decides
just those values with `light-dark()`. Also, **Chromium-based browsers return
gray for `AccentColor` instead of the OS accent color** (Firefox and Safari return
the OS color).

## Text input

Use [`TextInput`] for one line, [`TextArea`] for text with newlines,
[`PasswordInput`] for masked input, and [`SearchInput`] for filter or search
fields. Their APIs all have the same shape, and IME, copy / paste and undo are
left to the native side.

`TextInput` and `TextArea` can be made read-only with
[`set_read_only(true)`](TextArea::set_read_only). Unlike `set_enabled(false)`, it
is not dimmed and the text can be selected and copied, so use it for fields that
show logs or generated output (`IsReadOnly` / `editable` /
`gtk_editable_set_editable` / `readonly`).

```no_run
# use naui::{Length, Result, Sizing, Ui};
# fn build(ui: &Ui) -> Result<()> {
let memo = ui.text_area("")?;
memo.set_placeholder("A multi-line memo (newlines allowed)");
// Like a scroll view, it has no height that fits its contents, so specify one.
memo.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(96.0)));
memo.on_change(|text| println!("{} characters", text.chars().count()));
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `TextInput` | `TextBox` | `NSTextField` | `GtkEntry` | `<input type="text">` |
| `TextArea` | `TextBox` (`AcceptsReturn`) | `NSTextView` + `NSScrollView` | `GtkTextView` + `GtkScrolledWindow` | `<textarea>` |
| `PasswordInput` | `PasswordBox` | `NSSecureTextField` | `GtkPasswordEntry` | `<input type="password">` |
| `SearchInput` | `AutoSuggestBox` | `NSSearchField` | `GtkSearchEntry` | `<input type="search">` |

[`TextArea`] **wraps long lines and scrolls vertically when it overflows**. A
setting to choose whether to wrap is not part of what the three platforms share,
so there is none. macOS's `NSTextView` has no placeholder, so a dim label is
overlaid (clicks hit-test through to the `NSTextView` below).

`set_text` does not call `on_change` (as with `TextInput`, Windows alone calls it
because the native `TextChanged` fires).

**Confirmation with Enter is received through
[`on_submit`](TextInput::on_submit)** (`TextInput` / `PasswordInput` /
[`NumberInput`]). Use it for work that should run once the user finishes typing,
such as submitting a form or logging in.

```no_run
# use naui::{Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let name = ui.text_input("")?;
name.on_change(|text| println!("typing: {text}")); // on every keystroke
name.on_submit(|text| println!("register {text}")); // when confirmed with Enter
# Ok(())
# }
```

**It is not called for an Enter pressed while an IME is composing (which
confirms the conversion).** It is also not called for a read-only `TextInput`, or
when [`on_key_down`](TextInput::on_key_down) returned `Handled` for Enter.
`NumberInput` reads what has been typed into the value first (reverting to the
previous value if it cannot be read), then calls it with the confirmed value; if
the value changed, `on_change` is called first. Enter in a `TextArea` is a
newline, so it has none (pick it up in `on_key_down` if you use it for sending).
In `SearchInput`, the same notification is called `on_search`.

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `on_submit` | `PreviewKeyDown` for Enter | `insertNewline:` (`control:textView:doCommandBySelector:`) | `activate` | `keydown` for Enter |

[`PasswordInput`] only masks typed characters; its API is the same as
[`TextInput`]. **It has no button to temporarily reveal the text**
(`NSSecureTextField` has none). The entered string can be read with `text()`, so
handling it is the app's responsibility.

[`SearchInput`] is [`TextInput`] plus **a confirmation notification**.
[`on_change`](SearchInput::on_change) is called on every keystroke, and
[`on_search`](SearchInput::on_search) **when the user confirms with Enter** (on
Windows, clicking the magnifying glass also confirms). Use `on_change` to filter
while typing, and `on_search` to search only when asked.

```no_run
# use naui::{Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let search = ui.search_input()?;
search.set_placeholder("Search");
search.on_change(|text| println!("filter: {text}"));
search.on_search(|text| println!("search for {text}"));
# Ok(())
# }
```

The magnifying glass and the **clear button (✕) that appears once you start
typing are shown by the platform** (naui builds no visuals). **The clear button is
not a confirmation.** Becoming empty is reported through `on_change`, and
`on_search` is not called. Some browsers show no clear button (Firefox has none).

[`set_text`](SearchInput::set_text) does not notify (the same as `TextInput`). It
has no suggestion list. Windows's `AutoSuggestBox` has a mechanism for showing
suggestions, but it is not used because the search fields of the other three
platforms have none.

## Number input

Use [`NumberInput`] to have the user enter a number. The value is an `f64`, and
the range, step and decimal places are decided by [`NumberSpec`]. **The default is
integers** (step 1, 0 decimal places, no range limit), so to handle decimals,
specify both.

```no_run
# use naui::{Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let count = ui.number_input(1.0)?;
count.set_range(Some(1.0), Some(99.0)); // cannot go outside 1..=99
count.on_change(|value| println!("{value} items"));

let rate = ui.number_input(0.5)?;
rate.set_decimals(2); // show two digits, as in 0.50
rate.set_step(0.05); // how much the up/down buttons move it
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `NumberInput` | `NumberBox` | `NSTextField` + `NSStepper` | `GtkSpinButton` | `<input type="number">` |

The value is **rounded to the decimal places first, then clamped to the range**.
Passing an out-of-range value to [`set_value`](NumberInput::set_value) moves it to
the end without notifying. The display is not rewritten while typing, and
`on_change` is called once the text can be read. It has no width that fits its
content (the same as `TextInput`), so specify the width with
[`set_sizing`](NumberInput::set_sizing). The display is brought back in line with
the value **on confirmation** (Enter, leaving the field, pressing the step
buttons), and text that cannot be read as a number reverts to the previous value
on confirmation.

**On Windows, values are limited to 10 significant digits.** `NumberBox` rounds
to 10 significant digits before building its display, and on confirmation it
reads that display back, so the value matches it too. Even if
[`NumberSpec::decimals`] is given more digits than that, Windows cuts them at 10
(the other three platforms handle the full precision of `f64`).

## On/off switches

Switch between the two choices of on and off with a [`Checkbox`] or a
[`Toggle`]. Both have the same API; only the presentation differs. **Check boxes,
which put a mark, suit things decided together, such as "I agree"; switches,
which move a knob, suit settings that take effect immediately** (which to use
follows the platform's conventions).

```no_run
# use naui::{Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let backup = ui.toggle("Create a backup")?;
backup.set_on(true); // turn on without notifying
backup.on_toggle(|on| println!("backup: {on}"));
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `Checkbox` | `CheckBox` | `NSButton` (check type) | `GtkCheckButton` | `<input type="checkbox">` |
| `Toggle` | `ToggleSwitch` | `NSSwitch` + `NSTextField` | `GtkSwitch` + `GtkLabel` | `<input type="checkbox" switch>` |

[`set_checked`](Checkbox::set_checked) and [`set_on`](Toggle::set_on) are
operations from code, so they do not call `on_toggle`.

`NSSwitch` and `GtkSwitch` have no text, so on macOS and Linux a label is placed
next to them. On Windows the same text is put in `OnContent` / `OffContent`, so
it reads the same whether on or off. **The hit area for switching is the switch
part**; only on the Web, because it is built with a `<label>`, clicking the text
also switches it.

On the Web, naui only asks the browser to "draw it as a switch" with the
`switch` attribute and builds no visuals itself. **Only Safari 17.4 and later
support this attribute**, so Chrome and Firefox show it as a check box (value
handling, notifications and screen reader announcements (`role="switch"`) are the
same).

## Choice input

Use [`ComboBox`] to let the user choose one option in little space. The options
are shown in a drop-down, and the chosen one is returned as an index.

```no_run
# use naui::{Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let language = ui.combo_box()?;
language.set_items(&["Rust", "Swift", "TypeScript"]);
language.set_selected(0); // choose the initial value without notifying
language.on_select(|index| println!("item {index} was chosen"));
# Ok(())
# }
```

To also accept values that are not among the options, use [`EditableComboBox`].
It is a field you can type into, and the options are merely an input aid. **The
value is a string, not an index**, and `on_change` is called both for typing and
for choosing an option.

```no_run
# use naui::{Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let city = ui.editable_combo_box()?;
city.set_items(&["Tokyo", "Osaka", "Sapporo"]);
city.set_placeholder("City");
city.set_text("Kyoto"); // values not among the options go in too (no notification)
city.on_change(|text| println!("{text} was entered"));
# Ok(())
# }
```

[`selected`](EditableComboBox::selected) returns "the option that exactly matches
the current string", so it is `None` while a value not among the options is in
the field. `set_text` and `set_selected` do not notify, and `select` notifies just
as when the user chooses.

**Whether typed characters narrow down the option list differs by platform.**
naui does not build the list itself; it leaves it to each control.

| Platform | Typed characters and the option list |
| --- | --- |
| Windows | Not narrowed (the selection just moves to a matching option) |
| macOS | Not narrowed (the input field is completed instead) |
| Linux | Not narrowed (pressing the arrow shows all options) |
| Web | The browser narrows it (the rules depend on the browser and cannot be turned off) |

To narrow it on every platform, have the app call `set_items` again inside
`on_change` to replace the options themselves (on the Web, the browser's own
narrowing is layered on top).

To show all options on screen and let the user choose, use [`RadioGroup`]. Its API
is the same as `ComboBox`'s; only the presentation of the options differs.

```no_run
# use naui::{Orientation, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let plan = ui.radio_group()?;
plan.set_items(&["Free", "Standard", "Premium"]);
plan.set_orientation(Orientation::Horizontal); // vertical by default
plan.set_selected(0);
plan.on_select(|index| println!("item {index} was chosen"));
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `ComboBox` | `ComboBox` | `NSPopUpButton` | `GtkDropDown` | `<select>` |
| `RadioGroup` | A set of `RadioButton`s | Radio-type `NSButton`s | Grouped `GtkCheckButton`s | `<input type="radio">` |

For both, [`set_items`](ComboBox::set_items) changes what option indices mean, so
it clears the selection. `set_selected` / `clear_selection` do not notify, and
`select` calls `on_select` just like a user action. `ComboBox` is not an editable
combo box that accepts free input. A `RadioGroup` is one group, so options are
mutually exclusive only within it.

## Dates and times

To let the user pick a date or time, use [`DatePicker`]. What is picked is decided
by the [`DatePickerMode`] at creation, and values are exchanged as [`DateTime`]
(year, month, day, hour and minute).

```no_run
# use naui::{DatePickerMode, DateTime, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let start = ui.date_picker(DatePickerMode::Date)?;
start.set_value(DateTime::date(2026, 8, 22)); // set the value without notifying
start.set_range(Some(DateTime::date(2026, 1, 1)), None); // only a lower bound
start.on_change(|value| println!("{value} was chosen"));

let alarm = ui.date_picker(DatePickerMode::Time)?;
alarm.set_value(DateTime::time(7, 30));
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `DatePicker` | `DatePicker` / `TimePicker` | `NSDatePicker` | `GtkCalendar` + `GtkSpinButton` | `<input type="date">` and others |

The value right after creation is **the platform's current date and time (local
time)**, and there is no empty state (because `NSDatePicker` has no "nothing
selected"). It also has no seconds (see [`DateTime`]).

[`DatePickerMode::Date`] **only stops the user from picking** the time, and
[`DatePickerMode::Time`] the date; **neither discards it**. The part set with
`set_value` stays in [`DatePicker::value`] as is.

[`set_value`](DatePicker::set_value) does not notify, and values that are not
valid calendar dates (such as November 31) are rounded. The value cannot leave
[`set_range`](DatePicker::set_range), and range comparisons use **only the part
being picked**.

If you only want the user to pick a time, [`TimePicker`] is easier to handle. Its
value is a [`Time`] (hours and minutes only), so you do not carry around a date
you do not need.

## Picking a time

To let the user pick only a time, use [`TimePicker`]. Values are exchanged as
[`Time`] (hours and minutes), with no date.

```no_run
# use naui::{Result, Time, Ui};
# fn build(ui: &Ui) -> Result<()> {
let alarm = ui.time_picker()?;
alarm.set_value(Time::new(7, 30)); // set the value without notifying
alarm.set_range(Some(Time::new(6, 0)), Some(Time::new(9, 0)));
alarm.on_change(|value| println!("{value} was chosen")); // 07:30
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `TimePicker` | `TimePicker` | `NSDatePicker` (hours and minutes) | A set of `GtkSpinButton`s | `<input type="time">` |

The value right after creation is **the platform's current time (local time)**,
and there is no empty state (the same as [`DatePicker`]). **It has no seconds**
(see [`Time`]). Whether the 12-hour or 24-hour clock is used follows the
platform's locale.

[`set_value`](TimePicker::set_value) does not notify, and values that are not
valid clock times (such as 25:70) are clamped to the ends. There is no carry, so
`Time::new(25, 70)` becomes 23:59, not 2:10 the next day. The value cannot leave
[`set_range`](TimePicker::set_range). Ranges that cross midnight (such as 22:00
to 06:00 the next day) cannot be specified — [`Time`] has no date, and when the
lower bound is after the upper bound, the upper bound wins.

## Picking a color

To let the user pick a color, use [`ColorPicker`]. Values are exchanged as
[`Color`] (8-bit sRGB), and the platform's own color picking UI opens as is.

```no_run
# use naui::{Color, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let accent = ui.color_picker()?;
accent.set_value(Color::rgb(0x33, 0x66, 0xff)); // set the value without notifying
accent.on_change(|value| println!("{value} was chosen")); // #3366ff
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `ColorPicker` | `Button` + `Flyout` + `ColorPicker` | `NSColorWell` | `GtkColorDialogButton` | `<input type="color">` |

The value right after creation is black ([`Color::BLACK`]). **Opacity is not
handled** — `<input type="color">` returns only opaque colors, so naui sticks to
the range all four platforms share.

[`set_value`](ColorPicker::set_value) does not notify, and
[`pick`](ColorPicker::pick) calls `on_change` just as when the user picks (the
same rule as [`ComboBox::set_selected`] and [`ComboBox::select`]).

WinUI 3's `ColorPicker` is a **large surface** with a spectrum and sliders stacked
vertically, which differs in shape and layout from the other three platforms'
"press a color swatch to open the picking UI". So, following WinUI 3 practice, it
is put in a `Button`'s `Flyout`, and the button shows a swatch of the chosen color.

macOS's color panel also returns catalog colors (such as `systemBlue`), so they
are converted to sRGB before the components are read.

## Drawing surfaces

**Things the app draws itself**, such as charts, shapes and visualizations, are
drawn on a [`Canvas`]. Write what to draw to the [`Painter`] inside the closure
passed to [`on_draw`](Canvas::on_draw), and call [`redraw`](Canvas::redraw) when
you want it redrawn. When the surface's size changes, naui redraws it.

```no_run
# use naui::{Color, Point, Rect, Result, Sizing, Ui};
# fn build(ui: &Ui) -> Result<()> {
let chart = ui.canvas()?;
chart.set_sizing(Sizing::fill()); // the size is not decided by the content
chart.on_draw(|painter| {
    let bar = Rect::new(10.0, 10.0, 40.0, painter.height() - 20.0);
    painter.fill_rect(bar, Color::rgb(0x33, 0x66, 0xff));
    painter.text("Sales", Point::new(10.0, painter.height() - 16.0), 12.0, Color::BLACK);
});
chart.on_pointer(|event| println!("{:?} at {:?}", event.phase, event.point));
chart.redraw();
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `Canvas` | `Grid` + `Canvas` + `Path` / `TextBlock` | `NSView` + `drawRect:` (`NSBezierPath`) | `GtkDrawingArea` (cairo + Pango) | `<canvas>` + 2D context |

The `Painter` **only records** drawing commands; turning them into pixels is the
job of the platform's 2D API (naui has neither rasterization nor font metrics).
Coordinates are logical pixels with the origin at the top left and right and down
positive, and the platform applies the high-DPI scale. The vocabulary is the
minimum shared by all four platforms — fills and strokes of rectangles, circles,
lines, polylines, polygons and arbitrary [`Path`]s (lines and cubic Béziers), and
single lines of text — while opacity ([`set_opacity`](Painter::set_opacity)),
dashes ([`set_dash`](Painter::set_dash)) and text alignment
([`set_text_align`](Painter::set_text_align)) are held as state that applies to
later commands until changed. Circles and arcs are converted to Béziers by `Path`
before being passed on, so they have the same shape on every platform.

| Command | What it does |
| --- | --- |
| `fill_rect` / `stroke_rect` | Fill and stroke of a rectangle |
| `fill_circle` / `stroke_circle` | Fill and stroke of a circle |
| `line` / `polyline` / `fill_polygon` | Line, polyline, polygon |
| `fill_path` / `stroke_path` | An arbitrary `Path` (lines and cubic Béziers; quadratic Béziers, ellipses and arcs are converted to Béziers) |
| `text` | A single line of text |
| `set_opacity` / `set_dash` / `set_text_align` | State that applies to later commands (opacity, dashes, text alignment) |

The typeface is the platform's standard UI font, and `size` is the text size (in
logical pixels). `at` is the **top** of the text, and horizontally it follows the
alignment. **There is no API to measure text width** (because there are no font
metrics).

`redraw` **does not draw on the spot**. `on_draw` is called at the next draw (the
native drawing pass on macOS / Linux, the next frame on the Web, and the next turn
of the `DispatcherQueue` on Windows). Calling it many times in a row draws once,
and calling it from inside `on_draw` does not re-enter.

[`on_pointer`](Canvas::on_pointer) receives pointer (mouse, touch, pen) presses,
moves and releases, with positions relative to the surface's top left
([`PointerEvent`]). Moves arrive while not pressed (hover) too, and while pressed
they arrive even outside the surface (the pointer is captured). Presses and
releases carry which button as a [`PointerButton`] (primary, secondary, middle,
back, forward; touch and the pen tip are primary). Button numbers that differ by
platform are aligned by meaning. Modifier keys are not distinguished yet.

```no_run
# use naui::{PointerButton, PointerPhase, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
# let chart = ui.canvas()?;
chart.on_pointer(|event| {
    if event.phase == PointerPhase::Down && event.button == Some(PointerButton::Secondary) {
        println!("right click: {:?}", event.point); // e.g. to PopupMenu::open_at
    }
});
# Ok(())
# }
```

The wheel (including two-finger scrolling on a trackpad) arrives at
[`on_wheel`](Canvas::on_wheel) as a [`PointerEvent`] with
[`PointerPhase::Wheel`]. The amounts are [`wheel_x`](PointerEvent::wheel_x) /
[`wheel_y`](PointerEvent::wheel_y), **normalized to logical pixels**. Positive is
the reading direction, right and down (the same as page scrolling), and the
direction is the one after the OS "natural scrolling" setting has been applied.

| Platform | Native amount | Amount naui delivers |
| --- | --- | --- |
| Windows | `MouseWheelDelta` (120 per notch; vertical is positive when rolled away) | 120 converted to [`WHEEL_STEP`], vertical sign inverted |
| macOS | `scrollingDeltaX` / `Y` (pixels for trackpads, lines for wheels; positive in the direction the content moves) | Pixels as is, lines times `WHEEL_STEP`; sign inverted |
| Linux | `GtkEventControllerScroll` (pixels if the unit is `SURFACE`, notches if `WHEEL`) | Pixels as is, notches times `WHEEL_STEP` |
| Web | `WheelEvent` (`deltaMode` is pixels, lines or pages) | Pixels as is, lines times 1/3 of `WHEEL_STEP`, pages times the surface size |

One notch of a notched wheel becomes `WHEEL_STEP` (48). However, when a browser on
the Web passes pixels, the browser's amount is kept, so the amount per notch
differs by browser. When deciding "how much per notch", as for zooming, using the
amount divided by `WHEEL_STEP` as an exponent works smoothly even for fine
amounts.

`on_wheel` returns an [`EventResponse`]. With `Handled` the outer scroll (a
`Scroll` or the page) does not receive it, and with `Continue` the outside scrolls
as usual. A surface without `on_wheel` passes the wheel to the outside.

```no_run
# use naui::{EventResponse, Result, Ui, WHEEL_STEP};
# fn build(ui: &Ui) -> Result<()> {
# let chart = ui.canvas()?;
let mut scale = 1.0_f64;
chart.on_wheel(move |event| {
    scale *= 1.2_f64.powf(-event.wheel_y / WHEEL_STEP); // 1.2x per notch
    EventResponse::Handled // do not scroll the page
});
# Ok(())
# }
```

On the Web, the browser context menu that right-clicking opens is not shown over
the surface (because the other three platforms' drawing surfaces have none
either).

Only on Windows, **the Windows App SDK has no surface for drawing pixels** (Win2D
is a separate package). So naui places XAML's shape elements `Path` and
`TextBlock` on a `Canvas`, and rebuilds the whole surface on every redraw. For
drawings with more than several thousand commands, redrawing takes longer than on
the other three platforms.

## Navigation

Tabs, nav bars, docks, menus, breadcrumbs, pagination and links can be built with
the same API on every platform. Items are passed as a sequence of [`NavItem`]s,
and the chosen one is returned as an index.

```no_run
# use naui::{NavItem, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let navbar = ui.navbar("naui")?;
navbar.set_items(&NavItem::list(["Home", "Search", "Settings"]));
navbar.on_select(|index| println!("item {index} was chosen"));
navbar.set_selected(0); // change the selection without notifying
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `Tabs` | `Grid` + `ToggleButton` | `NSTabView` | `GtkNotebook` | `role="tablist"` |
| `Navbar` | A row of `ToggleButton`s | `NSSegmentedControl` | `GtkLabel` + a row of `GtkToggleButton`s | `<nav>` |
| `Dock` | A row of `ToggleButton`s | `NSSegmentedControl` | A row of `GtkToggleButton`s (equal widths) | `<nav>` |
| `Menu` | A column of `ToggleButton`s | A column of `NSButton`s | A column of `GtkToggleButton`s | `<nav><ul>` |
| `Breadcrumbs` | `HyperlinkButton` + separators | `NSPathControl` | `GtkLabel` links + separators | `<nav><ol><a>` |
| `Pagination` | `Button` + `ToggleButton` | `NSButton` + `NSSegmentedControl` | `GtkButton` + `GtkToggleButton` | `<nav>` |
| `Link` | `HyperlinkButton` | `NSButton` (link color) | `GtkLinkButton` | `<a>` |

`Menu` is **a vertical navigation list**, not a pop-up menu. For the one opened by
right-clicking, use [`PopupMenu`]. Since layout is only vertical and horizontal
stacks, pinning a `Dock` to the bottom edge is the app's responsibility.

## Toolbars

To line up frequently used actions along the top edge of a window, use
[`Toolbar`]. Unlike other widgets, **it is not placed in the layout**. That is
because macOS's `NSToolbar` is something attached to an `NSWindow`, and naui too
attaches it with [`Window::set_toolbar`] (it is not a [`Widget`]).

Unlike navigation, **it has no selected item**; each press runs a command on the
spot. Items are passed as a sequence of [`ToolbarItem`]s, and the pressed one is
returned as an index **into the sequence including separators**.

Items are shown as **icons**. Icon names differ by platform (see
[`ToolbarIcon`]), so naui takes only the kind of action and maps it to the
platform's standard icon. `label` is used for screen readers, tooltips and the
menu shown when not all items fit.

```no_run
# use naui::{Result, ToolbarIcon, ToolbarItem, Ui};
# fn build(ui: &Ui) -> Result<()> {
let window = ui.window("Editor", 800.0, 600.0)?;

let toolbar = ui.toolbar()?;
toolbar.set_items(&[
    ToolbarItem::new(ToolbarIcon::New, "New"),
    ToolbarItem::new(ToolbarIcon::Open, "Open"),
    ToolbarItem::separator(),        // cannot be pressed, so no notification either
    ToolbarItem::new(ToolbarIcon::Save, "Save").enabled(false),
]);
toolbar.on_activate(|index| println!("item {index} was pressed"));
toolbar.set_item_enabled(3, true);   // enable once saving becomes possible
window.set_toolbar(&toolbar);
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `Toolbar` | `CommandBar` + `AppBarButton` | `NSToolbar` + `NSToolbarItem` | `AdwHeaderBar` + `GtkButton` | `<div role="toolbar">` + `<button>` |
| Icons | Segoe Fluent Icons | SF Symbols | Icon theme | SVGs bundled with naui |

On macOS and Linux it is shown **merged with the window's title bar**. Windows's
`CommandBar` and the Web have no corresponding concept, so it is placed below the
title bar (on the Web, at the start of the window element).

On macOS, attaching a toolbar **hides the title text**. Not showing the title in a
window with a toolbar is the macOS convention, and if it stayed visible the title
would occupy the start and push the items to the right edge.
[`Window::set_title`]'s value remains the window's title (it appears in the
Window menu and Mission Control), and [`Window::title`] keeps returning it.
Removing the toolbar with [`Window::clear_toolbar`] brings the display back.

[`Toolbar::len`] is the number of items including separators, and
[`Toolbar::activate`] notifies just as if the user had pressed it (separators,
disabled items and out-of-range indices do nothing). [`Toolbar::set_enabled`]
disables the whole toolbar at once, and per-item settings are kept.

Browsers have no OS icon theme, so only on the Web does naui carry the shapes.
Only the actions listed in [`ToolbarIcon`] are provided, and arbitrary images
cannot be placed. Also, since naui identifies items by index, macOS's "Customize
Toolbar" (rearranging by the user) is turned off, because rearranging would change
the meaning of the indices in notifications.

## Menu bars (the OS application menu)

For menus of headings and items, such as "File", "Edit" and "View", use
[`MenuBar`]. Like [`Toolbar`], it is not placed in the layout but attached with
[`Window::set_menu_bar`] (it is not a [`Widget`]).

One heading is a [`MenuSpec`], and one line inside it is a [`MenuItem`]. An item
can open a submenu of further items ([`MenuItem::submenu`], to any depth) or show
a check mark ([`MenuItem::checked`]). Items are identified by **a path**: the
heading's index followed by the item's index at each level, counting separators.
When an item is pressed, [`MenuBar::on_activate`] is called with its path, which
slice patterns take apart nicely.

```no_run
# use naui::{MenuItem, MenuShortcut, MenuSpec, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let window = ui.window("Editor", 800.0, 600.0)?;

let menu_bar = ui.menu_bar()?;
menu_bar.set_menus(&[
    MenuSpec::new(
        "File",
        [
            MenuItem::new("New").shortcut(MenuShortcut::new('n')),
            MenuItem::new("Open").shortcut(MenuShortcut::new('o')),
            MenuItem::separator(),      // cannot be pressed, so no notification either
            MenuItem::new("Save").shortcut(MenuShortcut::new('s')).enabled(false),
        ],
    ),
    MenuSpec::new(
        "View",
        [
            MenuItem::new("Show Sidebar").checked(true),
            // A sequence of strings can also be passed as is.
            MenuItem::submenu("Zoom", ["Zoom In", "Zoom Out"]),
            MenuItem::new("Full Screen").shortcut(MenuShortcut::function(11)),
        ],
    ),
]);
menu_bar.on_activate({
    let menu_bar = menu_bar.clone();
    move |path| match *path {
        [0, item] => println!("file item {item}"),
        // The check mark has already flipped when the notification arrives.
        [1, 0] => println!("sidebar shown: {}", menu_bar.is_item_checked(path)),
        [1, 1, zoom] => println!("zoom item {zoom}"),
        [1, 2] => println!("full screen"),
        _ => {}
    }
});
menu_bar.set_item_enabled(&[0, 3], true); // enable once saving becomes possible
window.set_menu_bar(&menu_bar);
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `MenuBar` | A row of `Button`s | `NSMenu` (`NSApplication.mainMenu`) | `GtkPopoverMenuBar` + `GMenu` | `<div role="menubar">` |
| Menu | `MenuFlyout` | `NSMenu` | A `GMenu` submenu | `<div role="menu">` |
| Item | `MenuFlyoutItem` | `NSMenuItem` | `GMenuItem` + `GSimpleAction` | `<button role="menuitem">` |
| Item with a check mark | `ToggleMenuFlyoutItem` | `NSMenuItem.state` | A `GSimpleAction` with a boolean state | `<button role="menuitemcheckbox">` |
| Submenu | `MenuFlyoutSubItem` | `NSMenuItem.submenu` | A `GMenu` submenu with `submenu-action` | A nested `<div role="menu">` |

**On macOS there is one menu bar per app**, shown at the top of the screen
(`NSApplication.mainMenu`). [`Window::set_menu_bar`] affects the whole app no
matter which window it is called from. On the other three platforms it is laid
along the top of the window (below the title bar).

### Check marks

Choosing an item made with [`MenuItem::checked`] **flips its check mark before
[`MenuBar::on_activate`] is called**, by the user, by a shortcut or by
[`MenuBar::activate`] alike (WinUI's `ToggleMenuFlyoutItem` and GTK4's boolean
actions flip by themselves, and naui does the same on macOS and the Web). Read the
new state with [`MenuBar::is_item_checked`] inside the notification. To change it
from the app's side (for example when the sidebar is closed some other way), use
[`MenuBar::set_item_checked`], which does not notify.

### Shortcuts

A [`MenuShortcut`] is either **the primary modifier key and one alphanumeric
character** ([`MenuShortcut::new`]; the primary modifier is ⌘ on macOS and Ctrl
elsewhere) or **a function key** from F1 to F24 ([`MenuShortcut::function`]),
which works on its own. ⇧, ⌥ / Alt and the primary modifier are added with
[`shift`](MenuShortcut::shift), [`alt`](MenuShortcut::alt) and
[`primary`](MenuShortcut::primary). Symbol keys and character keys without the
primary modifier are not registered anywhere ([`MenuShortcut::is_valid`]).
Shortcuts work for items inside submenus too, but not while the submenu leading
to them is disabled.

On macOS and Linux, the OS (AppKit / GTK4) receives the key and invokes the item,
and the OS also builds the label at the item's right edge. Windows and the Web have
no such mechanism, so naui watches for the pressed keys and adds the right-edge
label (in the form `Ctrl+S` or `F5`) itself. Some function keys are taken by the OS
or the browser before the app sees them (see
[Known limitations](https://github.com/mokuichi147/naui/blob/main/docs/known-limitations.md)).

### The standard macOS menus

On macOS, ⌘C / ⌘V and so on are delivered as key equivalents of the main menu, so
without a menu **pasting into text input stops working**. So naui always provides
the app-name menu (first) and the "Edit" menu (last), and places the headings the
app passed between them. Removing it with [`Window::clear_menu_bar`] returns to the
default menu consisting only of these two.

### Other details

[`MenuBar::len`] is the number of headings, and [`MenuBar::menu_len`] is the
number of items in one heading including separators. [`MenuBar::activate`]
notifies just as if the user had chosen it (separators, submenu headings, disabled
items and out-of-range paths do nothing). Disabling a submenu with
[`MenuBar::set_item_enabled`] keeps it from opening, and the items inside keep
their own settings for when it is enabled again. [`MenuBar::set_enabled`] disables
the whole menu bar at once, and per-item settings are kept (on macOS, naui's
standard menus are not affected).

Radio items (one choice out of a group) are not provided; use check items and
turn the others off with [`MenuBar::set_item_checked`].

## Sidebars

For a **sidebar spanning the full height of the window**, like the left side of
System Settings or Finder, use [`Sidebar`]. Like [`Toolbar`], it is not placed in
the layout but attached with [`Window::set_sidebar`] (it is not a [`Widget`]).
Once attached, the child of [`Window::set_child`] is placed in the pane to the
right of the sidebar.

Items are [`SidebarItem`]s (text and an icon), grouped with [`SidebarSection`]s.
Groups without a heading are separated only by a gap (a separator line on Windows
and Linux). The chosen item is returned as **a running index across groups**.

```no_run
# use naui::{Result, SidebarItem, SidebarSection, ToolbarIcon, Ui};
# fn build(ui: &Ui) -> Result<()> {
let window = ui.window("Settings", 800.0, 600.0)?;
let body = ui.stack(naui::Orientation::Vertical)?;
window.set_child(&body); // placed to the right of the sidebar

let sidebar = ui.sidebar()?;
sidebar.set_sections(&[
    SidebarSection::untitled([
        SidebarItem::new("General").icon(ToolbarIcon::Settings),
        SidebarItem::new("About").icon(ToolbarIcon::Info),
    ]),
    SidebarSection::new("Locations", ["Documents", "Downloads"]), // with a heading
]);
sidebar.on_select(|index| println!("item {index} was chosen")); // 0 to 3
sidebar.set_selected(0);      // choose without notifying
sidebar.set_width(240.0);     // DEFAULT_SIDEBAR_WIDTH (220) by default
window.set_sidebar(&sidebar);
sidebar.set_collapsed(true);  // collapse (items and selection are kept)
sidebar.on_collapse(|collapsed| println!("the user toggled it: {collapsed}"));
sidebar.on_resize(|width| println!("the user set the width to {width}"));
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `Sidebar` | `NavigationView` (Left) | `NSSplitViewController`'s sidebar item | `GtkPaned` | `<aside>` + `<nav>` inside a `SplitView` |
| Item list | `NavigationViewItem` | `NSTableView` (source list) | `GtkListBox` (`.navigation-sidebar`) | `<ul>` + `<button>` |
| Heading | `NavigationViewItemHeader` | Group row | A row with a dim `GtkLabel` | `<div role="heading">` |

On macOS it **extends under the title bar** (floating glass on macOS 26, a frosted
material before that). So while it is attached, the window gets
`fullSizeContentView`, and the child's top edge is positioned to avoid the title
bar. On Linux the sidebar and the content each have their own header bar (the
GNOME convention). On Windows and the Web it starts below the title bar (and the
menu bar).

### Width and collapsing

**On all four platforms, the user can drag the divider to change the width** (the
minimum is [`SIDEBAR_MIN_WIDTH`]). Doing so calls [`Sidebar::on_resize`] (it is not
called by [`Sidebar::set_width`]). macOS and Linux use the standard dividers
(`NSSplitView` / `GtkPaned`), and Windows and the Web use the same 6 px grab area
as naui's [`SplitView`]. Linux's `AdwOverlaySplitView` cannot change its width, so
it is split with `GtkPaned`.

The user can also collapse and expand it with **the platform's standard sidebar
button**, which calls [`Sidebar::on_collapse`] (it is not called by
[`Sidebar::set_collapsed`]). The button's position is aligned across the four
platforms: **the left of the window's top edge (at the same height as the
toolbar)**. The button does not move when toggling, and the content does not shift
up or down.

**When the window narrows, the sidebar collapses by itself** so the content beside
it keeps at least [`SIDEBAR_COLLAPSE_CONTENT_WIDTH`] (360), and it expands again
when the window widens. These changes also call [`Sidebar::on_collapse`], so a
check mark in a menu stays in step. A sidebar the user collapsed stays collapsed,
and one opened while the window is narrow stays open.
[`Sidebar::set_auto_collapse`]`(false)` turns it off. macOS
(`canCollapseFromWindowResize`) and Windows (`NavigationView`'s
`PaneDisplayMode::Auto`) decide with their own mechanisms, so the details follow
them: on macOS the sidebar first narrows to [`SIDEBAR_MIN_WIDTH`] before
collapsing, and on Windows it becomes the strip of icons, and opening it while
narrow lays it over the content.

| Platform | Toggle button | When collapsed |
| --- | --- | --- |
| macOS | The sidebar button at the start of the toolbar (`NSToolbarToggleSidebarItemIdentifier`). Without a toolbar, a toolbar with just the button is attached | The whole pane is hidden (it also collapses when the divider is dragged left past the minimum) |
| Linux | `sidebar-show-symbolic` at the left end of the header bar | The whole pane is hidden |
| Windows | `NavigationView`'s pane collapse button | A narrow strip of icons only |
| Web | `<button aria-expanded>` at the start of the toolbar row (a row with just the button if there is no toolbar) | The whole pane is hidden |

Icons use the same kinds as [`ToolbarIcon`] and are mapped to the platform's
standard icons. `set_selected` / `clear_selection` do not notify, while `select`
and the user's choices do (the same as other navigation).

libadwaita 1.9 has `AdwSidebar`, which includes the item list, but 1.5, which naui
targets, does not, so Linux builds it with `GtkListBox` as recommended up to 1.5.
Browsers have no sidebar control, so on the Web, as with `Menu`, it is assembled
from standard elements and browser-default buttons.

## Pop-up (context) menus

A [`PopupMenu`] is not laid out on screen, so it is not a [`Widget`]. Items are
passed as a sequence of [`PopupItem`]s, which can open submenus
([`PopupItem::submenu`]) and show check marks ([`PopupItem::checked`]) the same
way as menu bar items. The chosen one is returned as **a path**: the index at each
level, separators included.

```no_run
# use naui::{PopupItem, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let label = ui.label("Right-click here")?;

let popup = ui.popup_menu()?;
popup.set_items(&[
    PopupItem::new("Copy"),
    PopupItem::separator(),          // cannot be chosen, so no notification either
    PopupItem::new("Delete").enabled(false),
    PopupItem::new("Wrap Lines").checked(true),
    PopupItem::submenu("Sort By", ["Name", "Date"]),
]);
popup.on_select(|path| match *path {
    [0] => println!("copy"),
    [3] => println!("wrap lines toggled"),
    [4, by] => println!("sort by {by}"),
    _ => {}
});
popup.attach(&label);                // opens on right-click
popup.open_at(&label, 0.0, 0.0);     // open from code (position from the top left)
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `PopupMenu` | `MenuFlyout` | `NSMenu` | `GtkPopoverMenu` + `GMenu` | `<div role="menu">` |

Choosing a check item flips it before [`PopupMenu::on_select`] is called (read it
with [`PopupMenu::is_item_checked`]). Popup menus show no shortcuts.

Native menus are used on Windows (`MenuFlyout`), macOS (`NSMenu`) and Linux
(`GtkPopoverMenu`), which also own keyboard operation. Browsers have no API to
replace the default context menu, so **the Web is synthesized**: it closes with
Escape or a click outside, and submenus open on click or hover, but there is no
moving with the arrow keys.

`attach` can attach the same menu to any number of widgets. On the Web, the
browser's default menu is suppressed **only over the attached widgets**. `select`
notifies just like a user action (for tests and automation), and ignores
separators, submenu headings and items that cannot be chosen.

## Lists

A [`List`] is a list of rows stacked vertically that scrolls by itself. Rows are
[`ListItem`]s, the way of selecting is [`SelectionMode`], and the selection is
returned as a sequence of indices. Adding [`ListItem::detail`] gives a row
secondary text.

```no_run
# use naui::{ListItem, Result, SelectionMode, Sizing, Ui};
# fn build(ui: &Ui) -> Result<()> {
let list = ui.list()?;
list.set_items(&[
    ListItem::new("Sapporo"),
    ListItem::new("Tokyo").detail("13,960,000 people"), // shown small on a second line
]);
list.set_selection_mode(SelectionMode::Multiple); // Single by default
list.on_select(|indices| println!("{indices:?} were chosen"));
list.set_selection(&[0, 2]); // replace the selection without notifying

// An Auto height follows all rows. Use Fixed / Fill only for lists whose height should be limited.
list.set_sizing(Sizing::fill_width());
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `List` | `ListView` + `ListViewItem` | `NSTableView` (1 column) + `NSScrollView` | `GtkListBox` + `GtkScrolledWindow` | `<select size>` / `<ul role="listbox">` |

To expand and collapse nested items while choosing, use [`Tree`].

The difference from `Menu` is the role: `Menu` is **navigation that switches
screens**, and `List` is **a list for choosing data**. Only `List` has multiple
selection and scrolling.

[`SelectionMode::Multiple`] takes the form "choose while holding ⌘ / Ctrl or
Shift" on every platform (on WinUI it is mapped to `Extended`, not `Multiple`).
With multiple selection the selection can become empty, so the notification takes
the form `on_select(|indices: &[usize]|)`, and an empty sequence is passed too.
`set_selection` / `set_selected` / `clear_selection` do not notify, and `select` /
`select_many` notify just like user actions.

[`ListItem::detail`] becomes a second line on every platform. The row content is
built from naui's `Stack` and [`Label`], and its smaller, dimmer look is decided by
[`TextStyle::Caption`] and [`TextColor::Secondary`], so it looks like the
platform's "secondary text". **On the Web the construction changes with the row
content**: text only gives a `<select size>`, and with `detail` it is a
synthesized `<ul role="listbox">` (because `<option>` can hold only text).

There are two ways to build rows. In both, **rows are identified by index**.

| How rows are built | Type used | Content |
| --- | --- | --- |
| [`List::set_items`] | [`ListItem`] | Text-only rows. Adding `detail` puts secondary text on a second line |
| [`List::set_rows`] | [`ListRow`] | Arbitrary widgets laid out as one row each |

For rows with a leading image, multi-line body text, or a trailing button or
toggle, as in settings screens, build them with the existing `Grid` / `Stack`,
pass them to [`ListRow::new`], and show them with [`List::set_rows`]. When only the
controls inside a row are operated and the whole row should not be selectable,
specify [`ListRow::selectable(false)`](ListRow::selectable). `ListItem` does not
enumerate kinds of accessories, so app-specific row layouts are handled by the
same mechanism.

What happens when the row's label or padding is pressed is received with
[`ListRow::on_activate`]. **It is not called when a button, check box or input
field is pressed directly**, so the row's handling and the inner control's
handling do not both happen (macOS uses `NSTableView`'s action, GTK / WinUI event
routing, and the Web walks up from the pressed element, all to the same split).
Whether a row can be pressed is decided when it is built, so specify it before
passing it to `set_rows`.

When the number or content of rows is decided at run time, bring a cloned `Ui`
into the callback and build the rows there
("[Creating widgets later](#creating-widgets-later)").

```no_run
# use naui::{ListRow, Orientation, Result, Sizing, Ui};
# fn build(ui: &Ui) -> Result<()> {
let content = ui.stack(Orientation::Horizontal)?;
let check = ui.checkbox("")?;
content.append(&check);
content.append(&ui.label("Wi-Fi")?);
content.append(&ui.button("Settings…")?);
content.set_sizing(Sizing::fill_width());

// Do not let the whole row be selected; pressing it only toggles the check.
let row = ListRow::new(&content).selectable(false);
row.on_activate(move || check.set_checked(!check.is_checked()));

let list = ui.list()?;
list.set_rows(&[row]);
# Ok(())
# }
```

### Activating rows and context menus

[`List::on_row_activate`] reports **row activation** (a double-click, or Enter
while the row has focus), and [`List::on_context_menu`] reports **a right-click on
a row**, with that row's index. The right-click position is in the coordinates
used when passing this `List` to [`PopupMenu::open_at`] (logical pixels, y
pointing down), so passing it as is gives a per-row context menu.

```no_run
# use naui::{ListItem, PopupItem, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let list = ui.list()?;
list.set_items(&ListItem::list(["Sapporo", "Tokyo", "Naha"]));
list.on_row_activate(|index| println!("open row {index}"));

let menu = ui.popup_menu()?;
menu.set_items(&[PopupItem::new("Open"), PopupItem::new("Delete")]);
list.on_context_menu({
    let list = list.downgrade();
    move |index, at| {
        let Some(list) = list.upgrade() else { return };
        println!("menu for row {index}");
        menu.open_at(&list, at.x, at.y);
    }
});
# Ok(())
# }
```

| | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| Activation | `DoubleTapped` / `KeyDown` (Enter) | `doubleAction` / `keyDown:` (Return) | The row's `GtkGestureClick` (2 presses) / Enter | `dblclick` / `keydown` (Enter) |
| Context menu | `ContextRequested` | `menuForEvent:` | The row's `GtkGestureClick` (secondary button) | `contextmenu` |

Neither **changes the selection**, so to select the right-clicked row before
acting on it, call `select` inside. To open everything in a multiple selection,
read `selection` inside the activation. They are not called for rows of
`ListItem`s that cannot be chosen (`enabled(false)`) or for empty space with no
row. Double-clicking a button or other control inside a row directly does not
activate it, but right-clicking gives that row's context menu (on the Web, only
over input fields inside the row, the browser's own menu is kept). The index is
looked up from the row's position at the time it was pressed, so it points to the
right row even after the rows have been rebuilt with `set_rows`.

Other ways of opening the menu besides right-clicking follow the platform: on
macOS Control + click, and on Windows a touch long-press and Shift + F10 / the
application key also open it (for key operations there is no position, so the
row's left edge is passed). Once `on_context_menu` is set, the platform's default
menu (including one attached with `PopupMenu::attach`) no longer appears over
rows. On macOS, [`PopupMenu::open_at`] does not return until the menu closes.

### Scrolling to a given row

[`List::scroll_to_row`], [`Table::scroll_to_row`] and [`Tree::scroll_to_item`]
scroll until that row is visible (to jump to a row found by a search, for
example). **The selection does not change**, so to select and show it, combine it
with `set_selected`. If it is already visible nothing moves, and if not, it is
aligned to the nearer edge. A table can scroll to rows it has not built yet even
when it builds only part of its rows. A tree expands the ancestors of an item
inside a collapsed branch before scrolling (the expansion is not notified).

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| List | `ListView.ScrollIntoView` | `scrollRowToVisible:` | `GtkAdjustment` (from the row's position) | `scrollTop` (from the row's position) |
| Table | `ScrollViewer.ChangeView` (from the row height) | `scrollRowToVisible:` | `GtkAdjustment` (from the row height) | `scrollTop` (from the row height) |
| Tree | `TreeViewItem.StartBringIntoView` | `scrollRowToVisible:` | `GtkAdjustment` (from the row's position) | `scrollTop` (from the row's position) |

On GTK, when expanding a collapsed branch of a tree to scroll to it, the scroll
happens in the frame after the row's location has been allocated.

## Trees

A [`Tree`] is a list whose nested items can be expanded and collapsed, and it
scrolls by itself. Items are [`TreeItem`]s, addressed by **a sequence of child
indices from the root (a path)**. `[0, 2]` means "the third child of the first
root", and an empty path means "no selection".

```no_run
# use naui::{Length, Result, Sizing, TreeItem, Ui};
# fn build(ui: &Ui) -> Result<()> {
let tree = ui.tree()?;
tree.set_items(&[
    TreeItem::new("src")
        .expanded(true) // show it expanded from the start
        .children([TreeItem::new("main.rs"), TreeItem::new("lib.rs")]),
    TreeItem::new("docs").child(TreeItem::new("guide.md").detail("12 KB")),
]);
tree.on_select(|path| println!("{path:?} was chosen"));
tree.on_expand(|path, expanded| println!("{path:?} is {expanded}"));
tree.set_selected(&[0, 1]); // choose without notifying (ancestors are expanded)

// A Tree does not decide its own height, so specify one.
tree.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(220.0)));
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `Tree` | `TreeView` | `NSOutlineView` + `NSScrollView` | `GtkListBox` + disclosure buttons | `<ul role="tree">` |

Only one item can be chosen (there is no multiple selection as in `List`).
`set_selected` / `clear_selection` / `set_expanded` / `expand_all` /
`collapse_all` do not notify, and `select` / `expand` / `collapse` notify just
like user actions.

Setting [`TreeItem::enabled`] to `false` **makes all its descendants unselectable
too**. Expansion is remembered per item, so collapsing a parent and expanding it
again brings back the expansion inside as it was (the same as macOS's Finder).

## Tables

A [`Table`] is a table with column headers that scrolls by itself. Columns are
[`TableColumn`]s and rows are [`TableRow`]s, and **rows are identified by index,
as in lists**. Selection notifications also return row indices.

```no_run
# use naui::{Align, Length, Result, Sizing, TableColumn, TableRow, Ui};
# fn build(ui: &Ui) -> Result<()> {
let table = ui.table()?;
table.set_columns(&[
    TableColumn::new("City").sortable(true),                     // sort by pressing the header
    TableColumn::new("Population").width(120.0).align(Align::End), // fixed width, right-aligned
]);
table.set_rows(&TableRow::list([
    ["Tokyo", "13,960,000"],
    ["Osaka", "8,838,000"],
]));
table.on_select(|indices| println!("{indices:?} were chosen"));

// A Table does not decide its own height, so specify one.
table.set_sizing(Sizing::new().width(Length::Fill).height(Length::Fixed(200.0)));
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `Table` | A 2-row `Grid` + `ListView` (rows are `Grid`s) | `NSTableView` + `NSScrollView` | `GtkListBox` + `GtkSizeGroup` | `<table role="grid">` |

Selection behaves as in `List`, and [`SelectionMode`] chooses between single and
multiple. `set_selection` / `set_selected` / `clear_selection` do not notify, and
`select` / `select_many` notify just like user actions. Rows whose
[`TableRow::enabled`] is `false` cannot be chosen.

Only the columns without a [`TableColumn::width`] share the leftover width.
**Only macOS lets the user drag to change column widths** (a feature of
`NSTableView`; the other platforms have no corresponding standard control).

### Placing widgets in cells

[`TableRow`] is a convenience API for text-only tables; to put buttons, check
boxes or icons in cells, use [`Table::set_row_builder`]. Pass the number of rows
and "a function that returns the content of that row", and **only the rows shown
on screen** are built. As with `List`'s [`ListRow`], [`TableCells::selectable`]
and [`TableCells::on_activate`] can be specified per row.

```no_run
# use naui::{Result, TableCells, TableColumn, Ui};
# fn build(ui: &Ui) -> Result<()> {
# let cities: Vec<String> = Vec::new();
let table = ui.table()?;
table.set_columns(&TableColumn::list(["City", "Actions"]));

let ui = ui.clone(); // rows are built inside the callback
table.set_row_builder(cities.len(), move |index| {
    let open = ui.button("Open")?;
    open.on_click(move || println!("open row {index}"));
    // Cells are in the same order as the columns. Text and widgets may be mixed.
    let cells = TableCells::new().text(&cities[index]).cell(&open);
    // A row used only through its buttons (the row itself cannot be selected).
    Ok(cells.selectable(false))
});
# Ok(())
# }
```

The widgets in a row may be recreated every time it is built. Keep per-row state
on the app's data side and build from it inside the function. When the content
changes, rebuild with [`Table::refresh`].

**If the builder function borrows the app's data (`RefCell::borrow`), release the
borrow before calling [`Table::refresh`] or [`Table::set_row_builder`].** They
rebuild the visible rows on the spot, so calling them while holding a
`borrow_mut` panics with a double borrow. Sort notifications tend to take this
shape.

```no_run
# use naui::{Result, TableRow, Ui};
# use std::cell::RefCell;
# use std::rc::Rc;
# fn build(ui: &Ui, data: Rc<RefCell<Vec<TableRow>>>) -> Result<()> {
# let table = ui.table()?;
table.on_sort({
    let table = table.downgrade();
    move |column, _order| {
        let Some(table) = table.upgrade() else { return };
        {
            let mut rows = data.borrow_mut();
            rows.sort_by(|a, b| a.cell(column).cmp(b.cell(column)));
        } // ← release the borrow here first
        table.refresh(); // ← then rebuild
    }
});
# Ok(())
# }
```

[`TableCells::on_activate`] is called only when the row's cells or padding are
pressed. **It is not called when a button, check box or input field is pressed
directly**, so the row's handling and the inner control's handling do not both
happen (the same split as [`ListRow::on_activate`]).

### Tables with many rows

When the number of rows exceeds [`ROW_WINDOW_THRESHOLD`], every platform builds
**only the range shown on screen (and a little before and after it)**. Whether you
pass hundreds of thousands of rows to [`Table::set_rows`] or a large `count` to
[`Table::set_row_builder`], the time it takes to open and the memory used are
determined by the screen size. The scroll bar's length and position stay those of
all rows (spacers above and below take up the height of the rows not shown).

For that, **all rows are assumed to have the same height**. By default it is
measured from a built row, but when buttons or images in cells change the height,
deciding it with [`Table::set_row_height`] keeps the scroll bar's length from
drifting from reality.

On the Web, when the number of rows × the row height exceeds the element height
the browser can handle (about 33 million px in Chromium), the mapping between the
scroll bar position and rows drifts (around 1 million rows for 30 px rows).
Selection by index and [`Table::set_rows`] work even with more than that.

Since the selection is remembered by index, rows outside the screen can be
selected too ([`Table::set_selection`]) and also appear in [`Table::selection`].
In a windowed table, only the selection of built rows reaches the native control,
so **whether to keep off-screen selections is decided by the modifier keys
pressed**. Adding and removing with ⌘ / Ctrl keeps them, and a plain click or a
Shift range selection drops them as a fresh selection (on macOS, `NSTableView`
holds the selection of all rows, so this decision is not needed at all).

For rows built with [`Table::set_row_builder`], naui does not know whether rows it
has not built yet can be chosen. By default they are assumed to be choosable, so if
you use [`TableCells::selectable(false)`](TableCells::selectable) for off-screen
rows, pass the same decision to [`Table::set_row_selectable`] as well.

```no_run
# use naui::{Result, TableCells, Ui};
# fn build(ui: &Ui, locked: std::rc::Rc<Vec<bool>>) -> Result<()> {
# let table = ui.table()?;
let rows = locked.clone();
table.set_row_builder(locked.len(), move |index| {
    Ok(TableCells::new()
        .text(format!("Row {index}"))
        .selectable(!rows[index]))
});
// Let `set_selection` make the same decision for off-screen rows.
table.set_row_selectable(move |index| !locked[index]);
# Ok(())
# }
```

### Sorting from headers

Columns with [`TableColumn::sortable`] set can be sorted by pressing their header.
Each press flips the direction `Ascending` → `Descending` → …, and naui also shows
the indicator (`NSTableView`'s ▲▼ on macOS; elsewhere an arrow on the header and
`aria-sort`).

**naui does not do the sorting itself.** Cells are strings, and only the app knows
whether they are numbers or dates. On receiving the notification, the app sorts its
own data and passes it again with [`Table::set_rows`].

```no_run
# use naui::{Result, SortOrder, TableRow, Ui};
# fn build(ui: &Ui) -> Result<()> {
# let table = ui.table()?;
# let mut rows: Vec<TableRow> = Vec::new();
table.on_sort(move |column, order| {
    rows.sort_by(|a, b| {
        let ordering = a.cell(column).cmp(b.cell(column));
        match order {
            SortOrder::Ascending => ordering,
            SortOrder::Descending => ordering.reverse(),
        }
    });
    // Pass the sorted rows again (the selection is cleared, so reselect if needed).
    // table.set_rows(&rows);
});
# Ok(())
# }
```

For tables that build rows with [`Table::set_row_builder`], the number of rows does
not change, so call [`Table::refresh`] (after sorting the app's data). Sorting
works the same when rows are windowed, and the indicator is on the header, so it
does not disappear when scrolling.

[`Table::set_sort`] moves only the indicator without notifying (use it to show the
default order at startup). [`Table::sort`] reads the current setting.

## Choosing files and folders

A [`FilePicker`] is a single button that, when pressed, opens the platform's
standard file chooser (`NSOpenPanel` on macOS, the Common Item Dialog on Windows,
the browser's on the Web).

```no_run
# use naui::{FileFilter, FilePickerMode, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let picker = ui.file_picker("Choose an image")?;
picker.set_mode(FilePickerMode::File); // File / Files / Folder
picker.set_filters(&[FileFilter::new("Images", ["png", "jpg"])]);
picker.on_select(|entries| {
    for entry in entries {
        println!("{}", entry.name());
    }
});
# Ok(())
# }
```

What is chosen is returned as [`FileEntry`]s. [`FileEntry::path`] is an absolute
path on macOS / Windows, but **on the Web it is always `None` because the browser
does not pass paths**. Also note that on the Web `open()` only works inside a
user-action event, and that folder selection, which comes back as a list of its
contents, is folded into one entry by naui.

## Saving files

A [`FileSaver`] is the matching button that, when pressed, opens the platform's
standard save dialog. **Instead of returning the destination path, it writes out
contents you passed beforehand.** Browsers have no concept of "the destination
path", and an API that returns a path could do nothing on the Web.

```no_run
# use naui::{FileFilter, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let saver = ui.file_saver("Save")?;
saver.set_file_name("memo"); // the extension is filled in from the filter
saver.set_filters(&[FileFilter::new("Text", ["txt"])]);
saver.set_contents("Hello".as_bytes());
saver.on_save(|entry| println!("saved to {}", entry.name()));
saver.on_error(|error| eprintln!("{error}"));
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `FileSaver` | `IFileSaveDialog` | `NSSavePanel` | `GtkFileDialog` (save) | `showSaveFilePicker` / `<a download>` |

The bytes of [`FileSaver::set_contents`] are written as is to the place the user
chose. On success the destination arrives at [`FileSaver::on_save`], and nothing
is called when the user cancels. [`FileSaver::on_error`] is called only when
writing fails. What is written is the content at the time the button is pressed,
so call `set_contents` again whenever the content changes.

The Web differs here too. With `showSaveFilePicker` (Chromium-based browsers), the
OS save dialog appears, but in browsers without it (Firefox / Safari) it becomes an
`<a download>` download, and **the destination is up to the browser; it may land in
the downloads folder without confirmation**. In both cases [`FileEntry::path`] is
`None`, and only the name is returned.

## Receiving dropped files

Every widget and every window can take files dragged in from the OS (Finder,
Explorer, a file manager) with `on_file_drop`. The files arrive as the same
[`FileEntry`]s that [`FilePicker`] returns.

```no_run
# use naui::{Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let window = ui.window("Viewer", 640.0, 480.0)?;
let preview = ui.image("")?;

// The image takes drops onto itself.
preview.on_file_drop({
    let preview = preview.clone();
    move |files| {
        if let Some(source) = files.first().and_then(|file| file.source()) {
            preview.set_source(source);
        }
    }
});
// Anything dropped elsewhere in the window lands here.
window.on_file_drop(move |files| {
    for file in files {
        println!("dropped {}", file.name());
    }
});
# Ok(())
# }
```

**The innermost widget under the pointer that has `on_file_drop` takes the
drop**, and the window's `on_file_drop` gets only the drops no widget takes. While
a drag passes over a place where nothing takes it, the pointer shows that it
cannot be dropped there. Only files are taken; dragged text and links are left to
the widgets that take them natively.

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `on_file_drop` | `AllowDrop` + `DragOver` / `Drop` on the window root | `registerForDraggedTypes:` on the window | `GtkDropTarget` (`GdkFileList`) | `dragover` / `drop` |

What arrives differs as with `FilePicker`:

- **On macOS, Windows and Linux, [`FileEntry::path`] is the absolute path** of
  each file or folder. Items without a local path (the inside of a zip shown by
  Explorer, remote locations) are left out.
- **On the Web, `path` is `None`**, because the browser hands over contents, not
  paths. [`FileEntry::source`] is a `blob:` URL that can be passed to
  `Image::set_source` and the like or read with `fetch`. It **stays valid until the
  next drop onto the same widget (or window)**, which revokes it, or until the
  widget is dropped. A dropped folder comes as one entry with only its name, as
  with the folder mode of `FilePicker`. Once any `on_file_drop` is set, files
  dropped where nothing takes them are refused, instead of the browser opening the
  file in place of the page.
- **On Windows the closure is called a moment after the drop**, once the files have
  been read (`GetStorageItemsAsync` completes asynchronously).
- **Text fields take dropped files as text themselves** on macOS and Linux, and
  `TextBox` handles drags itself on Windows, so drops onto text-input widgets do
  not reach `on_file_drop`. On the Web they do not reach it either, and are
  refused like other drops nothing takes, since the browser would otherwise open
  the file in place of the page.

## Dialogs

A [`Dialog`] shows the platform's standard modal. It has a heading, a message, an
arbitrary widget and buttons with roles (up to three), and the reason it closed is
returned as a [`DialogResponse`].

```no_run
# use naui::{DialogButtons, DialogResponse, Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let dialog = ui.dialog("Save changes?")?;
dialog.set_message("There are unsaved changes.");
dialog.set_child(&ui.checkbox("Don't ask again")?); // an arbitrary widget
dialog.set_buttons(
    DialogButtons::new()
        .primary("Save")
        .secondary("Don't Save")
        .cancel("Cancel"),
);
dialog.on_response(|response| match response {
    DialogResponse::Primary => println!("save"),
    DialogResponse::Secondary => println!("don't save"),
    DialogResponse::Cancel => println!("cancel"), // Esc comes here too
});
dialog.open();
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `Dialog` | `ContentDialog` | `NSAlert` (+ `accessoryView`) | `AdwAlertDialog` | `<dialog>` + `showModal()` |

The button order follows the platform's conventions (the primary action is at the
right end on macOS, and at the left end on WinUI 3). If no buttons are specified,
only "OK" is shown.

**On macOS, [`Dialog::open`] does not return until the dialog is closed**
(because `NSAlert` is app-modal). `on_response` is called inside it. On the Web and
Windows, `open()` returns immediately and the notification arrives later. Closing
with [`Dialog::close`] does not call `on_response` (as with `set_selected`, the
app's own operations are not notified).

## Toasts

A [`Toast`] is a notification that shows a short message at the bottom of the
screen and **disappears by itself**. Unlike a dialog it does not block the user, so
use it for news about something already done, such as "Saved".

```no_run
# use naui::{Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let toast = ui.toast("Saved")?;
toast.set_action("Undo"); // optional; an empty string removes it
toast.set_timeout(3.0); // seconds; with 0 it does not disappear by itself
toast.on_action(|| println!("undo"));
toast.show();
# Ok(())
# }
```

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| `Toast` | An `InfoBar` overlaid on the content | An `NSVisualEffectView` overlaid on the content | `AdwToast` + `AdwToastOverlay` | `<div role="status">` |

**Only Linux has a native toast** (`AdwToast`). Windows's `InfoBar` /
`TeachingTip` cannot be used because naui does not project them, and macOS and
browsers have nothing corresponding at all, so on the remaining three platforms
naui builds the same shape. Notifications sent to the OS notification center are a
separate mechanism that leaves the app, so they are not covered.

**Only one is shown at a time**, and a newly shown one replaces the previous one
(naui also aligns `AdwToastOverlay`'s queueing to this behavior). Dismissing with
[`Toast::dismiss`] and being replaced do not call `on_dismiss` (the same rule as
[`Dialog::close`]: the app's own operations are not notified). Only on Linux is the
time granularity **seconds**, so values under one second become one second.

## Other threads and async

Widget handles hold an `Rc`, so **they cannot be sent to other threads** (the same
goes for [`Ui`]). When you want to do heavy work on another thread and update the
screen, **put the receiving closure on the UI thread side and pass only the sending
side to the thread**. The entry point is [`Ui::tasks`], and the returned [`Tasks`]
can be cloned and brought into callbacks.

```no_run
# use naui::{Result, Ui};
# fn heavy_work() -> String { String::new() }
# fn build(ui: &Ui) -> Result<()> {
let label = ui.label("Waiting")?;
let sender = ui.tasks().channel({
    let label = label.clone();
    move |text: String| label.set_text(&text) // called on the UI thread
});
std::thread::spawn(move || {
    let _ = sender.send(heavy_work());        // from another thread
});
# Ok(())
# }
```

[`Sender`] is `Send + Sync + Clone`, so multiple threads can send to the same
receiver. Values sent to the same channel arrive **in the order they were sent**
(with several channels, the order across channels is not defined).
[`Sender::send`] does not call the closure on the spot; the value arrives once the
UI thread is free. The returned `Ok` means "the value was accepted and its
delivery to the UI was scheduled", not a guarantee that it was delivered. After
naui's UI runtime has ended it returns `Err` (browsers have no app termination,
so there it is always `Ok`).

To start asynchronous work from inside a handler, run a future with
[`Tasks::spawn`]. **The future does not need to be `Send`**, so widget handles can
be brought in as is. Bring in a cloned `Ui` and you can create widgets once the
wait is over.

```no_run
# use naui::{Result, Ui};
# async fn fetch() -> String { String::new() }
# fn build(ui: &Ui) -> Result<()> {
let label = ui.label("Waiting")?;
let button = ui.button("Load")?;
button.on_click({
    let tasks = ui.tasks();
    let label = label.clone();
    move || {
        let label = label.clone();
        tasks.spawn(async move { label.set_text(&fetch().await); });
    }
});
# Ok(())
# }
```

**naui does not bring in an async runtime.** [`Tasks::spawn`] is a small executor
that only advances futures on the UI thread's event loop, and tokio or the like is
not needed. Conversely, **blocking work inside a future freezes the screen**. Move
heavy work to `std::thread::spawn` and [`Tasks::channel`].

Also, **[`Tasks::spawn`] is not a substitute for a general-purpose runtime.** What
naui takes on is "poll the future on the UI thread, and poll it again via its
`Waker`"; futures that need a specific runtime's executor or reactor (such as
tokio's I/O) are not guaranteed to work. **Run those on that runtime and send only
the result back to the screen with a [`Sender`]**.

```ignore
let sender = ui.tasks().channel(move |result: String| label.set_text(&result));
tokio::spawn(async move {
    let result = tokio_specific_operation().await;
    let _ = sender.send(result);
});
```

Dropping a [`Task`] does not stop the task. To abort the previous work on each
press, keep the handle and call [`Task::cancel`] next time.

```no_run
# use naui::{Result, Task, Ui};
# use std::cell::RefCell;
# use std::rc::Rc;
# fn build(ui: &Ui) -> Result<()> {
let running: Rc<RefCell<Option<Task>>> = Rc::new(RefCell::new(None));
let button = ui.button("Retry")?;
button.on_click({
    let tasks = ui.tasks();
    let running = running.clone();
    move || {
        if let Some(task) = running.borrow_mut().take() {
            task.cancel();
        }
        *running.borrow_mut() = Some(tasks.spawn(async {}));
    }
});
# Ok(())
# }
```

Where work is posted differs by platform, but it is the same in that it is
**always deferred**.

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| Posted to | `DispatcherQueue.TryEnqueue` | main queue | `g_idle_add` | microtask queue |

**The Web (wasm) has no threads.** `std::thread::spawn` cannot be used, so put the
sending side inside a future run with [`Tasks::spawn`]. A Web Worker is a separate
JavaScript runtime, so a [`Sender`] cannot be passed to it (bridge with
`postMessage` and call `send` on the receiving side). The [`Tasks`] API itself is
the same on all four platforms, so no `cfg` branching is needed, but code that
includes `std::thread::spawn` does not run on the Web as is.

A panic inside a closure or future only stops that piece on the spot; the app does
not unwind (unwinding across the WinRT / GLib / libdispatch boundaries is not
possible, so all four platforms behave this way). The panic message itself goes to
standard error as usual.

### Timers

To call something after a delay, use [`Tasks::after`]; to call it at a fixed
interval, [`Tasks::every`]; and to wait inside a future, [`Tasks::sleep`]. All of
them are called on the UI thread, so widgets may be touched inside.

```no_run
# use std::cell::RefCell;
# use std::rc::Rc;
# use std::time::Duration;
# use naui::{Result, Timer, Ui};
# fn build(ui: &Ui) -> Result<()> {
// Search 300 ms after typing stops (debouncing).
let search = ui.search_input()?;
let pending: Rc<RefCell<Option<Timer>>> = Rc::default();
search.on_change({
    let tasks = ui.tasks();
    move |text: &str| {
        if let Some(timer) = pending.borrow_mut().take() {
            timer.cancel();
        }
        let text = text.to_string();
        *pending.borrow_mut() =
            Some(tasks.after(Duration::from_millis(300), move || println!("search: {text}")));
    }
});
# Ok(())
# }
```

Like [`Task`], a [`Timer`] **does not stop when dropped**. To stop it, call
[`Timer::cancel`] (calling it from inside an `every` callback is fine).
The next call of `every` is counted from the end of the previous call.

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| Waiting mechanism | `DispatcherQueueTimer` | `dispatch_after` (main queue) | `g_timeout_add` | `setTimeout` |

## Clipboard and URLs

[`Ui::clipboard`] reads and writes text, and [`Ui::open_url`] opens a URL in the
default app (a browser or mail client).

```no_run
# use naui::{Result, Ui};
# fn build(ui: &Ui) -> Result<()> {
let copy = ui.button("Copy")?;
let status = ui.label("")?;
copy.on_click({
    let clipboard = ui.clipboard();
    let status = status.clone();
    move || {
        let status = status.clone();
        clipboard.set_text("Text to share", move |result| match result {
            Ok(()) => status.set_text("Copied"),
            Err(error) => status.set_text(&format!("Could not copy: {error}")),
        });
    }
});
ui.clipboard().read_text(|text| println!("paste: {text:?}"));
# Ok(())
# }
```

Reading is asynchronous on some platforms, and writing is asynchronous in browsers,
so **on all four platforms the results of reading and writing are returned through a
callback** (not called on the spot). If there is no text to read, it is `None`. If
writing is refused, it is `Err`.

| naui | Windows | macOS | Linux | Web |
| --- | --- | --- | --- | --- |
| Clipboard | `Windows.ApplicationModel.DataTransfer.Clipboard` | `NSPasteboard` | `GdkClipboard` | `navigator.clipboard` |
| `open_url` | `ShellExecuteW` | `NSWorkspace` | `GtkUriLauncher` | `window.open` (new tab) |

The Web's Clipboard API is available only over https (or localhost), and reading
may ask the user for permission. Browsers may allow writing and `open_url` only
inside a user action (such as a click), so call them from a button's `on_click` and
the like. When refused, writing gets an `Err` in its callback, and `open_url` (when
the new tab was blocked) gets an `Err` as its return value.
