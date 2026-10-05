//! Equal-tail Beta intervals via Lanczos log-gamma, incomplete-beta continued fraction, and bisection.
pub(crate) fn log_gamma(z: f64) -> f64 {
    let coefficients = [
        676.5203681218851,
        -1259.1392167224028,
        771.3234287776531,
        -176.6150291621406,
        12.507343278686905,
        -0.13857109526572012,
        9.984369578019572e-6,
        1.5056327351493116e-7,
    ];
    let z = z - 1.0;
    let mut sum = 0.9999999999998099;
    for (i, c) in coefficients.into_iter().enumerate() {
        sum += c / (z + i as f64 + 1.0);
    }
    let t = z + 7.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (z + 0.5) * t.ln() - t + sum.ln()
}
fn nonzero(x: f64) -> f64 {
    if x.abs() < 1e-300 {
        if x < 0.0 { -1e-300 } else { 1e-300 }
    } else {
        x
    }
}
fn fraction(a: f64, b: f64, x: f64) -> f64 {
    let mut c = 1.0;
    let mut d = 1.0 / nonzero(1.0 - (a + b) * x / (a + 1.0));
    let mut result = d;
    for m in 1..=400 {
        let m = f64::from(m);
        for (step, coefficient) in [
            m * (b - m) * x / ((a + 2.0 * m - 1.0) * (a + 2.0 * m)),
            -(a + m) * (a + b + m) * x / ((a + 2.0 * m) * (a + 2.0 * m + 1.0)),
        ]
        .into_iter()
        .enumerate()
        {
            d = 1.0 / nonzero(1.0 + coefficient * d);
            c = nonzero(1.0 + coefficient / c);
            let delta = d * c;
            result *= delta;
            if step == 1 && (delta - 1.0).abs() < 1e-13 {
                return result;
            }
        }
    }
    result
}
fn cdf(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let factor =
        (log_gamma(a + b) - log_gamma(a) - log_gamma(b) + a * x.ln() + b * (-x).ln_1p()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        factor * fraction(a, b, x) / a
    } else {
        1.0 - factor * fraction(b, a, 1.0 - x) / b
    }
}
pub(crate) fn quantile(a: f64, b: f64, p: f64) -> f64 {
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..64 {
        let mid = (low + high) / 2.0;
        if cdf(a, b, mid) < p {
            low = mid;
        } else {
            high = mid;
        }
    }
    (low + high) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn beta_reference_values_and_intervals_shrink() {
        for p in [0.025, 0.5, 0.975] {
            assert!((quantile(1.0, 1.0, p) - p).abs() < 1e-10);
        }
        // Beta(2,2) CDF has the closed form 3x^2 - 2x^3.
        let x = quantile(2.0, 2.0, 0.025);
        assert!((3.0 * x * x - 2.0 * x * x * x - 0.025).abs() < 1e-10);
        for rate in [0.0, 0.2, 0.5, 0.8, 1.0] {
            let mut previous = 1.0;
            for n in [0.0, 10.0, 30.0, 100.0, 1000.0] {
                let (a, b) = (1.0 + n * rate, 1.0 + n * (1.0 - rate));
                let width = quantile(a, b, 0.975) - quantile(a, b, 0.025);
                assert!(width < previous, "rate={rate}, n={n}");
                previous = width;
            }
        }
    }
}
