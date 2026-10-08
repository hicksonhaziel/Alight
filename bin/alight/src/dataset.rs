//! Offline daily dataset command. The embedded exporter opens SQLite in read-only mode.
pub fn run(args: &[String]) -> Result<u8, String> {
    let status = std::process::Command::new("python3")
        .arg("-c").arg(include_str!("../../../scripts/export_dataset.py"))
        .args(&args[1..]).status().map_err(|_| "Dataset export requires Python 3; Parquet also requires scripts/dataset-requirements.txt")?;
    Ok(if status.success() { 0 } else { 2 })
}
