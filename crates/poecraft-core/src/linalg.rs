//! Small dense linear algebra for the exact engine: LU factorisation with partial pivoting.

/// Row-major dense matrix.
#[derive(Debug, Clone)]
pub struct Mat {
    pub rows: usize,
    pub cols: usize,
    pub a: Vec<f64>,
}

impl Mat {
    pub fn zeros(rows: usize, cols: usize) -> Mat {
        Mat {
            rows,
            cols,
            a: vec![0.0; rows * cols],
        }
    }
    pub fn identity(n: usize) -> Mat {
        let mut m = Mat::zeros(n, n);
        for i in 0..n {
            m.a[i * n + i] = 1.0;
        }
        m
    }
    #[inline]
    pub fn at(&self, r: usize, c: usize) -> f64 {
        self.a[r * self.cols + c]
    }
    #[inline]
    pub fn at_mut(&mut self, r: usize, c: usize) -> &mut f64 {
        &mut self.a[r * self.cols + c]
    }
    pub fn row(&self, r: usize) -> &[f64] {
        &self.a[r * self.cols..(r + 1) * self.cols]
    }
}

pub struct Lu {
    n: usize,
    lu: Vec<f64>,
    piv: Vec<usize>,
}

impl Lu {
    /// Factorise a square matrix. Returns None when it is singular.
    pub fn new(m: &Mat) -> Option<Lu> {
        let n = m.rows;
        let mut lu = m.a.clone();
        let mut piv: Vec<usize> = (0..n).collect();
        for k in 0..n {
            let (mut p, mut best) = (k, lu[k * n + k].abs());
            for i in k + 1..n {
                let v = lu[i * n + k].abs();
                if v > best {
                    best = v;
                    p = i;
                }
            }
            if best < 1e-300 {
                return None;
            }
            if p != k {
                for j in 0..n {
                    lu.swap(k * n + j, p * n + j);
                }
                piv.swap(k, p);
            }
            let d = lu[k * n + k];
            for i in k + 1..n {
                let f = lu[i * n + k] / d;
                if f == 0.0 {
                    continue;
                }
                lu[i * n + k] = f;
                let (top, bottom) = lu.split_at_mut(i * n);
                let rk = &top[k * n + k + 1..k * n + n];
                let ri = &mut bottom[k + 1..n];
                for (x, y) in ri.iter_mut().zip(rk) {
                    *x -= f * y;
                }
            }
        }
        Some(Lu { n, lu, piv })
    }

    /// Solve A X = B for every column of B (n x m, row-major).
    pub fn solve(&self, b: &Mat) -> Mat {
        let n = self.n;
        let m = b.cols;
        let mut x = Mat::zeros(n, m);
        for i in 0..n {
            x.a[i * m..(i + 1) * m].copy_from_slice(b.row(self.piv[i]));
        }
        // forward (unit lower)
        for i in 0..n {
            for k in 0..i {
                let f = self.lu[i * n + k];
                if f != 0.0 {
                    let (top, bottom) = x.a.split_at_mut(i * m);
                    let rk = &top[k * m..(k + 1) * m];
                    for (xi, xk) in bottom[..m].iter_mut().zip(rk) {
                        *xi -= f * xk;
                    }
                }
            }
        }
        // backward
        for i in (0..n).rev() {
            for k in i + 1..n {
                let f = self.lu[i * n + k];
                if f != 0.0 {
                    let (top, bottom) = x.a.split_at_mut(k * m);
                    let ri = &mut top[i * m..(i + 1) * m];
                    for (xi, xk) in ri.iter_mut().zip(&bottom[..m]) {
                        *xi -= f * xk;
                    }
                }
            }
            let d = self.lu[i * n + i];
            for v in &mut x.a[i * m..(i + 1) * m] {
                *v /= d;
            }
        }
        x
    }

    /// Solve A^T z = e (single right-hand side).
    pub fn solve_transpose(&self, e: &[f64]) -> Vec<f64> {
        let n = self.n;
        // A = P^T L U  =>  A^T = U^T L^T P ; solve U^T y = e, L^T w = y, z = P^T w
        let mut y = e.to_vec();
        for i in 0..n {
            let mut s = y[i];
            for k in 0..i {
                s -= self.lu[k * n + i] * y[k];
            }
            y[i] = s / self.lu[i * n + i];
        }
        for i in (0..n).rev() {
            let mut s = y[i];
            for k in i + 1..n {
                s -= self.lu[k * n + i] * y[k];
            }
            y[i] = s;
        }
        let mut z = vec![0.0; n];
        for i in 0..n {
            z[self.piv[i]] = y[i];
        }
        z
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn solves_small_system() {
        let mut a = Mat::zeros(3, 3);
        a.a = vec![2.0, 1.0, 1.0, 1.0, 3.0, 2.0, 1.0, 0.0, 0.0];
        let lu = Lu::new(&a).unwrap();
        let mut b = Mat::zeros(3, 1);
        b.a = vec![4.0, 5.0, 6.0];
        let x = lu.solve(&b);
        for i in 0..3 {
            let r: f64 = (0..3).map(|j| a.at(i, j) * x.at(j, 0)).sum();
            assert!((r - b.at(i, 0)).abs() < 1e-9);
        }
        let z = lu.solve_transpose(&[1.0, 0.0, 0.0]);
        for j in 0..3 {
            let r: f64 = (0..3).map(|i| a.at(i, j) * z[i]).sum();
            assert!(
                (r - if j == 0 { 1.0 } else { 0.0 }).abs() < 1e-9,
                "col {j}: {r}"
            );
        }
    }
}
