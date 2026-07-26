use crate::backfill;
use crate::cache::Cache;
use crate::ffi::CallbackHandle;
use crate::websocket::{self, WsCommand};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};

pub struct Engine {
    pub cache: Arc<Cache>,
    callback: Arc<CallbackHandle>,
    runtime: Mutex<Option<tokio::runtime::Runtime>>,
    ws_commands: Mutex<Option<mpsc::UnboundedSender<WsCommand>>>,
    shutdown_tx: Mutex<Option<watch::Sender<bool>>>,
    started: AtomicBool,
    last_error: Mutex<String>,
    http_client: reqwest::Client,
}

impl Engine {
    pub fn new(callback: CallbackHandle) -> Self {
        Self {
            cache: Arc::new(Cache::new()),
            callback: Arc::new(callback),
            runtime: Mutex::new(None),
            ws_commands: Mutex::new(None),
            shutdown_tx: Mutex::new(None),
            started: AtomicBool::new(false),
            last_error: Mutex::new(String::new()),
            http_client: reqwest::Client::new(),
        }
    }

    pub fn start(&self) -> Result<(), String> {
        if self.started.swap(true, Ordering::SeqCst) {
            return Ok(()); // Already started; no-op.
        }

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| format!("không tạo được tokio runtime: {e}"))?;

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<WsCommand>();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let cache = self.cache.clone();
        let callback = self.callback.clone();

        rt.spawn(websocket::run_ws_worker(
            cache,
            callback,
            cmd_rx,
            shutdown_rx,
        ));

        *self.runtime.lock().unwrap() = Some(rt);
        *self.ws_commands.lock().unwrap() = Some(cmd_tx);
        *self.shutdown_tx.lock().unwrap() = Some(shutdown_tx);

        Ok(())
    }

    pub fn stop(&self) {
        if let Some(tx) = self.shutdown_tx.lock().unwrap().take() {
            let _ = tx.send(true);
        }
        *self.ws_commands.lock().unwrap() = None;

        // Join the runtime thread during background drop to avoid blocking AmiBroker's UI thread.
        if let Some(rt) = self.runtime.lock().unwrap().take() {
            rt.shutdown_timeout(std::time::Duration::from_secs(3));
        }
        self.started.store(false, Ordering::SeqCst);
    }

    /// Request a symbol: run REST backfill once and subscribe to realtime WS data.
    pub fn request_symbol(&self, symbol: &str) {
        self.cache.ensure_symbol(symbol);

        let runtime_guard = self.runtime.lock().unwrap();
        let Some(rt) = runtime_guard.as_ref() else {
            self.set_error("engine chưa start");
            return;
        };

        // REST backfill is idempotent through cache.set_backfill_complete.
        if !self.cache.is_backfill_complete(symbol) {
            let cache = self.cache.clone();
            let client = self.http_client.clone();
            let symbol_owned = symbol.to_string();
            rt.spawn(async move {
                backfill::backfill_symbol(&client, &cache, &symbol_owned).await;
            });
        }

        // Subscribe to realtime WS data (idempotent).
        if !self.cache.is_subscribed(symbol) {
            self.cache.mark_subscribed(symbol);
            if let Some(tx) = self.ws_commands.lock().unwrap().as_ref() {
                let _ = tx.send(WsCommand::Subscribe(symbol.to_string()));
            }
        }
    }

    pub fn set_error(&self, msg: &str) {
        *self.last_error.lock().unwrap() = msg.to_string();
    }

    pub fn last_error(&self) -> String {
        self.last_error.lock().unwrap().clone()
    }
}
