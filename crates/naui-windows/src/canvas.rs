//! 描画面 (WinUI 3 の `Canvas` に `Path` と `TextBlock` を置いたもの)。
//!
//! WinUI 3 に「アプリが画素を描く面」はない。Win2D は Windows App SDK では
//! なく別パッケージで、`SwapChainPanel` は DirectX を自前で回す口なので、
//! XAML が持つ**図形の要素** (`Microsoft.UI.Xaml.Shapes.Path` と
//! `TextBlock`) を `Canvas` へ置く形で組み立てる。塗り・線・文字は
//! すべて XAML のレンダラー (Composition) が描くので、アンチエイリアス・
//! 表示倍率・ダークテーマの文字はほかのコントロールと同じ品質になる。
//!
//! `on_draw` が [`Painter`] に記録した命令は、描き直しのたびに 1 つの XAML
//! 文字列 (`<Canvas>` とその子) にして `XamlReader` へ渡し、できた要素で
//! 前の面を丸ごと置き換える。`Path` の形は XAML のパスの記法
//! (`M` / `L` / `C` / `Z`) で渡し、`Path` や `PathGeometry` の型を投影に
//! 足さなくて済むようにしている。
//!
//! 大きさが変わったことは `SizeChanged` で拾い、ポインターは `PointerPressed` /
//! `PointerMoved` / `PointerReleased` から取る。

use std::cell::Cell;
use std::fmt::Write as _;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use naui_core::{
    Align, Color, DrawCommand, Painter, Path, PathSegment, Point, PointerEvent, PointerPhase,
    Result,
};
use naui_winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};
use naui_winui3::Microsoft::UI::Xaml::Controls::{Canvas as XamlCanvas, Grid as XamlGrid};
use naui_winui3::Microsoft::UI::Xaml::Input::PointerEventHandler;
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{FrameworkElement, SizeChangedEventHandler, UIElement};
use windows::Foundation::Size;
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// 面の土台。`Background="Transparent"` で、何も描いていない場所でも
/// ポインターの当たり判定が残る (`null` だと素通りする)。
const HOST_XAML: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"/>"##;

/// 描く内容を決めるクロージャの置き場。
///
/// WinRT のイベント (`SizeChanged`) から呼ぶので [`UiThreadCell`] に載せ、
/// 呼んでいる間は取り出しておく (`ColorHandler` などと同じ決まり)。
#[derive(Clone)]
struct DrawHandler(HandlerCell<dyn FnMut(&mut Painter)>);

impl DrawHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(&mut Painter) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    /// 呼んで、描いたかどうかを返す。まだ設定されていなければ `false`。
    fn emit(&self, painter: &mut Painter) -> bool {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return false;
        };
        f(painter);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
        true
    }
}

#[derive(Clone)]
struct PointerHandler(HandlerCell<dyn FnMut(PointerEvent)>);

impl PointerHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, event: PointerEvent) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(event);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct CanvasInner {
    native: XamlGrid,
    draw: DrawHandler,
    pointer: PointerHandler,
    /// 描き直しを `DispatcherQueue` へ積んであるかどうか。
    scheduled: Cell<bool>,
}

/// アプリが自分で描く面 (`Grid` + `Canvas` + `Path` / `TextBlock`)。
///
/// 中身から大きさは決まらないので、`set_sizing` で指定する (`Scroll` と同じ)。
#[derive(Clone)]
pub struct Canvas(Rc<CanvasInner>);
impl_widget!(Canvas, native);

impl Canvas {
    pub(crate) fn new() -> Result<Self> {
        let native: XamlGrid = XamlReader::Load(&HSTRING::from(HOST_XAML))
            .and_then(|element| element.cast::<XamlGrid>())
            .map_err(|e| to_error("Canvas の生成", e))?;
        let this = Self(Rc::new(CanvasInner {
            native,
            draw: DrawHandler::new(),
            pointer: PointerHandler::new(),
            scheduled: Cell::new(false),
        }));
        this.track_size()?;
        this.track_pointer()?;
        Ok(this)
    }

    /// 大きさが決まった (変わった) ら描き直す。最初の描画もここから始まる。
    fn track_size(&self) -> Result<()> {
        let target = UiThreadCell::new(Rc::downgrade(&self.0));
        let changed = SizeChangedEventHandler::new(move |_, _| {
            let _ = target.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    inner.paint();
                }
            });
            Ok(())
        });
        self.0
            .native
            .cast::<FrameworkElement>()
            .and_then(|element| element.SizeChanged(&changed))
            .map_err(|e| to_error("Canvas の大きさの購読", e))?;
        Ok(())
    }

    fn track_pointer(&self) -> Result<()> {
        let element = self
            .0
            .native
            .cast::<UIElement>()
            .map_err(|e| to_error("Canvas のポインター購読", e))?;
        for phase in [PointerPhase::Down, PointerPhase::Move, PointerPhase::Up] {
            let target = UiThreadCell::new(Rc::downgrade(&self.0));
            let handler = PointerEventHandler::new(move |_, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let _ = target.try_with_mut(|weak: &mut Weak<CanvasInner>| {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let Ok(root) = inner.native.cast::<UIElement>() else {
                        return;
                    };
                    // 押している間は面の外へ出ても動きと解放が届くようにする。
                    if let Ok(pointer) = args.Pointer() {
                        match phase {
                            PointerPhase::Down => {
                                let _ = root.CapturePointer(&pointer);
                            }
                            PointerPhase::Up => {
                                let _ = root.ReleasePointerCapture(&pointer);
                            }
                            PointerPhase::Move => {}
                        }
                    }
                    let Ok(position) = args
                        .GetCurrentPoint(&root)
                        .and_then(|point| point.Position())
                    else {
                        return;
                    };
                    inner.pointer.emit(PointerEvent::new(
                        phase,
                        Point::new(f64::from(position.X), f64::from(position.Y)),
                    ));
                });
                Ok(())
            });
            match phase {
                PointerPhase::Down => element.PointerPressed(&handler),
                PointerPhase::Move => element.PointerMoved(&handler),
                PointerPhase::Up => element.PointerReleased(&handler),
            }
            .map_err(|e| to_error("Canvas のポインター購読", e))?;
        }
        Ok(())
    }

    /// 描く内容を決めるクロージャ。描き直すたびに新しい [`Painter`] で呼ばれる。
    ///
    /// 呼ばれるのは [`redraw`](Self::redraw) の後と、面の大きさが変わったとき。
    pub fn on_draw(&self, f: impl FnMut(&mut Painter) + 'static) {
        self.0.draw.set(f);
        self.redraw();
    }

    /// 描き直しを頼む。その場では描かず、UI スレッドの手が空いてから
    /// `on_draw` が呼ばれる (続けて何度呼んでも 1 回にまとまる)。
    pub fn redraw(&self) {
        if self.0.scheduled.replace(true) {
            return;
        }
        let Ok(queue) = DispatcherQueue::GetForCurrentThread() else {
            self.0.scheduled.set(false);
            self.0.paint();
            return;
        };
        let target = UiThreadCell::new(Rc::downgrade(&self.0));
        let enqueued = queue.TryEnqueue(&DispatcherQueueHandler::new(move || {
            let _ = target.try_with_mut(|weak| {
                if let Some(inner) = weak.upgrade() {
                    inner.scheduled.set(false);
                    inner.paint();
                }
            });
            Ok(())
        }));
        if !matches!(enqueued, Ok(true)) {
            self.0.scheduled.set(false);
            self.0.paint();
        }
    }

    /// ポインター (マウス・タッチ・ペン) の押下・移動・解放。位置は面の左上を
    /// 原点にした論理ピクセル。移動は押していない間 (ホバー) も届く。
    pub fn on_pointer(&self, f: impl FnMut(PointerEvent) + 'static) {
        self.0.pointer.set(f);
    }

    /// いまの面の大きさ (幅, 高さ)。まだレイアウトされていなければ 0。
    pub fn size(&self) -> (f64, f64) {
        self.0.actual_size()
    }

    /// その場で描く。**自動テスト専用**。
    #[doc(hidden)]
    pub fn draw_for_test(&self) {
        self.0.paint();
    }

    /// `on_draw` の命令を、指定した大きさの面の XAML にして返す。**自動テスト専用**。
    ///
    /// 画面に出さなくても、命令が XAML の図形へどう写るかを確かめられる。
    #[doc(hidden)]
    pub fn xaml_for_test(&self, width: f64, height: f64) -> String {
        let mut painter = Painter::new(width, height);
        self.0.draw.emit(&mut painter);
        scene_xaml(width, height, painter.commands())
    }

    /// 土台の `Grid`。バックエンド固有の脱出口として公開している。
    ///
    /// 描いたものは、この中に 1 つだけ入る `Canvas` の子として置かれる。
    pub fn native_grid(&self) -> XamlGrid {
        self.0.native.clone()
    }
}

impl CanvasInner {
    fn actual_size(&self) -> (f64, f64) {
        let Ok(element) = self.native.cast::<FrameworkElement>() else {
            return (0.0, 0.0);
        };
        (
            element.ActualWidth().unwrap_or(0.0),
            element.ActualHeight().unwrap_or(0.0),
        )
    }

    /// `on_draw` を呼び、その結果の XAML で面を置き換える。
    fn paint(&self) {
        let (width, height) = self.actual_size();
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let mut painter = Painter::new(width, height);
        if !self.draw.emit(&mut painter) {
            return;
        }
        let xaml = scene_xaml(width, height, painter.commands());
        let Ok(scene) = XamlReader::Load(&HSTRING::from(xaml)).and_then(|e| e.cast::<UIElement>())
        else {
            return;
        };
        align_text(&scene, painter.commands());
        if let Ok(children) = self.native.Children() {
            let _ = children.Clear();
            let _ = children.Append(&scene);
        }
    }
}

/// 面全体の XAML。`Canvas` は子を置くだけで自分の大きさを持たないので、
/// 幅と高さを与え、はみ出した分は `Clip` で切る。
fn scene_xaml(width: f64, height: f64, commands: &[DrawCommand]) -> String {
    let mut xaml = String::new();
    let _ = write!(
        xaml,
        r##"<Canvas xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" Width="{w}" Height="{h}" IsHitTestVisible="False"><Canvas.Clip><RectangleGeometry Rect="0,0,{w},{h}"/></Canvas.Clip>"##,
        w = number(width),
        h = number(height),
    );
    for (index, command) in commands.iter().enumerate() {
        match command {
            DrawCommand::Fill {
                path,
                color,
                opacity,
            } => {
                let _ = write!(
                    xaml,
                    r##"<Path Fill="{}" Data="{}"/>"##,
                    argb(*color, *opacity),
                    path_data(path),
                );
            }
            DrawCommand::Stroke {
                path,
                color,
                width,
                dash,
                opacity,
            } => {
                let _ = write!(
                    xaml,
                    r##"<Path Stroke="{}" StrokeThickness="{}" StrokeLineJoin="Miter" StrokeStartLineCap="Flat" StrokeEndLineCap="Flat""##,
                    argb(*color, *opacity),
                    number(*width),
                );
                if !dash.is_empty() {
                    // XAML の破線は**線の太さを 1 とする単位**で指定する。
                    let pattern: Vec<String> = dash.iter().map(|v| number(v / width)).collect();
                    let _ = write!(xaml, r##" StrokeDashArray="{}""##, pattern.join(" "));
                }
                let _ = write!(xaml, r##" Data="{}"/>"##, path_data(path));
            }
            DrawCommand::Text {
                text,
                at,
                size,
                color,
                opacity,
                ..
            } => {
                let _ = write!(
                    xaml,
                    r##"<TextBlock x:Name="{}" Canvas.Left="{}" Canvas.Top="{}" FontSize="{}" Foreground="{}" TextWrapping="NoWrap" Text="{}"/>"##,
                    text_name(index),
                    number(at.x),
                    number(at.y),
                    number(*size),
                    argb(*color, *opacity),
                    escape(text),
                );
            }
        }
    }
    xaml.push_str("</Canvas>");
    xaml
}

/// 中央・右寄せの文字を、測った幅のぶんだけ左へずらす。
///
/// XAML の `Canvas` は左上で置くことしかできないので、読み込んだ
/// `TextBlock` を測って `Canvas.Left` を書き直す。
fn align_text(scene: &UIElement, commands: &[DrawCommand]) {
    let Ok(root) = scene.cast::<FrameworkElement>() else {
        return;
    };
    for (index, command) in commands.iter().enumerate() {
        let DrawCommand::Text { at, align, .. } = command else {
            continue;
        };
        if matches!(align, Align::Start | Align::Fill) {
            continue;
        }
        let Ok(block) = root
            .FindName(&HSTRING::from(text_name(index)))
            .and_then(|value| value.cast::<UIElement>())
        else {
            continue;
        };
        let _ = block.Measure(Size {
            Width: f32::INFINITY,
            Height: f32::INFINITY,
        });
        let Ok(desired) = block.DesiredSize() else {
            continue;
        };
        let width = f64::from(desired.Width);
        let left = if matches!(align, Align::Center) {
            at.x - width / 2.0
        } else {
            at.x - width
        };
        let _ = XamlCanvas::SetLeft(&block, left);
    }
}

fn text_name(index: usize) -> String {
    format!("t{index}")
}

/// XAML のパスの記法。`F1` は nonzero (他の 3 環境の既定と同じ)。
fn path_data(path: &Path) -> String {
    let mut data = String::from("F1");
    for segment in path.segments() {
        match *segment {
            PathSegment::MoveTo(p) => {
                let _ = write!(data, " M{},{}", number(p.x), number(p.y));
            }
            PathSegment::LineTo(p) => {
                let _ = write!(data, " L{},{}", number(p.x), number(p.y));
            }
            PathSegment::CubicTo {
                control1,
                control2,
                to,
            } => {
                let _ = write!(
                    data,
                    " C{},{} {},{} {},{}",
                    number(control1.x),
                    number(control1.y),
                    number(control2.x),
                    number(control2.y),
                    number(to.x),
                    number(to.y),
                );
            }
            PathSegment::Close => data.push_str(" Z"),
        }
    }
    data
}

/// XAML へ書ける数。無限や NaN はパーサーが読めないので 0 にする。
fn number(value: f64) -> String {
    if value.is_finite() {
        format!("{value}")
    } else {
        "0".to_string()
    }
}

/// `#AARRGGBB`。透け具合はアルファに写す。
fn argb(color: Color, opacity: f64) -> String {
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        alpha, color.r, color.g, color.b
    )
}

/// 属性値として書けるように、XML の予約文字を実体参照に直す。
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' | '\r' | '\t' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use naui_core::Rect;

    #[test]
    fn path_data_uses_the_xaml_mini_language() {
        let path = Path::new()
            .move_to(Point::new(1.0, 2.0))
            .line_to(Point::new(3.5, 4.0))
            .cubic_to(
                Point::new(0.0, 0.0),
                Point::new(1.0, 1.0),
                Point::new(2.0, 2.0),
            )
            .close();
        assert_eq!(path_data(&path), "F1 M1,2 L3.5,4 C0,0 1,1 2,2 Z");
    }

    #[test]
    fn scene_carries_fill_stroke_and_text() {
        let mut painter = Painter::new(100.0, 50.0);
        painter.set_opacity(0.5);
        painter.fill_rect(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Color::rgb(0xff, 0x88, 0x00),
        );
        painter.set_opacity(1.0);
        painter.set_dash(&[4.0, 2.0]);
        painter.line(
            Point::new(0.0, 0.0),
            Point::new(9.0, 9.0),
            Color::BLACK,
            2.0,
        );
        painter.text("a<b>&\"c\"", Point::new(5.0, 6.0), 12.0, Color::WHITE);
        let xaml = scene_xaml(100.0, 50.0, painter.commands());
        assert!(xaml.starts_with("<Canvas "), "{xaml}");
        assert!(xaml.contains(r#"Width="100" Height="50""#));
        assert!(xaml.contains(r#"<RectangleGeometry Rect="0,0,100,50"/>"#));
        assert!(xaml.contains(r##"<Path Fill="#80FF8800" Data="F1 M0,0 L10,0 L10,10 L0,10 Z"/>"##));
        assert!(
            xaml.contains(r##"Stroke="#FF000000" StrokeThickness="2""##),
            "{xaml}"
        );
        assert!(
            xaml.contains(r#"StrokeDashArray="2 1""#),
            "破線は太さを 1 とする単位に直す: {xaml}"
        );
        assert!(
            xaml.contains(r#"Text="a&lt;b&gt;&amp;&quot;c&quot;""#),
            "{xaml}"
        );
        assert!(xaml.contains(r#"x:Name="t2""#));
        assert!(xaml.ends_with("</Canvas>"));
    }

    #[test]
    fn numbers_stay_parsable() {
        assert_eq!(number(1.5), "1.5");
        assert_eq!(number(-0.25), "-0.25");
        assert_eq!(number(f64::NAN), "0");
        assert_eq!(number(f64::INFINITY), "0");
        assert_eq!(argb(Color::rgb(1, 2, 3), 2.0), "#FF010203");
        assert_eq!(argb(Color::rgb(1, 2, 3), 0.0), "#00010203");
    }
}
