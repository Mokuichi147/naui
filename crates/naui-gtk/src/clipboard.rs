//! Reading and writing text and images on the clipboard.
//!
//! Reading is asynchronous on some platforms (GTK4, WinRT, browsers), and writing
//! is asynchronous in browsers too, so on all four platforms the result of reading
//! and writing is returned through a callback. The callback is called on the UI
//! thread after `read_text` / `set_text` has returned (it is not called on the spot
//! even on platforms where it could be synchronous).

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use naui_core::{Bitmap, BitmapData, Error, Result};

/// The clipboard (`GdkClipboard`). Get it with [`Ui::clipboard`](crate::Ui::clipboard).
#[derive(Clone)]
pub struct Clipboard;

impl Clipboard {
    fn native() -> Option<gdk::Clipboard> {
        gdk::Display::default().map(|display| display.clipboard())
    }

    /// Writes text, replacing the previous contents. Whether it was written is passed
    /// to `f`.
    pub fn set_text(&self, text: &str, f: impl FnOnce(Result<()>) + 'static) {
        let result = match Self::native() {
            Some(clipboard) => {
                clipboard.set_text(text);
                Ok(())
            }
            None => Err(Error::new("write to the clipboard", "there is no display")),
        };
        // その場では呼ばない約束なので、ループを 1 周させてから渡す。
        glib::idle_add_local_once(move || f(result));
    }

    /// Reads text. Passes `None` if the clipboard holds no text.
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        let Some(clipboard) = Self::native() else {
            // その場では呼ばない約束なので、ループを 1 周させてから渡す。
            glib::idle_add_local_once(move || f(None));
            return;
        };
        clipboard.read_text_async(None::<&gio::Cancellable>, move |result| {
            f(result.ok().flatten().map(|text| text.to_string()));
        });
    }

    /// Writes an image, replacing the previous contents. Whether it was written is
    /// passed to `f`.
    ///
    /// It is put on the clipboard as a `GdkTexture`, which GDK hands to other apps
    /// as PNG (and the other image types it can write). Like text, it is offered by
    /// this app, so it is gone when the app quits unless a clipboard manager keeps
    /// it.
    pub fn set_image(&self, image: &Bitmap, f: impl FnOnce(Result<()>) + 'static) {
        let result = match Self::native() {
            Some(clipboard) => texture_of(image).map(|texture| clipboard.set_texture(&texture)),
            None => Err(Error::new("write to the clipboard", "there is no display")),
        };
        // その場では呼ばない約束なので、ループを 1 周させてから渡す。
        glib::idle_add_local_once(move || f(result));
    }

    /// Reads an image. Passes `None` if the clipboard holds no image.
    ///
    /// GDK reads any image type it knows and the result is turned into PNG.
    pub fn read_image(&self, f: impl FnOnce(Option<Bitmap>) + 'static) {
        let Some(clipboard) = Self::native() else {
            // その場では呼ばない約束なので、ループを 1 周させてから渡す。
            glib::idle_add_local_once(move || f(None));
            return;
        };
        clipboard.read_texture_async(None::<&gio::Cancellable>, move |result| {
            let image = result
                .ok()
                .flatten()
                .and_then(|texture| Bitmap::from_bytes(&texture.save_to_png_bytes()).ok());
            f(image);
        });
    }
}

/// The texture of a bitmap. Files are read by `gdk::Texture::from_bytes`, as
/// `Image::set_bytes` reads them; pixels are handed over as they are (straight
/// RGBA).
fn texture_of(image: &Bitmap) -> Result<gdk::Texture> {
    match image.data() {
        BitmapData::Encoded(bytes) => gdk::Texture::from_bytes(&glib::Bytes::from(bytes))
            .map_err(|error| Error::new("write to the clipboard", error.to_string())),
        BitmapData::Pixels(rgba) => {
            let too_large = || Error::new("write to the clipboard", "the image is too large");
            let width = i32::try_from(image.width()).map_err(|_| too_large())?;
            let height = i32::try_from(image.height()).map_err(|_| too_large())?;
            Ok(gdk::MemoryTexture::new(
                width,
                height,
                gdk::MemoryFormat::R8g8b8a8,
                &glib::Bytes::from(rgba),
                image.width() as usize * 4,
            )
            .upcast())
        }
    }
}
