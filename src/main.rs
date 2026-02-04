use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::State,
};
use axum::extract::Path;

use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::net::SocketAddr;
use tracing_subscriber;
use uuid::Uuid;
use chrono::Utc;

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
}

#[derive(Serialize)]
struct Market {
    id: String,
    question: String,
    closes_at: String,
    status: String,
    created_at: String,
}

#[derive(Serialize)]
struct Report {
    id: String,
    market_id: String,
    source: String,
    value: f64,
    created_at: String,
}

#[derive(Deserialize)]
struct CreateReportRequest {
    source: String,
    value: f64,
    idempotency_key: String,
}


#[derive(Deserialize)]
struct CreateMarketRequest {
    question: String,
    closes_at: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Load env
    dotenvy::dotenv().ok();

    let db_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .expect("Failed to connect DB");

    let state = AppState { db: pool };

    let app = Router::new()
        .route("/health", get(health))
        .route("/markets", post(create_market).get(list_markets))
        .route("/markets/:id/reports", post(create_report).get(list_reports))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    tracing::info!("Server running on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap();

    axum::serve(listener, app)
        .await
        .unwrap();
}

async fn health() -> &'static str {
    "OK"
}

async fn create_market(
    State(state): State<AppState>,
    Json(payload): Json<CreateMarketRequest>,
) -> &'static str {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"
        INSERT INTO markets (id, question, closes_at, status, created_at)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&payload.question)
    .bind(&payload.closes_at)
    .bind("OPEN")
    .bind(&now)
    .execute(&state.db)
    .await
    .unwrap();

    "Market created"
}

async fn list_markets(
    State(state): State<AppState>,
) -> Json<Vec<Market>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, question, closes_at, status, created_at
        FROM markets
        ORDER BY created_at DESC
        "#
    )
    .fetch_all(&state.db)
    .await
    .unwrap();

    let markets = rows
        .into_iter()
        .map(|row| Market {
            id: row.id.unwrap_or_default(),
            question: row.question,
            closes_at: row.closes_at,
            status: row.status,
            created_at: row.created_at,
        })
        .collect();

    Json(markets)
}

async fn create_report(
    State(state): State<AppState>,
    Path(market_id): Path<String>,
    Json(payload): Json<CreateReportRequest>,
) -> Result<&'static str, (axum::http::StatusCode, String)> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let result = sqlx::query(
        r#"
        INSERT INTO reports
        (id, market_id, source, value, idempotency_key, created_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&market_id)
    .bind(&payload.source)
    .bind(payload.value)
    .bind(&payload.idempotency_key)
    .bind(&now)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => Ok("Report submitted"),

        Err(e) => {
            if e.to_string().contains("UNIQUE") {
                Err((
                    axum::http::StatusCode::CONFLICT,
                    "Duplicate report or idempotency key".to_string(),
                ))
            } else {
                Err((
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    e.to_string(),
                ))
            }
        }
    }
}

async fn list_reports(
    State(state): State<AppState>,
    Path(market_id): Path<String>,
) -> Json<Vec<Report>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, market_id, source, value, created_at
        FROM reports
        WHERE market_id = ?
        ORDER BY created_at ASC
        "#,
        market_id
    )
    .fetch_all(&state.db)
    .await
    .unwrap();

    let reports = rows
        .into_iter()
        .map(|row| Report {
            id: row.id.unwrap_or_default(),
            market_id: row.market_id,
            source: row.source,
            value: row.value,
            created_at: row.created_at,
        })
        .collect();

    Json(reports)
}



// let listener = tokio::net::TcpListener::bind(&addr)
//     .await
//     .unwrap();

// axum::serve(listener, app)
//     .await
//     .unwrap();