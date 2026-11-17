//! Reading and writing text and images on the clipboard.
//!
//! Reading is asynchronous on some platforms (GTK4, WinRT, browsers), and writing
//! is asynchronous in browsers too, so on all four platforms the result of reading
//! and writing is returned through a callback. The callback is called on the UI
//! thread after `read_text` / `set_text` has returned (it is not called on the spot
//! even on platforms where it could be synchronous).
//!
//! The browser's Clipboard API is available only in secure contexts (https or
//! localhost), and reading may ask the user for permission.

use js_sys::futures::JsFuture;
use naui_core::{Bitmap, BitmapData, Error, Result};
use wasm_bindgen::{JsCast, JsValue};

/// The image type written and read. It is the one image type the Clipboard API
/// requires browsers to support.
const PNG: &str = "image/png";

/// The clipboard (`navigator.clipboard`). Get it with
/// [`Ui::clipboard`](crate::Ui::clipboard).
#[derive(Clone)]
pub struct Clipboard;

impl Clipboard {
    fn native() -> Option<web_sys::Clipboard> {
        web_sys::window().map(|window| window.navigator().clipboard())
    }

    /// Writes text, replacing the previous contents. Whether it was written is passed
    /// to `f`.
    ///
    /// Browsers may allow writing only inside a user action (a click, for example), and
    /// refuse inside an `<iframe>` without permission. When refused, `Err` arrives, so
    /// call it from a button's `on_click` or similar and report something like "Copied"
    /// only after checking the result.
    pub fn set_text(&self, text: &str, f: impl FnOnce(Result<()>) + 'static) {
        let promise = Self::native().map(|clipboard| clipboard.write_text(text));
        js_sys::futures::spawn_local(async move {
            let result = match promise {
                Some(promise) => JsFuture::from(promise)
                    .await
                    .map(|_| ())
                    .map_err(|e| crate::to_error("write to the clipboard", e)),
                None => Err(unavailable("write to the clipboard")),
            };
            f(result);
        });
    }

    /// Reads text. Passes `None` if the clipboard holds no text or it could not be
    /// read.
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        let promise = Self::native().map(|clipboard| clipboard.read_text());
        js_sys::futures::spawn_local(async move {
            let text = match promise {
                Some(promise) => JsFuture::from(promise)
                    .await
                    .ok()
                    .and_then(|value| value.as_string()),
                None => None,
            };
            f(text.filter(|text| !text.is_empty()));
        });
    }

    /// Writes an image as `image/png`, replacing the previous contents. Whether it
    /// was written is passed to `f`.
    ///
    /// Like text, browsers may allow it only inside a user action, so call it from a
    /// button's `on_click` or similar. A JPEG or GIF file is turned into PNG by the
    /// browser (through a `<canvas>`); the `ClipboardItem` is handed a promise of
    /// it, so `write` is still called within the user action. Browsers without
    /// `ClipboardItem` (Firefox before 127) give `Err`.
    pub fn set_image(&self, image: &Bitmap, f: impl FnOnce(Result<()>) + 'static) {
        let promise = Self::native()
            .ok_or_else(|| unavailable("write to the clipboard"))
            .and_then(|clipboard| {
                let item = clipboard_item(image)?;
                Ok(clipboard.write(&js_sys::Array::of1(&item)))
            });
        js_sys::futures::spawn_local(async move {
            let result = match promise {
                Ok(promise) => JsFuture::from(promise)
                    .await
                    .map(|_| ())
                    .map_err(|e| crate::to_error("write to the clipboard", e)),
                Err(error) => Err(error),
            };
            f(result);
        });
    }

    /// Reads an image. Passes `None` if the clipboard holds no `image/png` or it
    /// could not be read.
    ///
    /// Browsers hand over images as `image/png` whatever the app that copied them
    /// wrote. Reading may ask the user for permission (Firefox shows a "Paste"
    /// button to press), and `None` arrives when it is refused.
    pub fn read_image(&self, f: impl FnOnce(Option<Bitmap>) + 'static) {
        let promise = Self::native().map(|clipboard| clipboard.read());
        js_sys::futures::spawn_local(async move {
            let image = match promise {
                Some(promise) => read_png(promise).await,
                None => None,
            };
            f(image);
        });
    }
}

fn unavailable(context: &'static str) -> Error {
    Error::new(context, "the Clipboard API is not available")
}

/// A `ClipboardItem` holding the image as `image/png`: a `Blob` of a PNG file or of
/// packed pixels, or the promise of one for other files.
fn clipboard_item(image: &Bitmap) -> Result<web_sys::ClipboardItem> {
    let failed = |e| crate::to_error("write to the clipboard", e);
    let value: JsValue = match image.data() {
        BitmapData::Encoded(bytes) if image.mime() == Some(PNG) => blob(bytes, PNG)?.into(),
        BitmapData::Encoded(bytes) => {
            let file = blob(bytes, image.mime().unwrap_or(""))?;
            js_sys::futures::future_to_promise(async move { to_png(&file).await.map(Into::into) })
                .into()
        }
        BitmapData::Pixels(rgba) => {
            let png = naui_core::media::encode_png(image.width(), image.height(), rgba)?;
            blob(&png, PNG)?.into()
        }
    };
    let record = js_sys::Object::new();
    js_sys::Reflect::set(&record, &PNG.into(), &value).map_err(failed)?;
    web_sys::ClipboardItem::new_with_record_from_str_to_blob_promise(&record).map_err(failed)
}

fn blob(bytes: &[u8], kind: &str) -> Result<web_sys::Blob> {
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(kind);
    web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)
        .map_err(|e| crate::to_error("write to the clipboard", e))
}

/// Has the browser decode an image file and encode it again as PNG.
async fn to_png(file: &web_sys::Blob) -> std::result::Result<web_sys::Blob, JsValue> {
    let window = web_sys::window().ok_or("there is no window")?;
    let decoded: web_sys::ImageBitmap = JsFuture::from(window.create_image_bitmap_with_blob(file)?)
        .await?
        .dyn_into()?;
    let canvas: web_sys::HtmlCanvasElement = window
        .document()
        .ok_or("there is no document")?
        .create_element("canvas")?
        .dyn_into()?;
    canvas.set_width(decoded.width());
    canvas.set_height(decoded.height());
    let context: web_sys::CanvasRenderingContext2d = canvas
        .get_context("2d")?
        .ok_or("the canvas has no 2d context")?
        .dyn_into()?;
    context.draw_image_with_image_bitmap(&decoded, 0.0, 0.0)?;
    decoded.close();
    let mut refused = None;
    let encoded = js_sys::Promise::new(&mut |resolve, _reject| {
        refused = canvas.to_blob_with_type(&resolve, PNG).err();
    });
    if let Some(error) = refused {
        return Err(error);
    }
    JsFuture::from(encoded)
        .await?
        .dyn_into()
        .map_err(|_| "the image could not be encoded as PNG".into())
}

/// The first `image/png` among the items `clipboard.read()` resolves to.
async fn read_png(promise: js_sys::Promise) -> Option<Bitmap> {
    let items: js_sys::Array = JsFuture::from(promise).await.ok()?.dyn_into().ok()?;
    for item in items.iter() {
        // 本物の `ClipboardItem` かどうかは問わない (types と getType だけを使う)。
        let item: web_sys::ClipboardItem = item.unchecked_into();
        if !item.types().includes(&PNG.into(), 0) {
            continue;
        }
        let blob: web_sys::Blob = JsFuture::from(item.get_type(PNG))
            .await
            .ok()?
            .dyn_into()
            .ok()?;
        let buffer = JsFuture::from(blob.array_buffer()).await.ok()?;
        return Bitmap::from_bytes(&js_sys::Uint8Array::new(&buffer).to_vec()).ok();
    }
    None
}
