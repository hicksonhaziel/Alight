use alight_model::{latency, signal};
use alight_sim::{Parameters, generate_focused, probability_with};
use alight_types::*;

#[test]
fn registered_signal_recovers_slope_flat_null_and_sparse_inconclusive() {
    for (slope, fee, never, expected) in [
        (0.45, 0.18, None, SignalVerdict::Discriminating),
        (0.0, 0.0, Some(0.0), SignalVerdict::Flat),
    ] {
        let params = Parameters {
            seed: 42,
            canaries: 100_000,
            slot_ms: 50,
            congestion: 0.1,
            tip_slope: slope,
            fee_slope: fee,
            never_land_mass: never,
            ..Parameters::default()
        };
        let dataset =
            generate_focused(&params, Route::BeamHttp, SizeClass::Small).expect("focused");
        let report = signal::report(
            &dataset.training().expect("training"),
            Source::Sim,
            "2026-10-05",
            &dataset.as_of_utc,
        )
        .expect("signal");
        let cell = report
            .cells
            .iter()
            .find(|c| c.route == Route::BeamHttp && c.size_class == SizeClass::Small)
            .expect("cell");
        eprintln!(
            "Signal slope={slope} samples={} verdict={:?}",
            cell.uniform_samples, cell.verdict
        );
        if cell.verdict != expected {
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&cell.effects).expect("effects")
            );
        }
        assert_eq!(cell.verdict, expected);
        assert!(
            report
                .cells
                .iter()
                .filter(|c| c.uniform_samples == 0)
                .all(|c| c.verdict == SignalVerdict::Inconclusive)
        );
        assert_eq!(report.bootstrap_replicates, 2000);
    }
}

#[test]
fn registered_continuous_latency_coverage_and_failure_mass() {
    let mut covered = [0u32; 2];
    for seed in 0..100 {
        let params = Parameters {
            seed,
            canaries: 4800,
            slot_ms: 50,
            tip_slope: 0.0,
            fee_slope: 0.0,
            never_land_mass: Some(0.0),
            continuous_latency: true,
            ..Parameters::default()
        };
        let dataset =
            generate_focused(&params, Route::BeamQuic, SizeClass::Small).expect("dataset");
        let samples = dataset.training().expect("training");
        let config = &dataset.ground_truth[0].config;
        let context = CurveContext {
            source: Source::Sim,
            regime_id: "sim-r0".into(),
            region: "synthetic-local".into(),
            as_of_utc: dataset.as_of_utc,
        };
        for (i, q) in [0.5, 0.9].into_iter().enumerate() {
            // Independent ground-truth inverse CDF of the artificial exponential leader mixture.
            let mut lo = 0.0;
            let mut hi = 16.0;
            for _ in 0..60 {
                let mid = (lo + hi) / 2.0;
                let cdf = [-1, 0, 1]
                    .into_iter()
                    .map(|l| 1.0 - (-mid * (0.2 + f64::from(l) * 0.2).exp()).exp())
                    .sum::<f64>()
                    / 3.0;
                if cdf < q {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let truth_ms = (lo + hi) / 2.0 * f64::from(params.slot_ms);
            let quantile = latency::estimate_quantile(&samples, config, &context, q, seed ^ 0x1234)
                .expect("quantile");
            if quantile.ms_interval_95[0]
                .zip(quantile.ms_interval_95[1])
                .is_some_and(|(a, b)| a <= truth_ms && truth_ms <= b)
            {
                covered[i] += 1;
            } else {
                eprintln!(
                    "coverage miss seed={seed} q={q} truth={truth_ms:.6} estimate={:?} interval={:?} n={:.3}",
                    quantile.ms, quantile.ms_interval_95, quantile.n_effective
                );
            }
        }
    }
    eprintln!(
        "latency coverage over 100 independent datasets: p50={} p90={}",
        covered[0], covered[1]
    );
    assert!(covered.into_iter().all(|n| (90..=99).contains(&n)));
    let params = Parameters {
        canaries: 4800,
        never_land_mass: Some(0.3),
        ..Parameters::default()
    };
    let dataset =
        generate_focused(&params, Route::BeamQuic, SizeClass::Small).expect("failed dataset");
    let context = CurveContext {
        source: Source::Sim,
        regime_id: "sim-r0".into(),
        region: "synthetic-local".into(),
        as_of_utc: dataset.as_of_utc.clone(),
    };
    let q = latency::estimate_quantile(
        &dataset.training().expect("training"),
        &dataset.ground_truth[0].config,
        &context,
        0.9,
        42,
    )
    .expect("failure quantile");
    assert_eq!(q.evidence, Evidence::Insufficient);
    assert!(q.slots.is_none());
    assert!(q.ms.is_none());
    assert!(probability_with(&params, &dataset.ground_truth[0].config, 0.0, 16) < 0.9);
}
