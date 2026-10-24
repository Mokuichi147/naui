//! Number input (`NumberBox`).
//!
//! WinUI 3's dedicated numeric control is used as is. The step buttons are hidden
//! by default, so `SpinButtonPlacementMode` is set to `Inline` to show them right of
//! the field.
//!
//! Rounding and range are decided by [`NumberSpec`]. The range, step and decimal
//! places are also written on the `NumberBox` side (`Minimum` / `Maximum` /
//! `SmallChange` / `NumberFormatter`), so the step buttons, up/down keys and display
//! agree.
//!
//! `NumberBox` decides its value only **on confirmation** (Enter, leaving the field,
//! the step buttons, up/down keys, the wheel), so per-keystroke notifications are
//! picked up from the `TextChanged` of the input field inside the template
//! (`InputBox`). If the standard template has been replaced and the input field
//! cannot be found, notifications happen only on confirmation.
//!
//! So while typing, only naui holds the value, and the `NumberBox`'s value stays
//! stale. Both reverting and stepping are based on the `NumberBox`'s own value, so
//! **once the display becomes unreadable, the value already received is passed to
//! it** ([`sync_native`](NumberInput::sync_native)). If the display is readable,
//! the `NumberBox` reads it itself, so nothing needs to be passed.
//!
//! Text typed so far is read with the same `NumberFormatter` the `NumberBox` uses
//! for confirmation. The decimal point and digit grouping vary with regional
//! settings, so if typing and confirmation used different readers, notifications
//! would come out differently.
//!
//! # Up to 10 significant digits
//!
//! Before building its display, `NumberBox` **rounds the value to 10 significant
//! digits** (a `NumberRounder` with `SignificantDigits(10)`). This rounding is
//! internal to `NumberBox` and cannot be removed by replacing the
//! `NumberFormatter`.
//!
//! So passing [`NumberSpec::decimals`] a digit count that requires more than 10
//! significant digits cuts the display at 10 digits. On confirmation, `NumberBox`
//! reads that display back, so the value also becomes 10 digits (so
//! [`value`](NumberInput::value) and the display do not stay out of step). It is not
//! suitable for uses that want to show the full precision of `f64` (15 to 17
//! digits).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use naui_core::{NumberSpec, Result};
use naui_winui3::Microsoft::UI::Xaml::Controls::{
    NumberBox as XamlNumberBox, NumberBoxSpinButtonPlacementMode, NumberBoxValidationMode,
    NumberBoxValueChangedEventArgs, TextBox, TextChangedEventHandler,
};
use naui_winui3::Microsoft::UI::Xaml::{RoutedEventHandler, TextAlignment, UIElement};
use windows::Foundation::TypedEventHandler;
use windows::Globalization::NumberFormatting::{DecimalFormatter, INumberParser};
use windows_core::{Interface, HSTRING};

use crate::to_error;
use crate::ui_thread::{HandlerCell, UiThreadCell};
use crate::widgets::{impl_widget, Widget};

/// The name of the input field in WinUI 3's `NumberBox` template.
const INPUT_BOX_PART: &str = "InputBox";

/// The bound passed to `NumberBox` when no range is specified.
///
/// `NumberBox` always has a minimum and maximum, so the same value as its default is
/// used in place of "no limit". [`NumberSpec`] holds the rounding and range, so
/// stopping here does not change naui's answer.
const UNBOUNDED: f64 = f64::MAX;

/// How many steps PageUp / PageDown move (the same ratio as `NumberBox`'s
/// default).
const LARGE_CHANGE_STEPS: f64 = 10.0;

/// The handler notified when the value changes.
///
/// WinRT delegates require `Send + Sync`, so it is placed in a [`UiThreadCell`].
/// The closure is taken out only while it is called, so operating on the same field
/// from inside the notification does not double-borrow.
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
    submit: ChangeHandler,
    /// Ignores notifications from WinUI only while writing the value.
    silent: Cell<bool>,
    /// The template's input field. `None` until found.
    field: RefCell<Option<TextBox>>,
    /// The `on_focus_change` handler.
    focus: crate::focus::Focus,
}

/// A control for entering a number (`NumberBox`).
///
/// The default is integers (step 1, 0 decimal places, no range limit).
#[derive(Clone)]
pub struct NumberInput(Rc<NumberInputInner>);
crate::keys::impl_key_down!(NumberInput);
crate::focus::impl_focus_change!(NumberInput);
impl_widget!(NumberInput, native);

impl NumberInput {
    pub(crate) fn new(value: f64) -> Result<Self> {
        let native = XamlNumberBox::new().map_err(|e| to_error("NumberBox の生成", e))?;
        // 増減ボタンは既定では出ない。欄の右へ並べる Inline にする。
        native
            .SetSpinButtonPlacementMode(NumberBoxSpinButtonPlacementMode::Inline)
            .map_err(|e| to_error("数値入力の増減ボタンの設定", e))?;
        // 読めない文字列は確定時に元の値へ戻し、範囲の外は端へ寄せる (既定)。
        //
        // 戻す先も増減の基準も `NumberBox` が持っている値なので、読めない表示に
        // なった時点で受け取り済みの値を渡しておく
        // ([`sync_native`](Self::sync_native) を参照)。これをしないと、`12` まで
        // 打ってから `12x` にしたときに古い値へ戻ってしまう。
        native
            .SetValidationMode(NumberBoxValidationMode::InvalidInputOverwritten)
            .map_err(|e| to_error("数値入力の検証方法の設定", e))?;

        let this = Self(Rc::new(NumberInputInner {
            native,
            spec: Cell::new(NumberSpec::default()),
            value: Cell::new(NumberSpec::default().clamp(value)),
            handler: ChangeHandler::new(),
            submit: ChangeHandler::new(),
            silent: Cell::new(false),
            field: RefCell::new(None),
            focus: Default::default(),
        }));
        this.write_native_spec()?;
        this.write_native(this.value());
        this.connect()?;
        Ok(this)
    }

    /// The current value.
    pub fn value(&self) -> f64 {
        self.0.value.get()
    }

    /// Changes the value without notifying. Rounds to the decimal places and clamps to
    /// the range.
    pub fn set_value(&self, value: f64) {
        let value = self.0.spec.get().clamp(value);
        self.0.value.set(value);
        self.write_native(value);
        self.write_native_text(value);
    }

    /// Sets the range of accepted values. `None` places no limit on that side.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying.
    pub fn set_range(&self, min: Option<f64>, max: Option<f64>) {
        self.update_spec(|spec| spec.range(min, max));
    }

    /// How much one press of the step buttons or keys moves the value (default 1).
    pub fn set_step(&self, step: f64) {
        self.update_spec(|spec| spec.step(step));
    }

    /// The number of decimal places shown (default 0 = integers).
    pub fn set_decimals(&self, decimals: u32) {
        self.update_spec(|spec| spec.decimals(decimals));
    }

    /// The current value rules (range, step, decimal places).
    pub fn spec(&self) -> NumberSpec {
        self.0.spec.get()
    }

    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.0.native.SetIsEnabled(enabled);
    }

    /// Called with the new value when the value changes. Setting it again removes the
    /// previous callback.
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.handler.set(f);
    }

    /// Called with the confirmed value when the user confirms with Enter.
    ///
    /// What has been typed so far is read into the value first (if the value changes,
    /// `on_change` is called first). Text that cannot be read as a number is reverted
    /// to the previous value before the call. Not called for an Enter pressed while an
    /// IME is composing.
    pub fn on_submit(&self, f: impl FnMut(f64) + 'static) {
        self.0.submit.set(f);
    }

    /// The corresponding `NumberBox`. Public as a backend-specific escape hatch.
    pub fn native_number_box(&self) -> XamlNumberBox {
        self.0.native.clone()
    }

    /// Connects the subscriptions for confirmation and typing.
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

        // Enter。`NumberBox` が値を確定するのはキーを離したとき (`KeyUp`) なので、
        // 押した時点ではまだ古い値のまま。ここで表示を読んで確定させてから
        // 知らせる。続く `NumberBox` の確定は同じ表示を読むだけになる。
        let element: UIElement = self
            .0
            .native
            .cast()
            .map_err(|e| to_error("数値入力の要素の取得", e))?;
        let weak = Rc::downgrade(&self.0);
        crate::keys::on_enter(&element, move || {
            if let Some(inner) = weak.upgrade() {
                let this = NumberInput(inner);
                this.commit_field();
                this.0.submit.emit(this.value());
            }
        })
        .map_err(|e| to_error("数値入力の Enter の購読", e))?;

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

    /// Finds the template's input field, decides its text alignment, and subscribes to
    /// per-keystroke changes.
    ///
    /// Does nothing if it is not found (confirmation arrives through `ValueChanged`).
    /// The text stays left-aligned.
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

        // 数字は右へそろえる。`NumberBox` の既定は左寄せだが、macOS の
        // `NSTextField` と 0.3.0 までの Windows は右寄せだった。桁の位置が
        // そろって読み比べやすいので、そちらへ合わせる。
        //
        // `NumberBox` 自身の `TextAlignment` は投影元の Windows App SDK に
        // まだ無いので、入力欄へ直に書く。
        let _ = field.SetTextAlignment(TextAlignment::Right);

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
            match text.and_then(|text| this.parse_shown(&text)) {
                Some(shown) => this.accept(shown, false),
                // 打っている途中で読めない表示 (空欄や `-` だけ) は確定まで
                // 待つ。ただし受け取り済みの値だけは `NumberBox` へ渡す。
                None => this.sync_native(),
            }
            Ok(())
        });
        if field.TextChanged(&typed).is_ok() {
            *self.0.field.borrow_mut() = Some(field);
        }
    }

    /// Confirms the field's text as the value. Unreadable text is reverted to the
    /// previous value.
    ///
    /// If the input field has not been found, the `NumberBox` confirms the value
    /// itself, so nothing is done.
    fn commit_field(&self) {
        let Some(field) = self.0.field.borrow().clone() else {
            return;
        };
        let shown = field
            .Text()
            .ok()
            .and_then(|text| self.parse_shown(&text.to_string()))
            .unwrap_or_else(|| self.value());
        self.accept(shown, true);
    }

    /// Reads the text typed so far as a number. `None` if it cannot be read.
    ///
    /// The reader is the same one the `NumberBox` uses for confirmation (a
    /// `NumberFormatter` is also an `INumberParser`). The decimal point and digit
    /// grouping vary with regional settings, so reading with [`NumberSpec::parse`]
    /// (which reads only `.`) would leave text like `1,5` unreadable only while typing,
    /// with the notification coming out only on confirmation.
    ///
    /// Only when the reader cannot be obtained does it fall back to [`NumberSpec`].
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
        parse_with(&parser, text)
    }

    /// If the display already matches the value, passes it to the `NumberBox` too.
    ///
    /// Whether the step buttons are disabled at the ends is decided by the value the
    /// `NumberBox` holds, so without passing it, typing `3` into a `1..=3` field would
    /// leave the up button enabled (pressing it still does not leave the range).
    ///
    /// It is not passed when the display would change (when showing `3.7` with two
    /// decimal places, for example), since that would rewrite the display mid-typing.
    /// On confirmation and stepping, the `NumberBox` reads the display itself, so this
    /// causes no trouble.
    fn sync_native_if_shown(&self, value: f64) {
        let Some(field) = self.0.field.borrow().clone() else {
            return;
        };
        let shown = self
            .0
            .native
            .NumberFormatter()
            .and_then(|formatter| formatter.FormatDouble(value));
        let (Ok(shown), Ok(text)) = (shown, field.Text()) else {
            return;
        };
        if shown != text {
            return;
        }
        self.write_native(value);
    }

    /// Passes the value already received to the `NumberBox`. **The display is left as
    /// is.**
    ///
    /// Both the revert target on confirmation (Enter, leaving the field) and the base
    /// for stepping (`StepValue`) are the value the `NumberBox` holds. Only naui holds
    /// the value while typing, so unless it is passed once the display becomes
    /// unreadable, typing `12` and then `12x` would start both reverting and stepping
    /// from the old value.
    ///
    /// While the display is readable it need not be passed: on both confirmation and
    /// stepping, the `NumberBox` **first reads the display into its value**
    /// (`ValidateInput`), so a readable display leads to the same value.
    ///
    /// Passing a value makes the `NumberBox` rebuild the display with its
    /// `NumberFormatter`, so the text typed so far (just `-`, or `12x`) and the
    /// selection are written back.
    fn sync_native(&self) {
        let Some(field) = self.0.field.borrow().clone() else {
            // 入力欄が見つかっていないなら打鍵の経路も無く、値はずれない。
            return;
        };
        let Ok(text) = field.Text() else {
            return;
        };
        let start = field.SelectionStart().unwrap_or_default();
        let length = field.SelectionLength().unwrap_or_default();

        self.write_native(self.value());

        // 値が変わらなければ `NumberBox` は表示に触っていない。
        if field.Text().is_ok_and(|shown| shown == text) {
            return;
        }
        let previous = self.0.silent.replace(true);
        let _ = field.SetText(&text);
        let _ = field.SetSelectionStart(start);
        let _ = field.SetSelectionLength(length);
        self.0.silent.set(previous);
    }

    /// Replaces the rules and applies them to the `NumberBox` and the current value.
    ///
    /// Writing the range makes the `NumberBox` clamp its current value into the range
    /// and fire `ValueChanged` for it. Replacing the rules is promised **not to
    /// notify**, so notifications are stopped while writing, and naui's value is
    /// brought into line afterwards.
    fn update_spec(&self, edit: impl FnOnce(NumberSpec) -> NumberSpec) {
        self.0.spec.set(edit(self.0.spec.get()));
        let previous = self.0.silent.replace(true);
        let _ = self.write_native_spec();
        self.0.silent.set(previous);
        self.set_value(self.value());
    }

    /// Writes the range, step and decimal places to the `NumberBox`.
    ///
    /// Writing the minimum first can make the `NumberBox` push up the maximum while it
    /// is still old, but the maximum is written right after, so it ends up as specified
    /// (the maximum winning when the minimum is larger is also the same as
    /// [`NumberSpec`]).
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

    /// Receives the value on screen. With `commit`, writes it back to the `NumberBox`
    /// too.
    fn accept(&self, shown: f64, commit: bool) {
        let accepted = self.0.spec.get().clamp(shown);
        if commit {
            self.write_native(accepted);
            self.write_native_text(accepted);
        } else {
            self.sync_native_if_shown(accepted);
        }
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.handler.emit(accepted);
    }

    /// Writes the value to the `NumberBox`. Notifications during this are ignored.
    ///
    /// If the value changes, the `NumberBox` rebuilds the display with its
    /// `NumberFormatter`. Call [`write_native_text`](Self::write_native_text) right
    /// after, for the case where it does not change.
    fn write_native(&self, value: f64) {
        let previous = self.0.silent.replace(true);
        let _ = self.0.native.SetValue(value);
        self.0.silent.set(previous);
    }

    /// Brings the display back in line with the value. Notifications during this are
    /// ignored.
    ///
    /// The `NumberBox` rebuilds the display only **when the value changes**, so cases
    /// where the value is the same but only the display is off (receiving `12`, then
    /// rewriting it to `12x` and leaving the field, for example) are fixed here. The
    /// format is the same one the `NumberBox` uses, so the display is the same as if the
    /// `NumberBox` had written it.
    fn write_native_text(&self, value: f64) {
        let Some(field) = self.0.field.borrow().clone() else {
            // 入力欄が見つかっていないなら打鍵の経路も無い。値と表示は
            // `NumberBox` が自分でそろえている。
            return;
        };
        let Ok(text) = self
            .0
            .native
            .NumberFormatter()
            .and_then(|formatter| formatter.FormatDouble(value))
        else {
            return;
        };
        if field.Text().is_ok_and(|shown| shown == text) {
            return;
        }
        let previous = self.0.silent.replace(true);
        let _ = field.SetText(&text);
        self.0.silent.set(previous);
    }
}

/// A format that always writes the full number of decimal places.
///
/// No digit grouping is added. The other three backends ([`NumberSpec::format`],
/// `GtkSpinButton`, `<input type="number">`) all omit grouping, so this matches
/// them. The decimal point itself follows the regional settings (the same as
/// `NumberBox`'s default).
/// Reads a number with the reader attached to the format. `None` if it cannot be
/// read.
///
/// **Leading and trailing whitespace is trimmed before passing it.**
/// `INumberParser` does not read text with whitespace around it, but `NumberBox`
/// trims it before reading only on confirmation (`ValidateInput`). Without
/// trimming, pasting `" 12 "` alone would produce no notification while typing, only
/// on confirmation. [`NumberSpec::parse`] also ignores surrounding whitespace, so
/// this matches it.
fn parse_with(parser: &INumberParser, text: &str) -> Option<f64> {
    parser
        .ParseDouble(&HSTRING::from(text.trim()))
        .ok()
        .and_then(|value| value.Value().ok())
        .filter(|value| value.is_finite())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    /// `DecimalFormatter` is an OS WinRT type, so the Windows App SDK runtime is not
    /// needed. Unlike the integration tests that create native controls, this runs on CI
    /// as is.
    fn parser() -> INumberParser {
        formatter(0)
            .expect("数の書式")
            .cast::<INumberParser>()
            .expect("書式は読み手でもある")
    }

    /// The decimal point and digit grouping both vary with regional settings, so only
    /// integers, which read the same in every region, are checked.
    #[test]
    fn the_parser_ignores_surrounding_whitespace() {
        let parser = parser();
        for text in ["12", " 12", "12 ", " 12 ", "\t12\n", "  12  "] {
            assert_eq!(parse_with(&parser, text), Some(12.0), "{text:?}");
        }
        assert_eq!(parse_with(&parser, " -3 "), Some(-3.0));
    }

    /// Text typed so far that is not yet a number stays unread (waiting for
    /// confirmation).
    #[test]
    fn text_that_is_not_a_number_stays_unread() {
        let parser = parser();
        for text in ["", " ", "   ", "-", "+", "abc", "12ab", "1 2"] {
            assert_eq!(parse_with(&parser, text), None, "{text:?}");
        }
    }
}
