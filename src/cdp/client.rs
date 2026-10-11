use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use tracing::{debug, error, info, trace, warn};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Default timeout for CDP requests
const CDP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeout for WebSocket connection handshake
const WEBSOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Timeout for sending requests to internal channel
const CHANNEL_SEND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Serialize)]
struct CdpRequest {
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct CdpResponse {
    id: Option<u64>,
    result: Option<Value>,
    error: Option<CdpError>,
    method: Option<String>,
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CdpError {
    code: i64,
    message: String,
    data: Option<Value>,
}

pub struct CdpClient {
    sender: mpsc::Sender<(CdpRequest, oneshot::Sender<Result<Value>>)>,
    events: broadcast::Sender<Value>,
    request_id: AtomicU64,
}

impl Drop for CdpClient {
    fn drop(&mut self) {
        // When CdpClient is dropped, the sender is dropped too,
        // which will cause the websocket_handler's recv loop to exit,
        // triggering cleanup of pending requests
        debug!("CdpClient dropped, closing connection");
    }
}

impl CdpClient {
    pub async fn connect(ws_url: &str) -> Result<Self> {
        // Add timeout to WebSocket connection to prevent hanging forever
        let (ws_stream, _) = tokio::time::timeout(WEBSOCKET_CONNECT_TIMEOUT, connect_async(ws_url))
            .await
            .map_err(|_| {
                anyhow!(
                    "WebSocket connection timed out after {}s",
                    WEBSOCKET_CONNECT_TIMEOUT.as_secs()
                )
            })?
            .map_err(|e| anyhow!("WebSocket connection failed: {}", e))?;

        let (tx, rx) = mpsc::channel::<(CdpRequest, oneshot::Sender<Result<Value>>)>(32);
        let (events, _) = broadcast::channel(128);

        // Spawn the WebSocket handler task
        tokio::spawn(Self::websocket_handler(ws_stream, rx, events.clone()));

        Ok(Self {
            sender: tx,
            events,
            request_id: AtomicU64::new(1),
        })
    }

    async fn websocket_handler(
        ws_stream: WsStream,
        mut request_rx: mpsc::Receiver<(CdpRequest, oneshot::Sender<Result<Value>>)>,
        events: broadcast::Sender<Value>,
    ) {
        let (mut ws_sink, mut ws_stream) = ws_stream.split();
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_for_recv = pending.clone();
        let pending_for_send = pending.clone();

        // Task to handle incoming WebSocket messages
        let recv_task = tokio::spawn(async move {
            while let Some(msg) = ws_stream.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        trace!("CDP recv: {}", text);
                        match serde_json::from_str::<CdpResponse>(&text) {
                            Ok(response) => {
                                if let Some(id) = response.id {
                                    // This is a response to a request
                                    let mut pending = pending_for_recv.lock().await;
                                    if let Some(sender) = pending.remove(&id) {
                                        let result = if let Some(error) = response.error {
                                            Err(anyhow!(
                                                "CDP error {}: {}",
                                                error.code,
                                                error.message
                                            ))
                                        } else {
                                            Ok(response.result.unwrap_or(Value::Null))
                                        };
                                        let _ = sender.send(result);
                                    } else {
                                        warn!("Received response for unknown request id: {}", id);
                                    }
                                } else if let Some(method) = response.method {
                                    // This is an event notification
                                    trace!("CDP event: {} {:?}", method, response.params);
                                    let _ = events.send(json!({"method":method,"params":response.params.unwrap_or(Value::Null)}));
                                }
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to parse CDP response: {} - raw: {}",
                                    e,
                                    &text[..text.len().min(200)]
                                );
                            }
                        }
                    }
                    Ok(Message::Binary(data)) => {
                        trace!("CDP recv binary: {} bytes", data.len());
                    }
                    Ok(Message::Ping(data)) => {
                        trace!("CDP recv ping: {} bytes", data.len());
                        // Pong is automatically sent by tungstenite
                    }
                    Ok(Message::Pong(_)) => {
                        trace!("CDP recv pong");
                    }
                    Ok(Message::Close(frame)) => {
                        if let Some(cf) = frame {
                            info!("WebSocket closed by server: {} - {}", cf.code, cf.reason);
                        } else {
                            info!("WebSocket closed by server");
                        }
                        break;
                    }
                    Ok(Message::Frame(_)) => {
                        // Raw frame, typically not used
                        trace!("CDP recv raw frame");
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                }
            }
        });

        // Task to send requests
        let send_task = tokio::spawn(async move {
            while let Some((request, response_tx)) = request_rx.recv().await {
                let id = request.id;
                let json = match serde_json::to_string(&request) {
                    Ok(j) => j,
                    Err(e) => {
                        let _ = response_tx.send(Err(anyhow!("Serialization error: {}", e)));
                        continue;
                    }
                };

                trace!("CDP send: {}", json);

                // Store the response channel before sending
                pending_for_send.lock().await.insert(id, response_tx);

                if let Err(e) = ws_sink.send(Message::Text(json)).await {
                    error!("Failed to send WebSocket message: {}", e);
                    // Remove from pending and notify the caller of the failure
                    if let Some(sender) = pending_for_send.lock().await.remove(&id) {
                        let _ =
                            sender.send(Err(anyhow!("Failed to send WebSocket message: {}", e)));
                    }
                }
            }
        });

        // Wait for either task to complete
        tokio::select! {
            _ = recv_task => {
                info!("WebSocket receive task ended");
            },
            _ = send_task => {
                info!("WebSocket send task ended");
            },
        }

        // Clean up: Send errors to all pending requests so callers don't hang
        let mut pending_guard = pending.lock().await;
        let pending_count = pending_guard.len();
        if pending_count > 0 {
            warn!(
                "WebSocket closed with {} pending requests - sending errors",
                pending_count
            );
            for (id, sender) in pending_guard.drain() {
                let _ = sender.send(Err(anyhow!(
                    "WebSocket connection closed while request {} was pending",
                    id
                )));
            }
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
    }

    pub async fn call_method(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let start_time = std::time::Instant::now();

        debug!("CDP request #{}: {}", id, method);

        let request = CdpRequest {
            id,
            method: method.to_string(),
            params: if params == json!({}) {
                None
            } else {
                Some(params)
            },
        };

        let (tx, rx) = oneshot::channel();

        // Use timeout on channel send to prevent blocking if channel is full
        tokio::time::timeout(CHANNEL_SEND_TIMEOUT, self.sender.send((request, tx)))
            .await
            .map_err(|_| {
                anyhow!(
                    "CDP request #{} '{}': timed out sending to handler - channel may be full",
                    id,
                    method
                )
            })?
            .map_err(|_| {
                anyhow!(
                    "CDP request #{} '{}': failed to send - connection closed",
                    id,
                    method
                )
            })?;

        // Use timeout to prevent hanging forever if response never arrives
        let result = tokio::time::timeout(CDP_REQUEST_TIMEOUT, rx)
            .await
            .map_err(|_| anyhow!("CDP request #{} '{}': timed out after {}s waiting for response", id, method, CDP_REQUEST_TIMEOUT.as_secs()))?
            .map_err(|_| anyhow!("CDP request #{} '{}': response channel closed - WebSocket connection may have dropped", id, method))?;

        let elapsed = start_time.elapsed();

        match &result {
            Ok(_) => debug!("CDP request #{}: {} completed in {:?}", id, method, elapsed),
            Err(e) => debug!(
                "CDP request #{}: {} failed after {:?}: {}",
                id, method, elapsed, e
            ),
        }

        // Warn if request took a long time
        if elapsed.as_secs() > 5 {
            warn!(
                "CDP request #{} '{}' took {:?} - this may indicate performance issues",
                id, method, elapsed
            );
        }

        result
    }

    pub async fn evaluate(&self, expression: &str, await_promise: bool) -> Result<Value> {
        // Log truncated expression for debugging (large JS can spam logs)
        let truncated_expr = if expression.len() > 100 {
            format!("{}... ({} chars)", &expression[..100], expression.len())
        } else {
            expression.to_string()
        };
        trace!(
            "Evaluating JS (await={}): {}",
            await_promise,
            truncated_expr
        );

        self.call_method(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": await_promise,
                "userGesture": true
            }),
        )
        .await
    }
}
