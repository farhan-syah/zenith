//! Deterministic scale and tick computation for chart axes.
//!
//! All functions are pure and free of floating-point non-determinism:
//! - No `f64::log10`, `f64::powf`, `f64::powi`, or `f64::ln`.
//! - Power-of-ten magnitude is computed via a multiply/divide loop.
//! - No `HashMap`/`HashSet`; no time, no randomness.

use zenith_core::ChartSeries;

// ── LinearScale ────────────────────────────────────────────────────────────────

/// A linear mapping from data space to pixel space.
///
/// `pixel_min` and `pixel_max` can be inverted (`pixel_min > pixel_max`) to
/// flip the axis, which is standard for Y axes where screen y grows downward
/// but data values grow upward.
#[derive(Clone, Copy, Debug)]
pub(super) struct LinearScale {
    /// Minimum data value (inclusive).
    pub(super) data_min: f64,
    /// Maximum data value (inclusive).
    pub(super) data_max: f64,
    /// Pixel coordinate corresponding to `data_min`.
    pub(super) pixel_min: f64,
    /// Pixel coordinate corresponding to `data_max`.
    pub(super) pixel_max: f64,
}

impl LinearScale {
    /// Map a data value to a pixel coordinate, clamped to
    /// `[pixel_min.min(pixel_max), pixel_min.max(pixel_max)]`.
    ///
    /// Returns `pixel_min` when the data range is zero (degenerate scale).
    pub(super) fn map(&self, value: f64) -> f64 {
        let data_range = self.data_max - self.data_min;
        if data_range == 0.0 || !data_range.is_finite() {
            return self.pixel_min;
        }
        let t = (value - self.data_min) / data_range;
        let px = self.pixel_min + t * (self.pixel_max - self.pixel_min);
        let lo = self.pixel_min.min(self.pixel_max);
        let hi = self.pixel_min.max(self.pixel_max);
        px.clamp(lo, hi)
    }
}

// ── Tick ───────────────────────────────────────────────────────────────────────

/// A single axis tick with its data value and computed pixel position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Tick {
    /// The data value at this tick.
    pub(super) value: f64,
    /// The pixel coordinate of this tick (from `scale.map(value)`).
    pub(super) pixel: f64,
}

// ── Internal helpers ───────────────────────────────────────────────────────────

/// Largest power of 10 that is ≤ `x` for `x > 0`.
///
/// Computed via a multiply/divide loop — no `log10`/`powf` used.
fn pow10_floor(x: f64) -> f64 {
    debug_assert!(x > 0.0 && x.is_finite());
    let mut p = 1.0_f64;
    if x >= 1.0 {
        while p * 10.0 <= x {
            p *= 10.0;
        }
    } else {
        while p > x {
            p /= 10.0;
        }
    }
    p
}

// ── nice_ticks ─────────────────────────────────────────────────────────────────

/// Compute Wilkinson 1/2/5×10^n "nice" tick values for a `LinearScale`.
///
/// Targets approximately `target_count` ticks. Returns an ascending list of
/// `Tick` values that cover (and may slightly exceed) `[scale.data_min,
/// scale.data_max]`. Returns an empty `Vec` when:
/// - The data range is ≤ 0 or non-finite.
/// - `target_count` is 0.
///
/// Deterministic: identical inputs always produce identical output. The tick
/// step is always a 1/2/5 × 10^n value.
pub(super) fn nice_ticks(scale: &LinearScale, target_count: u32) -> Vec<Tick> {
    if target_count == 0 {
        return Vec::new();
    }
    let range = scale.data_max - scale.data_min;
    if range <= 0.0 || !range.is_finite() {
        return Vec::new();
    }

    // Rough step size.
    let rough_step = range / target_count as f64;
    if rough_step <= 0.0 || !rough_step.is_finite() {
        return Vec::new();
    }

    // Snap to nearest 1/2/5 × 10^k.
    let magnitude = pow10_floor(rough_step);
    let normalized = rough_step / magnitude;
    let nice = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    let step = nice * magnitude;
    if step <= 0.0 || !step.is_finite() {
        return Vec::new();
    }

    // First tick: smallest multiple of `step` that is >= data_min.
    let first_raw = (scale.data_min / step).ceil() * step;
    // Tiny epsilon to catch floating-point overshoot at data_max.
    let epsilon = step * 1e-9;

    let mut ticks = Vec::new();
    let mut value = first_raw;
    let mut iters = 0_u32;
    while value <= scale.data_max + epsilon && iters <= 1000 {
        ticks.push(Tick {
            value,
            pixel: scale.map(value),
        });
        value += step;
        iters += 1;
    }

    ticks
}

// ── data_range ─────────────────────────────────────────────────────────────────

/// Compute the data min/max across all series values, applying optional
/// `axis_min`/`axis_max` overrides.
///
/// Non-finite values in series data are skipped. Returns `None` when there
/// are no finite values at all (empty or all-NaN/Inf data).
pub(super) fn data_range(
    series: &[ChartSeries],
    axis_min: Option<f64>,
    axis_max: Option<f64>,
) -> Option<(f64, f64)> {
    let mut found_min = f64::INFINITY;
    let mut found_max = f64::NEG_INFINITY;
    let mut has_any = false;

    for s in series {
        for &v in &s.values {
            if v.is_finite() {
                if v < found_min {
                    found_min = v;
                }
                if v > found_max {
                    found_max = v;
                }
                has_any = true;
            }
        }
    }

    if !has_any {
        // No finite data; overrides alone don't give us a range.
        match (axis_min, axis_max) {
            (Some(lo), Some(hi)) if lo.is_finite() && hi.is_finite() => {
                return Some((lo, hi));
            }
            _ => return None,
        }
    }

    let lo = axis_min.filter(|v| v.is_finite()).unwrap_or(found_min);
    let hi = axis_max.filter(|v| v.is_finite()).unwrap_or(found_max);
    Some((lo, hi))
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_scale(data_min: f64, data_max: f64) -> LinearScale {
        LinearScale {
            data_min,
            data_max,
            pixel_min: 200.0, // bottom (inverted Y: data_min → bottom pixel)
            pixel_max: 0.0,   // top
        }
    }

    // ── pow10_floor ──────────────────────────────────────────────────────────

    #[test]
    fn pow10_floor_exact_powers() {
        assert_eq!(pow10_floor(1.0), 1.0);
        assert_eq!(pow10_floor(10.0), 10.0);
        assert_eq!(pow10_floor(100.0), 100.0);
    }

    #[test]
    fn pow10_floor_between_powers() {
        assert_eq!(pow10_floor(7.0), 1.0);
        assert_eq!(pow10_floor(99.0), 10.0);
        assert_eq!(pow10_floor(999.0), 100.0);
    }

    #[test]
    fn pow10_floor_fractional() {
        // 0.05 → floor power of ten is 0.01
        assert!((pow10_floor(0.05) - 0.01).abs() < 1e-12);
    }

    // ── LinearScale::map ─────────────────────────────────────────────────────

    #[test]
    fn map_midpoint() {
        let scale = LinearScale {
            data_min: 0.0,
            data_max: 100.0,
            pixel_min: 0.0,
            pixel_max: 200.0,
        };
        assert!((scale.map(50.0) - 100.0).abs() < 1e-10);
    }

    #[test]
    fn map_extremes() {
        let scale = LinearScale {
            data_min: 0.0,
            data_max: 100.0,
            pixel_min: 0.0,
            pixel_max: 300.0,
        };
        assert!((scale.map(0.0) - 0.0).abs() < 1e-10);
        assert!((scale.map(100.0) - 300.0).abs() < 1e-10);
    }

    #[test]
    fn map_clamping() {
        let scale = LinearScale {
            data_min: 0.0,
            data_max: 100.0,
            pixel_min: 0.0,
            pixel_max: 100.0,
        };
        // Values outside the data range clamp to pixel bounds.
        assert!((scale.map(-50.0) - 0.0).abs() < 1e-10);
        assert!((scale.map(200.0) - 100.0).abs() < 1e-10);
    }

    #[test]
    fn map_inverted_y_axis() {
        // pixel_min = bottom (200), pixel_max = top (0): data_min maps to bottom.
        let scale = make_scale(0.0, 100.0);
        assert!((scale.map(0.0) - 200.0).abs() < 1e-10);
        assert!((scale.map(100.0) - 0.0).abs() < 1e-10);
        assert!((scale.map(50.0) - 100.0).abs() < 1e-10);
    }

    #[test]
    fn map_degenerate_zero_range() {
        let scale = LinearScale {
            data_min: 5.0,
            data_max: 5.0,
            pixel_min: 100.0,
            pixel_max: 0.0,
        };
        // Degenerate: returns pixel_min.
        assert_eq!(scale.map(5.0), 100.0);
    }

    // ── nice_ticks ───────────────────────────────────────────────────────────

    #[test]
    fn nice_ticks_zero_to_hundred_target_five() {
        let scale = LinearScale {
            data_min: 0.0,
            data_max: 100.0,
            pixel_min: 400.0,
            pixel_max: 0.0,
        };
        let ticks = nice_ticks(&scale, 5);
        // Must have at least 3 and at most 8 ticks.
        assert!(
            ticks.len() >= 3 && ticks.len() <= 8,
            "expected 3..=8 ticks, got {}",
            ticks.len()
        );
        // All tick values must be in/around the range.
        for t in &ticks {
            assert!(
                t.value >= -1.0 && t.value <= 101.0,
                "tick {} out of range",
                t.value
            );
        }
        // Values must be ascending.
        for w in ticks.windows(2) {
            assert!(w[1].value > w[0].value, "ticks not ascending");
        }
        // Step must be a 1/2/5 × 10^n value.
        if ticks.len() >= 2 {
            let step = ticks[1].value - ticks[0].value;
            assert!(step > 0.0, "step must be positive");
            let magnitude = pow10_floor(step);
            let normalized = (step / magnitude).round();
            assert!(
                normalized == 1.0 || normalized == 2.0 || normalized == 5.0 || normalized == 10.0,
                "step {} is not a 1/2/5×10^n value (normalized={})",
                step,
                normalized
            );
        }
        // First tick at 0.0 and last at 100.0 for this canonical range.
        assert!(
            (ticks.first().map(|t| t.value).unwrap_or(1.0)).abs() < 1e-9,
            "first tick should be 0"
        );
        assert!(
            (ticks.last().map(|t| t.value).unwrap_or(0.0) - 100.0).abs() < 1e-9,
            "last tick should be 100"
        );
    }

    #[test]
    fn nice_ticks_deterministic() {
        let scale = LinearScale {
            data_min: 0.0,
            data_max: 75.0,
            pixel_min: 300.0,
            pixel_max: 0.0,
        };
        let a = nice_ticks(&scale, 5);
        let b = nice_ticks(&scale, 5);
        assert_eq!(a, b, "nice_ticks must be deterministic");
    }

    #[test]
    fn nice_ticks_degenerate_zero_range() {
        let scale = LinearScale {
            data_min: 5.0,
            data_max: 5.0,
            pixel_min: 200.0,
            pixel_max: 0.0,
        };
        let ticks = nice_ticks(&scale, 5);
        assert!(ticks.is_empty(), "zero-range must yield empty tick list");
    }

    #[test]
    fn nice_ticks_degenerate_target_zero() {
        let scale = LinearScale {
            data_min: 0.0,
            data_max: 100.0,
            pixel_min: 200.0,
            pixel_max: 0.0,
        };
        let ticks = nice_ticks(&scale, 0);
        assert!(
            ticks.is_empty(),
            "target_count=0 must yield empty tick list"
        );
    }

    #[test]
    fn nice_ticks_degenerate_non_finite() {
        let scale = LinearScale {
            data_min: f64::NAN,
            data_max: 100.0,
            pixel_min: 200.0,
            pixel_max: 0.0,
        };
        let ticks = nice_ticks(&scale, 5);
        assert!(
            ticks.is_empty(),
            "non-finite range must yield empty tick list"
        );
    }

    // ── data_range ───────────────────────────────────────────────────────────

    fn series_from(values: Vec<f64>) -> ChartSeries {
        ChartSeries {
            label: None,
            color: None,
            label_color: None,
            data_ref: None,
            values,
        }
    }

    #[test]
    fn data_range_min_max_across_series() {
        let s1 = series_from(vec![10.0, 20.0, 5.0]);
        let s2 = series_from(vec![30.0, -3.0, 15.0]);
        let result = data_range(&[s1, s2], None, None);
        assert_eq!(result, Some((-3.0, 30.0)));
    }

    #[test]
    fn data_range_axis_min_override() {
        let s = series_from(vec![10.0, 50.0]);
        let result = data_range(&[s], Some(0.0), None);
        assert_eq!(result, Some((0.0, 50.0)));
    }

    #[test]
    fn data_range_axis_max_override() {
        let s = series_from(vec![10.0, 50.0]);
        let result = data_range(&[s], None, Some(100.0));
        assert_eq!(result, Some((10.0, 100.0)));
    }

    #[test]
    fn data_range_both_overrides() {
        let s = series_from(vec![10.0, 50.0]);
        let result = data_range(&[s], Some(-10.0), Some(80.0));
        assert_eq!(result, Some((-10.0, 80.0)));
    }

    #[test]
    fn data_range_empty_series() {
        let result = data_range(&[], None, None);
        assert!(result.is_none());
    }

    #[test]
    fn data_range_skips_non_finite() {
        let s = series_from(vec![f64::NAN, f64::INFINITY, 42.0]);
        let result = data_range(&[s], None, None);
        assert_eq!(result, Some((42.0, 42.0)));
    }

    #[test]
    fn data_range_all_non_finite_no_override() {
        let s = series_from(vec![f64::NAN, f64::INFINITY]);
        let result = data_range(&[s], None, None);
        assert!(result.is_none());
    }
}
