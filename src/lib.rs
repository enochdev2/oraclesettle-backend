pub mod state;
pub mod types;
pub mod routes;

pub mod eth;
pub mod models;
pub mod proof;
pub mod worker;

// Optional: expose a router builder so main.rs can be tiny
use axum::Router;
use state::AppState;

pub fn app(state: AppState) -> Router {
    routes::router(state)
}