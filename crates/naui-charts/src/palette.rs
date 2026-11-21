//! The colors series get when none is set.

use naui::Color;

/// How many colors the palette has before it starts over.
pub const PALETTE_LEN: usize = 8;

/// The series colors on a light surface, in the order series take them.
const LIGHT: [Color; PALETTE_LEN] = [
    Color::rgb(0x2a, 0x78, 0xd6), // 青
    Color::rgb(0xeb, 0x68, 0x34), // 橙
    Color::rgb(0x1b, 0xaf, 0x7a), // 青緑
    Color::rgb(0xed, 0xa1, 0x00), // 黄
    Color::rgb(0xe8, 0x7b, 0xa4), // 赤紫
    Color::rgb(0x00, 0x83, 0x00), // 緑
    Color::rgb(0x62, 0x50, 0xd6), // 紫
    Color::rgb(0xe3, 0x49, 0x48), // 赤
];

/// The same hues stepped for a dark surface (not the light ones inverted).
const DARK: [Color; PALETTE_LEN] = [
    Color::rgb(0x39, 0x87, 0xe5),
    Color::rgb(0xd9, 0x59, 0x26),
    Color::rgb(0x19, 0x9e, 0x70),
    Color::rgb(0xc9, 0x85, 0x00),
    Color::rgb(0xd5, 0x51, 0x81),
    Color::rgb(0x0c, 0xa3, 0x0c),
    Color::rgb(0x90, 0x85, 0xe9),
    Color::rgb(0xe6, 0x67, 0x67),
];

/// The color of the series at `index` when it has none of its own, for a light or
/// a dark surface.
///
/// The palette is fixed rather than built from the accent color: blue, orange,
/// aqua, yellow, magenta, green, violet, red. Neighbors in this order stay apart
/// for the common kinds of color blindness, and the dark colors are their own
/// steps of the same hues, chosen to stand out on a dark surface. After eight
/// series the colors start over; give further series a color of their own
/// ([`Series::color`](crate::Series::color)), or fold small ones into one.
///
/// ```
/// # use naui_charts::series_color;
/// assert_ne!(series_color(0, false), series_color(1, false));
/// assert_ne!(series_color(0, false), series_color(0, true));
/// assert_eq!(series_color(8, false), series_color(0, false));
/// ```
pub fn series_color(index: usize, dark: bool) -> Color {
    let palette = if dark { &DARK } else { &LIGHT };
    palette[index % PALETTE_LEN]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_differ_within_a_palette() {
        for dark in [false, true] {
            for a in 0..PALETTE_LEN {
                for b in a + 1..PALETTE_LEN {
                    assert_ne!(series_color(a, dark), series_color(b, dark));
                }
            }
        }
    }
}
