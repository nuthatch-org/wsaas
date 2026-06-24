//! WebSocket data-service handler.
//!
//! A consumer connects to `/ws/{chain}/{topic}?receipt=<TAP-Receipt JSON>`.
//! The TAP v2 receipt is validated + priced + persisted by horizon-core's
//! [`gate_request`](horizon_core::proxy::gate_request) *before* the upgrade — no
//! receipt, no stream. We then open the upstream Pinax WebSocket and relay every
//! message to the client. Settlement (RAV aggregation + on-chain collect) is the
//! shared horizon-core background machinery; only the transport differs.

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Extension, Path, Query, State,
    },
    http::StatusCode,
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as TMsg;
use tracing::{info, warn};

use horizon_core::AppState;

/// Upstream Pinax WebSocket coordinates — wsaas-specific config, carried as an
/// Axum `Extension` since it isn't part of horizon-core's `Config`.
#[derive(Debug, Clone, Deserialize)]
pub struct WsUpstream {
    /// Base wss:// URL, e.g. "wss://ws.pinax.network".
    pub pinax_ws_base: String,
    /// Pinax API token, sent as the `?token=` query param on the upstream URL.
    pub pinax_token: String,
}

impl WsUpstream {
    /// Load the Pinax fields from the `[backend]` table of `$GATEWAY_CONFIG`
    /// (defaults to `config.toml`). horizon-core ignores these fields; we ignore its.
    pub fn load() -> anyhow::Result<Self> {
        #[derive(Deserialize)]
        struct File {
            backend: WsUpstream,
        }
        let path = std::env::var("GATEWAY_CONFIG").unwrap_or_else(|_| "config.toml".to_string());
        let contents = std::fs::read_to_string(&path)?;
        Ok(toml::from_str::<File>(&contents)?.backend)
    }
}

#[derive(Deserialize)]
pub struct WsQuery {
    /// TAP v2 receipt (same JSON shape as the `TAP-Receipt` header), url-encoded.
    pub receipt: String,
}

pub async fn handler(
    State(state): State<AppState>,
    Extension(upstream): Extension<Arc<WsUpstream>>,
    Path((chain, topic)): Path<(String, String)>,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, (StatusCode, String)> {
    // Validate + price + persist the receipt before upgrading. The pricing policy
    // (see main.rs) sets the minimum receipt value for this path.
    let path = format!("/ws/{chain}/{topic}");
    let _validated = horizon_core::proxy::gate_request(&state, &q.receipt, &path).await?;

    let url = format!(
        "{}/ws/{}@{}?token={}",
        upstream.pinax_ws_base.trim_end_matches('/'),
        chain,
        topic,
        upstream.pinax_token,
    );
    info!(%chain, %topic, "ws session authorised; opening upstream");
    Ok(ws.on_upgrade(move |socket| relay(socket, url)))
}

/// Pipe the upstream Pinax stream to the consumer until either side closes.
async fn relay(mut client: WebSocket, upstream_url: String) {
    let (upstream, _) = match connect_async(&upstream_url).await {
        Ok(x) => x,
        Err(e) => {
            warn!(error = %e, "upstream connect failed");
            let _ = client
                .send(Message::Text(
                    format!("{{\"error\":\"upstream connect failed: {e}\"}}").into(),
                ))
                .await;
            return;
        }
    };
    let (mut up_tx, mut up_rx) = upstream.split();
    let mut delivered: u64 = 0;

    loop {
        tokio::select! {
            up = up_rx.next() => match up {
                Some(Ok(TMsg::Text(t))) => {
                    if client.send(Message::Text(t.to_string().into())).await.is_err() { break; }
                    delivered += 1;
                }
                Some(Ok(TMsg::Binary(b))) => {
                    if client.send(Message::Binary(b.to_vec().into())).await.is_err() { break; }
                    delivered += 1;
                }
                Some(Ok(TMsg::Close(_))) | None => break,
                Some(Ok(_)) => {} // ping/pong/frame — not billable
                Some(Err(e)) => { warn!(error = %e, "upstream stream error"); break; }
            },
            cl = client.recv() => match cl {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(_)) => break,
                _ => {} // subscription is encoded in the path; ignore client frames
            }
        }
    }
    let _ = up_tx.close().await;
    info!(delivered, "ws session closed");
}
