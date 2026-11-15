//! Reading and writing text and images on the clipboard.
//!
//! Reading is asynchronous on some platforms (GTK4, WinRT, browsers), and writing
//! is asynchronous in browsers too, so on all four platforms the result of reading
//! and writing is returned through a callback. The callback is called on the UI
//! thread after `read_text` / `set_text` has returned (it is not called on the spot
//! even on platforms where it could be synchronous).

use naui_core::{Bitmap, BitmapData, Error, Result, Tasks};
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSPasteboard, NSPasteboardTypePNG,
    NSPasteboardTypeString, NSPasteboardTypeTIFF,
};
use objc2_foundation::{NSData, NSDictionary, NSString};

/// The clipboard (the general `NSPasteboard`). Get it with
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
    pub fn set_text(&self, text: &str, f: impl FnOnce(Result<()>) + 'static) {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let result = if pasteboard
            .setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString })
        {
            Ok(())
        } else {
            Err(Error::new(
                "write to the clipboard",
                "NSPasteboard did not accept the text",
            ))
        };
        self.tasks.spawn(async move { f(result) });
    }

    /// Reads text. Passes `None` if the clipboard holds no text.
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        let text = NSPasteboard::generalPasteboard()
            .stringForType(unsafe { NSPasteboardTypeString })
            .map(|text| text.to_string());
        self.tasks.spawn(async move { f(text) });
    }

    /// Writes an image, replacing the previous contents. Whether it was written is
    /// passed to `f`.
    ///
    /// It is put on the pasteboard both as PNG and as TIFF, the two image types
    /// other apps read.
    pub fn set_image(&self, image: &Bitmap, f: impl FnOnce(Result<()>) + 'static) {
        let result = write_image(image);
        self.tasks.spawn(async move { f(result) });
    }

    /// Reads an image. Passes `None` if the clipboard holds no image.
    ///
    /// PNG is read as it is; any other image AppKit reads from the pasteboard (TIFF,
    /// PDF and so on) is turned into PNG first.
    pub fn read_image(&self, f: impl FnOnce(Option<Bitmap>) + 'static) {
        let image = read_image();
        self.tasks.spawn(async move { f(image) });
    }
}

fn write_image(image: &Bitmap) -> Result<()> {
    let unreadable = || Error::new("write to the clipboard", "the image could not be read");
    // 元が PNG のファイルならそのまま置く。画素は PNG に包む (無圧縮なので、
    // 置く前に AppKit に圧縮し直させる)。JPEG と GIF も PNG に直す。
    let (bytes, keep) = match image.data() {
        BitmapData::Encoded(bytes) => (bytes.to_vec(), image.mime() == Some("image/png")),
        BitmapData::Pixels(rgba) => (
            naui_core::media::encode_png(image.width(), image.height(), rgba)?,
            false,
        ),
    };
    let data = NSData::with_bytes(&bytes);
    let rep = NSBitmapImageRep::imageRepWithData(&data).ok_or_else(unreadable)?;
    let png = if keep {
        data
    } else {
        png_of(&rep).ok_or_else(unreadable)?
    };
    let tiff = rep.TIFFRepresentation().ok_or_else(unreadable)?;

    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    // SAFETY: 種類は AppKit が定めた定数。
    let written = unsafe {
        pasteboard.setData_forType(Some(&png), NSPasteboardTypePNG)
            && pasteboard.setData_forType(Some(&tiff), NSPasteboardTypeTIFF)
    };
    if written {
        Ok(())
    } else {
        Err(Error::new(
            "write to the clipboard",
            "NSPasteboard did not accept the image",
        ))
    }
}

fn read_image() -> Option<Bitmap> {
    let pasteboard = NSPasteboard::generalPasteboard();
    // SAFETY: 種類は AppKit が定めた定数。
    if let Some(png) = pasteboard.dataForType(unsafe { NSPasteboardTypePNG }) {
        if let Ok(bitmap) = Bitmap::from_bytes(&png.to_vec()) {
            return Some(bitmap);
        }
    }
    // TIFF・PDF など、AppKit が読めるものは NSImage に読ませてから PNG にする。
    if !NSImage::canInitWithPasteboard(&pasteboard) {
        return None;
    }
    let image = NSImage::initWithPasteboard(NSImage::alloc(), &pasteboard)?;
    let tiff = image.TIFFRepresentation()?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
    Bitmap::from_bytes(&png_of(&rep)?.to_vec()).ok()
}

fn png_of(rep: &NSBitmapImageRep) -> Option<Retained<NSData>> {
    // SAFETY: 属性は渡さない (空の辞書)。
    unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
}
