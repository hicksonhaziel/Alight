use alight_model::{estimate, pooled, scoring};
use alight_sim::{Parameters, generate};
use alight_types::*;
use std::collections::BTreeMap;

#[test]
fn registered_heldout_loss_and_monotonicity_with_blend_evidence() {
    for seed in [7, 42, 2026] {
        let dataset = generate(&Parameters {
            seed,
            canaries: 10_000,
            ..Parameters::default()
        })
        .expect("dataset");
        let samples: Vec<_> = dataset
            .canaries
            .into_iter()
            .map(|canary| TrainingCanary {
                canary,
                finalized: true,
                covariates: ModelCovariates {
                    congestion: Some(0.0),
                },
            })
            .collect();
        let context = CurveContext {
            source: Source::Sim,
            regime_id: "sim-r0".into(),
            region: "synthetic-local".into(),
            as_of_utc: samples[8000].canary.send_wall_utc.clone(),
        };
        let (mut m0, mut m1, mut total) = (0.0, 0.0, 0.0);
        for horizon in [1, 2, 4] {
            let model = pooled::fit(&samples[..8000], &context, horizon).expect("model");
            let mut cells = BTreeMap::new();
            for sample in &samples[8000..] {
                let c = &sample.canary;
                let id = serde_json::to_string(&c.config).expect("config");
                if !cells.contains_key(&id) {
                    cells.insert(
                        id.clone(),
                        estimate(&samples[..8000], &c.config, &context, horizon).expect("M0"),
                    );
                }
                let y = f64::from(
                    matches!(c.outcome, Some(Outcome::LandedOk | Outcome::LandedFailed))
                        && c.landed_slot
                            .is_some_and(|s| s - c.sent_slot <= u64::from(horizon)),
                );
                let p = pooled::probability(
                    &model,
                    &c.config,
                    &c.leader_class_next,
                    &sample.covariates,
                )
                .expect("predict")
                .0;
                m0 += scoring::log_loss(cells[&id].p_hat, y);
                m1 += scoring::log_loss(p, y);
                total += 1.0;
            }
            let mut config = samples[0].canary.config.clone();
            let mut previous = 0.0;
            for tip in [100_000, 150_000, 200_000, 300_000, 500_000, 1_000_000] {
                config.tip_lamports = tip;
                let p = pooled::probability(&model, &config, &[], &ModelCovariates::default())
                    .expect("tip")
                    .0;
                assert!(p >= previous - 1e-12);
                previous = p;
            }
            previous = 0.0;
            for fee in [0, 1, 100, 1000, 5000, 20_000] {
                config.cu_price_micro_lamports = fee;
                let p = pooled::probability(&model, &config, &[], &ModelCovariates::default())
                    .expect("fee")
                    .0;
                assert!(p >= previous - 1e-12);
                previous = p;
            }
        }
        assert!(m1 < m0, "seed={seed}, M0={}, M1={}", m0 / total, m1 / total);
        eprintln!(
            "M1 seed={seed} heldout_n={total} M0={:.6} M1={:.6}",
            m0 / total,
            m1 / total
        );
        let mut final_context = context.clone();
        final_context.as_of_utc = dataset.as_of_utc;
        let model = pooled::fit_blend(&samples, &final_context, 2).expect("blend");
        assert!(model.holdout_m1_log_loss.is_some());
        let original = &samples[0].canary.config;
        let p = pooled::predict(
            &samples,
            Some(&model),
            original,
            &final_context,
            2,
            &[],
            &ModelCovariates::default(),
        )
        .expect("measured");
        assert_eq!(p.evidence, Evidence::Measured);
        if original.route != Route::Rpc {
            let mut interpolated = original.clone();
            interpolated.tip_lamports = 150_000;
            let p = pooled::predict(
                &samples,
                Some(&model),
                &interpolated,
                &final_context,
                2,
                &[],
                &ModelCovariates::default(),
            )
            .expect("interpolated");
            assert_eq!(p.evidence, Evidence::Interpolated);
            interpolated.tip_lamports = 2_000_000;
            let p = pooled::predict(
                &samples,
                Some(&model),
                &interpolated,
                &final_context,
                2,
                &[],
                &ModelCovariates::default(),
            )
            .expect("extrapolated");
            assert_eq!(p.evidence, Evidence::Extrapolated);
        }
    }
}
