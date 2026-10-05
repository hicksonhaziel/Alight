use alight_model::estimate;
use alight_sim::{Parameters, Shift, generate};
use alight_types::{Canary, CurveContext, Evidence, Source, TrainingCanary};

#[test]
fn seeded_schema_and_environment_are_reproducible_without_keys() {
    let parameters = Parameters {
        canaries: 100,
        shift: Some(Shift {
            at_draw: 50,
            slot_ms: 400,
            congestion: 1.0,
        }),
        ..Parameters::default()
    };
    let a = generate(&parameters).expect("generate");
    let b = generate(&parameters).expect("repeat");
    assert_eq!(
        serde_json::to_value(&a).expect("a"),
        serde_json::to_value(&b).expect("b")
    );
    for c in &a.canaries {
        let encoded = serde_json::to_vec(c).expect("encode");
        let decoded: Canary = serde_json::from_slice(&encoded).expect("normal canary contract");
        assert_eq!(decoded.source, Source::Sim);
        assert!(decoded.signature.is_none());
        assert!(decoded.assignment_prob >= 0.30 / 81.0);
    }
    assert_eq!(a.canaries[49].regime_id, "sim-r0");
    assert_eq!(a.canaries[50].regime_id, "sim-r1");
    assert_ne!(
        a.ground_truth[0].p_within_1_slot,
        a.ground_truth[81].p_within_1_slot
    );
    let a0 = &a.canaries[51];
    let a1 = &a.canaries[52];
    assert_eq!(a1.send_mono_ns - a0.send_mono_ns, 400_000_000);
}

#[test]
fn registered_convergence_and_adaptation_thresholds() {
    for seed in [7, 42, 2026] {
        for shifted in [false, true] {
            let parameters = Parameters {
                seed,
                canaries: if shifted { 24_000 } else { 12_000 },
                shift: shifted.then_some(Shift {
                    at_draw: 12_000,
                    slot_ms: 400,
                    congestion: 1.0,
                }),
                ..Parameters::default()
            };
            let dataset = generate(&parameters).expect("simulation");
            let training: Vec<_> = dataset
                .canaries
                .iter()
                .cloned()
                .map(|canary| TrainingCanary {
                    covariates: Default::default(),
                    canary,
                    finalized: true,
                })
                .collect();
            let regime = if shifted { "sim-r1" } else { "sim-r0" };
            let context = CurveContext {
                source: Source::Sim,
                region: "synthetic-local".into(),
                regime_id: regime.into(),
                as_of_utc: dataset.as_of_utc,
            };
            let (mut count, mut total_error) = (0, 0.0);
            for truth in dataset
                .ground_truth
                .iter()
                .filter(|t| t.regime_id == regime)
            {
                for (horizon, probability) in [
                    (1, truth.p_within_1_slot),
                    (2, truth.p_within_2_slots),
                    (4, truth.p_within_4_slots),
                ] {
                    let row =
                        estimate(&training, &truth.config, &context, horizon).expect("estimate");
                    if row.evidence == Evidence::Measured {
                        count += 1;
                        total_error += (row.p_hat - probability).abs();
                    }
                }
            }
            // Registration scores measured evidence only; a genuinely stale cell must be refused.
            assert!(
                count > 0,
                "no eligible measured curves, seed={seed}, shifted={shifted}"
            );
            let mae = total_error / f64::from(count);
            assert!(mae <= 0.08, "seed={seed}, shifted={shifted}, MAE={mae}");
            eprintln!("seed={seed} shifted={shifted} measured={count} MAE={mae:.6}");
        }
    }
}
