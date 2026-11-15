//! Reading and writing text and images on the clipboard.
//!
//! Reading is asynchronous on some platforms (GTK4, WinRT, browsers), and writing
//! is asynchronous in browsers too, so on all four platforms the result of reading
//! and writing is returned through a callback. The callback is called on the UI
//! thread after `read_text` / `set_text` has returned (it is not called on the spot
//! even on platforms where it could be synchronous).

use naui_core::{Bitmap, BitmapData, Result, Tasks};
use windows::ApplicationModel::DataTransfer::{
    Clipboard as WinClipboard, DataPackage, StandardDataFormats,
};
use windows::Storage::Streams::{
    DataReader, DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference,
};
use windows::Win32::Graphics::Imaging::GUID_WICPixelFormat32bppRGBA;
use windows_core::HSTRING;

use crate::to_error;

/// The clipboard (`Windows.ApplicationModel.DataTransfer.Clipboard`). Get it with
/// [`Ui::clipboard`](crate::Ui::clipboard).
#[derive(Clone)]
pub struct Clipboard {
    tasks: Tasks,
}

impl Clipboard {
    pub(crate) fn new(tasks: Tasks) -> Self {
        Self { tasks }
    }

    /// Writes text, replacing the previous contents. Whether it was written is passed
    /// to `f`.
    ///
    /// It is `Flush`ed right after writing so that it remains even after the app
    /// closes.
    pub fn set_text(&self, text: &str, f: impl FnOnce(Result<()>) + 'static) {
        let result = write(text);
        self.tasks.spawn(async move { f(result) });
    }

    /// Reads text. Passes `None` if the clipboard holds no text.
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        self.tasks.spawn(async move {
            f(read().await);
        });
    }

    /// Writes an image, replacing the previous contents. Whether it was written is
    /// passed to `f`.
    ///
    /// It is handed to `DataPackage.SetBitmap` as PNG (JPEG and GIF files are
    /// turned into PNG first), and `Flush`ed like text so that it remains after the
    /// app closes.
    pub fn set_image(&self, image: &Bitmap, f: impl FnOnce(Result<()>) + 'static) {
        let png = png_of(image);
        self.tasks.spawn(async move {
            let result = match png {
                Ok(png) => write_image(&png).await,
                Err(error) => Err(error),
            };
            f(result);
        });
    }

    /// Reads an image. Passes `None` if the clipboard holds no image.
    ///
    /// A PNG is read as it is; anything else WIC can decode (the BMP of images
    /// copied by older apps, for example) arrives as RGBA pixels.
    pub fn read_image(&self, f: impl FnOnce(Option<Bitmap>) + 'static) {
        self.tasks.spawn(async move {
            f(read_image().await);
        });
    }
}

fn write(text: &str) -> Result<()> {
    let package = DataPackage::new().map_err(|e| to_error("create the clipboard contents", e))?;
    package
        .SetText(&HSTRING::from(text))
        .map_err(|e| to_error("set the clipboard contents", e))?;
    WinClipboard::SetContent(&package).map_err(|e| to_error("write to the clipboard", e))?;
    let _ = WinClipboard::Flush();
    Ok(())
}

async fn read() -> Option<String> {
    let content = WinClipboard::GetContent().ok()?;
    let format = StandardDataFormats::Text().ok()?;
    if !content.Contains(&format).unwrap_or(false) {
        return None;
    }
    let text = content.GetTextAsync().ok()?.await.ok()?;
    Some(text.to_string())
}

/// The image as the contents of a PNG file: a PNG file as it is, pixels packed by
/// `encode_png`, and other files decoded by WIC and packed the same way.
fn png_of(image: &Bitmap) -> Result<Vec<u8>> {
    match image.data() {
        BitmapData::Encoded(bytes) if image.mime() == Some("image/png") => Ok(bytes.to_vec()),
        BitmapData::Encoded(bytes) => {
            let (width, height, rgba) =
                crate::canvas::decode_file(bytes, &GUID_WICPixelFormat32bppRGBA)
                    .map_err(|e| to_error("read the image", e))?;
            naui_core::media::encode_png(width, height, &rgba)
        }
        BitmapData::Pixels(rgba) => {
            naui_core::media::encode_png(image.width(), image.height(), rgba)
        }
    }
}

async fn write_image(png: &[u8]) -> Result<()> {
    let failed = |e| to_error("write to the clipboard", e);
    let stream = InMemoryRandomAccessStream::new().map_err(failed)?;
    let writer = DataWriter::CreateDataWriter(&stream).map_err(failed)?;
    writer.WriteBytes(png).map_err(failed)?;
    writer.StoreAsync().map_err(failed)?.await.map_err(failed)?;
    // 書き手を捨てるとストリームも閉じられるので、切り離してから先頭へ戻す。
    writer.DetachStream().map_err(failed)?;
    stream.Seek(0).map_err(failed)?;

    let package = DataPackage::new().map_err(|e| to_error("create the clipboard contents", e))?;
    package
        .SetBitmap(&RandomAccessStreamReference::CreateFromStream(&stream).map_err(failed)?)
        .map_err(|e| to_error("set the clipboard contents", e))?;
    WinClipboard::SetContent(&package).map_err(failed)?;
    let _ = WinClipboard::Flush();
    Ok(())
}

async fn read_image() -> Option<Bitmap> {
    let content = WinClipboard::GetContent().ok()?;
    let format = StandardDataFormats::Bitmap().ok()?;
    if !content.Contains(&format).unwrap_or(false) {
        return None;
    }
    let reference = content.GetBitmapAsync().ok()?.await.ok()?;
    let stream = reference.OpenReadAsync().ok()?.await.ok()?;
    let size = u32::try_from(stream.Size().ok()?).ok()?;
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let loaded = reader.LoadAsync(size).ok()?.await.ok()?;
    let mut bytes = vec![0; loaded as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    image_of(&bytes)
}

/// The image in the bytes the clipboard gave. PNG is kept as a file; the rest is
/// decoded by WIC.
fn image_of(bytes: &[u8]) -> Option<Bitmap> {
    if naui_core::media::raster_info(bytes).is_some_and(|info| info.mime == "image/png") {
        if let Ok(image) = Bitmap::from_bytes(bytes) {
            return Some(image);
        }
    }
    let (width, height, mut rgba) =
        crate::canvas::decode_file(bytes, &GUID_WICPixelFormat32bppRGBA)
            .map_err(|error| eprintln!("naui-windows: could not read the clipboard image: {error}"))
            .ok()?;
    opaque_if_all_transparent(&mut rgba);
    Bitmap::from_pixels(width, height, &rgba).ok()
}

/// Makes every pixel opaque when none has any alpha. 32-bit bitmaps written by
/// older apps often leave the fourth byte at 0 without meaning transparency, and
/// a picture that is entirely transparent is not worth pasting either.
fn opaque_if_all_transparent(rgba: &mut [u8]) {
    if rgba.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0) {
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_transparent_pixels_become_opaque() {
        let mut rgba = [1, 2, 3, 0, 4, 5, 6, 0];
        opaque_if_all_transparent(&mut rgba);
        assert_eq!(rgba, [1, 2, 3, 255, 4, 5, 6, 255]);

        let mut rgba = [1, 2, 3, 0, 4, 5, 6, 10];
        opaque_if_all_transparent(&mut rgba);
        assert_eq!(
            rgba,
            [1, 2, 3, 0, 4, 5, 6, 10],
            "一部でも透けていれば触らない"
        );
    }

    #[test]
    fn png_files_are_kept_as_they_are() {
        let png = naui_core::media::encode_png(2, 1, &[0; 8]).unwrap();
        let image = image_of(&png).expect("PNG");
        assert!(matches!(image.data(), BitmapData::Encoded(bytes) if bytes == png));
        let bitmap = Bitmap::from_pixels(1, 1, &[9, 8, 7, 6]).unwrap();
        let written = png_of(&bitmap).unwrap();
        assert_eq!(
            naui_core::media::raster_info(&written).map(|info| info.mime),
            Some("image/png")
        );
    }
}
