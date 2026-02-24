use sqlx::postgres::PgPoolOptions;
use std::net::SocketAddr;

use oraclesettle_backend::{app, state::AppState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&db_url)
        .await
        .expect("Failed to connect DB");

    let state = AppState { db: pool };

    // spawn loops/workers here (or move them into lib as well)
    let worker_state = state.clone();
    tokio::spawn(async move { oraclesettle_backend::worker::run_worker(worker_state).await });

    let app = app(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    // let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}