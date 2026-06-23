//! Deterministic pattern instance-position computation.
//!
//! Computes the grid-cell or scatter-point offsets for a pattern's motif
//! instances, expressed relative to the pattern's own bounds origin (i.e.
//! `(0, 0)`-based within a `bounds_w × bounds_h` box). The caller adds the
//! absolute bounds origin `(bx, by)` on top.
//!
//! Both layout kinds are fully deterministic: fixed loop order, pure integer
//! bit-mixing via [`super::hash::hash_unit`]. Same inputs → same `Vec` on
//! every machine.

use super::hash::hash_unit;

/// Parameters for a single pattern layout computation.
///
/// All fields carry their already-resolved values. The caller is responsible
/// for applying defaults (`seed` → `0`, `jitter` → `0.0`) before constructing
/// this struct.
#[derive(Clone, Copy)]
pub struct PatternLayout<'a> {
    /// Layout kind: `"grid"` or `"scatter"`. Any other value yields an empty
    /// result (validation has already flagged it).
    pub kind: &'a str,
    /// Width of the bounds box in px.
    pub bounds_w: f64,
    /// Height of the bounds box in px.
    pub bounds_h: f64,
    /// Cell spacing in px (`grid` only). Required to be `> 0`; ignored for
    /// `scatter`.
    pub spacing: Option<f64>,
    /// Number of scatter instances (`scatter` only). Required to be `> 0`;
    /// ignored for `grid`.
    pub count: Option<i64>,
    /// Seed passed to the bit-mixing hash for both jitter and scatter.
    pub seed: i64,
    /// Fraction of spacing applied as positional noise (`grid` only). `0.0`
    /// disables jitter entirely.
    pub jitter: f64,
}

/// Different seed mix for the vertical jitter axis so x and y jitter are
/// uncorrelated for the same cell.
const JITTER_Y_SEED_MIX: i64 = 0x5555;

/// Compute deterministic instance offsets relative to the pattern bounds origin.
///
/// Returns a `Vec<(f64, f64)>` of `(ox, oy)` values, each in the range
/// `[0, bounds_w) × [0, bounds_h)` for `scatter`, or spanning the lattice for
/// `grid` (jitter may push a cell slightly outside the lattice but the clip
/// in the caller handles that). The caller adds the absolute bounds origin
/// `(bx, by)` to each offset before use.
///
/// Ordering:
/// - `grid`: row-major — `(col=0,row=0)`, `(col=1,row=0)`, …, then next row.
/// - `scatter`: ascending `i` from `0` to `count-1`.
///
/// Pure: no side effects, no randomness, no time, no allocations beyond the
/// returned `Vec`.
pub fn pattern_positions(p: PatternLayout<'_>) -> Vec<(f64, f64)> {
    match p.kind {
        "grid" => grid_positions(p),
        "scatter" => scatter_positions(p),
        _ => Vec::new(),
    }
}

fn grid_positions(p: PatternLayout<'_>) -> Vec<(f64, f64)> {
    let s = match p.spacing.filter(|&v| v > 0.0) {
        Some(v) => v,
        None => return Vec::new(),
    };

    let mut positions = Vec::new();
    let mut row: i64 = 0;
    while (row as f64) * s < p.bounds_h {
        let mut col: i64 = 0;
        while (col as f64) * s < p.bounds_w {
            let base_x = (col as f64) * s;
            let base_y = (row as f64) * s;
            let (jx, jy) = if p.jitter > 0.0 {
                let jx = (hash_unit(col, row, p.seed) * 2.0 - 1.0) * p.jitter * s;
                let jy =
                    (hash_unit(col, row, p.seed ^ JITTER_Y_SEED_MIX) * 2.0 - 1.0) * p.jitter * s;
                (jx, jy)
            } else {
                (0.0, 0.0)
            };
            positions.push((base_x + jx, base_y + jy));
            col += 1;
        }
        row += 1;
    }
    positions
}

fn scatter_positions(p: PatternLayout<'_>) -> Vec<(f64, f64)> {
    let count = match p.count.filter(|&v| v > 0) {
        Some(v) => v,
        None => return Vec::new(),
    };

    let mut positions = Vec::with_capacity(count as usize);
    for i in 0..count {
        let ox = hash_unit(i, 0, p.seed) * p.bounds_w;
        let oy = hash_unit(i, 1, p.seed) * p.bounds_h;
        positions.push((ox, oy));
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout_grid(
        bw: f64,
        bh: f64,
        spacing: f64,
        jitter: f64,
        seed: i64,
    ) -> PatternLayout<'static> {
        PatternLayout {
            kind: "grid",
            bounds_w: bw,
            bounds_h: bh,
            spacing: Some(spacing),
            count: None,
            seed,
            jitter,
        }
    }

    fn layout_scatter(bw: f64, bh: f64, count: i64, seed: i64) -> PatternLayout<'static> {
        PatternLayout {
            kind: "scatter",
            bounds_w: bw,
            bounds_h: bh,
            spacing: None,
            count: Some(count),
            seed,
            jitter: 0.0,
        }
    }

    #[test]
    fn grid_exact_four_cells_no_jitter() {
        // bounds 100×100, spacing 50, jitter 0
        // rows: 0*50=0 < 100, 1*50=50 < 100, 2*50=100 not < 100 → rows 0,1
        // cols: same → cols 0,1
        // row-major: (0,0),(50,0),(0,50),(50,50)
        let positions = pattern_positions(layout_grid(100.0, 100.0, 50.0, 0.0, 0));
        assert_eq!(positions.len(), 4, "expected 4 cells; got {positions:?}");
        assert_eq!(positions[0], (0.0, 0.0));
        assert_eq!(positions[1], (50.0, 0.0));
        assert_eq!(positions[2], (0.0, 50.0));
        assert_eq!(positions[3], (50.0, 50.0));
    }

    #[test]
    fn grid_missing_spacing_returns_empty() {
        let p = PatternLayout {
            kind: "grid",
            bounds_w: 100.0,
            bounds_h: 100.0,
            spacing: None,
            count: None,
            seed: 0,
            jitter: 0.0,
        };
        assert!(pattern_positions(p).is_empty());
    }

    #[test]
    fn grid_zero_spacing_returns_empty() {
        let positions = pattern_positions(layout_grid(100.0, 100.0, 0.0, 0.0, 0));
        assert!(positions.is_empty());
    }

    #[test]
    fn scatter_correct_count() {
        let positions = pattern_positions(layout_scatter(200.0, 150.0, 5, 7));
        assert_eq!(positions.len(), 5, "expected 5 scatter instances");
    }

    #[test]
    fn scatter_within_bounds() {
        let bw = 300.0;
        let bh = 200.0;
        let positions = pattern_positions(layout_scatter(bw, bh, 20, 42));
        for (ox, oy) in &positions {
            assert!(*ox >= 0.0 && *ox < bw, "scatter ox={ox} out of [0, {bw})");
            assert!(*oy >= 0.0 && *oy < bh, "scatter oy={oy} out of [0, {bh})");
        }
    }

    #[test]
    fn scatter_missing_count_returns_empty() {
        let p = PatternLayout {
            kind: "scatter",
            bounds_w: 100.0,
            bounds_h: 100.0,
            spacing: None,
            count: None,
            seed: 0,
            jitter: 0.0,
        };
        assert!(pattern_positions(p).is_empty());
    }

    #[test]
    fn scatter_zero_count_returns_empty() {
        let positions = pattern_positions(layout_scatter(100.0, 100.0, 0, 0));
        assert!(positions.is_empty());
    }

    #[test]
    fn determinism_grid_same_input_same_output() {
        let p = layout_grid(120.0, 80.0, 25.0, 0.4, 11);
        assert_eq!(pattern_positions(p), pattern_positions(p));
    }

    #[test]
    fn determinism_scatter_same_input_same_output() {
        let p = layout_scatter(200.0, 200.0, 7, 99);
        assert_eq!(pattern_positions(p), pattern_positions(p));
    }

    #[test]
    fn unknown_kind_returns_empty() {
        let p = PatternLayout {
            kind: "hexagonal",
            bounds_w: 100.0,
            bounds_h: 100.0,
            spacing: Some(20.0),
            count: Some(10),
            seed: 0,
            jitter: 0.0,
        };
        assert!(pattern_positions(p).is_empty());
    }
}
