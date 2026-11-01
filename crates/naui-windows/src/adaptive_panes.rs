//! A list and its detail, side by side or one at a time (WinUI 3's `TwoPaneView`).
//!
//! The Windows App SDK has the control for this, and it decides by itself: while
//! it is wider than `MinWideModeWidth` it shows `Pane1` and `Pane2` side by side
//! (`Wide`), and narrower than that only the pane `PanePriority` names
//! (`SinglePane`). naui sets:
//!
//! - `MinWideModeWidth` to the breakpoint. `TwoPaneView` compares with "wider
//!   than" and has no slack, unlike the other platforms.
//! - `TallModeConfiguration` to `SinglePane`, so a narrow but tall view never stacks
//!   the panes one above the other (`Tall`), which naui does not offer.
//! - `Pane1Length` to the list's width in pixels, and `Pane2Length` to `1*`.
//! - `PanePriority` to the pane chosen with `show`.
//!
//! `TwoPaneView` draws no back button, so a `Button` with the `Back` symbol (the
//! glyph of Fluent's back buttons) sits above the detail while only the detail is
//! shown, following `ModeChanged`. It draws no line between the panes either, and
//! naui adds none.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{ui_strings, Pane, PanesArrangement, Result, DEFAULT_LIST_PANE_WIDTH};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, Grid as XamlGrid, RowDefinition, TextBlock, TwoPaneView, TwoPaneViewMode,
    TwoPaneViewPriority, TwoPaneViewTallModeConfiguration, TwoPaneViewWideModeConfiguration,
};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, GridLength, GridUnitType, HorizontalAlignment, RoutedEventHandler, UIElement,
    VerticalAlignment, Visibility,
};
use windows::Foundation::TypedEventHandler;
use windows_core::{IInspectable, Interface, HSTRING};

use crate::list::ActivationHandler;
use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

/// The back button, with `{label}` standing for its text.
const BACK_XAML: &str = r##"<Button
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Margin="8"
    Visibility="Collapsed">
    <StackPanel Orientation="Horizontal" Spacing="8">
        <SymbolIcon Symbol="Back"/>
        <TextBlock Text="{label}"/>
    </StackPanel>
</Button>"##;

struct AdaptivePanesInner {
    native: TwoPaneView,
    list_pane: XamlGrid,
    detail_pane: XamlGrid,
    back: XamlButton,
    /// Holds the panes' handles, keeping them alive along with their callbacks.
    list: RefCell<Option<Box<dyn Widget>>>,
    detail: RefCell<Option<Box<dyn Widget>>>,
    list_width: Cell<f64>,
    /// The breakpoint and the pane shown while only one is. The width is
    /// `TwoPaneView`'s to decide, so only those two are used.
    arrangement: Cell<PanesArrangement>,
    /// Whether `TwoPaneView` shows one pane, as of its last `ModeChanged`. Both are
    /// shown until its template is applied.
    collapsed: Cell<bool>,
    on_back: ActivationHandler,
}

/// A list and its detail, side by side when wide and one at a time when narrow.
#[derive(Clone)]
pub struct AdaptivePanes(Rc<AdaptivePanesInner>);
impl_widget!(AdaptivePanes, native);

impl AdaptivePanes {
    pub(crate) fn new() -> Result<Self> {
        let native = TwoPaneView::new().map_err(|e| to_error("create the TwoPaneView", e))?;
        let arrangement = PanesArrangement::default();
        native
            .SetMinWideModeWidth(arrangement.breakpoint())
            .and_then(|()| {
                native.SetWideModeConfiguration(TwoPaneViewWideModeConfiguration::LeftRight)
            })
            .and_then(|()| {
                native.SetTallModeConfiguration(TwoPaneViewTallModeConfiguration::SinglePane)
            })
            .and_then(|()| native.SetPane1Length(pixels(DEFAULT_LIST_PANE_WIDTH)))
            .and_then(|()| {
                native.SetPane2Length(GridLength {
                    Value: 1.0,
                    GridUnitType: GridUnitType::Star,
                })
            })
            .and_then(|()| native.SetPanePriority(TwoPaneViewPriority::Pane1))
            .map_err(|e| to_error("set up the TwoPaneView", e))?;

        let list_pane = pane()?;
        let detail_pane = pane()?;
        let back = back_button()?;

        // 詳細の側は 2 行: 戻るボタン (Auto) と詳細 (残り)。
        let detail_side = pane()?;
        for length in [
            GridLength {
                Value: 0.0,
                GridUnitType: GridUnitType::Auto,
            },
            GridLength {
                Value: 1.0,
                GridUnitType: GridUnitType::Star,
            },
        ] {
            let row =
                RowDefinition::new().map_err(|e| to_error("create the TwoPaneView row", e))?;
            row.SetHeight(length)
                .and_then(|()| detail_side.RowDefinitions()?.Append(&row))
                .map_err(|e| to_error("add the TwoPaneView row", e))?;
        }
        let detail_element = detail_pane
            .cast::<FrameworkElement>()
            .map_err(|e| to_error("convert the TwoPaneView pane", e))?;
        XamlGrid::SetRow(&detail_element, 1).map_err(|e| to_error("place the detail", e))?;
        for part in [back.cast::<UIElement>(), detail_pane.cast::<UIElement>()] {
            let part = part.map_err(|e| to_error("convert the TwoPaneView pane", e))?;
            detail_side
                .Children()
                .and_then(|children| children.Append(&part))
                .map_err(|e| to_error("add to the TwoPaneView", e))?;
        }
        native
            .SetPane1(&list_pane)
            .and_then(|()| native.SetPane2(&detail_side))
            .map_err(|e| to_error("set the TwoPaneView panes", e))?;

        let this = Self(Rc::new(AdaptivePanesInner {
            native,
            list_pane,
            detail_pane,
            back,
            list: RefCell::new(None),
            detail: RefCell::new(None),
            list_width: Cell::new(DEFAULT_LIST_PANE_WIDTH),
            arrangement: Cell::new(arrangement),
            collapsed: Cell::new(false),
            on_back: ActivationHandler::new(),
        }));

        let target = UiThreadCell::new(Rc::downgrade(&this.0));
        let clicked = RoutedEventHandler::new(move |_, _| {
            if let Some(inner) = target.try_with_mut(|weak| weak.upgrade()).flatten() {
                let panes = AdaptivePanes(inner);
                panes.go_back();
                // ボタンごと隠れるので、フォーカスを一覧へ移す。
                let list = panes.0.list.borrow().as_ref().map(|l| l.native_element());
                if let Some(list) = list {
                    crate::interaction::request_focus(&list);
                }
            }
            Ok(())
        });
        this.0
            .back
            .Click(&clicked)
            .map_err(|e| to_error("subscribe to the back button", e))?;

        let target = UiThreadCell::new(Rc::downgrade(&this.0));
        let changed = TypedEventHandler::<TwoPaneView, IInspectable>::new(move |_, _| {
            if let Some(inner) = target.try_with_mut(|weak| weak.upgrade()).flatten() {
                inner.mode_changed();
            }
            Ok(())
        });
        this.0
            .native
            .ModeChanged(&changed)
            .map_err(|e| to_error("subscribe to TwoPaneView mode changes", e))?;
        Ok(this)
    }

    /// The list pane. Each call replaces it.
    pub fn set_list(&self, child: &dyn Widget) {
        put(&self.0.list_pane, &self.0.list, child);
    }

    /// The detail pane. Each call replaces it.
    pub fn set_detail(&self, child: &dyn Widget) {
        put(&self.0.detail_pane, &self.0.detail, child);
    }

    /// The narrowest width at which both panes are shown (`MinWideModeWidth`).
    pub fn set_breakpoint(&self, width: f64) {
        let mut arrangement = self.0.arrangement.get();
        arrangement.set_breakpoint(width);
        self.0.arrangement.set(arrangement);
        let _ = self.0.native.SetMinWideModeWidth(width);
    }

    pub fn breakpoint(&self) -> f64 {
        self.0.arrangement.get().breakpoint()
    }

    /// The list pane's width while both panes are shown (`Pane1Length`).
    pub fn set_list_width(&self, width: f64) {
        let width = width.max(0.0);
        self.0.list_width.set(width);
        let _ = self.0.native.SetPane1Length(pixels(width));
    }

    pub fn list_width(&self) -> f64 {
        self.0.list_width.get()
    }

    /// Whether only one pane is shown.
    pub fn is_collapsed(&self) -> bool {
        self.0.collapsed.get()
    }

    /// The pane shown while only one is.
    pub fn visible_pane(&self) -> Pane {
        self.0.arrangement.get().visible()
    }

    /// Chooses the pane shown while only one is (`PanePriority`). `on_back` is not
    /// called.
    pub fn show(&self, pane: Pane) {
        let mut arrangement = self.0.arrangement.get();
        if !arrangement.show(pane) {
            return;
        }
        self.0.arrangement.set(arrangement);
        let _ = self.0.native.SetPanePriority(match pane {
            Pane::List => TwoPaneViewPriority::Pane1,
            Pane::Detail => TwoPaneViewPriority::Pane2,
        });
        self.0.update_back();
    }

    /// Goes back to the list as if the user had pressed the back button. Calls
    /// `on_back`, even when the list is already shown.
    pub fn go_back(&self) {
        self.show(Pane::List);
        self.0.on_back.emit();
    }

    /// Called when the user goes back from the detail to the list.
    pub fn on_back(&self, f: impl FnMut() + 'static) {
        self.0.on_back.set(f);
    }

    /// The corresponding `TwoPaneView`. Public as a backend-specific escape hatch.
    pub fn native_two_pane_view(&self) -> TwoPaneView {
        self.0.native.clone()
    }

    /// The back button. Public as a backend-specific escape hatch.
    pub fn native_back_button(&self) -> XamlButton {
        self.0.back.clone()
    }
}

impl AdaptivePanesInner {
    /// `TwoPaneView` switched between one pane and two.
    fn mode_changed(&self) {
        let collapsed = self
            .native
            .Mode()
            .is_ok_and(|mode| mode == TwoPaneViewMode::SinglePane);
        self.collapsed.set(collapsed);
        self.update_back();
    }

    /// Shows the back button while only the detail is shown.
    fn update_back(&self) {
        let shown = self.collapsed.get() && self.arrangement.get().visible() == Pane::Detail;
        let _ = self.back.SetVisibility(if shown {
            Visibility::Visible
        } else {
            Visibility::Collapsed
        });
    }
}

/// Puts `child` into a pane, replacing what was there.
fn put(pane: &XamlGrid, slot: &RefCell<Option<Box<dyn Widget>>>, child: &dyn Widget) {
    if let Ok(children) = pane.Children() {
        let _ = children.Clear();
    }
    let element = child.native_element();
    let appended = pane
        .Children()
        .and_then(|children| children.Append(&element));
    if appended.is_ok() {
        *slot.borrow_mut() = Some(child.boxed_clone());
    }
}

/// The container for one pane. Its content is placed to fill the allotted space.
fn pane() -> Result<XamlGrid> {
    let pane = XamlGrid::new().map_err(|e| to_error("create the TwoPaneView pane", e))?;
    let element = pane
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("convert the TwoPaneView pane", e))?;
    let _ = element.SetHorizontalAlignment(HorizontalAlignment::Stretch);
    let _ = element.SetVerticalAlignment(VerticalAlignment::Stretch);
    Ok(pane)
}

/// The back button. Where the XAML cannot be loaded, it falls back to a button with
/// only the text.
fn back_button() -> Result<XamlButton> {
    let label = ui_strings().back;
    let xaml = BACK_XAML.replace("{label}", &escape_xml(label));
    let button = match XamlReader::Load(&HSTRING::from(xaml))
        .and_then(|element| element.cast::<XamlButton>())
    {
        Ok(button) => button,
        Err(error) => {
            eprintln!("naui-windows: failed to create the back button: {error}");
            let button = XamlButton::new().map_err(|e| to_error("create the back button", e))?;
            let text = TextBlock::new().map_err(|e| to_error("create the back button", e))?;
            text.SetText(&HSTRING::from(label))
                .and_then(|()| button.SetContent(&text))
                .and_then(|()| button.SetVisibility(Visibility::Collapsed))
                .map_err(|e| to_error("set up the back button", e))?;
            button
        }
    };
    let element = button
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("convert the back button", e))?;
    let _ = element.SetHorizontalAlignment(HorizontalAlignment::Left);
    Ok(button)
}

fn pixels(value: f64) -> GridLength {
    GridLength {
        Value: value,
        GridUnitType: GridUnitType::Pixel,
    }
}

/// Escapes text for an XML attribute value.
fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
