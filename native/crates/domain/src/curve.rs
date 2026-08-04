//! Part-level expression curves (`UCurve`).
//!
//! Mirrors `OpenUtau.Core/Ustx/UCurve.cs`. Note that OpenUtau's `UCurve`
//! has no `shape` array — only `xs`, `ys` and `abbr`. The `simplify()` /
//! `MergeCurves` editor helpers are not implemented here; they are UI-side
//! features with no effect on the serialized format.

use serde::{Deserialize, Serialize};

use crate::csharp_round;

/// Part-level expression curve (`UCurve`): `xs`/`ys` point pairs (ticks
/// relative to the part start / expression units) plus the expression
/// abbreviation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UCurve {
    #[serde(default)]
    pub xs: Vec<i32>,
    #[serde(default)]
    pub ys: Vec<i32>,
    #[serde(default)]
    pub abbr: String,
}

impl UCurve {
    /// Point snapping interval in ticks (`UCurve.interval`).
    pub const INTERVAL: i32 = 5;

    pub fn new(abbr: impl Into<String>) -> Self {
        UCurve { xs: Vec::new(), ys: Vec::new(), abbr: abbr.into() }
    }

    /// A curve is empty when it has no points or all points are zero.
    pub fn is_empty(&self) -> bool {
        self.xs.is_empty() || self.ys.iter().all(|&y| y == 0)
    }

    /// Sample the curve at `x` with linear interpolation between points.
    /// Returns `None` when the curve has no points or `x` is outside the
    /// point range (OpenUtau falls back to the descriptor default value in
    /// that case; callers should apply `default_value` themselves).
    pub fn sample(&self, x: i32) -> Option<i32> {
        if self.xs.is_empty() {
            return None;
        }
        let idx = self.xs.partition_point(|&v| v < x);
        if idx < self.xs.len() && self.xs[idx] == x {
            return Some(self.ys[idx]);
        }
        if idx > 0 && idx < self.xs.len() {
            let (x1, y1) = (self.xs[idx - 1], self.ys[idx - 1]);
            let (x2, y2) = (self.xs[idx], self.ys[idx]);
            let t = (x - x1) as f64 / (x2 - x1) as f64;
            return Some(csharp_round(y1 as f64 + (y2 - y1) as f64 * t));
        }
        None
    }

    /// `UCurve.IsEmptyBetween`: the curve has the default value at both
    /// endpoints and every stored point strictly between them.
    pub fn is_empty_between(&self, x0: i32, x1: i32, default_value: i32) -> bool {
        if self.sample(x0) != Some(default_value) || self.sample(x1) != Some(default_value) {
            return false;
        }
        let mut idx = self.xs.partition_point(|&v| v < x0);
        while idx < self.xs.len() && self.xs[idx] <= x1 {
            if self.ys[idx] != default_value {
                return false;
            }
            idx += 1;
        }
        true
    }

    /// Insert or replace a point, keeping `xs` sorted (`UCurve.Insert`).
    pub fn insert(&mut self, x: i32, y: i32) {
        let idx = self.xs.partition_point(|&v| v < x);
        if idx < self.xs.len() && self.xs[idx] == x {
            self.ys[idx] = y;
        } else {
            self.xs.insert(idx, x);
            self.ys.insert(idx, y);
        }
    }

    /// `UCurve.Set`: edit a point at `x` (with the previous edit at
    /// `last_x`), snapping both to the 5-tick interval and rewriting the
    /// neighborhood so the curve stays continuous.
    pub fn set(&mut self, x: i32, y: i32, last_x: i32, _last_y: i32) {
        let x = csharp_round(x as f64 / Self::INTERVAL as f64) * Self::INTERVAL;
        let last_x = csharp_round(last_x as f64 / Self::INTERVAL as f64) * Self::INTERVAL;
        if x == last_x {
            let left_y = self.sample(x - Self::INTERVAL);
            let right_y = self.sample(x + Self::INTERVAL);
            self.insert(x - Self::INTERVAL, left_y.unwrap_or(0));
            self.insert(x, y);
            self.insert(x + Self::INTERVAL, right_y.unwrap_or(0));
        } else if x < last_x {
            let left_y = self.sample(x - Self::INTERVAL);
            self.delete_between_exclusive(x, last_x);
            self.insert(x - Self::INTERVAL, left_y.unwrap_or(0));
            self.insert(x, y);
        } else {
            let right_y = self.sample(x + Self::INTERVAL);
            self.delete_between_exclusive(last_x, x);
            self.insert(x, y);
            self.insert(x + Self::INTERVAL, right_y.unwrap_or(0));
        }
    }

    /// Remove all points strictly between `x1` and `x2` (`UCurve.DeleteBetweenExclusive`).
    /// Removes exactly the range `[li, ri]` where `li` = first point > x1 and
    /// `ri` = last point < x2, matching C# `List.BinarySearch` semantics.
    pub fn delete_between_exclusive(&mut self, x1: i32, x2: i32) {
        let li = self.xs.partition_point(|&v| v <= x1);
        let ri = self.xs.partition_point(|&v| v < x2) as isize - 1;
        if ri >= li as isize {
            self.xs.drain(li..=(ri as usize));
            self.ys.drain(li..=(ri as usize));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_interpolates_and_rounds() {
        let c = UCurve { abbr: "dyn".into(), xs: vec![0, 100, 200], ys: vec![0, 100, 0] };
        assert_eq!(c.sample(0), Some(0));
        assert_eq!(c.sample(100), Some(100));
        assert_eq!(c.sample(50), Some(50));
        assert_eq!(c.sample(150), Some(50));
        assert_eq!(c.sample(33), Some(33)); // 0 + 100 * 33/100
        assert_eq!(c.sample(-10), None);
        assert_eq!(c.sample(300), None);
        assert_eq!(UCurve::new("x").sample(0), None);
    }

    #[test]
    fn insert_replaces_and_sorts() {
        let mut c = UCurve::new("x");
        c.insert(10, 1);
        c.insert(0, 2);
        c.insert(10, 3);
        assert_eq!(c.xs, vec![0, 10]);
        assert_eq!(c.ys, vec![2, 3]);
    }

    #[test]
    fn set_snaps_to_interval() {
        let mut c = UCurve::new("x");
        // 7 -> 5 (snapped); x > last_x: insert (5, y) and (10, rightY=0).
        c.set(7, 5, 0, 0);
        assert_eq!(c.xs, vec![5, 10]);
        assert_eq!(c.ys, vec![5, 0]);
        // 12 -> 10; replaces the point at 10 and appends (15, rightY=0).
        c.set(12, 9, 5, 5);
        assert_eq!(c.xs, vec![5, 10, 15]);
        assert_eq!(c.ys, vec![5, 9, 0]);
        // x == last_x: keep neighbors, replace in place.
        c.set(10, 7, 10, 9);
        assert_eq!(c.xs, vec![5, 10, 15]);
        assert_eq!(c.ys, vec![5, 7, 0]);
        // x < last_x: insert (0, leftY=0) and (5, y).
        c.set(3, 1, 10, 7);
        assert_eq!(c.xs, vec![0, 5, 10, 15]);
        assert_eq!(c.ys, vec![0, 1, 7, 0]);
    }

    #[test]
    fn delete_between_exclusive_matches_csharp() {
        // xs = [0, 10, 20]
        let mut c = UCurve { abbr: "x".into(), xs: vec![0, 10, 20], ys: vec![1, 2, 3] };
        c.delete_between_exclusive(0, 15);
        assert_eq!(c.xs, vec![0, 20]); // removes only 10
        c.delete_between_exclusive(0, 10);
        assert_eq!(c.xs, vec![0, 20]); // nothing strictly between
        let mut c2 = UCurve { abbr: "x".into(), xs: vec![0, 10, 20], ys: vec![1, 2, 3] };
        c2.delete_between_exclusive(-5, 25);
        assert!(c2.xs.is_empty());
        let mut c3 = UCurve { abbr: "x".into(), xs: vec![0, 10, 20], ys: vec![1, 2, 3] };
        c3.delete_between_exclusive(5, 15);
        assert_eq!(c3.xs, vec![0, 20]);
    }

    #[test]
    fn empty_checks() {
        let c = UCurve { abbr: "x".into(), xs: vec![0, 480], ys: vec![0, 0] };
        assert!(c.is_empty());
        assert!(c.is_empty_between(0, 480, 0));
        let c2 = UCurve { abbr: "x".into(), xs: vec![0, 480], ys: vec![0, 10] };
        assert!(!c2.is_empty());
        assert!(!c2.is_empty_between(0, 480, 0));
    }
}
