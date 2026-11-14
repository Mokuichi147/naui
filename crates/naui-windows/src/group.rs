//! Dividing and grouping controls (`Separator` and `Group`).
//!
//! WinUI 3 has neither a standalone separator nor a group box: `MenuFlyoutSeparator`
//! and `AppBarSeparator` work only inside menus and command bars. So both are built
//! from the pieces Fluent itself uses for them:
//!
//! - `Separator` is a `Border` that draws only one edge, in the theme's divider
//!   color (`DividerStrokeColorDefaultBrush`). The edge is part of the `Border`'s
//!   desired size, so the line stays 1 px thick whatever size is specified along it.
//! - `Group` is a heading (`BodyStrongTextBlockStyle`) above a card (a `Border` with
//!   `CardBackgroundFillColorDefaultBrush`, `CardStrokeColorDefaultBrush` and
//!   `OverlayCornerRadius`), the way the Settings app groups its controls.
//!   Assistive technology sees it as a group named by its heading
//!   (`AutomationControlType` and `LabeledBy`), so the heading gives the controls
//!   inside their context.
//!
//! The brushes are `{ThemeResource}`s, so they follow the window's theme.

use std::cell::RefCell;
use std::rc::Rc;

use naui_core::{Orientation, Padding, Result, Sizing, TextColor, TextStyle};
use naui_winui3::Microsoft::UI::Xaml::Automation::AutomationProperties;
use naui_winui3::Microsoft::UI::Xaml::Controls::{Border, Grid as XamlGrid, TextBlock};
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{
    DependencyObject, FrameworkElement, TextTrimming, TextWrapping, Thickness, UIElement,
    Visibility,
};
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::widgets::{impl_widget, Widget};

/// The horizontal line. `xmlns` is required for `XamlReader`.
const SEPARATOR_HORIZONTAL_XAML: &str = r##"<Border
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    BorderBrush="{ThemeResource DividerStrokeColorDefaultBrush}"
    BorderThickness="0,1,0,0"/>"##;

/// The vertical line.
const SEPARATOR_VERTICAL_XAML: &str = r##"<Border
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    BorderBrush="{ThemeResource DividerStrokeColorDefaultBrush}"
    BorderThickness="1,0,0,0"/>"##;

/// The card holding a group's content.
const CARD_XAML: &str = r##"<Border
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="{ThemeResource CardBackgroundFillColorDefaultBrush}"
    BorderBrush="{ThemeResource CardStrokeColorDefaultBrush}"
    BorderThickness="1"
    CornerRadius="{ThemeResource OverlayCornerRadius}"/>"##;

/// The group itself: the heading's row (`Auto`) above the card's (the rest).
///
/// A `Grid` does not appear to assistive technology on its own, so it declares
/// itself a group (`AutomationControlType`, which the projection does not carry, so
/// it is set here).
const GROUP_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    AutomationProperties.AutomationControlType="Group">
    <Grid.RowDefinitions>
        <RowDefinition Height="Auto"/>
        <RowDefinition Height="*"/>
    </Grid.RowDefinitions>
</Grid>"##;

/// The gap between a group's heading and its card.
const HEADING_GAP: f64 = 8.0;

/// Loads a `Border` from XAML, falling back to a plain one if the theme resources
/// cannot be found (the line or card is then invisible, but the layout holds).
fn load_border(xaml: &str, what: &str) -> Result<Border> {
    match XamlReader::Load(&HSTRING::from(xaml)).and_then(|element| element.cast::<Border>()) {
        Ok(border) => Ok(border),
        Err(error) => {
            eprintln!("naui-windows: failed to create the {what}: {error}");
            Border::new().map_err(|e| to_error("create the Border", e))
        }
    }
}

fn thickness(padding: Padding) -> Thickness {
    Thickness {
        Left: padding.left,
        Top: padding.top,
        Right: padding.right,
        Bottom: padding.bottom,
    }
}

// -------------------------------------------------------------- Separator

struct SeparatorInner {
    native: Border,
}

/// A thin line that divides controls (a `Border` drawing one edge).
#[derive(Clone)]
pub struct Separator(Rc<SeparatorInner>);
impl_widget!(Separator, native);

impl Separator {
    pub(crate) fn new(orientation: Orientation) -> Result<Self> {
        let native = load_border(
            if orientation.is_vertical() {
                SEPARATOR_VERTICAL_XAML
            } else {
                SEPARATOR_HORIZONTAL_XAML
            },
            "separator",
        )?;
        let this = Self(Rc::new(SeparatorInner { native }));
        // 線は引いた向きいっぱいに伸ばす。
        this.set_sizing(if orientation.is_vertical() {
            Sizing::fill_height()
        } else {
            Sizing::fill_width()
        });
        Ok(this)
    }
}

// ------------------------------------------------------------------ Group

struct GroupInner {
    /// Two rows: the heading (`Auto`) and the card (the rest).
    native: XamlGrid,
    heading: TextBlock,
    card: Border,
    /// Holds the content's handle, keeping it alive along with its callbacks.
    child: RefCell<Option<Box<dyn Widget>>>,
}

/// A frame with a heading around related controls (a heading above a Fluent card).
#[derive(Clone)]
pub struct Group(Rc<GroupInner>);
impl_widget!(Group, native);

impl Group {
    pub(crate) fn new(text: &str) -> Result<Self> {
        let native = XamlReader::Load(&HSTRING::from(GROUP_XAML))
            .and_then(|element| element.cast::<XamlGrid>())
            .map_err(|e| to_error("create the Group", e))?;

        let heading = TextBlock::new().map_err(|e| to_error("create the Group heading", e))?;
        if let Some(style) = crate::widgets::label_style(TextStyle::Heading, TextColor::Default) {
            let _ = heading.SetStyle(&style);
        }
        let _ = heading.SetTextWrapping(TextWrapping::NoWrap);
        let _ = heading.SetTextTrimming(TextTrimming::CharacterEllipsis);
        let _ = heading.SetMargin(Thickness {
            Bottom: HEADING_GAP,
            ..Thickness::default()
        });

        let card = load_border(CARD_XAML, "group card")?;
        let _ = card.SetPadding(thickness(naui_core::DEFAULT_GROUP_PADDING));
        let card_element = card
            .cast::<FrameworkElement>()
            .map_err(|e| to_error("convert the Group card", e))?;
        XamlGrid::SetRow(&card_element, 1).map_err(|e| to_error("place the Group card", e))?;

        for part in [heading.cast::<UIElement>(), card.cast::<UIElement>()] {
            let part = part.map_err(|e| to_error("convert the Group part", e))?;
            native
                .Children()
                .and_then(|children| children.Append(&part))
                .map_err(|e| to_error("add to the Group", e))?;
        }
        // 見出しをグループの名前にする。`AutomationProperties.Name` は利用者の
        // `set_accessible_label` のために空けておき、それが無いときに読まれる
        // `LabeledBy` で結ぶ。
        let _ = native
            .cast::<DependencyObject>()
            .and_then(|object| AutomationProperties::SetLabeledBy(&object, &heading));

        let this = Self(Rc::new(GroupInner {
            native,
            heading,
            card,
            child: RefCell::new(None),
        }));
        this.set_text(text);
        Ok(this)
    }

    /// The heading text. Empty when there is none.
    pub fn text(&self) -> String {
        self.0
            .heading
            .Text()
            .map(|text| text.to_string())
            .unwrap_or_default()
    }

    /// The heading text. An empty string removes the heading, leaving only the card.
    pub fn set_text(&self, text: &str) {
        let _ = self.0.heading.SetText(&HSTRING::from(text));
        let _ = self.0.heading.SetVisibility(if text.is_empty() {
            Visibility::Collapsed
        } else {
            Visibility::Visible
        });
    }

    /// The space between the card's edge and the content. The default is
    /// [`DEFAULT_GROUP_PADDING`](naui_core::DEFAULT_GROUP_PADDING).
    ///
    /// `left` and `right` are the physical sides, also in right-to-left text.
    pub fn set_padding(&self, padding: Padding) {
        let _ = self.0.card.SetPadding(thickness(padding));
    }

    /// The content inside the card. Each call replaces it.
    ///
    /// The content fills the inside of the card. A child still in another container
    /// is refused, and the current content stays.
    pub fn set_child(&self, child: &dyn Widget) {
        let element = child.native_element();
        let previous = self
            .0
            .child
            .borrow()
            .as_ref()
            .map(|child| child.native_element());
        if crate::layout::refuses(&element, previous.as_ref()) {
            return;
        }
        if self.0.card.SetChild(&element).is_ok() {
            if let Some(previous) = previous {
                if crate::layout::element_key(&previous) != crate::layout::element_key(&element) {
                    crate::layout::clear_parent_layout(&previous);
                }
            }
            crate::layout::note_place(&element, crate::layout::Place::border(&self.0.card));
            crate::layout::set_frame_parent(&element);
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// The card holding the content. Public as a backend-specific escape hatch.
    pub fn native_card(&self) -> Border {
        self.0.card.clone()
    }

    /// The heading. Public as a backend-specific escape hatch.
    pub fn native_heading(&self) -> TextBlock {
        self.0.heading.clone()
    }
}
