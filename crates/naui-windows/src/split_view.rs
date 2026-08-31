//! 分割ビュー (WinUI 3 の `Grid` + つまめる仕切り)。
//!
//! **WinUI 3 に「動かせる仕切りで区画を分ける」コントロールは無い。**
//! 名前の似た `SplitView` は開閉するナビゲーションのペインで、仕切りを
//! ドラッグして幅を変えるものではなく、`GridSplitter` は Windows App SDK
//! ではなく Community Toolkit の側にある。そこで Fluent のアプリが同じことを
//! するときと同じ形 — 3 つの列 (行) を持つ `Grid` の真ん中に仕切りを置く形 —
//! で組み立てる。
//!
//! 組み立てるのは**位置と当たり判定だけ**で、仕切りの色はテーマリソース
//! (`ControlStrokeColorDefaultBrush`) から引くので、ライト / ダークの
//! 切り替えにそのまま追従する。
//!
//! **見えるのは 1 px の線だけ**で、残りは透明なつかみ代になっている
//! (`Background="Transparent"` + 片側だけの `BorderThickness`)。塗りつぶしの
//! 帯にすると、区切りというより 1 つの部品のように見えて周りから浮くため。
//! `Transparent` は `null` と違って当たり判定が残るので、つかみ代としては
//! そのまま働く。
//!
//! 列 (行) の幅は `Pixel` / `Star` で決める。start 側は指定した大きさの
//! `Pixel`、end 側は残りを受け取る `Star` なので、ウィンドウが広がった分は
//! end 側が受け取る。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{clamp_split_position, Orientation, Result, DEFAULT_SPLIT_POSITION};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    ColumnDefinition, Grid as XamlGrid, RowDefinition,
};
use naui_winui3::Microsoft::UI::Xaml::Input::PointerEventHandler;
use naui_winui3::Microsoft::UI::Xaml::Markup::XamlReader;
use naui_winui3::Microsoft::UI::Xaml::{
    FrameworkElement, GridLength, GridUnitType, HorizontalAlignment, UIElement, VerticalAlignment,
};
use windows::Foundation::EventHandler;
use windows_core::{IInspectable, Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

/// 仕切りが占める太さ (論理ピクセル)。マウスでつまめる幅にしてある。
///
/// このうち見えるのは境目に引く 1 px の線だけで、残りは透明。
const DIVIDER_THICKNESS: f64 = 6.0;

/// 区画が横に並ぶときの仕切り。境目 (左端) にだけ線を引く。
const DIVIDER_XAML_HORIZONTAL: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"
    BorderThickness="1,0,0,0"
    BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"/>"##;

/// 区画が縦に並ぶときの仕切り。境目 (上端) にだけ線を引く。
const DIVIDER_XAML_VERTICAL: &str = r##"<Grid
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    Background="Transparent"
    BorderThickness="0,1,0,0"
    BorderBrush="{ThemeResource ControlStrokeColorDefaultBrush}"/>"##;

/// 仕切りが動いたことの通知先。
///
/// 呼ぶ間はセルから取り出しておく ([`SelectHandler`](crate::widgets) と同じ)。
/// 通知の中から同じ分割ビューを操作しても、借用が衝突しない。
type ResizeCallback = Box<dyn FnMut(f64)>;

#[derive(Clone)]
struct ResizeHandler(Arc<UiThreadCell<Option<ResizeCallback>>>);

impl ResizeHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(f64) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, position: f64) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(position);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

/// 仕切りをドラッグするのに要る、イベントから触る状態だけをまとめたもの。
struct Geometry {
    native: XamlGrid,
    divider: XamlGrid,
    orientation: Orientation,
    /// アプリが指定した仕切りの位置 (start 側の大きさ)。
    ///
    /// せまくて入りきらないときは端へ寄せて**表示**するが、この値はそのまま
    /// 残す。広がったときに元の位置へ戻すため。
    position: f64,
    min_start: f64,
    min_end: f64,
    /// いま画面に出ている位置。同じなら列の幅を書き直さない
    /// (`LayoutUpdated` の中から書くとレイアウトが回り続けるため)。
    shown: f64,
    dragging: bool,
    /// つかんだ場所と線の位置とのずれ。線をポインターの下へ飛ばさないために覚える。
    grab: f64,
}

impl Geometry {
    /// 2 つの区画が分け合える大きさ (仕切りの分を除く)。まだ 0 なら 0。
    fn total(&self) -> f64 {
        let Ok(element) = self.native.cast::<FrameworkElement>() else {
            return 0.0;
        };
        let length = if self.orientation.is_vertical() {
            element.ActualHeight()
        } else {
            element.ActualWidth()
        };
        let length = length.unwrap_or(0.0);
        if length <= 0.0 {
            return 0.0;
        }
        (length - DIVIDER_THICKNESS).max(0.0)
    }

    fn clamp(&self, position: f64) -> f64 {
        clamp_split_position(position, self.total(), self.min_start, self.min_end)
    }

    /// 指定された位置を、いまの大きさに収めて列 (行) の幅へ書く。
    fn apply(&mut self) {
        let shown = self.clamp(self.position);
        if (shown - self.shown).abs() < 0.5 {
            return;
        }
        self.shown = shown;
        let length = GridLength {
            Value: shown,
            GridUnitType: GridUnitType::Pixel,
        };
        if self.orientation.is_vertical() {
            if let Ok(rows) = self.native.RowDefinitions() {
                if let Ok(row) = rows.GetAt(0) {
                    let _ = row.SetHeight(length);
                }
            }
        } else if let Ok(columns) = self.native.ColumnDefinitions() {
            if let Ok(column) = columns.GetAt(0) {
                let _ = column.SetWidth(length);
            }
        }
    }

    /// ポインターの位置を、分割ビューの始端からの距離へ読み替える。
    fn pointer_offset(&self, x: f64, y: f64) -> f64 {
        if self.orientation.is_vertical() {
            y
        } else {
            x
        }
    }
}

struct SplitViewInner {
    native: XamlGrid,
    start_pane: XamlGrid,
    end_pane: XamlGrid,
    orientation: Orientation,
    /// 区画のハンドルを保持し、コールバックごと生かしておく。
    start: RefCell<Option<Box<dyn Widget>>>,
    end: RefCell<Option<Box<dyn Widget>>>,
    /// 表示上の位置とは別に、naui が覚えている位置。
    position: Cell<f64>,
    geometry: Arc<UiThreadCell<Geometry>>,
    handler: ResizeHandler,
}

/// 2 つの区画を、動かせる仕切りで分けるコンテナ。
#[derive(Clone)]
pub struct SplitView(Rc<SplitViewInner>);
impl_widget!(SplitView, native);

impl SplitView {
    pub(crate) fn new(orientation: Orientation) -> Result<Self> {
        let native = XamlGrid::new().map_err(|e| to_error("SplitView の Grid の生成", e))?;
        let start_pane = pane()?;
        let end_pane = pane()?;
        let divider = divider(orientation)?;

        define_tracks(&native, orientation)?;
        for (index, element) in [
            start_pane.cast::<UIElement>(),
            divider.cast::<UIElement>(),
            end_pane.cast::<UIElement>(),
        ]
        .into_iter()
        .enumerate()
        {
            let element = element.map_err(|e| to_error("SplitView の区画の要素化", e))?;
            place(&element, index as i32, orientation)?;
            native
                .Children()
                .and_then(|children| children.Append(&element))
                .map_err(|e| to_error("SplitView への追加", e))?;
        }

        let geometry = Arc::new(UiThreadCell::new(Geometry {
            native: native.clone(),
            divider: divider.clone(),
            orientation,
            position: DEFAULT_SPLIT_POSITION,
            min_start: 0.0,
            min_end: 0.0,
            // 列の初期値は 0 なので、最初の apply で必ず書き込まれる。
            shown: -1.0,
            dragging: false,
            grab: 0.0,
        }));
        let handler = ResizeHandler::new();

        let this = Self(Rc::new(SplitViewInner {
            native,
            start_pane,
            end_pane,
            orientation,
            start: RefCell::new(None),
            end: RefCell::new(None),
            position: Cell::new(DEFAULT_SPLIT_POSITION),
            geometry,
            handler,
        }));
        this.track_pointer(&divider)?;
        this.track_layout()?;
        Ok(this)
    }

    /// 仕切りのつまみ方 (ポインター) をつなぐ。
    fn track_pointer(&self, divider: &XamlGrid) -> Result<()> {
        let pressed = {
            let geometry = self.0.geometry.clone();
            PointerEventHandler::new(move |_, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                geometry.try_with_mut(|geometry| {
                    if let Ok(pointer) = args.Pointer() {
                        let _ = geometry.divider.CapturePointer(&pointer);
                    }
                    // つかんだ場所と線とのずれを保つ (どこをつかんでも線が
                    // ポインターの下へ飛ばないようにする)。
                    if let Some(offset) = geometry
                        .native
                        .cast::<UIElement>()
                        .ok()
                        .and_then(|root| args.GetCurrentPoint(&root).ok())
                        .and_then(|point| point.Position().ok())
                        .map(|point| {
                            geometry.pointer_offset(f64::from(point.X), f64::from(point.Y))
                        })
                    {
                        geometry.grab = offset - geometry.position;
                    }
                    geometry.dragging = true;
                });
                Ok(())
            })
        };
        divider
            .PointerPressed(&pressed)
            .map_err(|e| to_error("SplitView のポインター購読", e))?;

        let moved = {
            let geometry = self.0.geometry.clone();
            let handler = self.0.handler.clone();
            PointerEventHandler::new(move |_, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let moved = geometry.try_with_mut(|geometry| {
                    if !geometry.dragging {
                        return None;
                    }
                    let root = geometry.native.cast::<UIElement>().ok()?;
                    let point = args.GetCurrentPoint(&root).ok()?.Position().ok()?;
                    let position = geometry.pointer_offset(f64::from(point.X), f64::from(point.Y))
                        - geometry.grab;
                    let clamped = geometry.clamp(position);
                    if (clamped - geometry.position).abs() < 0.5 {
                        return None;
                    }
                    geometry.position = clamped;
                    geometry.apply();
                    Some(clamped)
                });
                if let Some(Some(position)) = moved {
                    handler.emit(position);
                }
                Ok(())
            })
        };
        divider
            .PointerMoved(&moved)
            .map_err(|e| to_error("SplitView のポインター購読", e))?;

        let released = {
            let geometry = self.0.geometry.clone();
            PointerEventHandler::new(move |_, args| {
                geometry.try_with_mut(|geometry| {
                    if let Some(args) = args.as_ref() {
                        if let Ok(pointer) = args.Pointer() {
                            let _ = geometry.divider.ReleasePointerCapture(&pointer);
                        }
                    }
                    geometry.dragging = false;
                });
                Ok(())
            })
        };
        divider
            .PointerReleased(&released)
            .map_err(|e| to_error("SplitView のポインター購読", e))?;

        let lost = {
            let geometry = self.0.geometry.clone();
            PointerEventHandler::new(move |_, _| {
                geometry.try_with_mut(|geometry| geometry.dragging = false);
                Ok(())
            })
        };
        divider
            .PointerCaptureLost(&lost)
            .map_err(|e| to_error("SplitView のポインター購読", e))?;
        Ok(())
    }

    /// 大きさが決まった (変わった) ときに、位置を収め直す。
    ///
    /// 大きさが変わったことは `LayoutUpdated` で拾う (`SizeChanged` でも
    /// 拾えるが、仕切りを動かした直後の更新まで拾いたいのでこちら)。
    /// 表示中の位置が変わらないときは何も書かないので、
    /// レイアウトが回り続けることはない。
    fn track_layout(&self) -> Result<()> {
        let geometry = self.0.geometry.clone();
        let updated = EventHandler::<IInspectable>::new(move |_, _| {
            geometry.try_with_mut(|geometry| geometry.apply());
            Ok(())
        });
        self.0
            .native
            .cast::<FrameworkElement>()
            .and_then(|element| element.LayoutUpdated(&updated))
            .map_err(|e| to_error("SplitView のレイアウト購読", e))?;
        Ok(())
    }

    /// 並べる向き。`Horizontal` なら区画が横に並ぶ (仕切りは縦)。
    pub fn orientation(&self) -> Orientation {
        self.0.orientation
    }

    /// 左 (または上) の区画。呼ぶたびに置き換わる。
    pub fn set_start(&self, child: &dyn Widget) {
        self.0.set_pane(true, child);
    }

    /// 右 (または下) の区画。呼ぶたびに置き換わる。
    pub fn set_end(&self, child: &dyn Widget) {
        self.0.set_pane(false, child);
    }

    /// いまの仕切りの位置 (start 側の大きさ、論理ピクセル)。
    pub fn position(&self) -> f64 {
        self.0.position.get()
    }

    /// 仕切りを動かす。`on_resize` は呼ばれない。
    pub fn set_position(&self, position: f64) {
        let clamped = self.0.geometry.with_mut(|geometry| {
            let clamped = geometry.clamp(position);
            geometry.position = clamped;
            geometry.apply();
            clamped
        });
        self.0.position.set(clamped);
    }

    /// 利用者がドラッグしたのと同じく仕切りを動かす。`on_resize` を呼ぶ。
    pub fn drag_to(&self, position: f64) {
        self.set_position(position);
        self.0.handler.emit(self.0.position.get());
    }

    /// 両側の区画の最小の大きさ。既定はどちらも 0。
    pub fn set_min_sizes(&self, start: f64, end: f64) {
        let clamped = self.0.geometry.with_mut(|geometry| {
            geometry.min_start = start.max(0.0);
            geometry.min_end = end.max(0.0);
            // いまの位置が範囲の外なら押し戻す (通知はしない)。
            geometry.position = geometry.clamp(geometry.position);
            geometry.apply();
            geometry.position
        });
        self.0.position.set(clamped);
    }

    /// 利用者が仕切りを動かすたび、動いた後の位置で呼ばれる。
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.handler.set(f);
    }
}

impl SplitViewInner {
    fn set_pane(&self, is_start: bool, child: &dyn Widget) {
        let pane = if is_start {
            &self.start_pane
        } else {
            &self.end_pane
        };
        if let Ok(children) = pane.Children() {
            let _ = children.Clear();
        }
        let element = child.native_element();
        let appended = pane
            .Children()
            .and_then(|children| children.Append(&element));
        if appended.is_ok() {
            let slot = if is_start { &self.start } else { &self.end };
            *slot.borrow_mut() = Some(child.boxed_clone());
        }
    }
}

/// 区画 1 つぶんの入れ物。中身は配られた場所いっぱいに置く。
fn pane() -> Result<XamlGrid> {
    let pane = XamlGrid::new().map_err(|e| to_error("SplitView の区画の生成", e))?;
    let element = pane
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("SplitView の区画の要素化", e))?;
    let _ = element.SetHorizontalAlignment(HorizontalAlignment::Stretch);
    let _ = element.SetVerticalAlignment(VerticalAlignment::Stretch);
    Ok(pane)
}

/// 仕切り。テーマリソースが引けない環境では、色なしの `Grid` に落とす。
fn divider(orientation: Orientation) -> Result<XamlGrid> {
    let xaml = if orientation.is_vertical() {
        DIVIDER_XAML_VERTICAL
    } else {
        DIVIDER_XAML_HORIZONTAL
    };
    let divider = match XamlReader::Load(&HSTRING::from(xaml))
        .and_then(|element| element.cast::<XamlGrid>())
    {
        Ok(divider) => divider,
        Err(error) => {
            eprintln!("naui-windows: 分割ビューの仕切りの生成に失敗: {error}");
            XamlGrid::new().map_err(|e| to_error("SplitView の仕切りの生成", e))?
        }
    };
    let element = divider
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("SplitView の仕切りの要素化", e))?;
    let _ = element.SetHorizontalAlignment(HorizontalAlignment::Stretch);
    let _ = element.SetVerticalAlignment(VerticalAlignment::Stretch);
    Ok(divider)
}

/// start / 仕切り / end の 3 つの列 (行) を作る。
fn define_tracks(native: &XamlGrid, orientation: Orientation) -> Result<()> {
    let lengths = [
        GridLength {
            Value: DEFAULT_SPLIT_POSITION,
            GridUnitType: GridUnitType::Pixel,
        },
        GridLength {
            Value: DIVIDER_THICKNESS,
            GridUnitType: GridUnitType::Pixel,
        },
        GridLength {
            Value: 1.0,
            GridUnitType: GridUnitType::Star,
        },
    ];
    for length in lengths {
        if orientation.is_vertical() {
            let row = RowDefinition::new().map_err(|e| to_error("SplitView の行の生成", e))?;
            row.SetHeight(length)
                .map_err(|e| to_error("SplitView の行の設定", e))?;
            native
                .RowDefinitions()
                .and_then(|rows| rows.Append(&row))
                .map_err(|e| to_error("SplitView の行の追加", e))?;
        } else {
            let column =
                ColumnDefinition::new().map_err(|e| to_error("SplitView の列の生成", e))?;
            column
                .SetWidth(length)
                .map_err(|e| to_error("SplitView の列の設定", e))?;
            native
                .ColumnDefinitions()
                .and_then(|columns| columns.Append(&column))
                .map_err(|e| to_error("SplitView の列の追加", e))?;
        }
    }
    Ok(())
}

/// 要素を `index` 番目の列 (行) へ置く。
fn place(element: &UIElement, index: i32, orientation: Orientation) -> Result<()> {
    let framework = element
        .cast::<FrameworkElement>()
        .map_err(|e| to_error("SplitView の配置", e))?;
    if orientation.is_vertical() {
        XamlGrid::SetRow(&framework, index).map_err(|e| to_error("SplitView の配置", e))?;
    } else {
        XamlGrid::SetColumn(&framework, index).map_err(|e| to_error("SplitView の配置", e))?;
    }
    Ok(())
}
