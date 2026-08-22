//! 数値入力 (`TextBox` + 増減ボタン)。
//!
//! WinUI 3 の `NumberBox` は `winio-winui3` のバインディングに無いため、
//! 数字を打つ `TextBox` と `-` / `+` の `Button` を `StackPanel` へ並べる
//! (`NumberBox` の既定である Inline のスピンボタンと同じ並び)。
//!
//! 値の丸めと範囲は [`NumberSpec`] が決める。打っている最中に表示を
//! 書き換えると打ちづらいので、**書き戻しは確定 (欄を離れたとき) と
//! ボタンを押したときだけ**行う。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{NumberSpec, Result};
use windows_core::{Interface, HSTRING};
use winui3::Microsoft::UI::Xaml::Controls::{
    Button as XamlButton, Orientation as XamlOrientation, StackPanel, TextBlock, TextBox,
    TextChangedEventHandler,
};
use winui3::Microsoft::UI::Xaml::{RoutedEventHandler, TextAlignment, UIElement};

use crate::to_error;
use crate::ui_thread::UiThreadCell;
use crate::widgets::{impl_widget, Widget};

/// 数字の欄の最小幅 (論理ピクセル)。中身に合わせると狭くなりすぎるため。
const FIELD_MIN_WIDTH: f64 = 96.0;
/// 増減ボタンの幅。WinUI の `NumberBox` のスピンボタンに合わせる。
const SPIN_WIDTH: f64 = 34.0;

type ChangeCallback = Box<dyn FnMut(f64)>;

/// 値が変わったことの通知先。呼ぶ間だけクロージャを取り出して再入を許す。
struct ChangeHandler(Arc<UiThreadCell<Option<ChangeCallback>>>);

impl ChangeHandler {
    fn new() -> Self {
        Self(Arc::new(UiThreadCell::new(None)))
    }

    fn set(&self, f: impl FnMut(f64) + 'static) {
        self.0.with_mut(|slot| *slot = Some(Box::new(f)));
    }

    fn emit(&self, value: f64) {
        let Some(Some(mut f)) = self.0.try_with_mut(|slot| slot.take()) else {
            return;
        };
        f(value);
        let _ = self.0.try_with_mut(|slot| {
            if slot.is_none() {
                *slot = Some(f);
            }
        });
    }
}

struct NumberInputInner {
    native: StackPanel,
    field: TextBox,
    down: XamlButton,
    up: XamlButton,
    spec: Cell<NumberSpec>,
    value: Cell<f64>,
    handler: ChangeHandler,
    /// 値を書き込んでいる間だけ、WinUI からの通知を無視する。
    silent: Cell<bool>,
    /// 付け替えのために覚えておくイベントのトークン。
    tokens: RefCell<Vec<i64>>,
}

/// 数値を入力させるコントロール (`TextBox` + 増減ボタン)。
///
/// 既定は整数 (刻み 1、小数桁 0、範囲の制限なし)。
#[derive(Clone)]
pub struct NumberInput(Rc<NumberInputInner>);
impl_widget!(NumberInput, native);

impl NumberInput {
    pub(crate) fn new(value: f64) -> Result<Self> {
        let native = StackPanel::new().map_err(|e| to_error("StackPanel の生成", e))?;
        native
            .SetOrientation(XamlOrientation::Horizontal)
            .map_err(|e| to_error("数値入力の向きの設定", e))?;

        let field = TextBox::new().map_err(|e| to_error("TextBox の生成", e))?;
        field
            .SetTextAlignment(TextAlignment::Right)
            .map_err(|e| to_error("数値入力の文字ぞろえの設定", e))?;
        field
            .SetMinWidth(FIELD_MIN_WIDTH)
            .map_err(|e| to_error("数値入力の幅の設定", e))?;

        let down = spin_button("\u{2212}")?; // −
        let up = spin_button("\u{FF0B}")?; // ＋

        let this = Self(Rc::new(NumberInputInner {
            native,
            field,
            down,
            up,
            spec: Cell::new(NumberSpec::default()),
            value: Cell::new(NumberSpec::default().clamp(value)),
            handler: ChangeHandler::new(),
            silent: Cell::new(false),
            tokens: RefCell::new(Vec::new()),
        }));
        this.assemble()?;
        this.write_native(this.value());
        this.connect()?;
        Ok(this)
    }

    /// いまの値。
    pub fn value(&self) -> f64 {
        self.0.value.get()
    }

    /// 値を通知せずに変える。小数桁へ丸め、範囲の外なら端へ寄せる。
    pub fn set_value(&self, value: f64) {
        let value = self.0.spec.get().clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// 入れられる範囲を決める。`None` はその側に制限を置かない。
    ///
    /// いまの値が範囲から外れていれば、通知せずに端へ寄せる。
    pub fn set_range(&self, min: Option<f64>, max: Option<f64>) {
        self.update_spec(|spec| spec.range(min, max));
    }

    /// 増減ボタンやキーで 1 回に動く量 (既定は 1)。
    pub fn set_step(&self, step: f64) {
        self.update_spec(|spec| spec.step(step));
    }

    /// 表示する小数の桁数 (既定は 0 = 整数)。
    pub fn set_decimals(&self, decimals: u32) {
        self.update_spec(|spec| spec.decimals(decimals));
    }

    /// いまの値の決まり (範囲・刻み・小数桁)。
    pub fn spec(&self) -> NumberSpec {
        self.0.spec.get()
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.field.SetIsEnabled(enabled);
        let _ = self.0.down.SetIsEnabled(enabled);
        let _ = self.0.up.SetIsEnabled(enabled);
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.handler.set(f);
    }

    /// 数字を打つ欄。バックエンド固有の脱出口として公開している。
    pub fn native_text_box(&self) -> TextBox {
        self.0.field.clone()
    }

    /// 減らす・増やすボタン。
    pub fn native_spin_buttons(&self) -> (XamlButton, XamlButton) {
        (self.0.down.clone(), self.0.up.clone())
    }

    /// 欄とボタンを `StackPanel` へ並べる。
    fn assemble(&self) -> Result<()> {
        let children = self
            .0
            .native
            .Children()
            .map_err(|e| to_error("数値入力の子の取得", e))?;
        for element in [
            self.0.field.cast::<UIElement>(),
            self.0.down.cast::<UIElement>(),
            self.0.up.cast::<UIElement>(),
        ] {
            let element = element.map_err(|e| to_error("数値入力の要素化", e))?;
            children
                .Append(&element)
                .map_err(|e| to_error("数値入力への追加", e))?;
        }
        Ok(())
    }

    /// 打鍵・確定・増減ボタンの購読をつなぐ。
    fn connect(&self) -> Result<()> {
        let mut tokens = Vec::new();

        // 打鍵のたびに、読める値なら受け取る。
        let typed = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = TextChangedEventHandler::new(move |_sender, _args| {
            let Some(inner) = typed.try_with_mut(|weak| weak.upgrade()).flatten() else {
                return Ok(());
            };
            let this = NumberInput(inner);
            if this.0.silent.get() {
                return Ok(());
            }
            let text = this.0.field.Text().unwrap_or_default().to_string();
            if let Some(shown) = this.0.spec.get().parse(&text) {
                this.accept(shown, false);
            }
            Ok(())
        });
        tokens.push(
            self.0
                .field
                .TextChanged(&handler)
                .map_err(|e| to_error("数値入力の打鍵購読", e))?,
        );

        // 欄を離れたら確定する。読めなかった表示はここで元へ戻す。
        let left = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            let Some(inner) = left.try_with_mut(|weak| weak.upgrade()).flatten() else {
                return Ok(());
            };
            let this = NumberInput(inner);
            let text = this.0.field.Text().unwrap_or_default().to_string();
            let shown = this
                .0
                .spec
                .get()
                .parse(&text)
                .unwrap_or_else(|| this.value());
            this.accept(shown, true);
            Ok(())
        });
        tokens.push(
            self.0
                .field
                .LostFocus(&handler)
                .map_err(|e| to_error("数値入力の確定購読", e))?,
        );

        // 増減ボタン。刻みと範囲は NumberSpec が守る。
        for (button, steps) in [(&self.0.down, -1.0), (&self.0.up, 1.0)] {
            let pressed = UiThreadCell::new(Rc::downgrade(&self.0));
            let handler = RoutedEventHandler::new(move |_sender, _args| {
                let Some(inner) = pressed.try_with_mut(|weak| weak.upgrade()).flatten() else {
                    return Ok(());
                };
                let this = NumberInput(inner);
                let stepped = this.0.spec.get().stepped(this.value(), steps);
                this.accept(stepped, true);
                Ok(())
            });
            tokens.push(
                button
                    .Click(&handler)
                    .map_err(|e| to_error("数値入力のボタン購読", e))?,
            );
        }

        *self.0.tokens.borrow_mut() = tokens;
        Ok(())
    }

    fn update_spec(&self, edit: impl FnOnce(NumberSpec) -> NumberSpec) {
        self.0.spec.set(edit(self.0.spec.get()));
        self.set_value(self.value());
    }

    /// 画面に出ている値を受け取る。`commit` なら表示も書き直す。
    fn accept(&self, shown: f64, commit: bool) {
        let accepted = self.0.spec.get().clamp(shown);
        if commit {
            self.write_native(accepted);
        }
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.handler.emit(accepted);
    }

    /// 値を欄へ書く。この間の `TextChanged` は無視する。
    fn write_native(&self, value: f64) {
        let previous = self.0.silent.replace(true);
        let _ = self
            .0
            .field
            .SetText(&HSTRING::from(self.0.spec.get().format(value)));
        self.0.silent.set(previous);
    }
}

/// `-` / `+` のボタン。文字だけを見せる小さなボタンにする。
fn spin_button(text: &str) -> Result<XamlButton> {
    let button = XamlButton::new().map_err(|e| to_error("Button の生成", e))?;
    let label = TextBlock::new().map_err(|e| to_error("Button ラベルの生成", e))?;
    label
        .SetText(&HSTRING::from(text))
        .map_err(|e| to_error("Button ラベルの設定", e))?;
    button
        .SetContent(&label)
        .map_err(|e| to_error("Button への内容設定", e))?;
    button
        .SetWidth(SPIN_WIDTH)
        .map_err(|e| to_error("Button の幅の設定", e))?;
    Ok(button)
}
