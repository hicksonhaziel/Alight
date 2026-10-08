use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReceiptQuery {
    capture: Option<String>,
    region: Option<String>,
    target_p: Option<f64>,
    horizon_slots: Option<u32>,
    max_curve_age_s: Option<u32>,
}
pub(super) async fn receipt(
    State(s): State<ApiState>,
    Path(wallet): Path<String>,
    query: Result<Query<ReceiptQuery>, QueryRejection>,
) -> Result<Json<WalletReceipt>, ApiError> {
    let Query(q) = query.map_err(|_| s.invalid())?;
    if !(32..=44).contains(&wallet.len())
        || !wallet
            .bytes()
            .all(|b| b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz".contains(&b))
    {
        return Err(s.invalid());
    }
    let hash = q.capture.ok_or_else(|| s.error(StatusCode::SERVICE_UNAVAILABLE, "HISTORY_UNAVAILABLE", "Wallet history requires an explicitly imported capture; Data API history is unverified"))?;
    if hash.len() != 71
        || !hash.starts_with("sha256:")
        || !hash[7..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(s.invalid());
    }
    let _permit = s.work()?;
    let capture = s
        .store
        .wallet_capture(s.source, &wallet, &hash)
        .await
        .map_err(|_| s.unavailable())?
        .ok_or_else(|| {
            s.error(
                StatusCode::NOT_FOUND,
                "HISTORY_NOT_FOUND",
                "No matching source-scoped wallet capture",
            )
        })?;
    if utc(&capture.through_utc).map_err(|_| s.unavailable())?
        > utc(&s.now().await).map_err(|_| s.unavailable())?
    {
        return Err(s.invalid());
    }
    let request = WalletReceiptRequest {
        region: q.region.unwrap_or(s.region.clone()),
        target_p: q.target_p.unwrap_or(0.9),
        horizon_slots: q.horizon_slots.unwrap_or(2),
        max_curve_age_s: q.max_curve_age_s.unwrap_or(300),
    };
    Ok(Json(
        alight_tape::receipts::evaluate(&s.store, &capture, &request)
            .await
            .map_err(|e| match e {
                alight_tape::receipts::ReceiptError::Store(_) => s.unavailable(),
                _ => s.invalid(),
            })?,
    ))
}
