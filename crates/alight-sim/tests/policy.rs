use alight_canary::policy::{AdaptivePolicy, Policy};
use alight_model::quote::nominal_cost;
use alight_sim::{Parameters, probability_with};
use alight_types::FeeBucket;

#[test]
fn adaptive_exploration_restarts_and_has_lower_mean_regret_at_equal_spend() {
    let params = Parameters::default();
    for seed in [7, 42, 2026] {
        let mut adaptive = AdaptivePolicy::new(seed, 0.30).expect("adaptive");
        let mut v0 = Policy::new(seed, 0.30).expect("v0");
        let mut cells = adaptive.cells().to_vec();
        for c in &mut cells {
            c.cu_price_micro_lamports = match c.fee_bucket {
                FeeBucket::Zero => 0,
                FeeBucket::LocalMedian => 1000,
                FeeBucket::LocalP90 => 5000,
            };
        }
        let utility = |c: &alight_types::CanaryConfig| {
            probability_with(&params, c, 0.0, 1) - nominal_cost(c) as f64 / 500_000.0
        };
        let oracle = cells.iter().map(utility).fold(f64::NEG_INFINITY, f64::max);
        let mut posterior = vec![(1.0, 1.0); cells.len()];
        let mut state = seed ^ 0xd1b54a32d192ed03;
        let mut random = || {
            state = state.wrapping_add(0x9e3779b97f4a7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut results = Vec::new();
        for policy in 0..2 {
            let (mut spend, mut regret, mut draws) = (0.0, 0.0, 0.0);
            let ceiling = 2_000_000_000.0;
            while spend < ceiling {
                let a = if policy == 0 {
                    v0.assign(1000, 5000)
                } else {
                    adaptive.assign(1000, 5000, &posterior).expect("assign")
                };
                assert!(a.assignment_prob >= 0.30 / 81.0);
                let cost = nominal_cost(&a.config) as f64;
                let fraction = ((ceiling - spend) / cost).min(1.0);
                regret += fraction * (oracle - utility(&a.config));
                draws += fraction;
                spend += fraction * cost;
                if policy == 1 {
                    let i = cells.iter().position(|c| *c == a.config).expect("cell");
                    let y = f64::from(random() < probability_with(&params, &a.config, 0.0, 1));
                    posterior[i].0 += y;
                    posterior[i].1 += 1.0 - y;
                }
            }
            results.push((spend, regret / draws, draws));
        }
        assert_eq!(results[0].0, results[1].0);
        assert!(results[1].1 < results[0].1, "seed={seed}, {results:?}");
        eprintln!(
            "policy seed={seed} equal_spend={} mean_regret_v0={:.6} mean_regret_v1={:.6} draws_v0={:.1} draws_v1={:.1}",
            results[0].0, results[0].1, results[1].1, results[0].2, results[1].2
        );
        let mut resumed = AdaptivePolicy::new(seed, 0.30).expect("resume");
        let next_draw = adaptive
            .clone()
            .assign(1000, 5000, &posterior)
            .expect("next")
            .draw;
        resumed.resume(next_draw).expect("resume seed stream");
        let a = adaptive.assign(1000, 5000, &posterior).expect("a");
        let b = resumed.assign(1000, 5000, &posterior).expect("b");
        assert_eq!(
            serde_json::to_value(a).expect("a"),
            serde_json::to_value(b).expect("b")
        );
    }
}
