//! # miui-widgets
//!
//! miui の標準ウィジェット。すべてテーマのトークンだけを見て描画するため、
//! 同じツリーが Windows では Fluent 2、macOS では macOS 風、
//! GNOME では Adwaita 風に見える。

#![forbid(unsafe_code)]

pub mod button;
pub mod layout;
pub mod scroll;
pub mod slider;
pub mod style;
pub mod text;
pub mod text_input;
pub mod toggle;

pub use button::{Button, ButtonVariant, IconButton, IconGlyph};
pub use layout::{column, row, Axis, Container, Divider, Flex, SizedBox, Spacer};
pub use scroll::{Scroll, ScrollState};
pub use slider::{ProgressBar, Slider};
pub use text::{Text, TextRole};
pub use text_input::{TextInput, TextInputState};
pub use toggle::{Checkbox, Radio, Switch};
