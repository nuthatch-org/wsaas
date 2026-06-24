//! ws-gateway — Horizon TAP v2 (GraphTally) WebSocket data service.
//!
//! A consumer streams pre-parsed chain data over `/ws/{chain}/{topic}`, paying per
//! connection with a signed TAP receipt. All payment machinery (validation, pricing,
//! persistence, RAV aggregation, on-chain collection) is horizon-core; this binary
//! adds the one thing horizon-core's HTTP proxy can't do — a WebSocket relay — via
//! the composable [`horizon_core::run_with`] entry point.
//!
//! DISCLAIMER: experimental community project. Not affiliated with or endorsed by
//! The Graph Foundation or Edge & Node.

use std::sync::Arc;

use axum::{routing::get, Extension, Router};
use horizon_core::{pricing::FnPricing, Config, SharedPricing};

mod pricing;
mod ws;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ws_gateway=info,horizon_core=info".into()),
        )
        .init();

    let config = Config::load()?;
    let upstream = Arc::new(ws::WsUpstream::load()?);

    // Per-endpoint compute-unit pricing: minimum receipt value = cu_cost(path) * base.
    let base = pricing::DEFAULT_BASE_PRICE_PER_CU;
    let policy: SharedPricing =
        Arc::new(FnPricing(move |path: &str| pricing::min_receipt_value(path, base)));

    // The custom WebSocket route, sharing horizon-core's AppState + rate limiter.
    let ws_routes = Router::new()
        .route("/ws/{chain}/{topic}", get(ws::handler))
        .layer(Extension(upstream));

    tracing::info!(
        data_service = %config.tap.data_service_address,
        "ws-gateway starting — WebSocket data service on Horizon"
    );

    horizon_core::run_with(config, policy, ws_routes).await
}
