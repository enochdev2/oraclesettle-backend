use axum::{
    routing::{get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

use crate::state::AppState;

pub mod market;
pub mod report;
pub mod settlement;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/markets", post(market::create_market).get(market::list_markets))
        .route(
            "/markets/:id/reports",
            post(report::create_report).get(report::list_reports),
        )
        .route("/markets/:id/settlement", get(settlement::get_settlement))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

async fn health() -> &'static str {
    "OK"
}