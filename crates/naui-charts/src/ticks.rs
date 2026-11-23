//! Tick values for an axis, at steps that read well.

/// The steps an axis is divided into, times a power of ten.
const STEPS: [f64; 4] = [1.0, 2.0, 2.5, 5.0];

/// Values beyond this are brought back to it, so that spans and steps stay finite.
const LIMIT: f64 = 1e300;

/// Ranges narrower than this are treated as one value.
const TINY: f64 = 1e-200;

/// The most ticks one axis gets, whatever is asked for.
const MAX_TICKS: usize = 1000;

/// Evenly spaced values for an axis, a step of 1, 2, 2.5 or 5 times a power of ten
/// apart, with labels that show as many decimals as the step needs.
///
/// [`nice`](Self::nice) widens the range out to whole steps, the way an axis that
/// is fitted to its data does; [`within`](Self::within) keeps to the range given,
/// for an axis whose range was set.
///
/// ```
/// # use naui_charts::Ticks;
/// let ticks = Ticks::nice(3.0, 97.0, 6);
/// assert_eq!(ticks.values().collect::<Vec<_>>(), [0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
/// assert_eq!(ticks.label(40.0), "40");
///
/// let ticks = Ticks::within(0.0, 1.0, 4);
/// assert_eq!(ticks.step(), 0.25);
/// assert_eq!(ticks.label(0.25), "0.25");
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ticks {
    start: f64,
    step: f64,
    count: usize,
    decimals: usize,
    /// Labels are written with an exponent (very large or very small values).
    exponent: bool,
}

impl Ticks {
    /// Ticks covering `min..=max`, widened out to whole steps: the first tick is at
    /// or below `min` and the last at or above `max`.
    ///
    /// `target` is about how many ticks are wanted: the step is the finest that
    /// gives no more than one tick over it. A range that is empty, reversed or not finite is mended first: the ends
    /// are swapped, one value gets a range around it, and anything not finite
    /// becomes `0..1`.
    pub fn nice(min: f64, max: f64, target: usize) -> Ticks {
        let (low, high) = mend(min, max);
        let step = choose_step(low, high, target, |low, high, step| {
            snap(high, step, f64::ceil) - snap(low, step, f64::floor)
        });
        let start = snap(low, step, f64::floor);
        let end = snap(high, step, f64::ceil);
        Ticks::from_parts(start, end, step)
    }

    /// Ticks at whole steps inside `min..=max` only. The ends are ticks only when
    /// they fall on a step. Empty, reversed or non-finite ranges are mended as in
    /// [`nice`](Self::nice).
    pub fn within(min: f64, max: f64, target: usize) -> Ticks {
        let (low, high) = mend(min, max);
        let step = choose_step(low, high, target, |low, high, step| {
            snap(high, step, f64::floor) - snap(low, step, f64::ceil)
        });
        let start = snap(low, step, f64::ceil);
        let end = snap(high, step, f64::floor);
        Ticks::from_parts(start, end, step)
    }

    fn from_parts(start: f64, end: f64, step: f64) -> Ticks {
        let count = if end >= start {
            // 本数は上限で止める (`as usize` の飽和の後で 1 を足すと溢れる)。
            ((end - start) / step).round().min(MAX_TICKS as f64 - 1.0) as usize + 1
        } else {
            0
        };
        let largest = start.abs().max(end.abs());
        let exponent = largest >= 1e15 || (step < 1e-4 && step > 0.0);
        let decimals = if exponent {
            // 仮数部は、刻みが最大の値の桁で何桁目に来るかで決める。
            let magnitude = 10f64.powi(largest.log10().floor() as i32);
            decimals_of(step / magnitude).min(6)
        } else {
            decimals_of(step)
        };
        Ticks {
            start,
            step,
            count,
            decimals,
            exponent,
        }
    }

    /// The distance between two ticks.
    pub fn step(&self) -> f64 {
        self.step
    }

    /// How many ticks there are.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether there are no ticks (a [`within`](Self::within) range narrower than a
    /// step and holding none).
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The tick values, from the lowest.
    pub fn values(&self) -> impl Iterator<Item = f64> + '_ {
        // 足し込むと誤差が溜まるので、毎回 start + i * step で作る。
        (0..self.count).map(move |i| {
            let value = self.start + i as f64 * self.step;
            if value.abs() < self.step * 1e-9 {
                0.0
            } else {
                value
            }
        })
    }

    /// The lowest and the highest tick, or `None` when there are none.
    pub fn range(&self) -> Option<(f64, f64)> {
        let mut values = self.values();
        let first = values.next()?;
        Some((first, values.last().unwrap_or(first)))
    }

    /// `value` written with the decimals the step needs: "0.25" for a step of 0.25,
    /// "40" for a step of 20, and with an exponent for very large or small values.
    /// It never writes "-0".
    pub fn label(&self, value: f64) -> String {
        let text = if self.exponent {
            format!("{:.*e}", self.decimals, value)
        } else {
            format!("{:.*}", self.decimals, value)
        };
        match text.strip_prefix('-') {
            // 丸めて 0 になった負の値に "-" を付けない。
            Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
            Some(rest) if self.exponent && is_zero_mantissa(rest) => rest.to_string(),
            _ => text,
        }
    }
}

/// Whether an exponent form such as "0.00e0" is zero.
fn is_zero_mantissa(text: &str) -> bool {
    text.split('e')
        .next()
        .is_some_and(|mantissa| mantissa.chars().all(|c| c == '0' || c == '.'))
}

/// Puts the range in order and gives it some width.
fn mend(min: f64, max: f64) -> (f64, f64) {
    if !min.is_finite() || !max.is_finite() {
        return (0.0, 1.0);
    }
    let (low, high) = if min <= max { (min, max) } else { (max, min) };
    let (low, high) = (low.clamp(-LIMIT, LIMIT), high.clamp(-LIMIT, LIMIT));
    let largest = low.abs().max(high.abs());
    // 幅が値に比べて無いに等しいと、刻みが浮動小数点の精度を下回る。
    // 幅そのものが極端に小さいときも、10 の累乗が非正規化数で崩れる。
    if high - low <= largest * 1e-12 || high - low < TINY {
        let center = (low + high) / 2.0;
        if center.abs() < TINY {
            return (0.0, 1.0);
        }
        let half = center.abs() * 0.1;
        return (center - half, center + half);
    }
    (low, high)
}

/// The finest step of 1, 2, 2.5 or 5 times a power of ten with which `low..high`
/// takes no more than `target + 1` ticks. `span` tells the distance from the first
/// tick to the last for a step.
fn choose_step(low: f64, high: f64, target: usize, span: impl Fn(f64, f64, f64) -> f64) -> f64 {
    let target = target.clamp(2, MAX_TICKS);
    let allowed = (target + 1) as f64;
    // 目安の刻み (target - 1 等分) の 1 桁下から細かい順に試し、収まる最初のものを取る。
    let raw = (high - low) / (target - 1) as f64;
    let first = raw.log10().floor() as i32 - 1;
    let mut last = high - low;
    for exponent in first..first + 4 {
        let magnitude = 10f64.powi(exponent);
        for factor in STEPS {
            let step = factor * magnitude;
            if !step.is_finite() || step <= 0.0 {
                continue;
            }
            last = step;
            let count = span(low, high, step) / step + 1.0;
            if count <= allowed + 1e-9 {
                return step;
            }
        }
    }
    last
}

/// `value` moved to a whole number of steps with `round` (floor or ceil), allowing
/// for values a hair off a step.
fn snap(value: f64, step: f64, round: fn(f64) -> f64) -> f64 {
    let steps = value / step;
    let nearest = steps.round();
    if (steps - nearest).abs() < 1e-9 {
        nearest * step
    } else {
        round(steps) * step
    }
}

/// How many decimals it takes to write `step` exactly (0.25 takes 2).
fn decimals_of(step: f64) -> usize {
    for decimals in 0..=17 {
        let scaled = step * 10f64.powi(decimals as i32);
        if (scaled - scaled.round()).abs() <= scaled.abs() * 1e-9 {
            return decimals;
        }
    }
    17
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(ticks: &Ticks) -> Vec<f64> {
        ticks.values().collect()
    }

    #[test]
    fn steps_are_one_two_two_and_a_half_or_five() {
        assert_eq!(Ticks::nice(0.0, 10.0, 6).step(), 2.0);
        assert_eq!(Ticks::nice(0.0, 1.0, 5).step(), 0.2);
        assert_eq!(Ticks::nice(0.0, 1.0, 4).step(), 0.25);
        assert_eq!(Ticks::nice(0.0, 100.0, 3).step(), 50.0);
        assert_eq!(Ticks::nice(0.0, 7.0, 8).step(), 1.0);
        assert_eq!(Ticks::nice(0.0, 0.03, 4).step(), 0.01);
        assert_eq!(Ticks::nice(-120.0, 380.0, 6).step(), 100.0);
    }

    #[test]
    fn nice_covers_the_data_and_within_keeps_inside() {
        let ticks = Ticks::nice(3.0, 97.0, 6);
        assert_eq!(values(&ticks), [0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        let ticks = Ticks::within(3.0, 97.0, 6);
        assert_eq!(values(&ticks), [20.0, 40.0, 60.0, 80.0]);
        // 端がちょうど刻みに乗るときは含める。
        let ticks = Ticks::within(0.0, 100.0, 6);
        assert_eq!(ticks.range(), Some((0.0, 100.0)));
        // 負の範囲。
        let ticks = Ticks::nice(-7.0, -1.0, 4);
        assert_eq!(values(&ticks), [-8.0, -6.0, -4.0, -2.0, 0.0]);
    }

    #[test]
    fn values_do_not_drift() {
        // 0.1 を足し込むと 0.30000000000000004 になる。
        let ticks = Ticks::nice(0.0, 1.0, 11);
        assert_eq!(ticks.step(), 0.1);
        let labels: Vec<_> = ticks.values().map(|v| ticks.label(v)).collect();
        assert_eq!(labels[3], "0.3");
        assert_eq!(labels.last().map(String::as_str), Some("1.0"));
        // 0 をまたぐと -0 が出やすい。
        let ticks = Ticks::nice(-0.3, 0.3, 7);
        assert!(ticks.values().any(|v| v == 0.0 && v.is_sign_positive()));
        assert_eq!(ticks.label(-0.0), "0.0");
        assert_eq!(ticks.label(-1e-12), "0.0");
    }

    #[test]
    fn labels_take_the_decimals_of_the_step() {
        assert_eq!(Ticks::nice(0.0, 100.0, 6).label(40.0), "40");
        assert_eq!(Ticks::nice(0.0, 1.0, 4).label(0.5), "0.50");
        assert_eq!(Ticks::nice(0.0, 1.0, 3).label(0.5), "0.5");
        assert_eq!(Ticks::nice(1000.0, 5000.0, 5).label(2000.0), "2000");
    }

    #[test]
    fn extreme_values_use_an_exponent() {
        let ticks = Ticks::nice(0.0, 3e18, 4);
        assert!(ticks.label(1e18).contains('e'), "{}", ticks.label(1e18));
        let ticks = Ticks::nice(0.0, 3e-7, 4);
        assert!(ticks.label(1e-7).contains('e'), "{}", ticks.label(1e-7));
        assert!(!ticks.label(0.0).starts_with('-'));
    }

    #[test]
    fn broken_ranges_are_mended() {
        // 空・全部 NaN の系列は NaN や無限大の範囲になる。
        for (min, max) in [
            (f64::NAN, f64::NAN),
            (f64::INFINITY, f64::NEG_INFINITY),
            (0.0, f64::NAN),
        ] {
            let ticks = Ticks::nice(min, max, 5);
            assert_eq!(ticks.range(), Some((0.0, 1.0)), "{min}..{max}");
        }
        // 1 つの値だけ。
        assert_eq!(Ticks::nice(0.0, 0.0, 5).range(), Some((0.0, 1.0)));
        let ticks = Ticks::nice(5.0, 5.0, 5);
        let (low, high) = ticks.range().unwrap();
        assert!(low < 5.0 && high > 5.0, "{low}..{high}");
        // 逆向き。
        assert_eq!(Ticks::nice(10.0, 0.0, 6), Ticks::nice(0.0, 10.0, 6));
        // 値に比べて幅が無いに等しい。
        let ticks = Ticks::nice(1.0, 1.0 + 1e-15, 5);
        assert!(ticks.len() <= 10, "{ticks:?}");
    }

    #[test]
    fn huge_and_tiny_ranges_stay_finite_and_few() {
        for (min, max) in [
            (-f64::MAX, f64::MAX),
            (0.0, f64::MAX),
            (-1e-300, 1e-300),
            (0.0, f64::MIN_POSITIVE),
            (1e15, 1e15 + 3.0),
        ] {
            for target in [0, 1, 2, 5, 12, usize::MAX] {
                let ticks = Ticks::nice(min, max, target);
                assert!(ticks.len() >= 2, "{min}..{max} / {target}: {ticks:?}");
                assert!(ticks.len() <= MAX_TICKS);
                assert!(ticks.values().all(f64::is_finite), "{ticks:?}");
                assert!(ticks.step() > 0.0 && ticks.step().is_finite());
            }
        }
    }

    #[test]
    fn the_finest_step_that_fits_is_taken() {
        // ±55 を 4 本前後で: 50 刻みだと -100..100 の 5 本で、目標 +1 に収まる。
        let ticks = Ticks::nice(-55.0, 55.0, 4);
        assert_eq!(ticks.range(), Some((-100.0, 100.0)));
        assert_eq!(ticks.step(), 50.0);
        let ticks = Ticks::nice(-55.0, 55.0, 7);
        assert_eq!(ticks.range(), Some((-60.0, 60.0)));
        for target in 2..20 {
            for (min, max) in [(0.0, 1.0), (-55.0, 55.0), (3.0, 97.0), (140.0, 190.0)] {
                assert!(Ticks::nice(min, max, target).len() <= target + 1);
                assert!(Ticks::within(min, max, target).len() <= target + 1);
            }
        }
    }

    #[test]
    fn many_ticks_are_capped() {
        let ticks = Ticks::nice(0.0, 1.0, usize::MAX);
        assert!(ticks.len() <= MAX_TICKS);
        assert!(Ticks::nice(0.0, 1.0, 0).len() <= 3);
    }

    #[test]
    fn a_narrow_within_range_may_hold_no_tick() {
        let ticks = Ticks::within(0.11, 0.19, 2);
        assert!(ticks.len() <= 3, "{ticks:?}");
        assert!(
            ticks.values().all(|v| (0.11..=0.19).contains(&v)),
            "{ticks:?}"
        );
        let ticks = Ticks::within(0.21, 0.29, 2);
        assert!(ticks.is_empty() || ticks.values().all(|v| (0.21..=0.29).contains(&v)));
    }
}
