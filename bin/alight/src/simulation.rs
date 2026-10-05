use alight_model::{estimate, methodology_hash};
use alight_sim::{Dataset, Parameters, Shift, generate};
use alight_store::Store;
use alight_types::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct Report {
    methodology_hash: String,
    dataset: Dataset,
    snapshots: Vec<CurveSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    phase2: Option<Phase2Report>,
}
#[derive(Serialize, Deserialize)]
struct Phase2Report {
    forecasts: Vec<ForecastEntry>,
    grades: Vec<ForecastGrade>,
    ticks: Vec<serde_json::Value>,
    verified_entries: u64,
    signal: SignalReport,
}

async fn phase2(store: &Store, dataset: &Dataset) -> Result<Phase2Report, String> {
    let training = dataset.training().map_err(|e| e.to_string())?;
    for row in &training {
        store
            .import_training(row)
            .await
            .map_err(|e| e.to_string())?;
    }
    let count = dataset.canaries.len();
    if count < 3 {
        return Err("Phase 2 lifecycle simulation requires at least three canaries".into());
    }
    let frozen_at = &dataset.canaries[count / 3].send_wall_utc;
    let issued_at = &dataset.canaries[count * 2 / 3].send_wall_utc;
    let mut frozen_tick = alight_forecast::tick(store, Source::Sim, "synthetic-local", frozen_at)
        .await
        .map_err(|e| e.to_string())?;
    let frozen_hash = frozen_tick["model_snapshot_hash"]
        .as_str()
        .ok_or("Missing frozen model hash")?;
    let context =
        alight_forecast::current_context(store, Source::Sim, "synthetic-local", issued_at)
            .await
            .map_err(|e| e.to_string())?;
    let candidates: Vec<_> = alight_forecast::candidates(&training, &context)
        .into_iter()
        .filter(|c| c.route == Route::BeamHttp && c.size_class == SizeClass::Small)
        .collect();
    if candidates.is_empty() {
        return Err("Simulation has no Beam HTTP small candidates at issue time".into());
    }
    let time =
        chrono::DateTime::parse_from_rfc3339(issued_at).map_err(|_| "Bad simulator clock")?;
    let last = chrono::DateTime::parse_from_rfc3339(&dataset.canaries[count - 1].send_wall_utc)
        .map_err(|_| "Bad simulator clock")?;
    let ttl = ((last - time).num_seconds() + 2).clamp(1, 86400) as u32;
    // Explicit artificial market tape, separate from owned canaries and tagged source=sim.
    let tape: Vec<_> = [100_000, 200_000, 500_000]
        .into_iter()
        .enumerate()
        .map(|(i, tip)| TipTapeObservation {
            id: format!("synthetic-tape-{i}"),
            source: Source::Sim,
            observed_at_utc: (time - chrono::Duration::seconds(60)).to_rfc3339(),
            tip_lamports: tip,
        })
        .collect();
    let congestion = dataset
        .parameters
        .shift
        .as_ref()
        .filter(|s| count as u32 * 2 / 3 >= s.at_draw)
        .map_or(dataset.parameters.congestion, |s| s.congestion);
    let mut forecasts = Vec::new();
    for target in [
        PredictionTarget::Probability {
            target_p: 0.5,
            horizon_slots: 2,
        },
        PredictionTarget::LatencyQuantile {
            quantile: 0.5,
            max_slots: Some(8.0),
            max_ms: None,
        },
    ] {
        let request = ModelQuoteRequest {
            context: context.clone(),
            candidates: candidates.clone(),
            covariates: ModelCovariates {
                congestion: Some(congestion),
            },
            leader_class_next: vec![],
            target,
        };
        forecasts.push(
            alight_forecast::issue(store, request, ttl, Some(frozen_hash), &tape)
                .await
                .map_err(|e| e.to_string())?,
        );
    }
    let mut final_tick =
        alight_forecast::tick(store, Source::Sim, "synthetic-local", &dataset.as_of_utc)
            .await
            .map_err(|e| e.to_string())?;
    // Write counts are side effects; keep replay independent of previously saved grades.
    for row in [&mut frozen_tick, &mut final_tick] {
        if let Some(object) = row.as_object_mut() {
            object.remove("grades");
        }
    }
    let grades = store
        .grades(Source::Sim)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|g| {
            g.graded_at_utc == dataset.as_of_utc
                && forecasts.iter().any(|f| f.hash == g.forecast_hash)
        })
        .collect();
    let signal =
        alight_forecast::signal_report(store, Source::Sim, "2026-10-05", &dataset.as_of_utc)
            .await
            .map_err(|e| e.to_string())?;
    store
        .save_signal(&signal)
        .await
        .map_err(|e| e.to_string())?;
    // Count this run's entries; verification still checks the entire source chain.
    store
        .verify_ledger(Source::Sim)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Phase2Report {
        verified_entries: forecasts.len() as u64,
        forecasts,
        grades,
        ticks: vec![frozen_tick, final_tick],
        signal,
    })
}

fn snapshots(dataset: &Dataset) -> Result<Vec<CurveSnapshot>, String> {
    // Only this artificial dataset establishes finalization without the live resolver.
    if dataset.source != Source::Sim || dataset.canaries.iter().any(|c| c.source != Source::Sim) {
        return Err("Offline simulator replay requires source=sim; live outcomes require resolver finalization".into());
    }
    dataset.parameters.validate().map_err(|e| e.to_string())?;
    let training = dataset.training().map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    for truth in &dataset.ground_truth {
        let context = CurveContext {
            source: Source::Sim,
            regime_id: truth.regime_id.clone(),
            region: "synthetic-local".into(),
            as_of_utc: dataset.as_of_utc.clone(),
        };
        for horizon in [1, 2, 4] {
            rows.push(
                estimate(&training, &truth.config, &context, horizon).map_err(|e| e.to_string())?,
            );
        }
    }
    Ok(rows)
}

fn write(path: &str, report: &Report) -> Result<(), String> {
    let path = Path::new(path);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|_| "Could not create simulation directory")?;
    }
    let data =
        serde_json::to_vec_pretty(report).map_err(|_| "Could not encode simulation report")?;
    if path.exists() {
        if std::fs::read(path).map_err(|_| "Could not read existing report")? == data {
            return Ok(());
        }
        return Err(
            "Output already exists with different contents; choose a new --output path".into(),
        );
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, data).map_err(|_| "Could not write simulation report")?;
    std::fs::rename(temporary, path).map_err(|_| "Could not save simulation report")?;
    Ok(())
}

pub async fn run(args: &[String]) -> Result<u8, String> {
    let replay = args[0] == "replay-model";
    let mut parameters = Parameters::default();
    let mut complete = false;
    let mut output = if replay {
        ".alight/phase2-replay.json"
    } else {
        ".alight/phase2-sim.json"
    }
    .to_owned();
    let mut database = ".alight/phase2-sim.db".to_owned();
    let mut input = None;
    let (mut shift_at, mut shift_slot_ms, mut shift_congestion) = (None, None, None);
    let mut i = 1;
    while i < args.len() {
        let option = &args[i];
        if option == "--phase2" && !replay {
            complete = true;
            i += 1;
            continue;
        }
        if option == "--flat-tip" && !replay {
            parameters.tip_slope = 0.0;
            i += 1;
            continue;
        }
        i += 1;
        let value = args
            .get(i)
            .ok_or_else(|| format!("{option} requires a value"))?;
        let invalid = || format!("Invalid value for {option}");
        match option.as_str() {
            "--output" => output = value.clone(),
            "--database" => database = value.clone(),
            "--input" if replay => input = Some(value.clone()),
            "--seed" if !replay => parameters.seed = value.parse().map_err(|_| invalid())?,
            "--canaries" if !replay => {
                parameters.canaries = value.parse().map_err(|_| invalid())?
            }
            "--slot-ms" if !replay => parameters.slot_ms = value.parse().map_err(|_| invalid())?,
            "--congestion" if !replay => {
                parameters.congestion = value.parse().map_err(|_| invalid())?
            }
            "--shift-at" if !replay => shift_at = Some(value.parse().map_err(|_| invalid())?),
            "--shift-slot-ms" if !replay => {
                shift_slot_ms = Some(value.parse().map_err(|_| invalid())?)
            }
            "--shift-congestion" if !replay => {
                shift_congestion = Some(value.parse().map_err(|_| invalid())?)
            }
            _ => return Err("Unknown simulation option; run alight for usage".into()),
        }
        i += 1;
    }
    if [
        shift_at.is_some(),
        shift_slot_ms.is_some(),
        shift_congestion.is_some(),
    ]
    .iter()
    .any(|v| *v)
    {
        parameters.shift = Some(Shift {
            at_draw: shift_at.ok_or(
                "--shift-at, --shift-slot-ms and --shift-congestion are required together",
            )?,
            slot_ms: shift_slot_ms.ok_or(
                "--shift-at, --shift-slot-ms and --shift-congestion are required together",
            )?,
            congestion: shift_congestion.ok_or(
                "--shift-at, --shift-slot-ms and --shift-congestion are required together",
            )?,
        });
    }
    let mut report = if replay {
        let input = input.ok_or("replay-model requires --input")?;
        if std::fs::metadata(&input)
            .map_err(|_| "Could not read simulator report")?
            .len()
            > 256 * 1024 * 1024
        {
            return Err("Simulator report exceeds 256 MiB".into());
        }
        let data = std::fs::read(input).map_err(|_| "Could not read simulator report")?;
        if data.len() > 256 * 1024 * 1024 {
            return Err("Simulator report exceeds 256 MiB".into());
        }
        let previous: Report =
            serde_json::from_slice(&data).map_err(|_| "Invalid simulator report")?;
        if previous.methodology_hash != methodology_hash() {
            return Err(
                "Methodology differs from this build; replay with the registered version".into(),
            );
        }
        let rebuilt = snapshots(&previous.dataset)?;
        if serde_json::to_value(&rebuilt).map_err(|_| "Could not encode replay")?
            != serde_json::to_value(&previous.snapshots)
                .map_err(|_| "Could not encode previous curves")?
        {
            return Err("Replay curves disagree with the saved report".into());
        }
        Report {
            snapshots: rebuilt,
            ..previous
        }
    } else {
        let dataset = generate(&parameters).map_err(|e| e.to_string())?;
        Report {
            methodology_hash: methodology_hash(),
            snapshots: snapshots(&dataset)?,
            dataset,
            phase2: None,
        }
    };
    let store = Store::open(Path::new(&database), 512 * 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    for row in &report.snapshots {
        store
            .save_curve_snapshot(row)
            .await
            .map_err(|e| e.to_string())?;
    }
    if complete || report.phase2.is_some() {
        let rebuilt = phase2(&store, &report.dataset).await?;
        if let Some(previous) = &report.phase2
            && serde_json::to_value(previous).map_err(|_| "Cannot encode lifecycle report")?
                != serde_json::to_value(&rebuilt).map_err(|_| "Cannot encode lifecycle replay")?
        {
            return Err("Replay forecast lifecycle disagrees with the saved report".into());
        }
        report.phase2 = Some(rebuilt);
    }
    store.close().await;
    write(&output, &report)?;
    let measured = report
        .snapshots
        .iter()
        .filter(|r| r.evidence == Evidence::Measured)
        .count();
    println!(
        "{}",
        serde_json::json!({
            "execution_mode":if replay { "replay" } else { "sim" },
            "source":"sim","canaries":report.dataset.canaries.len(),
            "snapshots":report.snapshots.len(),"measured":measured,
            "insufficient":report.snapshots.len()-measured,
            "methodology_hash":report.methodology_hash,"output":output,
            "database":database,"network_requests":0,"transactions_sent":0,
            "replay_verified":replay
            ,"phase2_lifecycle":report.phase2.is_some(),"verified_forecasts":report.phase2.as_ref().map_or(0,|r| r.verified_entries)
        })
    );
    Ok(0)
}
