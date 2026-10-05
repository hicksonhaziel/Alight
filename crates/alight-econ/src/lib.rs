//! Fixture-backed Blur adapters, bounded price evidence and conditional cost estimates.
pub mod blur;
pub mod optimizer;
pub mod series;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EconError {
    #[error("invalid economics input or market schema")]
    Invalid,
    #[error("invalid Blur endpoint or credential configuration")]
    Configuration,
    #[error("Blur request or connection failed (details withheld)")]
    Transport,
    #[error("Blur HTTP status {0}")]
    Http(u16),
    #[error("Blur response exceeds byte limit")]
    TooLarge,
    #[error("market trade conflicts with retained evidence")]
    Conflict,
    #[error("Blur consumer closed")]
    Closed,
}

/// Parse a preserved decimal string for approximate arithmetic, without replacing its text.
pub(crate) fn positive_decimal(text: &str) -> Result<f64, EconError> {
    let value = text.parse::<f64>().map_err(|_| EconError::Invalid)?;
    if !value.is_finite()
        || value <= 0.0
        || text.len() > 128
        || !text.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return Err(EconError::Invalid);
    }
    Ok(value)
}

pub(crate) fn utc(text: &str) -> Result<chrono::DateTime<chrono::Utc>, EconError> {
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|t| t.with_timezone(&chrono::Utc))
        .map_err(|_| EconError::Invalid)
}
