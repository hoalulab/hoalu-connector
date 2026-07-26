use serde::Deserialize;
use std::collections::VecDeque;

/// Internal bar using f64 for precision; convert to f32 only when copying over FFI.
#[derive(Debug, Clone, Copy, Default)]
pub struct Bar {
    pub unix_microseconds: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub open_interest: f64,
    pub aux1: f64,
    pub aux2: f64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RecentQuote {
    pub last: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub previous_close: f64,
    pub change: f64,
    pub change_percent: f64,
    pub bid: f64,
    pub ask: f64,
    pub trade_volume: f64,
    pub total_volume: f64,
    pub bid_size: i32,
    pub ask_size: i32,
    pub unix_microseconds: i64,
    pub flags: i32,
}

/// Cached state for each symbol.
pub struct SymbolState {
    /// key = periodicity_seconds; value = bars in ascending chronological order.
    pub bars_by_period: std::collections::HashMap<i32, VecDeque<Bar>>,
    pub recent: RecentQuote,
    pub backfill_complete: bool,
    pub subscribed: bool,
}

impl Default for SymbolState {
    fn default() -> Self {
        Self {
            bars_by_period: std::collections::HashMap::new(),
            recent: RecentQuote::default(),
            backfill_complete: false,
            subscribed: false,
        }
    }
}

pub const MAX_BARS_PER_SYMBOL: usize = 20_000;

// ---- Data types deserialized from the REST API (localhost:3000/api) ----
// Expected response: [{ "t": 1721270400000000, "o":1.0,"h":1.0,"l":1.0,"c":1.0,"v":100.0 }, ...]
// t is Unix microseconds. Adjust this if the real API uses different units or field names.
#[derive(Debug, Deserialize)]
pub struct RestBar {
    pub t: i64,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    #[serde(default)]
    pub v: f64,
    #[serde(default)]
    pub oi: f64,
}

/// DNSE /price/ohlc response. Timestamps are Unix seconds and fields at the
/// same array index form one candle.
#[derive(Debug, Deserialize)]
pub struct OhlcResponse {
    pub t: Vec<i64>,
    pub o: Vec<f64>,
    pub h: Vec<f64>,
    pub l: Vec<f64>,
    pub c: Vec<f64>,
    #[serde(default)]
    pub v: Vec<f64>,
    #[serde(default, rename = "nextTime")]
    pub next_time: i64,
}

impl OhlcResponse {
    pub fn into_bars(self) -> Vec<Bar> {
        let count = [
            self.t.len(),
            self.o.len(),
            self.h.len(),
            self.l.len(),
            self.c.len(),
        ]
        .into_iter()
        .min()
        .unwrap_or(0);
        (0..count)
            .map(|i| Bar {
                unix_microseconds: self.t[i].saturating_mul(1_000_000),
                open: self.o[i],
                high: self.h[i],
                low: self.l[i],
                close: self.c[i],
                volume: self.v.get(i).copied().unwrap_or_default(),
                ..Bar::default()
            })
            .collect()
    }
}

impl From<&RestBar> for Bar {
    fn from(r: &RestBar) -> Self {
        Bar {
            unix_microseconds: r.t,
            open: r.o,
            high: r.h,
            low: r.l,
            close: r.c,
            volume: r.v,
            open_interest: r.oi,
            aux1: 0.0,
            aux2: 0.0,
        }
    }
}

// ---- Data types deserialized from WS (localhost:3000/ws) ----
// Expected server JSON:
// { "type": "tick", "symbol": "VNM", "t": 1721270400000000,
//   "last":1.0,"bid":1.0,"ask":1.0,"bid_size":1,"ask_size":1,"volume":1.0 }
// or { "type": "bar", "symbol":"VNM", "periodicity":60, "bar": {...RestBar...} }
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    Tick {
        symbol: String,
        t: i64,
        last: f64,
        #[serde(default)]
        bid: f64,
        #[serde(default)]
        ask: f64,
        #[serde(default)]
        bid_size: i32,
        #[serde(default)]
        ask_size: i32,
        #[serde(default)]
        volume: f64,
        #[serde(default)]
        open: f64,
        #[serde(default)]
        high: f64,
        #[serde(default)]
        low: f64,
        #[serde(default)]
        prev_close: f64,
        #[serde(default)]
        change: f64,
        #[serde(default)]
        change_percent: f64,
        #[serde(default)]
        acc_volume: f64,
        #[serde(default)]
        #[serde(rename = "acc_value")]
        _acc_value: f64,
    },
    Bar {
        symbol: String,
        periodicity: i32,
        bar: RestBar,
    },
    #[serde(other)]
    Unknown,
}
