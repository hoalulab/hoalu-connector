use crate::cache::Cache;
use crate::model::{Bar, OhlcResponse};
use std::sync::Arc;

pub const API_BASE_URL: &str = "http://localhost:3000";

/// Periodicities in seconds to backfill when a symbol is first requested.
/// 60 = one minute, 300 = five minutes, and 86400 = one day.
pub const DEFAULT_PERIODICITIES: [i32; 3] = [60, 300, 86400];

pub async fn backfill_symbol(client: &reqwest::Client, cache: &Arc<Cache>, symbol: &str) {
    for &periodicity in DEFAULT_PERIODICITIES.iter() {
        if let Err(err) = backfill_one(client, cache, symbol, periodicity).await {
            eprintln!(
                "[market_data_core] backfill lỗi symbol={symbol} periodicity={periodicity}: {err}"
            );
        }
    }
    cache.set_backfill_complete(symbol);
}

async fn backfill_one(
    client: &reqwest::Client,
    cache: &Arc<Cache>,
    symbol: &str,
    periodicity: i32,
) -> Result<(), reqwest::Error> {
    let resolution = resolution_for(periodicity);
    let url = format!(
        "{API_BASE_URL}/price/ohlc?symbol={}&resolution={}&limit=5000",
        urlencode(symbol),
        resolution
    );

    let response = client.get(&url).send().await?;
    let payload: OhlcResponse = response.json().await?;
    let _next_time = payload.next_time;
    let bars: Vec<Bar> = payload.into_bars();
    cache.replace_bars(symbol, periodicity, bars);

    Ok(())
}

fn resolution_for(periodicity: i32) -> String {
    match periodicity {
        86400 => "1D".to_string(),
        3600 => "1H".to_string(),
        14400 => "4H".to_string(),
        seconds if seconds >= 60 && seconds % 60 == 0 => (seconds / 60).to_string(),
        seconds => seconds.to_string(),
    }
}

fn urlencode(s: &str) -> String {
    // Minimal encoding suitable for typical ticker symbols: letters, digits, dots, and dashes.
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}
