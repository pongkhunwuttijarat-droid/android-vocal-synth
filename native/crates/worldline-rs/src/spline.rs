//! Natural cubic spline — a faithful port of the `tk::spline` subset the
//! worldline effects use (`spline(x, y)` with default `cspline` type and
//! `second_deriv` boundaries, i.e. natural cubic splines).
//!
//! Only evaluation is needed (`GetTensionCoefficients` builds a spline over
//! spectral-bin positions and evaluates it on the integer bin grid), so
//! this keeps the same coefficients and extrapolation rules as the C++
//! implementation without porting the band-matrix LU machinery.

/// Natural cubic spline (`tk::spline` defaults).
#[derive(Debug, Clone)]
pub struct NaturalSpline {
    x: Vec<f64>,
    y: Vec<f64>,
    b: Vec<f64>,
    c: Vec<f64>,
    d: Vec<f64>,
}

impl NaturalSpline {
    /// Build a natural cubic spline through `(x[i], y[i])`.
    ///
    /// Returns `None` when there are fewer than 3 strictly increasing
    /// points — `tk::spline` asserts the same precondition.
    pub fn new(x: Vec<f64>, y: Vec<f64>) -> Option<Self> {
        let n = x.len();
        if n < 3 || y.len() != n {
            return None;
        }
        for i in 0..n - 1 {
            if !(x[i] < x[i + 1]) {
                return None;
            }
        }
        // Interior system for c[1..n-1] (natural boundaries fix c[0]=c[n-1]=0):
        //   (h_{i-1}/3) c[i-1] + (2/3 (x[i+1]-x[i-1])) c[i] + (h_i/3) c[i+1] = rhs[i]
        let mut c = vec![0.0; n];
        let m = n - 2; // number of interior unknowns
        if m > 0 {
            let mut lower = vec![0.0; m]; // sub-diagonal
            let mut diag = vec![0.0; m];
            let mut upper = vec![0.0; m]; // super-diagonal
            let mut rhs = vec![0.0; m];
            for i in 1..n - 1 {
                let h_prev = x[i] - x[i - 1];
                let h_next = x[i + 1] - x[i];
                let k = i - 1;
                if k > 0 {
                    lower[k] = h_prev / 3.0;
                }
                diag[k] = 2.0 / 3.0 * (x[i + 1] - x[i - 1]);
                if k + 1 < m {
                    upper[k] = h_next / 3.0;
                }
                rhs[k] = (y[i + 1] - y[i]) / h_next - (y[i] - y[i - 1]) / h_prev;
            }
            // Thomas algorithm.
            let mut cp = vec![0.0; m];
            let mut dp = vec![0.0; m];
            cp[0] = if diag[0].abs() > 0.0 {
                upper[0] / diag[0]
            } else {
                0.0
            };
            dp[0] = if diag[0].abs() > 0.0 {
                rhs[0] / diag[0]
            } else {
                0.0
            };
            for k in 1..m {
                let denom = diag[k] - lower[k] * cp[k - 1];
                cp[k] = if denom.abs() > 0.0 {
                    upper[k] / denom
                } else {
                    0.0
                };
                dp[k] = if denom.abs() > 0.0 {
                    (rhs[k] - lower[k] * dp[k - 1]) / denom
                } else {
                    0.0
                };
            }
            c[m] = dp[m - 1];
            for k in (0..m - 1).rev() {
                c[k + 1] = dp[k] - cp[k] * c[k + 2];
            }
        }
        let mut b = vec![0.0; n];
        let mut d = vec![0.0; n];
        for i in 0..n - 1 {
            let h = x[i + 1] - x[i];
            d[i] = (c[i + 1] - c[i]) / (3.0 * h);
            b[i] = (y[i + 1] - y[i]) / h - (2.0 * c[i] + c[i + 1]) * h / 3.0;
        }
        // Right-hand tail coefficients (`tk::spline` zeroes the cubic term of
        // the last segment and continues the previous segment's slope).
        let h = x[n - 1] - x[n - 2];
        d[n - 1] = 0.0;
        b[n - 1] = 3.0 * d[n - 2] * h * h + 2.0 * c[n - 2] * h + b[n - 2];
        Some(Self { x, y, b, c, d })
    }

    /// Evaluate the spline (with `tk::spline`'s linear-ish extrapolation).
    pub fn eval(&self, x: f64) -> f64 {
        let n = self.x.len();
        let idx = self.find_closest(x);
        let h = x - self.x[idx];
        if x < self.x[0] {
            // left extrapolation uses c0 == c[0] for natural boundaries
            (self.c[0] * h + self.b[0]) * h + self.y[0]
        } else if x > self.x[n - 1] {
            (self.c[n - 1] * h + self.b[n - 1]) * h + self.y[n - 1]
        } else {
            ((self.d[idx] * h + self.c[idx]) * h + self.b[idx]) * h + self.y[idx]
        }
    }

    fn find_closest(&self, x: f64) -> usize {
        // `tk::spline::find_closest`: the segment whose left knot is the
        // largest one <= x (clamped to the valid segment range).
        let n = self.x.len();
        if x <= self.x[0] {
            return 0;
        }
        if x >= self.x[n - 1] {
            return n - 2;
        }
        let mut low = 0usize;
        let mut high = n - 1;
        while high - low > 1 {
            let mid = (low + high) / 2;
            if self.x[mid] > x {
                high = mid;
            } else {
                low = mid;
            }
        }
        low
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reproduces_a_plane_exactly() {
        let x = vec![0.0, 1.0, 2.0, 5.0, 9.0];
        let y = vec![3.0, 3.0, 3.0, 3.0, 3.0];
        let s = NaturalSpline::new(x, y).unwrap();
        for i in 0..10 {
            assert!((s.eval(i as f64) - 3.0).abs() < 1e-12, "x={i}");
        }
    }

    #[test]
    fn interpolates_its_knots() {
        let x = vec![0.0, 1.0, 3.0, 4.0];
        let y = vec![0.0, 2.0, -1.0, 5.0];
        let s = NaturalSpline::new(x.clone(), y.clone()).unwrap();
        for (i, &xi) in x.iter().enumerate() {
            assert!((s.eval(xi) - y[i]).abs() < 1e-9, "knot {i}");
        }
    }

    #[test]
    fn rejects_degenerate_input() {
        assert!(NaturalSpline::new(vec![0.0, 1.0], vec![0.0, 1.0]).is_none());
        assert!(NaturalSpline::new(vec![0.0, 1.0, 1.0], vec![0.0, 1.0, 2.0]).is_none());
    }
}
