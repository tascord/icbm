//! CPU-intensive computations to stress the compiler and the machine under
//! test.
//!
//! Intentionally exercises:
//! * Rayon parallel iterators
//! * Generic numeric algorithms
//! * Complex iterator chains
//! * Trait objects and dynamic dispatch

use rayon::prelude::*;

// ---------------------------------------------------------------------------
// Sorting algorithms
// ---------------------------------------------------------------------------

/// Parallel merge-sort using Rayon.
pub fn parallel_sort<T: Ord + Clone + Send + Sync>(data: &[T]) -> Vec<T> {
    if data.len() <= 1 {
        return data.to_vec();
    }

    let mid = data.len() / 2;
    let (left, right) = rayon::join(|| parallel_sort(&data[..mid]), || parallel_sort(&data[mid..]));
    merge(left, right)
}

fn merge<T: Ord + Clone>(a: Vec<T>, b: Vec<T>) -> Vec<T> {
    let mut result = Vec::with_capacity(a.len() + b.len());
    let mut ai = 0;
    let mut bi = 0;
    while ai < a.len() && bi < b.len() {
        if a[ai] <= b[bi] {
            result.push(a[ai].clone());
            ai += 1;
        } else {
            result.push(b[bi].clone());
            bi += 1;
        }
    }
    result.extend_from_slice(&a[ai..]);
    result.extend_from_slice(&b[bi..]);
    result
}

// ---------------------------------------------------------------------------
// Matrix operations
// ---------------------------------------------------------------------------

/// Row-major dense matrix.
#[derive(Debug, Clone)]
pub struct Matrix {
    pub rows: usize,
    pub cols: usize,
    data: Vec<f64>,
}

impl Matrix {
    pub fn zeros(rows: usize, cols: usize) -> Self { Matrix { rows, cols, data: vec![0.0; rows * cols] } }

    pub fn from_fn<F: Fn(usize, usize) -> f64 + Copy>(rows: usize, cols: usize, f: F) -> Self {
        let data = (0..rows).flat_map(|r| (0..cols).map(move |c| f(r, c))).collect();
        Matrix { rows, cols, data }
    }

    pub fn get(&self, row: usize, col: usize) -> f64 { self.data[row * self.cols + col] }

    pub fn set(&mut self, row: usize, col: usize, val: f64) { self.data[row * self.cols + col] = val; }

    /// Parallel matrix multiply using Rayon.
    pub fn mul(&self, other: &Matrix) -> Matrix {
        assert_eq!(self.cols, other.rows, "Matrix dimension mismatch");
        let rows = self.rows;
        let cols = other.cols;
        let k = self.cols;

        let data: Vec<f64> = (0..rows)
            .into_par_iter()
            .flat_map(|r| {
                (0..cols).map(|c| (0..k).map(|i| self.get(r, i) * other.get(i, c)).sum::<f64>()).collect::<Vec<_>>()
            })
            .collect();

        Matrix { rows, cols, data }
    }

    /// Transpose.
    pub fn transpose(&self) -> Matrix { Matrix::from_fn(self.cols, self.rows, |r, c| self.get(c, r)) }

    /// Frobenius norm.
    pub fn frobenius_norm(&self) -> f64 { self.data.iter().map(|v| v * v).sum::<f64>().sqrt() }
}

// ---------------------------------------------------------------------------
// Statistical functions
// ---------------------------------------------------------------------------

/// Computes a histogram with `bins` buckets over `data`.
/// Returns `(edges, counts)` where `edges` has `bins + 1` entries.
pub fn histogram(data: &[f64], bins: usize) -> (Vec<f64>, Vec<usize>) {
    if data.is_empty() || bins == 0 {
        return (vec![], vec![]);
    }

    let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = if (max - min).abs() < f64::EPSILON { 1.0 } else { max - min };

    let edges: Vec<f64> = (0..=bins).map(|i| min + range * i as f64 / bins as f64).collect();

    let mut counts = vec![0usize; bins];
    for &v in data {
        let bin = ((v - min) / range * bins as f64).floor() as usize;
        let bin = bin.min(bins - 1);
        counts[bin] += 1;
    }

    (edges, counts)
}

/// Parallel Pearson correlation between two equal-length slices.
pub fn pearson(x: &[f64], y: &[f64]) -> f64 {
    assert_eq!(x.len(), y.len());
    let n = x.len() as f64;
    let mean_x: f64 = x.par_iter().sum::<f64>() / n;
    let mean_y: f64 = y.par_iter().sum::<f64>() / n;
    let num: f64 = x.par_iter().zip(y.par_iter()).map(|(xi, yi)| (xi - mean_x) * (yi - mean_y)).sum();
    let dx: f64 = x.par_iter().map(|xi| (xi - mean_x).powi(2)).sum::<f64>().sqrt();
    let dy: f64 = y.par_iter().map(|yi| (yi - mean_y).powi(2)).sum::<f64>().sqrt();
    if dx.abs() < f64::EPSILON || dy.abs() < f64::EPSILON {
        0.0
    } else {
        num / (dx * dy)
    }
}

// ---------------------------------------------------------------------------
// Sieve of Eratosthenes
// ---------------------------------------------------------------------------

/// Returns all primes up to (and including) `limit`.
pub fn sieve(limit: usize) -> Vec<usize> {
    if limit < 2 {
        return vec![];
    }
    let mut is_prime = vec![true; limit + 1];
    is_prime[0] = false;
    is_prime[1] = false;

    let mut i = 2;
    while i * i <= limit {
        if is_prime[i] {
            let mut j = i * i;
            while j <= limit {
                is_prime[j] = false;
                j += i;
            }
        }
        i += 1;
    }

    is_prime.iter().enumerate().filter_map(|(n, &p)| if p { Some(n) } else { None }).collect()
}

// ---------------------------------------------------------------------------
// Trait for pluggable algorithms
// ---------------------------------------------------------------------------

pub trait Sorter<T: Ord + Clone + Send + Sync>: Send + Sync {
    fn sort(&self, data: &[T]) -> Vec<T>;
    fn name(&self) -> &'static str;
}

pub struct ParallelMergeSorter;
pub struct StdSorter;

impl<T: Ord + Clone + Send + Sync> Sorter<T> for ParallelMergeSorter {
    fn sort(&self, data: &[T]) -> Vec<T> { parallel_sort(data) }

    fn name(&self) -> &'static str { "parallel_merge_sort" }
}

impl<T: Ord + Clone + Send + Sync> Sorter<T> for StdSorter {
    fn sort(&self, data: &[T]) -> Vec<T> {
        let mut v = data.to_vec();
        v.sort();
        v
    }

    fn name(&self) -> &'static str { "std_sort" }
}

/// Run `sorter` on `data` and verify the result is sorted.
pub fn run_sorter<T: Ord + Clone + Send + Sync>(sorter: &dyn Sorter<T>, data: &[T]) -> (Vec<T>, bool) {
    let result = sorter.sort(data);
    let sorted = result.windows(2).all(|w| w[0] <= w[1]);
    (result, sorted)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_sort() {
        let data: Vec<i32> = vec![5, 3, 8, 1, 9, 2, 7, 4, 6, 0];
        let sorted = parallel_sort(&data);
        assert_eq!(sorted, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_matrix_mul() {
        let a = Matrix::from_fn(2, 3, |r, c| (r * 3 + c + 1) as f64);
        let b = Matrix::from_fn(3, 2, |r, c| (r * 2 + c + 1) as f64);
        let c = a.mul(&b);
        assert_eq!(c.rows, 2);
        assert_eq!(c.cols, 2);
        // Row 0: [1,2,3] × [[1,2],[3,4],[5,6]] = [22, 28]
        assert!((c.get(0, 0) - 22.0).abs() < 1e-9);
        assert!((c.get(0, 1) - 28.0).abs() < 1e-9);
    }

    #[test]
    fn test_sieve() {
        let primes = sieve(30);
        assert_eq!(primes, vec![2, 3, 5, 7, 11, 13, 17, 19, 23, 29]);
    }

    #[test]
    fn test_histogram() {
        let data: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let (_, counts) = histogram(&data, 5);
        assert_eq!(counts.iter().sum::<usize>(), 10);
    }

    #[test]
    fn test_sorter_trait() {
        let data = vec![4u32, 1, 3, 2];
        let (sorted, ok) = run_sorter(&StdSorter, &data);
        assert!(ok);
        assert_eq!(sorted, vec![1, 2, 3, 4]);
    }
}
