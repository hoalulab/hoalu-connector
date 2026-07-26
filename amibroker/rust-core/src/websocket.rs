use crate::cache::Cache;
use crate::ffi::CallbackHandle;
use crate::model::{Bar, RecentQuote, WsMessage};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

pub const WS_URL: &str = "ws://localhost:3000/ws";

/// Commands sent from the engine to the WS worker, such as a new symbol request.
pub enum WsCommand {
    Subscribe(String),
}

pub async fn run_ws_worker(
    cache: Arc<Cache>,
    callback: Arc<CallbackHandle>,
    mut commands: mpsc::UnboundedReceiver<WsCommand>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    // Subscribed symbols must be sent again after every reconnect.
    let mut subscribed_symbols: Vec<String> = Vec::new();

    loop {
        if *shutdown.borrow() {
            return;
        }

        match tokio_tungstenite::connect_async(WS_URL).await {
            Ok((stream, _response)) => {
                eprintln!("[market_data_core] WS connected: {WS_URL}");
                let (mut write, mut read) = stream.split();

                // Resubscribe to symbols that existed before the connection was lost.
                for symbol in &subscribed_symbols {
                    let _ = send_subscribe(&mut write, symbol).await;
                }

                let mut heartbeat = tokio::time::interval(Duration::from_secs(20));

                loop {
                    tokio::select! {
                        _ = shutdown.changed() => {
                            if *shutdown.borrow() {
                                let _ = write.close().await;
                                return;
                            }
                        }
                        _ = heartbeat.tick() => {
                            if write.send(Message::Ping(vec![])).await.is_err() {
                                break;
                            }
                        }
                        cmd = commands.recv() => {
                            match cmd {
                                Some(WsCommand::Subscribe(symbol)) => {
                                    if !subscribed_symbols.contains(&symbol) {
                                        subscribed_symbols.push(symbol.clone());
                                    }
                                    let _ = send_subscribe(&mut write, &symbol).await;
                                }
                                None => return, // The engine has been destroyed.
                            }
                        }
                        msg = read.next() => {
                            match msg {
                                Some(Ok(Message::Text(text))) => {
                                    handle_message(&cache, &callback, &text);
                                }
                                Some(Ok(Message::Ping(payload))) => {
                                    let _ = write.send(Message::Pong(payload)).await;
                                }
                                Some(Ok(_)) => {}
                                Some(Err(err)) => {
                                    eprintln!("[market_data_core] WS lỗi: {err}");
                                    break;
                                }
                                None => {
                                    eprintln!("[market_data_core] WS đóng kết nối, sẽ reconnect");
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Err(err) => {
                eprintln!("[market_data_core] Không kết nối được WS: {err}");
            }
        }

        if *shutdown.borrow() {
            return;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn send_subscribe(
    write: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    symbol: &str,
) -> Result<(), tokio_tungstenite::tungstenite::Error> {
    // The server is expected to accept: {"type":"subscribe","symbol":"VNM"}
    let payload = serde_json::json!({ "type": "subscribe", "symbol": symbol });
    write.send(Message::Text(payload.to_string())).await
}

fn handle_message(cache: &Arc<Cache>, callback: &Arc<CallbackHandle>, text: &str) {
    let parsed: Result<WsMessage, _> = serde_json::from_str(text);
    let Ok(message) = parsed else {
        return; // Ignore malformed messages without panicking.
    };

    match message {
        WsMessage::Tick {
            symbol,
            t,
            last,
            bid,
            ask,
            bid_size,
            ask_size,
            volume,
            open,
            high,
            low,
            prev_close,
            change,
            change_percent,
            acc_volume,
            _acc_value: _,
        } => {
            let recent = RecentQuote {
                last,
                open,
                high,
                low,
                previous_close: prev_close,
                change,
                change_percent,
                bid,
                ask,
                trade_volume: volume,
                total_volume: if acc_volume > 0.0 { acc_volume } else { volume },
                bid_size,
                ask_size,
                unix_microseconds: t,
                flags: 0,
            };
            cache.update_recent(&symbol, recent);
            // No lock is held during the callback; cache.update_recent releases it first.
            callback.invoke(&symbol, &recent);
        }
        WsMessage::Bar {
            symbol,
            periodicity,
            bar,
        } => {
            let bar: Bar = (&bar).into();
            cache.upsert_last_bar(&symbol, periodicity, bar);
            // A bar update must notify AmiBroker independently from quote
            // ticks. Otherwise Realtime Quote changes while GetQuotesEx is
            // never called again and the chart remains stale.
            if let Some(recent) = cache.get_recent(&symbol) {
                callback.invoke(&symbol, &recent);
            }
        }
        WsMessage::Unknown => {}
    }
}
