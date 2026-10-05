use alight_ingest::{Config, HttpProbe};
use alight_types::Verdict;
use std::process::ExitCode;
mod forecasting;
mod simulation;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

async fn run() -> Result<u8, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(
        args.first().map(String::as_str),
        Some("quote" | "ledger" | "model-tick" | "grade" | "signal")
    ) {
        return forecasting::run(&args).await;
    }

    if matches!(
        args.first().map(String::as_str),
        Some("sim" | "replay-model")
    ) {
        return simulation::run(&args).await;
    }
    if args.first().map(String::as_str) != Some("doctor") {
        eprintln!(
            "Usage: alight doctor [--network] [--output PATH]\n       alight sim [--seed N] [--canaries N] [--slot-ms N] [--congestion X] [--flat-tip]\n                  [--shift-at N --shift-slot-ms N --shift-congestion X] [--output PATH] [--database PATH]\n       alight replay-model --input PATH [--output PATH] [--database PATH]\nalight quote --database PATH --request JSON [--ttl S] [--frozen-model HASH] [--tape JSON]\n       alight ledger verify --database PATH --source live|sim|replay\n       alight model-tick|grade --database PATH --source MODE --as-of UTC [--region NAME]\n       alight signal --database PATH --source MODE --day YYYY-MM-DD --as-of UTC\nDoctor defaults to configuration presence only; --network runs read-only probes. Sim/replay need no environment, keys or network."
        );
        return Ok(2);
    }
    let mut network = false;
    let mut output = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--network" => network = true,
            "--output" => {
                i += 1;
                output = Some(args.get(i).ok_or("--output requires a path")?.clone());
            }
            _ => return Err("Unknown option; see alight doctor usage".into()),
        }
        i += 1;
    }
    let config = Config::load().map_err(|e| e.to_string())?;
    if !network {
        println!(
            "{}",
            serde_json::to_string_pretty(&config.presence())
                .map_err(|_| "Could not encode report")?
        );
        return Ok(0);
    }
    let report = HttpProbe::new()
        .map_err(|e| e.to_string())?
        .doctor(&config)
        .await;
    if let Some(path) = output {
        if let Some(parent) = std::path::Path::new(&path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| "Could not create evidence directory")?;
        }
        let data = serde_json::to_vec_pretty(&report).map_err(|_| "Could not encode evidence")?;
        std::fs::write(path, data).map_err(|_| "Could not write evidence")?;
    }
    for check in &report.checks {
        println!(
            "{:?}: {} ({} ms)",
            check.verdict, check.name, check.elapsed_ms
        );
        if check.verdict == Verdict::Fail {
            println!("  {}", check.data);
        }
    }
    println!(
        "Read-only checks complete. Canaries sent: {}.",
        report.canaries_sent
    );
    Ok(
        if report.checks.iter().any(|c| c.verdict == Verdict::Fail) {
            1
        } else if report
            .checks
            .iter()
            .any(|c| c.verdict == Verdict::Inconclusive)
        {
            3
        } else {
            0
        },
    )
}
