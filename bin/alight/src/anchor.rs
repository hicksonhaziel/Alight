use alight_store::Store;
use alight_types::{AnchorDraft, Source};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
};
pub async fn run(args: &[String]) -> Result<u8, String> {
    let command=args.get(1).map(String::as_str).ok_or("Usage: alight anchor prepare|verify --database DB [--source MODE --output FILE | --input FILE]")?;
    let allowed: &[&str] = match command {
        "prepare" => &["--database", "--source", "--output"],
        "verify" => &["--database", "--input"],
        _ => return Err("Only unsigned anchor prepare and offline verify are supported".into()),
    };
    let mut options = BTreeMap::new();
    for pair in args[2..].chunks(2) {
        if pair.len() != 2
            || !allowed.contains(&pair[0].as_str())
            || options.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err("Unknown/repeated anchor option or missing value".into());
        }
    }
    let required = |key| {
        options
            .get(key)
            .copied()
            .ok_or_else(|| format!("{key} is required"))
    };
    let store = Store::open_read_only(Path::new(required("--database")?))
        .await
        .map_err(|_| "Cannot open ledger read-only")?;
    if command == "prepare" {
        let source = match required("--source")? {
            "live" => Source::Live,
            "sim" => Source::Sim,
            "replay" => Source::Replay,
            _ => return Err("Invalid anchor source".into()),
        };
        let draft = alight_canary::anchor::prepare(&store, source)
            .await
            .map_err(|_| "Anchor preparation requires a verified nonempty ledger")?;
        let bytes =
            serde_json::to_vec_pretty(&draft).map_err(|_| "Cannot encode unsigned draft")?;
        let path = required("--output")?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| "Anchor output must be a new file")?;
        if file
            .write_all(&bytes)
            .and_then(|()| file.sync_all())
            .is_err()
        {
            let _ = std::fs::remove_file(path);
            return Err("Cannot save anchor draft".into());
        }
        println!(
            "{}",
            serde_json::json!({"source":source,"status":"PREPARED_UNSIGNED","sequence":draft.sequence.to_string(),"transactions_sent":0,"output":path})
        );
    } else {
        let file =
            std::fs::File::open(required("--input")?).map_err(|_| "Cannot read anchor draft")?;
        let mut bytes = Vec::new();
        file.take(4097)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read anchor draft")?;
        if bytes.len() > 4096 {
            return Err("Anchor draft exceeds bounds".into());
        }
        let draft: AnchorDraft =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid unsigned anchor JSON")?;
        alight_canary::anchor::verify(&store, &draft)
            .await
            .map_err(|_| "Unsigned anchor verification failed")?;
        println!(
            "{}",
            serde_json::json!({"source":draft.source,"status":"OFFLINE_VERIFIED_UNSIGNED","sequence":draft.sequence.to_string(),"transactions_sent":0})
        );
    }
    store.close().await;
    Ok(0)
}
