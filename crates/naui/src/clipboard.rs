use crate::sys;
use naui_core::{Bitmap, Result};

/// The clipboard (`GdkClipboard`). Get it with [`Ui::clipboard`](crate::Ui::clipboard).
///
/// Text and images can be read and written. The results arrive through a callback,
/// called on the UI thread after the method has returned (see
/// [Clipboard and URLs](crate::guide#clipboard-and-urls)).
#[derive(Clone)]
pub struct Clipboard(pub(crate) sys::Clipboard);

impl_backend!(Clipboard);

impl Clipboard {
    /// Writes text, replacing the previous contents. Whether it was written is passed
    /// to `f`.
    #[inline]
    pub fn set_text(&self, text: &str, f: impl FnOnce(Result<()>) + 'static) {
        self.0.set_text(text, f)
    }

    /// Reads text. Passes `None` if the clipboard holds no text.
    #[inline]
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        self.0.read_text(f)
    }

    /// Writes an image, replacing the previous contents. Whether it was written is
    /// passed to `f`.
    ///
    /// The image is a [`Bitmap`], made from the same sources as
    /// [`Image::set_bytes`](crate::Image::set_bytes) and
    /// [`Image::set_pixels`](crate::Image::set_pixels): the contents of a PNG, JPEG
    /// or GIF file, or RGBA pixels. Other apps are given it as PNG (and as TIFF on
    /// macOS).
    ///
    /// ```no_run
    /// # fn build(ui: &naui::Ui) -> naui::Result<()> {
    /// // A 2 x 1 image: a red pixel and a blue one.
    /// let image = naui::Bitmap::from_pixels(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255])?;
    /// ui.clipboard().set_image(&image, |result| {
    ///     if let Err(error) = result {
    ///         eprintln!("could not copy: {error}");
    ///     }
    /// });
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn set_image(&self, image: &Bitmap, f: impl FnOnce(Result<()>) + 'static) {
        self.0.set_image(image, f)
    }

    /// Reads an image. Passes `None` if the clipboard holds no image, or it could not
    /// be read.
    ///
    /// The [`Bitmap`] can be drawn on a [`Canvas`](crate::Canvas) or shown with
    /// [`Image::set_bitmap`](crate::Image::set_bitmap). Its size is the image's size
    /// in pixels. What it was made from depends on the platform (PNG file contents
    /// or RGBA pixels), so read it through [`Bitmap::data`] only if you need the
    /// pixels themselves.
    ///
    /// ```no_run
    /// # fn build(ui: &naui::Ui) -> naui::Result<()> {
    /// let preview = ui.image("")?;
    /// ui.clipboard().read_image(move |image| {
    ///     if let Some(image) = image {
    ///         preview.set_bitmap(&image);
    ///     }
    /// });
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn read_image(&self, f: impl FnOnce(Option<Bitmap>) + 'static) {
        self.0.read_image(f)
    }
}
