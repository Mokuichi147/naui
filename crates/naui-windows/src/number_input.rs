//! 数値入力 (`NumberBox`)。
//!
//! WinUI 3 の数値専用コントロールをそのまま使う。増減ボタンは既定で隠れて
//! いるので、`SpinButtonPlacementMode` を `Inline` にして欄の右へ出す。
//!
//! 値の丸めと範囲は [`NumberSpec`] が決める。範囲・刻み・小数桁は
//! `NumberBox` 側 (`Minimum` / `Maximum` / `SmallChange` / `NumberFormatter`)
//! にも書いて、増減ボタンや上下キーの動きと表示をそろえる。
//!
//! `NumberBox` が値を決めるのは**確定したとき** (Enter・欄を離れたとき・
//! 増減ボタン・上下キー・ホイール) だけなので、1 文字ごとの通知は
//! テンプレートの中にある入力欄 (`InputBox`) の `TextChanged` から拾う。
//! 標準のテンプレートが差し替えられていて入力欄が見つからないときは、
//! 確定したときだけ通知される。
//!
//! 打っている途中の表示は、`NumberBox` が確定に使うのと同じ `NumberFormatter`
//! で読む。小数点や桁区切りは地域設定で変わるので、打鍵中と確定とで読み手が
//! 違うと通知の出かたがずれる。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{NumberSpec, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    NumberBox as XamlNumberBox, NumberBoxSpinButtonPlacementMode, NumberBoxValidationMode,
    NumberBoxValueChangedEventArgs, TextBox, TextChangedEventHandler,
};
use naui_winui3::Microsoft::UI::Xaml::{RoutedEventHandler, UIElement};
use windows::Foundation::TypedEventHandler;
use windows::Globalization::NumberFormatting::{DecimalFormatter, INumberParser};
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// WinUI 3 の `NumberBox` テンプレートが持つ入力欄の名前。
const INPUT_BOX_PART: &str = "InputBox";

/// 範囲を指定されていないときに `NumberBox` へ渡す端の値。
///
/// `NumberBox` は下限・上限を必ず持つので、その既定値と同じものを
/// 「制限なし」の代わりに使う。丸めと範囲は [`NumberSpec`] が持っているので、
/// ここで止まっても naui の答えは変わらない。
const UNBOUNDED: f64 = f64::MAX;

/// PageUp / PageDown で動く量は刻みの何倍か (`NumberBox` の既定と同じ比)。
const LARGE_CHANGE_STEPS: f64 = 10.0;

/// 値が変わったことの通知先。
///
/// WinRT のデリゲートは `Send + Sync` を要求するので [`UiThreadCell`] に
/// 載せる。呼ぶ間だけクロージャを取り出すので、通知の中から同じ欄を操作しても
/// 二重借用にならない。
#[derive(Clone)]
struct ChangeHandler(HandlerCell<dyn FnMut(f64)>);

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
    native: XamlNumberBox,
    spec: Cell<NumberSpec>,
    value: Cell<f64>,
    handler: ChangeHandler,
    /// 値を書き込んでいる間だけ、WinUI からの通知を無視する。
    silent: Cell<bool>,
    /// テンプレートの入力欄。見つかるまでは `None`。
    field: RefCell<Option<TextBox>>,
}

/// 数値を入力させるコントロール (`NumberBox`)。
///
/// 既定は整数 (刻み 1、小数桁 0、範囲の制限なし)。
#[derive(Clone)]
pub struct NumberInput(Rc<NumberInputInner>);
impl_widget!(NumberInput, native);

impl NumberInput {
    pub(crate) fn new(value: f64) -> Result<Self> {
        let native = XamlNumberBox::new().map_err(|e| to_error("NumberBox の生成", e))?;
        // 増減ボタンは既定では出ない。欄の右へ並べる Inline にする。
        native
            .SetSpinButtonPlacementMode(NumberBoxSpinButtonPlacementMode::Inline)
            .map_err(|e| to_error("数値入力の増減ボタンの設定", e))?;
        // 数として読めない文字列は確定時に元の値へ戻す。既定と同じだが、
        // naui が約束している動きなので明示しておく。
        native
            .SetValidationMode(NumberBoxValidationMode::InvalidInputOverwritten)
            .map_err(|e| to_error("数値入力の検証方法の設定", e))?;

        let this = Self(Rc::new(NumberInputInner {
            native,
            spec: Cell::new(NumberSpec::default()),
            value: Cell::new(NumberSpec::default().clamp(value)),
            handler: ChangeHandler::new(),
            silent: Cell::new(false),
            field: RefCell::new(None),
        }));
        this.write_native_spec()?;
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
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.handler.set(f);
    }

    /// 対応する `NumberBox`。バックエンド固有の脱出口として公開している。
    pub fn native_number_box(&self) -> XamlNumberBox {
        self.0.native.clone()
    }

    /// 確定と打鍵の購読をつなぐ。
    fn connect(&self) -> Result<()> {
        // 確定 (Enter・欄を離れたとき・増減ボタン・上下キー・ホイール)。
        let committed = UiThreadCell::new(Rc::downgrade(&self.0));
        let changed = TypedEventHandler::<XamlNumberBox, NumberBoxValueChangedEventArgs>::new(
            move |_sender, args| {
                let Some(inner) = committed.try_with_mut(|weak| weak.upgrade()).flatten() else {
                    return Ok(());
                };
                let this = NumberInput(inner);
                if this.0.silent.get() {
                    return Ok(());
                }
                // 欄が空だと `NumberBox` は値を NaN にする。読めなかった表示と
                // 同じ扱いにして、いまの値へ戻す。
                let shown = args
                    .as_ref()
                    .and_then(|args| args.NewValue().ok())
                    .filter(|value| value.is_finite())
                    .unwrap_or_else(|| this.value());
                this.accept(shown, true);
                Ok(())
            },
        );
        self.0
            .native
            .ValueChanged(&changed)
            .map_err(|e| to_error("数値入力の確定購読", e))?;

        // テンプレートが展開されてから入力欄を探す。
        let loaded = UiThreadCell::new(Rc::downgrade(&self.0));
        let handler = RoutedEventHandler::new(move |_sender, _args| {
            if let Some(inner) = loaded.try_with_mut(|weak| weak.upgrade()).flatten() {
                NumberInput(inner).watch_input_box();
            }
            Ok(())
        });
        self.0
            .native
            .Loaded(&handler)
            .map_err(|e| to_error("数値入力の読み込み購読", e))?;
        Ok(())
    }

    /// テンプレートの入力欄を見つけて、1 文字ごとの変化を購読する。
    ///
    /// 見つからないときは何もしない (確定は `ValueChanged` で届く)。
    fn watch_input_box(&self) {
        if self.0.field.borrow().is_some() {
            return;
        }
        let Ok(part) = self
            .0
            .native
            .GetTemplateChild(&HSTRING::from(INPUT_BOX_PART))
        else {
            return;
        };
        let Ok(field) = part.cast::<TextBox>() else {
            return;
        };

        let state = UiThreadCell::new(Rc::downgrade(&self.0));
        let typed = TextChangedEventHandler::new(move |_sender, _args| {
            let Some(inner) = state.try_with_mut(|weak| weak.upgrade()).flatten() else {
                return Ok(());
            };
            let this = NumberInput(inner);
            if this.0.silent.get() {
                return Ok(());
            }
            // `NumberBox.Text` は確定するまで追いつかないので、入力欄から直に読む。
            let text = this
                .0
                .field
                .borrow()
                .as_ref()
                .and_then(|field| field.Text().ok())
                .map(|text| text.to_string());
            // 打っている途中で読めない表示 (空欄や `-` だけ) は確定まで待つ。
            if let Some(shown) = text.and_then(|text| this.parse_shown(&text)) {
                this.accept(shown, false);
            }
            Ok(())
        });
        if field.TextChanged(&typed).is_ok() {
            *self.0.field.borrow_mut() = Some(field);
        }
    }

    /// 打っている途中の表示を数として読む。読めなければ `None`。
    ///
    /// 読み手は `NumberBox` が確定に使うものと同じ (`NumberFormatter` は
    /// `INumberParser` でもある) にする。小数点や桁区切りは地域設定で変わる
    /// ので、[`NumberSpec::parse`] (`.` しか読まない) で読むと、`1,5` のような
    /// 表示が打っている間だけ読めず、確定して初めて通知が出ることになる。
    ///
    /// 読み手を取れないときだけ [`NumberSpec`] へ戻す。
    fn parse_shown(&self, text: &str) -> Option<f64> {
        let parser = self
            .0
            .native
            .NumberFormatter()
            .ok()
            .and_then(|formatter| formatter.cast::<INumberParser>().ok());
        let Some(parser) = parser else {
            return self.0.spec.get().parse(text);
        };
        parser
            .ParseDouble(&HSTRING::from(text))
            .ok()
            .and_then(|value| value.Value().ok())
            .filter(|value| value.is_finite())
    }

    /// 決まりを差し替え、`NumberBox` と現在値へ反映する。
    ///
    /// 範囲を書くと `NumberBox` は現在値を範囲の中へ寄せ、その `ValueChanged`
    /// を出す。決まりの差し替えは**通知しない**約束なので、書いている間は
    /// 止めておき、あとから naui 側の値をそろえ直す。
    fn update_spec(&self, edit: impl FnOnce(NumberSpec) -> NumberSpec) {
        self.0.spec.set(edit(self.0.spec.get()));
        let previous = self.0.silent.replace(true);
        let _ = self.write_native_spec();
        self.0.silent.set(previous);
        self.set_value(self.value());
    }

    /// 範囲・刻み・小数桁を `NumberBox` へ書く。
    ///
    /// 下限を先に書くと、上限がまだ古いままなら `NumberBox` が上限を押し上げる
    /// ことがあるが、続けて上限を書くので最後には指定どおりになる
    /// (下限が上限より大きいときに上限が勝つのも [`NumberSpec`] と同じ)。
    fn write_native_spec(&self) -> Result<()> {
        let spec = self.0.spec.get();
        self.0
            .native
            .SetMinimum(spec.min.unwrap_or(-UNBOUNDED))
            .map_err(|e| to_error("数値入力の下限の設定", e))?;
        self.0
            .native
            .SetMaximum(spec.max.unwrap_or(UNBOUNDED))
            .map_err(|e| to_error("数値入力の上限の設定", e))?;
        self.0
            .native
            .SetSmallChange(spec.step)
            .map_err(|e| to_error("数値入力の刻みの設定", e))?;
        self.0
            .native
            .SetLargeChange(spec.step * LARGE_CHANGE_STEPS)
            .map_err(|e| to_error("数値入力の大きい刻みの設定", e))?;
        self.0
            .native
            .SetNumberFormatter(&formatter(spec.decimals)?)
            .map_err(|e| to_error("数値入力の書式の設定", e))?;
        Ok(())
    }

    /// 画面に出ている値を受け取る。`commit` なら `NumberBox` へも書き戻す。
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

    /// 値を `NumberBox` へ書く。この間の通知は無視する。
    ///
    /// 表示は `NumberBox` が `NumberFormatter` で書き直すので、ここでは触らない。
    fn write_native(&self, value: f64) {
        let previous = self.0.silent.replace(true);
        let _ = self.0.native.SetValue(value);
        self.0.silent.set(previous);
    }
}

/// 小数桁ぶんを必ず書く書式。
///
/// 桁区切りは入れない。ほかの 3 バックエンド ([`NumberSpec::format`]・
/// `GtkSpinButton`・`<input type="number">`) がどれも区切らないので、そこへ
/// そろえる。小数点そのものは地域設定のままにする (`NumberBox` の既定と同じ)。
fn formatter(decimals: u32) -> Result<DecimalFormatter> {
    let formatter = DecimalFormatter::new().map_err(|e| to_error("数の書式の生成", e))?;
    formatter
        .SetIntegerDigits(1)
        .map_err(|e| to_error("数の書式の整数桁の設定", e))?;
    formatter
        .SetFractionDigits(decimals as i32)
        .map_err(|e| to_error("数の書式の小数桁の設定", e))?;
    formatter
        .SetIsGrouped(false)
        .map_err(|e| to_error("数の書式の桁区切りの設定", e))?;
    Ok(formatter)
}
