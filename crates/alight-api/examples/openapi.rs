fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&alight_api::openapi_document())?
    );
    Ok(())
}
