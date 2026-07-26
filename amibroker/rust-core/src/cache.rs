use crate::model::{Bar, RecentQuote, SymbolState, MAX_BARS_PER_SYMBOL};
use std::collections::HashMap;
use std::sync::RwLock;

/// Cache shared by the REST backfill worker, WS worker, and FFI reader (GetQuotesEx).
/// Locks are held only briefly while copying data and never during network calls
/// or callbacks into C++.
pub struct Cache {
    symbols: RwLock<HashMap<String, SymbolState>>,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            symbols: RwLock::new(HashMap::new()),
        }
    }

    pub fn ensure_symbol(&self, symbol: &str) {
        let mut guard = self.symbols.write().unwrap();
        guard.entry(symbol.to_string()).or_default();
    }

    pub fn mark_subscribed(&self, symbol: &str) {
        let mut guard = self.symbols.write().unwrap();
        guard.entry(symbol.to_string()).or_default().subscribed = true;
    }

    pub fn is_subscribed(&self, symbol: &str) -> bool {
        let guard = self.symbols.read().unwrap();
        guard.get(symbol).map(|s| s.subscribed).unwrap_or(false)
    }

    pub fn set_backfill_complete(&self, symbol: &str) {
        let mut guard = self.symbols.write().unwrap();
        guard
            .entry(symbol.to_string())
            .or_default()
            .backfill_complete = true;
    }

    pub fn is_backfill_complete(&self, symbol: &str) -> bool {
        let guard = self.symbols.read().unwrap();
        guard
            .get(symbol)
            .map(|s| s.backfill_complete)
            .unwrap_or(false)
    }

    /// Load all historical bars after REST backfill, replacing the previous series.
    pub fn replace_bars(&self, symbol: &str, periodicity: i32, mut bars: Vec<Bar>) {
        bars.sort_by_key(|b| b.unix_microseconds);
        if bars.len() > MAX_BARS_PER_SYMBOL {
            let excess = bars.len() - MAX_BARS_PER_SYMBOL;
            bars.drain(0..excess);
        }
        let mut guard = self.symbols.write().unwrap();
        let state = guard.entry(symbol.to_string()).or_default();
        state
            .bars_by_period
            .insert(periodicity, bars.into_iter().collect());
    }

    /// Add or update the current streaming bar. Update the final bar when it has
    /// the same aggregated time bucket; otherwise append a new bar.
    pub fn upsert_last_bar(&self, symbol: &str, periodicity: i32, bar: Bar) {
        let mut guard = self.symbols.write().unwrap();
        let state = guard.entry(symbol.to_string()).or_default();
        let deque = state.bars_by_period.entry(periodicity).or_default();

        match deque.back_mut() {
            Some(last) if last.unix_microseconds == bar.unix_microseconds => {
                *last = bar;
            }
            _ => {
                deque.push_back(bar);
                if deque.len() > MAX_BARS_PER_SYMBOL {
                    deque.pop_front();
                }
            }
        }
    }

    pub fn update_recent(&self, symbol: &str, recent: RecentQuote) {
        let mut guard = self.symbols.write().unwrap();
        guard.entry(symbol.to_string()).or_default().recent = recent;
    }

    /// Copy up to `capacity` recent bars into `out` and return the copied count.
    pub fn copy_bars(&self, symbol: &str, periodicity: i32, out: &mut [Bar]) -> usize {
        let guard = self.symbols.read().unwrap();
        let Some(state) = guard.get(symbol) else {
            return 0;
        };
        let Some(deque) = state.bars_by_period.get(&periodicity) else {
            return 0;
        };

        let take = deque.len().min(out.len());
        let skip = deque.len() - take;
        for (i, bar) in deque.iter().skip(skip).enumerate() {
            out[i] = *bar;
        }
        take
    }

    pub fn get_recent(&self, symbol: &str) -> Option<RecentQuote> {
        let guard = self.symbols.read().unwrap();
        guard.get(symbol).map(|s| s.recent)
    }
}
