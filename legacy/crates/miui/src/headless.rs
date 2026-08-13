//! ウィンドウを開かずに UI をピクセルバッファへ描く。
//!
//! スクリーンショットの生成、見た目の回帰テスト、CI での検証に使う。
//! ランタイムと同じ「組み立て → レイアウト → 描画」を通るので、
//! 実際のウィンドウに出るものと同じ結果になる。

use std::collections::HashSet;

use miui_core::event::Event;
use miui_core::geometry::{Point, Size};
use miui_core::layout::BoxConstraints;
use miui_core::theme::Theme;
use miui_core::widget::{EventCx, Interaction, LayoutCx, PaintCx, StateStore};
use miui_render::{Canvas, Fonts};

use crate::app::Application;

/// ヘッドレス描画のためのコンテキスト。
pub struct Headless {
    pub fonts: Fonts,
    pub store: StateStore,
    pub interaction: Interaction,
}

impl Default for Headless {
    fn default() -> Self {
        Self::new()
    }
}

impl Headless {
    /// OS 標準フォントを読み込んで初期化する。
    pub fn new() -> Self {
        Self {
            fonts: Fonts::with_system_fonts(),
            store: StateStore::new(),
            interaction: Interaction::default(),
        }
    }

    pub fn with_fonts(fonts: Fonts) -> Self {
        Self {
            fonts,
            store: StateStore::new(),
            interaction: Interaction::default(),
        }
    }

    /// イベントを 1 つ流し込み、生じたメッセージをアプリへ適用する。
    ///
    /// 座標は論理ピクセル。`size` は現在のウィンドウサイズ (論理)。
    pub fn dispatch<A: Application>(
        &mut self,
        app: &mut A,
        theme: &Theme,
        size: Size,
        event: &Event,
    ) -> usize {
        let mut dummy = vec![0u32; 4];
        let mut canvas = Canvas::new(&mut dummy, 2, 2, 1.0, &mut self.fonts);
        let mut tree = app.view();
        let mut alive = HashSet::new();
        let mut focusables = Vec::new();
        {
            let mut cx = LayoutCx::new(
                &mut canvas,
                theme,
                &mut self.store,
                &mut alive,
                &mut focusables,
                1.0,
            );
            tree.layout(&mut cx, BoxConstraints::tight(size));
        }
        if matches!(event, Event::PointerMoved(_)) {
            self.interaction.hovered = None;
        }
        let mut messages = Vec::new();
        {
            let mut cx = EventCx::new(
                theme,
                &mut self.interaction,
                &mut canvas,
                &mut self.store,
                &mut messages,
            );
            tree.event(&mut cx, event, Point::ZERO);
        }
        let n = messages.len();
        for m in messages {
            app.update(m);
        }
        n
    }

    /// `width` x `height` 物理ピクセルのバッファ (0x00RRGGBB) を返す。
    pub fn render<A: Application>(
        &mut self,
        app: &A,
        theme: &Theme,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Vec<u32> {
        let mut buffer = vec![0u32; (width * height) as usize];
        let logical = Size::new(width as f32 / scale, height as f32 / scale);
        let mut canvas = Canvas::new(
            &mut buffer,
            width as usize,
            height as usize,
            scale,
            &mut self.fonts,
        );

        let mut tree = app.view();
        let mut alive = HashSet::new();
        let mut focusables = Vec::new();
        {
            let mut cx = LayoutCx::new(
                &mut canvas,
                theme,
                &mut self.store,
                &mut alive,
                &mut focusables,
                scale,
            );
            tree.layout(&mut cx, BoxConstraints::tight(logical));
        }
        self.store.retain_alive(&alive);

        canvas.clear(theme.color.window_bg);
        {
            let mut cx = PaintCx {
                painter: &mut canvas,
                theme,
                interaction: &self.interaction,
                store: &self.store,
            };
            tree.paint(&mut cx, Point::ZERO);
        }
        drop(canvas);
        buffer
    }
}

/// バッファを BMP (24bpp 無圧縮) としてエンコードする。
///
/// 外部クレートを増やさずに結果を目視できるようにするための最小実装。
pub fn to_bmp(buffer: &[u32], width: u32, height: u32) -> Vec<u8> {
    let row_bytes = (width * 3).div_ceil(4) * 4;
    let pixel_bytes = row_bytes * height;
    let file_size = 54 + pixel_bytes;
    let mut out = Vec::with_capacity(file_size as usize);

    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    // BITMAPINFOHEADER
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    // 正の高さ = ボトムアップ。
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&pixel_bytes.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    for y in (0..height).rev() {
        let mut written = 0u32;
        for x in 0..width {
            let px = buffer[(y * width + x) as usize];
            out.push((px & 0xFF) as u8);
            out.push(((px >> 8) & 0xFF) as u8);
            out.push(((px >> 16) & 0xFF) as u8);
            written += 3;
        }
        while written < row_bytes {
            out.push(0);
            written += 1;
        }
    }
    out
}
