//! Values shared by `Separator` and `Group`.

use crate::Padding;

/// The space between a `Group`'s frame and its content, in logical pixels.
///
/// The same on every platform, so that a group looks equally roomy everywhere.
/// Change it with `Group::set_padding`.
pub const DEFAULT_GROUP_PADDING: Padding = Padding::all(12.0);
