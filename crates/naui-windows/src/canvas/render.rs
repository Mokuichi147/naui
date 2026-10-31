//! Replaying [`DrawCommand`]s with Direct2D and DirectWrite.
//!
//! One set of devices is made per UI thread and shared by every surface on it: a
//! Direct3D 11 device (hardware, or WARP when there is none), the Direct2D device
//! made from it, and per `Compositor` the `CompositionGraphicsDevice` that hands out
//! drawing surfaces backed by that device. Text is laid out by DirectWrite.
//!
//! When the GPU is reset or removed, drawing fails with `DXGI_ERROR_DEVICE_REMOVED`
//! and friends. Then new devices are made and handed to the existing graphics
//! devices (`SetRenderingDevice`), which keeps every surface valid but empties it,
//! so every surface is drawn again. Loss is noticed when a draw fails, or before
//! that when Direct3D signals its removal event ([`RemovalWatch`]).

use std::cell::RefCell;
use std::collections::HashMap;

use naui_core::{Align, Color, DrawCommand, Path, PathSegment, Point};
use naui_winui3::Microsoft::UI::Composition::{CompositionGraphicsDevice, Compositor};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};
use naui_winui3::{ICompositionGraphicsDeviceInterop, ICompositorInterop};
use windows::Win32::Foundation::{
    CloseHandle, D2DERR_RECREATE_TARGET, E_FAIL, HANDLE, HMODULE, INVALID_HANDLE_VALUE,
};
use windows::Win32::Globalization::GetUserDefaultLocaleName;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING, D2D1_PIXEL_FORMAT,
    D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1, ID2D1GeometrySink,
    ID2D1PathGeometry1, ID2D1SolidColorBrush, ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_ALIASED,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_CAP_STYLE_FLAT, D2D1_DASH_STYLE_CUSTOM, D2D1_DASH_STYLE_SOLID,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
    D2D1_FACTORY_TYPE_MULTI_THREADED, D2D1_LINE_JOIN_MITER, D2D1_MAP_OPTIONS_READ,
    D2D1_STROKE_STYLE_PROPERTIES1, D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL,
    D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_9_3,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11Device4, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET,
};
use windows::Win32::System::Threading::{
    CreateEventW, RegisterWaitForSingleObject, SetEvent, UnregisterWaitEx, INFINITE,
    WT_EXECUTEONLYONCE,
};
use windows_core::{Interface, HSTRING};
use windows_numerics::{Matrix3x2, Vector2};

/// The typefaces tried for text, first match wins. WinUI's controls use Segoe UI
/// Variable (Windows 11); Windows 10 has only Segoe UI.
const FONT_FAMILIES: [&str; 2] = ["Segoe UI Variable Text", "Segoe UI"];

/// How many text formats (one per size) are kept at most.
const MAX_TEXT_FORMATS: usize = 32;

/// The devices that are replaced when lost. The Direct2D device holds the Direct3D
/// device it was made from.
struct Devices {
    d2d: ID2D1Device,
    /// The widest and tallest surface the device can hold, in pixels.
    max_surface: u32,
    /// Counts up each time the devices are made again, so that one loss reported
    /// twice (by a failed draw and by the removal event) is recovered from once.
    generation: u64,
    /// Absent when Direct3D cannot report removal (before Windows 10 1703).
    watch: Option<RemovalWatch>,
}

impl Devices {
    fn new(factory: &ID2D1Factory1, generation: u64) -> windows_core::Result<Self> {
        let d3d = create_d3d_device(D3D_DRIVER_TYPE_HARDWARE)
            .or_else(|_| create_d3d_device(D3D_DRIVER_TYPE_WARP))?;
        let dxgi: IDXGIDevice = d3d.cast()?;
        // SAFETY: 作ったばかりの DXGI デバイスを渡すだけ。
        let d2d = unsafe { factory.CreateDevice(&dxgi)? };
        // SAFETY: 作ったばかりのデバイスに問い合わせるだけ。
        let max_surface = max_texture_size(unsafe { d3d.GetFeatureLevel() });
        Ok(Self {
            d2d,
            max_surface,
            generation,
            watch: RemovalWatch::new(&d3d, generation).ok(),
        })
    }
}

/// The widest and tallest texture a device of `level` can hold. Devices are made
/// with Direct3D's default list of feature levels, which goes down to 9_1, so older
/// or basic GPUs can be well under 16384.
fn max_texture_size(level: D3D_FEATURE_LEVEL) -> u32 {
    match level {
        level if level.0 >= D3D_FEATURE_LEVEL_11_0.0 => 16384,
        level if level.0 >= D3D_FEATURE_LEVEL_10_0.0 => 8192,
        D3D_FEATURE_LEVEL_9_3 => 4096,
        _ => 2048,
    }
}

/// Waits for Direct3D to report the device removed (a GPU reset, a driver update),
/// and then makes the devices again and redraws every surface on the UI thread.
///
/// Without it, loss is noticed only when a draw fails, so a surface the app never
/// redraws (a chart that does not change) would stay empty. The event is waited on
/// by the thread pool, which hands the work to the UI thread's `DispatcherQueue`.
struct RemovalWatch {
    device: ID3D11Device4,
    cookie: u32,
    event: HANDLE,
    wait: HANDLE,
    /// Read by [`on_device_removed`] until the wait is unregistered.
    context: *mut RemovalContext,
}

struct RemovalContext {
    queue: DispatcherQueue,
    generation: u64,
}

impl RemovalWatch {
    fn new(d3d: &ID3D11Device, generation: u64) -> windows_core::Result<Self> {
        let device: ID3D11Device4 = d3d.cast()?;
        let queue = DispatcherQueue::GetForCurrentThread()?;
        // SAFETY: 作ったイベントと待機は、失敗したところまでを戻してから返す。
        // 成功したものは `Drop` で外す。
        unsafe {
            let event = CreateEventW(None, false, false, None)?;
            let cookie = match device.RegisterDeviceRemovedEvent(event) {
                Ok(cookie) => cookie,
                Err(error) => {
                    let _ = CloseHandle(event);
                    return Err(error);
                }
            };
            let context = Box::into_raw(Box::new(RemovalContext { queue, generation }));
            let mut wait = HANDLE::default();
            if let Err(error) = RegisterWaitForSingleObject(
                &mut wait,
                event,
                Some(on_device_removed),
                Some(context as *const core::ffi::c_void),
                INFINITE,
                WT_EXECUTEONLYONCE,
            ) {
                device.UnregisterDeviceRemoved(cookie);
                let _ = CloseHandle(event);
                drop(Box::from_raw(context));
                return Err(error);
            }
            Ok(Self {
                device,
                cookie,
                event,
                wait,
                context,
            })
        }
    }
}

impl Drop for RemovalWatch {
    fn drop(&mut self) {
        // SAFETY: 待機を外し (走っている呼び出しがあれば終わるまで待つ)、
        // もう読まれなくなってから文脈を解放する。
        unsafe {
            let _ = UnregisterWaitEx(self.wait, Some(INVALID_HANDLE_VALUE));
            self.device.UnregisterDeviceRemoved(self.cookie);
            let _ = CloseHandle(self.event);
            drop(Box::from_raw(self.context));
        }
    }
}

/// Runs on a thread-pool thread when the device is removed. Only hands the work to
/// the UI thread, where the devices and surfaces live.
unsafe extern "system" fn on_device_removed(context: *mut core::ffi::c_void, _timed_out: bool) {
    // SAFETY: `RemovalWatch` が待機を外すまで文脈は生きている。
    let context = unsafe { &*(context as *const RemovalContext) };
    let generation = context.generation;
    let _ = context
        .queue
        .TryEnqueue(&DispatcherQueueHandler::new(move || {
            super::recover_devices(generation);
            Ok(())
        }));
}

fn create_d3d_device(driver: D3D_DRIVER_TYPE) -> windows_core::Result<ID3D11Device> {
    let mut device = None;
    // SAFETY: 出力先は手元の `Option` だけで、ほかの引数は既定のまま。
    unsafe {
        D3D11CreateDevice(
            None,
            driver,
            HMODULE::default(),
            // Direct2D が描くには BGRA の対応が要る。
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
    }
    device.ok_or_else(|| windows_core::Error::from(E_FAIL))
}

/// What the surfaces on one UI thread share.
pub(super) struct Renderer {
    factory: ID2D1Factory1,
    devices: Devices,
    dwrite: IDWriteFactory,
    /// The typeface name (UTF-16, NUL-terminated) and the locale used for text.
    font_family: HSTRING,
    locale: HSTRING,
    /// One `CompositionGraphicsDevice` per `Compositor`.
    graphics: Vec<(Compositor, CompositionGraphicsDevice)>,
    /// Text formats by size (`f32` bits). Sizes repeat within a drawing.
    formats: HashMap<u32, IDWriteTextFormat>,
}

/// Where this thread's renderer is kept.
///
/// What is still here when the thread ends is **leaked rather than released**.
/// Thread-local values are dropped while the process exits, after the Windows App
/// SDK has shut Composition down, and releasing the graphics device then ends the
/// process with `STATUS_INVALID_PARAMETER` (seen with WARP, the device used where
/// there is no GPU). The shutdown path releases it earlier with [`release`].
struct Slot(Option<Renderer>);

impl Drop for Slot {
    fn drop(&mut self) {
        std::mem::forget(self.0.take());
    }
}

thread_local! {
    static RENDERER: RefCell<Slot> = const { RefCell::new(Slot(None)) };
}

/// Runs `f` with this thread's renderer, making it the first time. `None` when the
/// devices cannot be made (nothing can be drawn then).
pub(super) fn with_renderer<R>(f: impl FnOnce(&mut Renderer) -> R) -> Option<R> {
    RENDERER.with(|slot| {
        let slot = &mut slot.borrow_mut().0;
        if slot.is_none() {
            *slot = Renderer::new().ok();
        }
        slot.as_mut().map(f)
    })
}

/// The generation of this thread's devices. `None` before any surface was drawn.
pub(super) fn current_generation() -> Option<u64> {
    RENDERER
        .try_with(|slot| slot.borrow().0.as_ref().map(|r| r.devices.generation))
        .ok()
        .flatten()
}

/// Makes the devices again if `generation` is still the current one, and returns
/// whether it did (every surface then needs drawing again).
pub(super) fn recover_if_current(generation: u64) -> bool {
    RENDERER
        .try_with(|slot| match slot.borrow_mut().0.as_mut() {
            Some(renderer) if renderer.devices.generation == generation => {
                renderer.recover().is_ok()
            }
            _ => false,
        })
        .unwrap_or(false)
}

/// Signals the removal event as Direct3D would. **For automated tests only.**
/// Returns whether there was an event to signal.
pub(super) fn signal_removal_for_test() -> bool {
    RENDERER
        .try_with(|slot| {
            let slot = slot.borrow();
            let Some(watch) = slot.0.as_ref().and_then(|r| r.devices.watch.as_ref()) else {
                return false;
            };
            // SAFETY: 生きている自前のイベントを立てるだけ。
            unsafe { SetEvent(watch.event).is_ok() }
        })
        .unwrap_or(false)
}

/// Releases this thread's renderer while Composition is still running. A surface
/// drawn after this makes a new one.
pub(super) fn release() {
    let _ = RENDERER.try_with(|slot| {
        // 中の COM の解放が何かを呼び返しても借用が重ならないよう、外へ出してから落とす。
        let renderer = slot.borrow_mut().0.take();
        drop(renderer);
    });
}

impl Renderer {
    fn new() -> windows_core::Result<Self> {
        // 同じデバイスを Composition も使うので、排他を Direct2D に任せられる
        // マルチスレッド用で作る。
        // SAFETY: 既定の設定でファクトリを作るだけ。
        let factory: ID2D1Factory1 =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_MULTI_THREADED, None)? };
        let devices = Devices::new(&factory, 0)?;
        // SAFETY: 共有のファクトリを取るだけ。
        let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let font_family = HSTRING::from(pick_font_family(&dwrite));
        Ok(Self {
            factory,
            devices,
            dwrite,
            font_family,
            locale: user_locale(),
            graphics: Vec::new(),
            formats: HashMap::new(),
        })
    }

    /// The graphics device for surfaces shown through `compositor`.
    pub(super) fn graphics_device(
        &mut self,
        compositor: &Compositor,
    ) -> windows_core::Result<CompositionGraphicsDevice> {
        if let Some((_, device)) = self.graphics.iter().find(|(c, _)| c == compositor) {
            return Ok(device.clone());
        }
        let interop: ICompositorInterop = compositor.cast()?;
        // SAFETY: 生きている Direct2D デバイスを渡す。
        let device = unsafe { interop.CreateGraphicsDevice(&self.devices.d2d)? };
        self.graphics.push((compositor.clone(), device.clone()));
        Ok(device)
    }

    /// Replaces lost devices. Every surface made from the graphics devices is kept but
    /// emptied, so the caller draws all of them again.
    fn recover(&mut self) -> windows_core::Result<()> {
        self.devices = Devices::new(&self.factory, self.devices.generation + 1)?;
        for (_, device) in &self.graphics {
            let interop: ICompositionGraphicsDeviceInterop = device.cast()?;
            // SAFETY: 作り直したばかりの Direct2D デバイスを渡す。
            unsafe { interop.SetRenderingDevice(&self.devices.d2d)? };
        }
        Ok(())
    }

    fn text_format(&mut self, size: f32) -> windows_core::Result<IDWriteTextFormat> {
        if let Some(format) = self.formats.get(&size.to_bits()) {
            return Ok(format.clone());
        }
        // 大きさを動かし続ける描画で増え続けないよう、溜まったら捨てる。
        if self.formats.len() >= MAX_TEXT_FORMATS {
            self.formats.clear();
        }
        // SAFETY: 文字列はどちらも NUL 終端の HSTRING。
        let format = unsafe {
            self.dwrite.CreateTextFormat(
                &self.font_family,
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                &self.locale,
            )?
        };
        // SAFETY: 作ったばかりの書式に折り返しの有無を設定するだけ。
        unsafe { format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)? };
        self.formats.insert(size.to_bits(), format.clone());
        Ok(format)
    }

    /// Draws `commands` onto `context`, which is already between `BeginDraw` and
    /// `EndDraw`. Units are logical pixels from the current transform and DPI.
    pub(super) fn draw(
        &mut self,
        context: &ID2D1DeviceContext,
        commands: &[DrawCommand],
    ) -> windows_core::Result<()> {
        // SAFETY: 渡された描画中のコンテキストへ命令を順に流すだけ。
        unsafe {
            // 背景が透明なので ClearType (不透明な背景が前提) は使えない。
            context.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let brush: ID2D1SolidColorBrush =
                context.CreateSolidColorBrush(&color_f(Color::BLACK, 1.0), None)?;
            let mut strokes: HashMap<Vec<u32>, ID2D1StrokeStyle> = HashMap::new();
            for command in commands {
                match command {
                    DrawCommand::Fill {
                        path,
                        color,
                        opacity,
                    } => {
                        brush.SetColor(&color_f(*color, *opacity));
                        // 棒グラフや点の印に多い長方形は、ジオメトリを作らずに塗る。
                        if let Some(rect) = axis_aligned_rect(path) {
                            context.FillRectangle(&rect, &brush);
                        } else {
                            context.FillGeometry(&self.geometry(path)?, &brush, None);
                        }
                    }
                    DrawCommand::Stroke {
                        path,
                        color,
                        width,
                        dash,
                        opacity,
                    } => {
                        let pattern = dash_pattern(dash, *width);
                        let key: Vec<u32> = pattern.iter().map(|v| v.to_bits()).collect();
                        let style = match strokes.get(&key) {
                            Some(style) => style.clone(),
                            None => {
                                let style = self.stroke_style(&pattern)?;
                                strokes.insert(key, style.clone());
                                style
                            }
                        };
                        brush.SetColor(&color_f(*color, *opacity));
                        // 折れ線グラフの線分に多い 2 点の直線も同じ。
                        if let [PathSegment::MoveTo(from), PathSegment::LineTo(to)] =
                            path.segments()
                        {
                            context.DrawLine(
                                point(*from),
                                point(*to),
                                &brush,
                                finite(*width),
                                &style,
                            );
                        } else {
                            let geometry = self.geometry(path)?;
                            context.DrawGeometry(&geometry, &brush, finite(*width), &style);
                        }
                    }
                    DrawCommand::Text {
                        text,
                        at,
                        size,
                        color,
                        align,
                        opacity,
                    } => {
                        let format = self.text_format(finite(*size))?;
                        let utf16: Vec<u16> = single_line(text).encode_utf16().collect();
                        let layout =
                            self.dwrite
                                .CreateTextLayout(&utf16, &format, f32::MAX, f32::MAX)?;
                        let mut metrics = DWRITE_TEXT_METRICS::default();
                        layout.GetMetrics(&mut metrics)?;
                        let left = text_left(
                            at.x,
                            f64::from(metrics.widthIncludingTrailingWhitespace),
                            *align,
                        );
                        brush.SetColor(&color_f(*color, *opacity));
                        context.DrawTextLayout(
                            Vector2::new(finite(left), finite(at.y)),
                            &layout,
                            &brush,
                            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn geometry(&self, path: &Path) -> windows_core::Result<ID2D1PathGeometry1> {
        // SAFETY: 作ったばかりのジオメトリへ図形を書き込み、閉じてから返す。
        unsafe {
            let geometry = self.factory.CreatePathGeometry()?;
            let sink = geometry.Open()?;
            sink.SetFillMode(D2D1_FILL_MODE_WINDING);
            replay(path, &mut D2dSink(&sink));
            sink.Close()?;
            Ok(geometry)
        }
    }

    fn stroke_style(&self, pattern: &[f32]) -> windows_core::Result<ID2D1StrokeStyle> {
        let properties = D2D1_STROKE_STYLE_PROPERTIES1 {
            startCap: D2D1_CAP_STYLE_FLAT,
            endCap: D2D1_CAP_STYLE_FLAT,
            dashCap: D2D1_CAP_STYLE_FLAT,
            lineJoin: D2D1_LINE_JOIN_MITER,
            // 他の 3 つの既定と同じ。
            miterLimit: 10.0,
            dashStyle: if pattern.is_empty() {
                D2D1_DASH_STYLE_SOLID
            } else {
                D2D1_DASH_STYLE_CUSTOM
            },
            dashOffset: 0.0,
            // 線の太さも変換に従う (論理画素で描くための DPI の倍率を受ける)。
            transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
        };
        let dashes = (!pattern.is_empty()).then_some(pattern);
        // SAFETY: 手元の設定と破線の並びを渡すだけ。
        unsafe { self.factory.CreateStrokeStyle(&properties, dashes)?.cast() }
    }

    /// Draws `commands` onto a Composition drawing surface of `pixels` physical pixels
    /// covering `width` × `height` logical pixels at `scale`.
    ///
    /// `context` and `offset` are what the surface's `BeginDraw` returned. The surface
    /// may sit inside a texture shared with others, starting `offset` pixels in, so
    /// drawing is moved there and kept inside the surface.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_on_surface(
        &mut self,
        context: &ID2D1DeviceContext,
        offset: (i32, i32),
        pixels: (u32, u32),
        scale: f64,
        width: f64,
        height: f64,
        commands: &[DrawCommand],
    ) -> windows_core::Result<()> {
        let dpi = (96.0 * scale) as f32;
        // SAFETY: `BeginDraw` 済みのコンテキストの状態を整えるだけ。
        unsafe {
            context.SetDpi(dpi, dpi);
            context.SetTransform(&Matrix3x2::translation(
                (f64::from(offset.0) / scale) as f32,
                (f64::from(offset.1) / scale) as f32,
            ));
        }
        let (surface, logical) = surface_rects(pixels, scale, width, height);
        self.clear_and_draw(context, Some(surface), logical, commands)
    }

    /// Clears `surface` (all of the target when `None`) to transparent, then draws
    /// `commands` clipped to `logical`.
    ///
    /// Surfaces are whole pixels, rounded up from the logical size, so the last
    /// column and row can lie partly outside `logical`. They are still shown, so they
    /// are cleared too; otherwise they keep whatever the texture held before.
    fn clear_and_draw(
        &mut self,
        context: &ID2D1DeviceContext,
        surface: Option<D2D_RECT_F>,
        logical: D2D_RECT_F,
        commands: &[DrawCommand],
    ) -> windows_core::Result<()> {
        // SAFETY: 描画中のコンテキストで、積んだ切り抜きは必ず同じ数だけ戻す。
        unsafe {
            if let Some(surface) = &surface {
                context.PushAxisAlignedClip(surface, D2D1_ANTIALIAS_MODE_ALIASED);
            }
            context.Clear(Some(&color_f(Color::BLACK, 0.0)));
            context.PushAxisAlignedClip(&logical, D2D1_ANTIALIAS_MODE_ALIASED);
            let drawn = self.draw(context, commands);
            context.PopAxisAlignedClip();
            if surface.is_some() {
                context.PopAxisAlignedClip();
            }
            drawn
        }
    }

    /// The widest and tallest surface the current device can hold, in pixels.
    pub(super) fn max_surface_pixels(&self) -> u32 {
        self.devices.max_surface
    }

    /// Draws `commands` into an offscreen bitmap of `width` × `height` logical pixels at
    /// `scale` (lowered as on screen when it would not fit, see [`surface_size`]), and
    /// returns its size in pixels and its premultiplied BGRA pixels.
    pub(super) fn pixels(
        &mut self,
        width: f64,
        height: f64,
        scale: f64,
        commands: &[DrawCommand],
    ) -> windows_core::Result<(u32, u32, Vec<u8>)> {
        let (scale, (pixel_width, pixel_height)) =
            surface_size(width, height, scale, self.max_surface_pixels());
        let size = D2D_SIZE_U {
            width: pixel_width,
            height: pixel_height,
        };
        let dpi = (96.0 * scale) as f32;
        let properties = |options| D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi,
            dpiY: dpi,
            bitmapOptions: options,
            colorContext: core::mem::ManuallyDrop::new(None),
        };
        // SAFETY: 手元で作ったビットマップへ描き、読み取り用へ写してから読む。
        unsafe {
            let context = self
                .devices
                .d2d
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let target =
                context.CreateBitmap(size, None, 0, &properties(D2D1_BITMAP_OPTIONS_TARGET))?;
            context.SetTarget(&target);
            context.SetDpi(dpi, dpi);
            context.BeginDraw();
            let (_, logical) = surface_rects((pixel_width, pixel_height), scale, width, height);
            let drawn = self.clear_and_draw(&context, None, logical, commands);
            context.EndDraw(None, None)?;
            drawn?;
            let readable = context.CreateBitmap(
                size,
                None,
                0,
                &properties(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW),
            )?;
            readable.CopyFromBitmap(None, &target, None)?;
            let mapped = readable.Map(D2D1_MAP_OPTIONS_READ)?;
            let row = pixel_width as usize * 4;
            let mut pixels = Vec::with_capacity(row * pixel_height as usize);
            for y in 0..pixel_height as usize {
                let start = mapped.bits.add(y * mapped.pitch as usize);
                pixels.extend_from_slice(std::slice::from_raw_parts(start, row));
            }
            readable.Unmap()?;
            Ok((pixel_width, pixel_height, pixels))
        }
    }
}

/// Whether `error` means the device was lost and must be made again.
pub(super) fn is_device_lost(error: &windows_core::Error) -> bool {
    [
        DXGI_ERROR_DEVICE_REMOVED,
        DXGI_ERROR_DEVICE_RESET,
        D2DERR_RECREATE_TARGET,
    ]
    .contains(&error.code())
}

/// The scale to draw a `width` × `height` surface at, and its size in pixels, on a
/// device whose surfaces are at most `max` pixels wide and tall.
///
/// Normally the display scale, so that one surface pixel is one screen pixel. A
/// surface that would not fit is drawn at a lower scale instead, so it is still
/// shown whole, stretched and a little soft, rather than failing to be made.
pub(super) fn surface_size(width: f64, height: f64, scale: f64, max: u32) -> (f64, (u32, u32)) {
    let mut fit = scale;
    for length in [width, height] {
        if length > 0.0 && length * scale > f64::from(max) {
            fit = fit.min(f64::from(max) / length);
        }
    }
    let pixels = (
        pixel_extent(width, fit, max),
        pixel_extent(height, fit, max),
    );
    (fit, pixels)
}

/// The number of pixels covering `length` logical pixels at `scale`. At least 1, and
/// at most `max`.
fn pixel_extent(length: f64, scale: f64, max: u32) -> u32 {
    let pixels = (length * scale).ceil();
    if pixels.is_finite() {
        pixels.clamp(1.0, f64::from(max)) as u32
    } else {
        1
    }
}

/// The rectangles, in logical pixels, of a surface of `pixels` physical pixels at
/// `scale` (all of it, to clear) and of the `width` × `height` drawn on it (to clip
/// drawing to).
fn surface_rects(
    pixels: (u32, u32),
    scale: f64,
    width: f64,
    height: f64,
) -> (D2D_RECT_F, D2D_RECT_F) {
    let rect = |right: f64, bottom: f64| D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: right as f32,
        bottom: bottom as f32,
    };
    (
        rect(f64::from(pixels.0) / scale, f64::from(pixels.1) / scale),
        rect(width, height),
    )
}

/// The first of [`FONT_FAMILIES`] installed.
fn pick_font_family(dwrite: &IDWriteFactory) -> &'static str {
    let mut collection = None;
    // SAFETY: システムのフォント一覧を取り、名前を引くだけ。
    let found = unsafe {
        dwrite
            .GetSystemFontCollection(&mut collection, false)
            .ok()
            .and(collection)
            .and_then(|collection| {
                FONT_FAMILIES.into_iter().find(|name| {
                    let mut index = 0;
                    let mut exists = windows_core::BOOL::default();
                    collection
                        .FindFamilyName(&HSTRING::from(*name), &mut index, &mut exists)
                        .is_ok()
                        && exists.as_bool()
                })
            })
    };
    found.unwrap_or(FONT_FAMILIES[FONT_FAMILIES.len() - 1])
}

/// The user's locale name (`ja-JP` and so on). It decides which glyph forms CJK text
/// falls back to. Empty if it cannot be read.
fn user_locale() -> HSTRING {
    // `LOCALE_NAME_MAX_LENGTH`.
    let mut name = [0u16; 85];
    // SAFETY: 手元の配列へ書かせるだけ。
    let length = unsafe { GetUserDefaultLocaleName(&mut name) };
    let length = usize::try_from(length).unwrap_or(0).saturating_sub(1);
    HSTRING::from_wide(&name[..length])
}

/// Where text starts horizontally: `at_x` is the left edge, the center or the right
/// edge depending on `align`.
fn text_left(at_x: f64, width: f64, align: Align) -> f64 {
    match align {
        Align::Center => at_x - width / 2.0,
        Align::End => at_x - width,
        _ => at_x,
    }
}

/// Text is one line on every platform, so line breaks and tabs become spaces.
fn single_line(text: &str) -> String {
    text.chars()
        .map(|c| {
            if matches!(c, '\n' | '\r' | '\t') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// Direct2D's dash lengths are **in units of the line width**, so the logical-pixel
/// pattern is divided by it.
fn dash_pattern(dash: &[f64], width: f64) -> Vec<f32> {
    dash.iter().map(|v| finite(v / width)).collect()
}

/// A coordinate Direct2D can take. Infinities and NaN put a geometry in an error
/// state, so they become 0.
fn finite(value: f64) -> f32 {
    let value = value as f32;
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

fn point(p: Point) -> Vector2 {
    Vector2::new(finite(p.x), finite(p.y))
}

/// The color with the opacity as alpha (straight, not premultiplied).
fn color_f(color: Color, opacity: f64) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: opacity.clamp(0.0, 1.0) as f32,
    }
}

/// The rectangle `path` traces, if it is one rectangle with sides along the axes
/// (what [`Path::rect`] makes). Filling it directly gives the same pixels as filling
/// the geometry, without building one.
fn axis_aligned_rect(path: &Path) -> Option<D2D_RECT_F> {
    let [PathSegment::MoveTo(a), PathSegment::LineTo(b), PathSegment::LineTo(c), PathSegment::LineTo(d), PathSegment::Close] =
        *path.segments()
    else {
        return None;
    };
    let horizontal_first = a.y == b.y && b.x == c.x && c.y == d.y && d.x == a.x;
    let vertical_first = a.x == b.x && b.y == c.y && c.x == d.x && d.y == a.y;
    if !(horizontal_first || vertical_first) {
        return None;
    }
    let (left, right) = (finite(a.x.min(c.x)), finite(a.x.max(c.x)));
    let (top, bottom) = (finite(a.y.min(c.y)), finite(a.y.max(c.y)));
    Some(D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    })
}

/// The figure-by-figure form of a path that Direct2D's geometry sink takes.
pub(super) trait FigureSink {
    fn begin(&mut self, at: Point);
    fn line(&mut self, to: Point);
    fn cubic(&mut self, control1: Point, control2: Point, to: Point);
    fn end(&mut self, closed: bool);
}

/// Turns a [`Path`] into figures.
///
/// Direct2D needs every figure opened at a point and ended, while a `Path` (like the
/// other platforms' 2D APIs) only marks where subpaths start. A line or curve right
/// after `Close` starts a new figure at the point the closed one began, as on the other
/// three platforms.
pub(super) fn replay(path: &Path, sink: &mut impl FigureSink) {
    let mut open = false;
    let mut start = Point::new(0.0, 0.0);
    for segment in path.segments() {
        match *segment {
            PathSegment::MoveTo(p) => {
                if open {
                    sink.end(false);
                }
                sink.begin(p);
                open = true;
                start = p;
            }
            PathSegment::LineTo(p) => {
                if !open {
                    sink.begin(start);
                    open = true;
                }
                sink.line(p);
            }
            PathSegment::CubicTo {
                control1,
                control2,
                to,
            } => {
                if !open {
                    sink.begin(start);
                    open = true;
                }
                sink.cubic(control1, control2, to);
            }
            PathSegment::Close => {
                if open {
                    sink.end(true);
                    open = false;
                }
            }
        }
    }
    if open {
        sink.end(false);
    }
}

struct D2dSink<'a>(&'a ID2D1GeometrySink);

impl FigureSink for D2dSink<'_> {
    fn begin(&mut self, at: Point) {
        // SAFETY: 開いているシンクへ書くだけ。
        unsafe { self.0.BeginFigure(point(at), D2D1_FIGURE_BEGIN_FILLED) };
    }

    fn line(&mut self, to: Point) {
        // SAFETY: 同上。
        unsafe { self.0.AddLine(point(to)) };
    }

    fn cubic(&mut self, control1: Point, control2: Point, to: Point) {
        let bezier = D2D1_BEZIER_SEGMENT {
            point1: point(control1),
            point2: point(control2),
            point3: point(to),
        };
        // SAFETY: 同上。
        unsafe { self.0.AddBezier(&bezier) };
    }

    fn end(&mut self, closed: bool) {
        let end = if closed {
            D2D1_FIGURE_END_CLOSED
        } else {
            D2D1_FIGURE_END_OPEN
        };
        // SAFETY: 同上。
        unsafe { self.0.EndFigure(end) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Direct3D::{
        D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_12_1, D3D_FEATURE_LEVEL_9_1,
        D3D_FEATURE_LEVEL_9_2,
    };

    #[derive(Default)]
    struct Recorder(Vec<String>);

    impl FigureSink for Recorder {
        fn begin(&mut self, at: Point) {
            self.0.push(format!("M{},{}", at.x, at.y));
        }
        fn line(&mut self, to: Point) {
            self.0.push(format!("L{},{}", to.x, to.y));
        }
        fn cubic(&mut self, c1: Point, c2: Point, to: Point) {
            self.0.push(format!(
                "C{},{} {},{} {},{}",
                c1.x, c1.y, c2.x, c2.y, to.x, to.y
            ));
        }
        fn end(&mut self, closed: bool) {
            self.0.push(if closed { "Z" } else { "E" }.to_string());
        }
    }

    fn figures(path: &Path) -> String {
        let mut recorder = Recorder::default();
        replay(path, &mut recorder);
        recorder.0.join(" ")
    }

    #[test]
    fn subpaths_become_figures() {
        let path = Path::new()
            .move_to(Point::new(1.0, 2.0))
            .line_to(Point::new(3.5, 4.0))
            .cubic_to(
                Point::new(0.0, 0.0),
                Point::new(1.0, 1.0),
                Point::new(2.0, 2.0),
            )
            .move_to(Point::new(9.0, 9.0))
            .line_to(Point::new(8.0, 8.0));
        assert_eq!(
            figures(&path),
            "M1,2 L3.5,4 C0,0 1,1 2,2 E M9,9 L8,8 E",
            "次の move_to と最後で開いたまま終える"
        );
    }

    #[test]
    fn drawing_after_close_starts_at_the_closed_start() {
        let path = Path::new()
            .move_to(Point::new(1.0, 1.0))
            .line_to(Point::new(5.0, 1.0))
            .close()
            .line_to(Point::new(1.0, 5.0))
            .close()
            .close();
        assert_eq!(figures(&path), "M1,1 L5,1 Z M1,1 L1,5 Z");
    }

    #[test]
    fn only_axis_aligned_rects_take_the_short_cut() {
        let rect = axis_aligned_rect(&Path::new().rect(naui_core::Rect::new(1.0, 2.0, 3.0, 4.0)))
            .expect("Path::rect は長方形");
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (1.0, 2.0, 4.0, 6.0)
        );
        let flipped = Path::new()
            .move_to(Point::new(4.0, 6.0))
            .line_to(Point::new(4.0, 2.0))
            .line_to(Point::new(1.0, 2.0))
            .line_to(Point::new(1.0, 6.0))
            .close();
        assert!(
            axis_aligned_rect(&flipped).is_some(),
            "逆回りで縦から始めても長方形"
        );
        let diamond = Path::new().polygon(&[
            Point::new(1.0, 0.0),
            Point::new(2.0, 1.0),
            Point::new(1.0, 2.0),
            Point::new(0.0, 1.0),
        ]);
        assert!(axis_aligned_rect(&diamond).is_none());
        let open = Path::new()
            .move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(1.0, 0.0))
            .line_to(Point::new(1.0, 1.0))
            .line_to(Point::new(0.0, 1.0));
        assert!(
            axis_aligned_rect(&open).is_none(),
            "閉じていなければ形が変わりうるので使わない"
        );
    }

    #[test]
    fn dashes_are_in_units_of_the_line_width() {
        assert_eq!(dash_pattern(&[4.0, 2.0], 2.0), [2.0, 1.0]);
        assert!(dash_pattern(&[], 3.0).is_empty());
    }

    #[test]
    fn numbers_stay_drawable() {
        assert_eq!(finite(1.5), 1.5);
        assert_eq!(finite(f64::NAN), 0.0);
        assert_eq!(finite(f64::INFINITY), 0.0);
        assert_eq!(color_f(Color::rgb(255, 0, 0), 2.0).a, 1.0);
        assert_eq!(color_f(Color::rgb(255, 0, 0), 0.5).r, 1.0);
    }

    #[test]
    fn clearing_covers_the_rounded_up_pixels() {
        // 幅 100.1・倍率 2 は 201 列。最後の列は描く範囲 (200.2 列目まで) から
        // はみ出すが、表示されるので消す範囲には入れる。
        let (scale, pixels) = surface_size(100.1, 50.0, 2.0, 16384);
        assert_eq!((scale, pixels), (2.0, (201, 100)));
        let (surface, logical) = surface_rects(pixels, 2.0, 100.1, 50.0);
        assert_eq!(
            f64::from(surface.right) * 2.0,
            201.0,
            "消すのは確保した 201 列すべて"
        );
        assert_eq!(f64::from(surface.bottom) * 2.0, 100.0);
        assert!((logical.right - 100.1).abs() < 1e-4, "描くのは論理の幅まで");
        assert!(surface.right > logical.right);
    }

    #[test]
    fn surfaces_cover_whole_pixels() {
        assert_eq!(pixel_extent(100.0, 1.0, 16384), 100);
        assert_eq!(pixel_extent(100.5, 1.5, 16384), 151);
        assert_eq!(pixel_extent(0.0, 1.0, 16384), 1, "0 の面は作れないので 1");
        assert_eq!(pixel_extent(1.0e9, 2.0, 16384), 16384);
        assert_eq!(pixel_extent(f64::NAN, 1.0, 16384), 1);
    }

    #[test]
    fn surfaces_too_big_for_the_device_are_drawn_at_a_lower_scale() {
        // 上限 2048 の機器で、幅 1500 を倍率 2 で描くと 3000 画素になって作れない。
        // 倍率を下げて 2048 画素に収め、見た目は同じ幅に引き伸ばす。
        let (scale, pixels) = surface_size(1500.0, 300.0, 2.0, 2048);
        assert!((scale - 2048.0 / 1500.0).abs() < 1e-9, "倍率 {scale}");
        assert_eq!(pixels, (2048, (300.0 * scale).ceil() as u32));
        assert!(
            (f64::from(pixels.0) / scale - 1500.0).abs() < 1.0,
            "見た目の幅は論理の幅のまま"
        );
        // 縦と横で厳しいほうに合わせる (倍率は 1 つ)。
        let (scale, pixels) = surface_size(1000.0, 3000.0, 1.5, 4096);
        assert!((scale - 4096.0 / 3000.0).abs() < 1e-9);
        assert_eq!(pixels.1, 4096);
        assert!(pixels.0 <= 4096);
        // 収まるならそのまま。
        assert_eq!(surface_size(800.0, 600.0, 1.25, 2048), (1.25, (1000, 750)));
    }

    #[test]
    fn the_surface_limit_follows_the_feature_level() {
        assert_eq!(max_texture_size(D3D_FEATURE_LEVEL_12_1), 16384);
        assert_eq!(max_texture_size(D3D_FEATURE_LEVEL_11_0), 16384);
        assert_eq!(max_texture_size(D3D_FEATURE_LEVEL_10_1), 8192);
        assert_eq!(max_texture_size(D3D_FEATURE_LEVEL_10_0), 8192);
        assert_eq!(max_texture_size(D3D_FEATURE_LEVEL_9_3), 4096);
        assert_eq!(max_texture_size(D3D_FEATURE_LEVEL_9_2), 2048);
        assert_eq!(max_texture_size(D3D_FEATURE_LEVEL_9_1), 2048);
    }

    #[test]
    fn text_aligns_by_its_width() {
        assert_eq!(text_left(50.0, 20.0, Align::Start), 50.0);
        assert_eq!(text_left(50.0, 20.0, Align::Center), 40.0);
        assert_eq!(text_left(50.0, 20.0, Align::End), 30.0);
        assert_eq!(single_line("a\nb\tc"), "a b c");
    }
}
