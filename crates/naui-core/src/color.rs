//! 色の値 (sRGB の 8 bit)。
//!
//! 色を選ばせるコントロールは 4 環境とも持ち方が違う。AppKit は `NSColor`
//! (色空間つきの浮動小数)、GTK4 は `GdkRGBA` (f32)、WinUI 3 は
//! `Windows.UI.Color` (u8 の ARGB)、DOM は `"#rrggbb"` の文字列。
//! naui はこの [`Color`] を共通の受け渡し口にして、変換だけを
//! 各バックエンドが行う。

use std::fmt;

/// sRGB の色。各成分は 0..=255。
///
/// 透明度は持たない。`<input type="color">` が不透明な色しか返さないため、
/// 4 環境でそろう範囲に合わせている。
///
/// ```
/// # use naui_core::Color;
/// let orange = Color::rgb(0xff, 0x88, 0x00);
/// assert_eq!(orange.to_hex(), "#ff8800");
/// assert_eq!(Color::from_hex("#F80"), Some(Color::rgb(0xff, 0x88, 0x00)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    /// 黒 (`#000000`)。色ピッカーの初期値でもある。
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// 白 (`#ffffff`)。
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// 16 進表記から読む。`#` は省略でき、3 桁の短縮形も受け付ける。
    ///
    /// 読めない文字列では `None` を返す (長さ違い・16 進でない文字)。
    ///
    /// ```
    /// # use naui_core::Color;
    /// assert_eq!(Color::from_hex("336699"), Some(Color::rgb(0x33, 0x66, 0x99)));
    /// assert_eq!(Color::from_hex("#369"), Some(Color::rgb(0x33, 0x66, 0x99)));
    /// assert_eq!(Color::from_hex("#12345"), None);
    /// ```
    pub fn from_hex(text: &str) -> Option<Color> {
        let body = text.trim().strip_prefix('#').unwrap_or(text.trim());
        if !body.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let digit = |c: u8| (c as char).to_digit(16).map(|v| v as u8);
        let bytes = body.as_bytes();
        match bytes.len() {
            // 3 桁は各桁を 2 回並べたものとして読む (`#369` = `#336699`)。
            3 => {
                let v: Vec<u8> = bytes.iter().filter_map(|&c| digit(c)).collect();
                Some(Color::rgb(v[0] * 17, v[1] * 17, v[2] * 17))
            }
            6 => {
                let v: Vec<u8> = bytes.iter().filter_map(|&c| digit(c)).collect();
                Some(Color::rgb(
                    v[0] * 16 + v[1],
                    v[2] * 16 + v[3],
                    v[4] * 16 + v[5],
                ))
            }
            _ => None,
        }
    }

    /// `#rrggbb` の小文字表記へ直す。
    ///
    /// `<input type="color">` が返す形と同じなので、Web バックエンドは
    /// これをそのまま値として使える。
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// 0.0..=1.0 の 3 つ組から作る。範囲の外は端へ寄せる。
    ///
    /// AppKit と GTK4 が浮動小数で色を持つため、その変換に使う。
    pub fn from_unit(r: f64, g: f64, b: f64) -> Color {
        Color::rgb(to_byte(r), to_byte(g), to_byte(b))
    }

    /// 0.0..=1.0 の 3 つ組へ直す。
    pub fn to_unit(self) -> (f64, f64, f64) {
        (
            f64::from(self.r) / 255.0,
            f64::from(self.g) / 255.0,
            f64::from(self.b) / 255.0,
        )
    }
}

/// 0.0..=1.0 を 0..=255 へ丸める。数として読めない値は 0 として扱う。
fn to_byte(v: f64) -> u8 {
    if !v.is_finite() {
        return 0;
    }
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let color = Color::rgb(0x12, 0x34, 0x56);
        assert_eq!(color.to_hex(), "#123456");
        assert_eq!(Color::from_hex(&color.to_hex()), Some(color));
        assert_eq!(color.to_string(), "#123456");
    }

    #[test]
    fn hex_accepts_short_form_and_missing_hash() {
        assert_eq!(Color::from_hex("#369"), Some(Color::rgb(51, 102, 153)));
        assert_eq!(Color::from_hex("369"), Some(Color::rgb(51, 102, 153)));
        assert_eq!(Color::from_hex("FFF"), Some(Color::WHITE));
        assert_eq!(Color::from_hex(" #000000 "), Some(Color::BLACK));
    }

    #[test]
    fn hex_rejects_anything_else() {
        assert_eq!(Color::from_hex(""), None);
        assert_eq!(Color::from_hex("#12345"), None);
        assert_eq!(Color::from_hex("#gggggg"), None);
        assert_eq!(Color::from_hex("#1234567"), None);
    }

    #[test]
    fn unit_conversion_clamps_and_rounds() {
        assert_eq!(Color::from_unit(1.0, 0.5, 0.0), Color::rgb(255, 128, 0));
        assert_eq!(Color::from_unit(2.0, -1.0, f64::NAN), Color::rgb(255, 0, 0));
        let (r, g, b) = Color::WHITE.to_unit();
        assert_eq!((r, g, b), (1.0, 1.0, 1.0));
        assert_eq!(Color::from_unit(r, g, b), Color::WHITE);
    }

    #[test]
    fn default_is_black() {
        assert_eq!(Color::default(), Color::BLACK);
    }
}
