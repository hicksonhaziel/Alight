//! Small dense ridge logistic regression, with projected nonnegative coefficients.
use crate::ModelError;

#[derive(Clone)]
pub struct Row {
    pub x: Vec<f64>,
    pub successes: f64,
    pub trials: f64,
}
pub struct Fit {
    pub coefficients: Vec<f64>,
    pub covariance: Vec<Vec<f64>>,
}
pub fn sigmoid(x: f64) -> f64 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        let e = x.exp();
        e / (1.0 + e)
    }
}
pub fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
pub fn inverse(matrix: &[Vec<f64>]) -> Result<Vec<Vec<f64>>, ModelError> {
    let n = matrix.len();
    let mut a = matrix.to_vec();
    let mut result = vec![vec![0.0; n]; n];
    for (i, row) in result.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for i in 0..n {
        let pivot = (i..n)
            .max_by(|a0, b0| a[*a0][i].abs().total_cmp(&a[*b0][i].abs()))
            .ok_or(ModelError::Invalid)?;
        if !a[pivot][i].is_finite() || a[pivot][i].abs() < 1e-12 {
            return Err(ModelError::Invalid);
        }
        a.swap(i, pivot);
        result.swap(i, pivot);
        let scale = a[i][i];
        for j in 0..n {
            a[i][j] /= scale;
            result[i][j] /= scale;
        }
        for k in 0..n {
            if k == i {
                continue;
            }
            let scale = a[k][i];
            for j in 0..n {
                a[k][j] -= scale * a[i][j];
                result[k][j] -= scale * result[i][j];
            }
        }
    }
    Ok(result)
}
fn loss(rows: &[Row], b: &[f64], ridge: &[f64]) -> f64 {
    rows.iter()
        .map(|r| {
            let z = dot(&r.x, b);
            let logden = if z > 0.0 {
                z + (-z).exp().ln_1p()
            } else {
                z.exp().ln_1p()
            };
            r.trials * logden - r.successes * z
        })
        .sum::<f64>()
        + b.iter()
            .zip(ridge)
            .map(|(x, l)| 0.5 * l * x * x)
            .sum::<f64>()
}
/// Fits aggregate Bernoulli rows; successes/trials can be decayed likelihood weights.
pub fn fit(rows: &[Row], ridge: &[f64], nonnegative: &[usize]) -> Result<Fit, ModelError> {
    let n = ridge.len();
    if n == 0
        || rows.is_empty()
        || rows.iter().any(|r| {
            r.x.len() != n
                || !r.trials.is_finite()
                || r.trials <= 0.0
                || r.successes < 0.0
                || r.successes > r.trials
                || r.x.iter().any(|v| !v.is_finite())
        })
    {
        return Err(ModelError::Invalid);
    }
    let mut b = vec![0.0; n];
    let mut covariance = vec![vec![0.0; n]; n];
    for _ in 0..40 {
        let mut gradient = vec![0.0; n];
        let mut hessian = vec![vec![0.0; n]; n];
        for i in 0..n {
            gradient[i] = -ridge[i] * b[i];
            hessian[i][i] = ridge[i].max(1e-8);
        }
        for row in rows {
            let p = sigmoid(dot(&row.x, &b));
            let residual = row.successes - row.trials * p;
            let weight = row.trials * (p * (1.0 - p)).max(1e-8);
            let active: Vec<_> = row
                .x
                .iter()
                .enumerate()
                .filter(|(_, x)| **x != 0.0)
                .collect();
            for &(i, x) in &active {
                gradient[i] += x * residual;
                for &(j, y) in &active {
                    hessian[i][j] += weight * x * y;
                }
            }
        }
        covariance = inverse(&hessian)?;
        let delta: Vec<_> = covariance.iter().map(|row| dot(row, &gradient)).collect();
        let old = loss(rows, &b, ridge);
        let mut step = 1.0;
        let mut next = b.clone();
        for _ in 0..20 {
            next = b
                .iter()
                .zip(&delta)
                .map(|(x, d)| (x + step * d).clamp(-30.0, 30.0))
                .collect();
            for &i in nonnegative {
                next[i] = next[i].max(0.0);
            }
            if loss(rows, &next, ridge) <= old + 1e-8 {
                break;
            }
            step *= 0.5;
        }
        let change = next
            .iter()
            .zip(&b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f64::max);
        b = next;
        if change < 1e-7 {
            break;
        }
    }
    Ok(Fit {
        coefficients: b,
        covariance,
    })
}
