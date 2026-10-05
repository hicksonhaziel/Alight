use alight_model::{estimate, methodology_hash};
use alight_sim::{Dataset, Parameters, Shift, generate};
use alight_store::Store;
use alight_types::{CurveContext, CurveSnapshot, Evidence, Source, TrainingCanary};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct Report {
    methodology_hash: String,
    dataset: Dataset,
    snapshots: Vec<CurveSnapshot>,
}

fn snapshots(dataset: &Dataset) -> Result<Vec<CurveSnapshot>, String> {
    // Only this artificial dataset establishes finalization without the live resolver.
    if dataset.source != Source::Sim || dataset.canaries.iter().any(|c| c.source != Source::Sim) {
        return Err("Offline simulator replay requires source=sim; live outcomes require resolver finalization".into());
    }
    dataset.parameters.validate().map_err(|e| e.to_string())?;
    let training: Vec<_> = dataset
        .canaries
        .iter()
        .cloned()
        .map(|canary| TrainingCanary {
            canary,
            finalized: true,
        })
        .collect();
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
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, data).map_err(|_| "Could not write simulation report")?;
    std::fs::rename(temporary, path).map_err(|_| "Could not save simulation report")?;
    Ok(())
}

pub async fn run(args: &[String]) -> Result<u8, String> {
    let replay = args[0] == "replay-model";
    let mut parameters = Parameters::default();
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
    let report = if replay {
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
        })
    );
    Ok(0)
}
