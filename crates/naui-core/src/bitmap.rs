//! Images drawn on a `Canvas` ([`Bitmap`]).
//!
//! A `Bitmap` holds what the app gave it (an encoded file, or RGBA pixels) and its
//! size. Decoding is left to each platform, done once at the first draw and kept
//! with the `Bitmap` (see [`Bitmap::native`]), so drawing the same `Bitmap` every
//! frame does not decode it again.

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::media::{check_pixels, raster_info};

/// An image to draw on a `Canvas` with [`Painter::draw_image`](crate::Painter::draw_image).
///
/// Made from the contents of a PNG, JPEG or GIF file ([`from_bytes`](Self::from_bytes))
/// or from RGBA pixels the app made itself ([`from_pixels`](Self::from_pixels)), the
/// same sources as `Image::set_bytes` and `Image::set_pixels`. Cloning is cheap (the
/// data is shared), so make one when the image is loaded and draw it from `on_draw`
/// as often as needed.
///
/// ```
/// # use naui_core::{Bitmap, Painter, Rect};
/// // A 2 x 1 image: a red pixel and a half-transparent blue one.
/// let bitmap = Bitmap::from_pixels(2, 1, &[255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
/// assert_eq!(bitmap.size(), (2, 1));
///
/// let mut painter = Painter::new(100.0, 100.0);
/// painter.draw_image(&bitmap, Rect::new(10.0, 10.0, 40.0, 20.0));
/// assert_eq!(painter.commands().len(), 1);
/// ```
#[derive(Clone)]
pub struct Bitmap(Rc<BitmapInner>);

struct BitmapInner {
    width: u32,
    height: u32,
    data: Data,
    /// What the backend decoded, kept for the next draw.
    native: RefCell<Option<Rc<dyn Any>>>,
}

enum Data {
    Encoded(Vec<u8>),
    Pixels(Vec<u8>),
}

/// What a [`Bitmap`] was made from. Read by backends to decode it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitmapData<'a> {
    /// The contents of a PNG, JPEG or GIF file. The [`media type`](Bitmap::mime) says
    /// which.
    Encoded(&'a [u8]),
    /// Rows from top to bottom, each pixel 4 bytes of red, green, blue and alpha
    /// (straight, not premultiplied, alpha).
    Pixels(&'a [u8]),
}

impl Bitmap {
    /// An image from the contents of a PNG, JPEG or GIF file held in memory, such as
    /// one embedded with `include_bytes!`.
    ///
    /// The size is read from the file's header here; the pixels are decoded by the
    /// platform when the image is first drawn. An animated GIF draws its first frame.
    ///
    /// Returns `Err` for other formats, SVG included, and for files whose header
    /// cannot be read. A file whose header reads but whose data is broken draws
    /// nothing.
    pub fn from_bytes(bytes: &[u8]) -> crate::Result<Self> {
        let info = raster_info(bytes).ok_or_else(|| {
            crate::Error::new(
                "read the image",
                "only PNG, JPEG and GIF files can be drawn on a Canvas",
            )
        })?;
        Ok(Self::new(
            info.width,
            info.height,
            Data::Encoded(bytes.to_vec()),
        ))
    }

    /// An image from pixels the app made itself: `width` × `height` pixels, rows from
    /// top to bottom, each pixel 4 bytes of red, green, blue and alpha (straight, not
    /// premultiplied, alpha).
    ///
    /// Returns `Err` when `rgba` is not exactly `width * height * 4` bytes, or either
    /// side is 0.
    pub fn from_pixels(width: u32, height: u32, rgba: &[u8]) -> crate::Result<Self> {
        check_pixels(width, height, rgba)?;
        Ok(Self::new(width, height, Data::Pixels(rgba.to_vec())))
    }

    fn new(width: u32, height: u32, data: Data) -> Self {
        Self(Rc::new(BitmapInner {
            width,
            height,
            data,
            native: RefCell::new(None),
        }))
    }

    /// The width in pixels.
    pub fn width(&self) -> u32 {
        self.0.width
    }

    /// The height in pixels.
    pub fn height(&self) -> u32 {
        self.0.height
    }

    /// The size in pixels (width, height).
    pub fn size(&self) -> (u32, u32) {
        (self.0.width, self.0.height)
    }

    /// What it was made from. Backends decode this.
    pub fn data(&self) -> BitmapData<'_> {
        match &self.0.data {
            Data::Encoded(bytes) => BitmapData::Encoded(bytes),
            Data::Pixels(rgba) => BitmapData::Pixels(rgba),
        }
    }

    /// The media type of the file it was made from (`image/png` and so on). `None`
    /// for pixels.
    pub fn mime(&self) -> Option<&'static str> {
        match &self.0.data {
            Data::Encoded(bytes) => raster_info(bytes).map(|info| info.mime),
            Data::Pixels(_) => None,
        }
    }

    /// What the backend made from it, made by `make` the first time and kept with the
    /// `Bitmap` (and every clone of it) from then on. **For backends.**
    ///
    /// `make` is called again only if something of another type was kept, which does
    /// not happen within one backend. It must not call `native` on the same `Bitmap`.
    #[doc(hidden)]
    pub fn native<T: 'static>(&self, make: impl FnOnce(&Bitmap) -> T) -> Rc<T> {
        if let Some(kept) = self.0.native.borrow().as_ref() {
            if let Ok(native) = kept.clone().downcast::<T>() {
                return native;
            }
        }
        let native = Rc::new(make(self));
        *self.0.native.borrow_mut() = Some(native.clone());
        native
    }

    /// Whether the two are the same image (clones of one `Bitmap`).
    pub fn ptr_eq(&self, other: &Bitmap) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Bitmaps are equal when they are the same image (clones of one `Bitmap`). Two
/// made from the same bytes are different images.
impl PartialEq for Bitmap {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

impl fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match &self.0.data {
            Data::Encoded(_) => self.mime().unwrap_or("encoded"),
            Data::Pixels(_) => "pixels",
        };
        f.debug_struct("Bitmap")
            .field("width", &self.0.width)
            .field("height", &self.0.height)
            .field("data", &kind)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::encode_png;

    #[test]
    fn bytes_must_be_a_raster_file() {
        let png = encode_png(4, 3, &[0; 4 * 3 * 4]).unwrap();
        let bitmap = Bitmap::from_bytes(&png).unwrap();
        assert_eq!(bitmap.size(), (4, 3));
        assert_eq!(bitmap.mime(), Some("image/png"));
        assert!(matches!(bitmap.data(), BitmapData::Encoded(bytes) if bytes == png));
        assert!(Bitmap::from_bytes(b"<svg width='1' height='1'/>").is_err());
        assert!(Bitmap::from_bytes(b"").is_err());
    }

    #[test]
    fn pixels_must_fill_the_size() {
        let bitmap = Bitmap::from_pixels(1, 1, &[1, 2, 3, 4]).unwrap();
        assert_eq!(bitmap.mime(), None);
        assert_eq!(bitmap.data(), BitmapData::Pixels(&[1, 2, 3, 4]));
        assert!(Bitmap::from_pixels(2, 1, &[0; 4]).is_err());
        assert!(Bitmap::from_pixels(0, 1, &[]).is_err());
    }

    #[test]
    fn native_is_made_once_and_shared_by_clones() {
        let bitmap = Bitmap::from_pixels(1, 1, &[0; 4]).unwrap();
        let clone = bitmap.clone();
        let made = std::cell::Cell::new(0);
        let first = bitmap.native(|b| {
            made.set(made.get() + 1);
            b.width() * 10
        });
        let second = clone.native(|_| {
            made.set(made.get() + 1);
            0_u32
        });
        assert_eq!((*first, *second, made.get()), (10, 10, 1));
        assert_eq!(bitmap, clone);
        assert_ne!(bitmap, Bitmap::from_pixels(1, 1, &[0; 4]).unwrap());
    }
}
