use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::State,
};
use sha2::{Sha256, Digest};
use axum::extract::Path;
use tower_http::cors::{CorsLayer, Any};


use serde::{Deserialize, Serialize};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::net::SocketAddr;
use tracing_subscriber;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::proof::{hash_leaf, build_merkle_root};
// use crate::eth::submit::submit_settlement;
use crate::models::outbox::SettlementPayload; 


mod models;
mod worker;
mod proof;
mod eth;


#[derive(Clone)]
struct AppState {
    db: PgPool,
}

#[derive(Serialize)]
struct Market {
    id: Uuid,
    question: String,
    closes_at: DateTime<Utc>,
    status: String,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct Report {
    id: Uuid,
    market_id: Uuid,
    source: String,
    value: f64,
    created_at: DateTime<Utc>,
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
    // accept RFC3339 string from client, parse into DateTime<Utc>
    closes_at: String,
}



#[derive(Serialize)]
struct Settlement {
    id: String,
    market_id: String,
    outcome: f64,
    decided_at: String,
}

#[derive(Serialize)]
struct SettlementView {
    market_id: Uuid,
    outcome: f64,
    decided_at: DateTime<Utc>,
    reports: Vec<Report>,
    hash: String,
}


#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Load env
    dotenvy::dotenv().ok();

    let db_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
    .max_connections(10)
    .connect(&db_url)
    .await
    .expect("Failed to connect DB");

    let state = AppState { db: pool };
    
    let resolver_state = state.clone();

    tokio::spawn(async move {
        resolver_loop(resolver_state).await;
    });

    let batch_state = state.clone();

    tokio::spawn(async move {
        batcher_loop(batch_state).await;
    });

    let worker_state = state.clone();
    tokio::spawn(async move {
        worker::run_worker(worker_state).await;
    });




    let app = Router::new()
    .route("/health", get(health))
    .route("/markets", post(create_market).get(list_markets))
    .route("/markets/:id/reports", post(create_report).get(list_reports))
    .route("/markets/:id/settlement", get(get_settlement))
    .layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any),
    )
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
) -> Result<&'static str, (axum::http::StatusCode, String)> {
    let id = Uuid::new_v4();
    let now = Utc::now();

    let closes_at = chrono::DateTime::parse_from_rfc3339(&payload.closes_at)
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?
        .with_timezone(&Utc);
    

    sqlx::query(
        r#"
        INSERT INTO markets (id, question, closes_at, status, created_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(id)
    .bind(&payload.question)
    .bind(closes_at)
    .bind("OPEN")
    .bind(now)
    .execute(&state.db)
    .await
    .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok("Market created")
}


async fn list_markets(State(state): State<AppState>) -> Json<Vec<Market>> {
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
            id: row.id,
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
    Path(market_id): Path<Uuid>,
    Json(payload): Json<CreateReportRequest>,
) -> Result<&'static str, (axum::http::StatusCode, String)> {
    let id = Uuid::new_v4();
    let now = Utc::now();

    let market = sqlx::query!(
        "SELECT status FROM markets WHERE id = $1",
        market_id
    )
    .fetch_one(&state.db)
    .await
    .map_err(|_| (axum::http::StatusCode::NOT_FOUND, "Market not found".to_string()))?;

    if market.status != "OPEN" {
        return Err((axum::http::StatusCode::BAD_REQUEST, "Market is closed".to_string()));
    }

    let result = sqlx::query(
        r#"
        INSERT INTO reports (id, market_id, source, value, idempotency_key, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(market_id)
    .bind(&payload.source)
    .bind(payload.value)
    .bind(&payload.idempotency_key)
    .bind(now)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => Ok("Report submitted"),
        Err(e) => {
            // better pg unique detection: check SQLSTATE 23505
            if let Some(db_err) = e.as_database_error() {
                if db_err.code().as_deref() == Some("23505") {
                    return Err((axum::http::StatusCode::CONFLICT, "Duplicate report or idempotency key".to_string()));
                }
            }
            Err((axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

async fn list_reports(
    State(state): State<AppState>,
    Path(market_id): Path<Uuid>,
) -> Json<Vec<Report>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, market_id, source, value, created_at
        FROM reports
        WHERE market_id = $1
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
            id: row.id,
            market_id: row.market_id,
            source: row.source,
            value: row.value,
            created_at: row.created_at,
        })
        .collect();

    Json(reports)
}

async fn resolver_loop(state: AppState) {
    
    loop {
    auto_close_markets(&state).await;
    resolve_markets(&state).await;

    tokio::time::sleep(
        std::time::Duration::from_secs(10),
    )
    .await;
}

}


async fn resolve_markets(state: &AppState) {
    let markets = sqlx::query!(
        r#"
        SELECT id, closes_at
        FROM markets
        WHERE status = 'CLOSED'
        LIMIT 10
        "#
    )
    .fetch_all(&state.db)
    .await
    .unwrap();

    let now = Utc::now();

    for market in markets {
        if now < market.closes_at {
            continue;
        }

        let reports = sqlx::query!(
            r#"SELECT value FROM reports WHERE market_id = $1"#,
            market.id
        )
        .fetch_all(&state.db)
        .await
        .unwrap();

        let values: Vec<f64> = reports.into_iter().map(|r| r.value).collect();

        if let Some(outcome) = try_resolve(&values) {
            finalize_market(state, market.id, outcome).await;
        }
    }
}


async fn finalize_market(state: &AppState, market_id: Uuid, outcome: f64) {
    let settlement_id = Uuid::new_v4();
    let now = Utc::now();

    let mut hasher = Sha256::new();
    hasher.update(market_id.as_bytes());
    let market_hash: [u8; 32] = hasher.finalize().into();

    let data = format!("{}:{}:{}", market_id, outcome, now.to_rfc3339());
    let leaf = hash_leaf(&data);

    let outcome_u64 = outcome as u64;
    let ts = now.timestamp() as u64;

    let payload = SettlementPayload {
        market_id: market_id.to_string(),
        market_hash_hex: hex::encode(market_hash),
        leaf_hex: hex::encode(leaf),
        outcome_u64,
        ts,
    };

    let payload_json = serde_json::to_value(&payload).unwrap();

    let mut tx = state.db.begin().await.unwrap();

    sqlx::query(
        r#"
        INSERT INTO settlements (id, market_id, outcome, decided_at)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(settlement_id)
    .bind(market_id)
    .bind(outcome)
    .bind(now)
    .execute(&mut *tx)
    .await
    .unwrap();

    sqlx::query(
        r#"
        UPDATE markets
        SET status = 'RESOLVED'
        WHERE id = $1 AND status = 'CLOSED'
        "#,
    )
    .bind(market_id)
    .execute(&mut *tx)
    .await
    .unwrap();

    let outbox_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO outbox
        (id, market_id, payload, status, retries, last_error, created_at, updated_at)
        VALUES ($1, $2, $3, 'PENDING', 0, NULL, $4, $5)
        "#,
    )
    .bind(outbox_id)
    .bind(market_id)
    .bind(payload_json)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .unwrap();

    tx.commit().await.unwrap();

    tracing::info!("Queued settlement in outbox id={}", outbox_id);
}

async fn get_settlement(
    State(state): State<AppState>,
    Path(market_id): Path<Uuid>,
) -> Result<Json<SettlementView>, axum::http::StatusCode> {
    let settlement = sqlx::query!(
        r#"
        SELECT outcome, decided_at
        FROM settlements
        WHERE market_id = $1
        "#,
        market_id
    )
    .fetch_optional(&state.db)
    .await
    .unwrap()
    .ok_or(axum::http::StatusCode::NOT_FOUND)?;

    let reports_rows = sqlx::query!(
        r#"
        SELECT id, market_id, source, value, created_at
        FROM reports
        WHERE market_id = $1
        ORDER BY created_at ASC
        "#,
        market_id
    )
    .fetch_all(&state.db)
    .await
    .unwrap();

    let reports: Vec<Report> = reports_rows
        .into_iter()
        .map(|r| Report {
            id: r.id,
            market_id: r.market_id,
            source: r.source,
            value: r.value,
            created_at: r.created_at,
        })
        .collect();

    let hash = settlement_hash(
        &market_id.to_string(),
        settlement.outcome,
        &settlement.decided_at.to_rfc3339(),
        &reports
            .iter()
            .map(|r| Report {
                id: r.id,
                market_id: r.market_id,
                source: r.source.clone(),
                value: r.value,
                created_at: r.created_at.to_rfc3339(),
            })
            .collect::<Vec<_>>(),
    );

    Ok(Json(SettlementView {
        market_id,
        outcome: settlement.outcome,
        decided_at: settlement.decided_at,
        reports,
        hash,
    }))
}


fn settlement_hash(
    market_id: &str,
    outcome: f64,
    decided_at: &str,
    reports: &[Report],
) -> String {
    let mut hasher = Sha256::new();

    hasher.update(market_id.as_bytes());
    hasher.update(outcome.to_string());
    hasher.update(decided_at.as_bytes());

    for r in reports {
        hasher.update(r.id.as_bytes());
        hasher.update(r.source.as_bytes());
        hasher.update(r.value.to_string());
        hasher.update(r.created_at.as_bytes());
    }

    hex::encode(hasher.finalize())
}

async fn collect_unbatched_settlements(
    state: &AppState,
) -> Vec<(String, String)> {
    let rows = sqlx::query!(
        r#"
        SELECT market_id, decided_at
        FROM settlements
        WHERE market_id NOT IN (
            SELECT market_id FROM batches
        )
        "#
    )
    .fetch_all(&state.db)
    .await
    .unwrap();

    rows.into_iter()
        .map(|r| (r.market_id, r.decided_at))
        .collect()
}

async fn batcher_loop(state: AppState) {
    loop {
        create_batch(&state).await;

        tokio::time::sleep(
            std::time::Duration::from_secs(30),
        )
        .await;
    }
}

async fn create_batch(state: &AppState) {
    let rows = sqlx::query!(
        r#"
        SELECT s.market_id, s.outcome, s.decided_at
        FROM settlements s
        LEFT JOIN batch_items b
          ON s.market_id = b.market_id
        WHERE b.market_id IS NULL
        "#
    )
    .fetch_all(&state.db)
    .await
    .unwrap();

    if rows.is_empty() {
        return;
    }

    let mut leaves = Vec::new();

    for r in &rows {
        let data = format!(
            "{}:{}:{}",
            r.market_id,
            r.outcome,
            r.decided_at
        );

        let hash = hash_leaf(&data);
        leaves.push(hash);
    }

    let root = build_merkle_root(leaves);

    let batch_id = Uuid::new_v4();
    let now = Utc::now();

    let root_hex = hex::encode(root);

    let mut tx = state.db.begin().await.unwrap();

    sqlx::query(
        r#"
        INSERT INTO batches
        (id, merkle_root, created_at)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(&batch_id)
    .bind(&root_hex)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .unwrap();

    for r in rows {
        sqlx::query(
            r#"
            INSERT INTO batch_items
            (batch_id, market_id)
            VALUES ($1, $2)
            "#,
        )
        .bind(&batch_id)
        .bind(&r.market_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    }

    tx.commit().await.unwrap();

    tracing::info!(
        "Created batch {} root={}",
        batch_id,
        root_hex
    );
}

fn try_resolve(values: &[f64]) -> Option<f64> {
    if values.len() < 3 {
        return None;
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let min = sorted[0];
    let max = sorted[sorted.len() - 1];

    let diff = (max - min) / min;

    if diff <= 0.01 {
        let avg = sorted.iter().sum::<f64>() / sorted.len() as f64;
        Some(avg)
    } else {
        None
    }
}

async fn auto_close_markets(state: &AppState) {
    let now = Utc::now();

    let res = sqlx::query!(
        r#"
        UPDATE markets
        SET status = 'CLOSED'
        WHERE status = 'OPEN'
        AND closes_at <= $1
        "#,
        now
    )
    .execute(&state.db)
    .await
    .unwrap();

    if res.rows_affected() > 0 {
        tracing::info!(
            "Auto-closed {} markets",
            res.rows_affected()
        );
    }
}





use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::models::outbox::SettlementPayload;
use crate::proof::{build_merkle_root, hash_leaf};

mod eth;
mod models;
mod proof;
mod worker;

#[derive(Clone)]
struct AppState {
    db: PgPool,
}

#[derive(Serialize)]
struct Market {
    id: Uuid,
    question: String,
    closes_at: DateTime<Utc>,
    status: String,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, Clone)]
struct Report {
    id: Uuid,
    market_id: Uuid,
    source: String,
    value: f64,
    created_at: DateTime<Utc>,
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
    // RFC3339 string from client
    closes_at: String,
}

#[derive(Serialize)]
struct SettlementView {
    market_id: Uuid,
    outcome: f64,
    decided_at: DateTime<Utc>,
    reports: Vec<Report>,
    hash: String,
}

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

    let resolver_state = state.clone();
    tokio::spawn(async move { resolver_loop(resolver_state).await });

    let batch_state = state.clone();
    tokio::spawn(async move { batcher_loop(batch_state).await });

    let worker_state = state.clone();
    tokio::spawn(async move { worker::run_worker(worker_state).await });

    let app = Router::new()
        .route("/health", get(health))
        .route("/markets", post(create_market).get(list_markets))
        .route("/markets/:id/reports", post(create_report).get(list_reports))
        .route("/markets/:id/settlement", get(get_settlement))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    // let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Server running on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health() -> &'static str {
    "OK"
}

async fn create_market(
    State(state): State<AppState>,
    Json(payload): Json<CreateMarketRequest>,
) -> Result<&'static str, (axum::http::StatusCode, String)> {
    let id = Uuid::new_v4();
    let now = Utc::now();

    let closes_at = chrono::DateTime::parse_from_rfc3339(&payload.closes_at)
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?
        .with_timezone(&Utc);

    sqlx::query(
        r#"
        INSERT INTO markets (id, question, closes_at, status, created_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(id)
    .bind(&payload.question)
    .bind(closes_at)
    .bind("OPEN")
    .bind(now)
    .execute(&state.db)
    .await
    .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok("Market created")
}

async fn list_markets(State(state): State<AppState>) -> Json<Vec<Market>> {
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
            id: row.id,
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
    Path(market_id): Path<Uuid>,
    Json(payload): Json<CreateReportRequest>,
) -> Result<&'static str, (axum::http::StatusCode, String)> {
    let id = Uuid::new_v4();
    let now = Utc::now();

    let market = sqlx::query!("SELECT status FROM markets WHERE id = $1", market_id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| (axum::http::StatusCode::NOT_FOUND, "Market not found".to_string()))?;

    if market.status != "OPEN" {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "Market is closed".to_string(),
        ));
    }

    let result = sqlx::query(
        r#"
        INSERT INTO reports (id, market_id, source, value, idempotency_key, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(market_id)
    .bind(&payload.source)
    .bind(payload.value)
    .bind(&payload.idempotency_key)
    .bind(now)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => Ok("Report submitted"),
        Err(e) => {
            // Postgres unique violation SQLSTATE: 23505
            if let Some(db_err) = e.as_database_error() {
                if db_err.code().as_deref() == Some("23505") {
                    return Err((
                        axum::http::StatusCode::CONFLICT,
                        "Duplicate report or idempotency key".to_string(),
                    ));
                }
            }
            Err((axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

async fn list_reports(State(state): State<AppState>, Path(market_id): Path<Uuid>) -> Json<Vec<Report>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, market_id, source, value, created_at
        FROM reports
        WHERE market_id = $1
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
            id: row.id,
            market_id: row.market_id,
            source: row.source,
            value: row.value,
            created_at: row.created_at,
        })
        .collect();

    Json(reports)
}

async fn resolver_loop(state: AppState) {
    loop {
        auto_close_markets(&state).await;
        resolve_markets(&state).await;

        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}

async fn resolve_markets(state: &AppState) {
    let markets = sqlx::query!(
        r#"
        SELECT id, closes_at
        FROM markets
        WHERE status = 'CLOSED'
        LIMIT 10
        "#
    )
    .fetch_all(&state.db)
    .await
    .unwrap();

    let now = Utc::now();

    for market in markets {
        if now < market.closes_at {
            continue;
        }

        let reports = sqlx::query!(r#"SELECT value FROM reports WHERE market_id = $1"#, market.id)
            .fetch_all(&state.db)
            .await
            .unwrap();

        let values: Vec<f64> = reports.into_iter().map(|r| r.value).collect();

        if let Some(outcome) = try_resolve(&values) {
            finalize_market(state, market.id, outcome).await;
        }
    }
}

async fn finalize_market(state: &AppState, market_id: Uuid, outcome: f64) {
    let settlement_id = Uuid::new_v4();
    let now = Utc::now();

    let mut hasher = Sha256::new();
    hasher.update(market_id.as_bytes());
    let market_hash: [u8; 32] = hasher.finalize().into();

    let data = format!("{}:{}:{}", market_id, outcome, now.to_rfc3339());
    let leaf = hash_leaf(&data);

    let outcome_u64 = outcome as u64;
    let ts = now.timestamp() as u64;

    let payload = SettlementPayload {
        market_id: market_id.to_string(),
        market_hash_hex: hex::encode(market_hash),
        leaf_hex: hex::encode(leaf),
        outcome_u64,
        ts,
    };

    let payload_json = serde_json::to_value(&payload).unwrap();

    let mut tx = state.db.begin().await.unwrap();

    sqlx::query(
        r#"
        INSERT INTO settlements (id, market_id, outcome, decided_at)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(settlement_id)
    .bind(market_id)
    .bind(outcome)
    .bind(now)
    .execute(&mut *tx)
    .await
    .unwrap();

    sqlx::query(
        r#"
        UPDATE markets
        SET status = 'RESOLVED'
        WHERE id = $1 AND status = 'CLOSED'
        "#,
    )
    .bind(market_id)
    .execute(&mut *tx)
    .await
    .unwrap();

    let outbox_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO outbox
        (id, market_id, payload, status, retries, last_error, created_at, updated_at)
        VALUES ($1, $2, $3, 'PENDING', 0, NULL, $4, $5)
        "#,
    )
    .bind(outbox_id)
    .bind(market_id)
    .bind(payload_json)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .unwrap();

    tx.commit().await.unwrap();

    tracing::info!("Queued settlement in outbox id={}", outbox_id);
}

async fn get_settlement(
    State(state): State<AppState>,
    Path(market_id): Path<Uuid>,
) -> Result<Json<SettlementView>, axum::http::StatusCode> {
    let settlement = sqlx::query!(
        r#"
        SELECT outcome, decided_at
        FROM settlements
        WHERE market_id = $1
        "#,
        market_id
    )
    .fetch_optional(&state.db)
    .await
    .unwrap()
    .ok_or(axum::http::StatusCode::NOT_FOUND)?;

    let reports_rows = sqlx::query!(
        r#"
        SELECT id, market_id, source, value, created_at
        FROM reports
        WHERE market_id = $1
        ORDER BY created_at ASC
        "#,
        market_id
    )
    .fetch_all(&state.db)
    .await
    .unwrap();

    let reports: Vec<Report> = reports_rows
        .into_iter()
        .map(|r| Report {
            id: r.id,
            market_id: r.market_id,
            source: r.source,
            value: r.value,
            created_at: r.created_at,
        })
        .collect();

    let hash = settlement_hash(market_id, settlement.outcome, settlement.decided_at, &reports);

    Ok(Json(SettlementView {
        market_id,
        outcome: settlement.outcome,
        decided_at: settlement.decided_at,
        reports,
        hash,
    }))
}

fn settlement_hash(
    market_id: Uuid,
    outcome: f64,
    decided_at: DateTime<Utc>,
    reports: &[Report],
) -> String {
    let mut hasher = Sha256::new();

    hasher.update(market_id.as_bytes());
    hasher.update(outcome.to_string().as_bytes());
    hasher.update(decided_at.to_rfc3339().as_bytes());

    for r in reports {
        hasher.update(r.id.as_bytes());
        hasher.update(r.source.as_bytes());
        hasher.update(r.value.to_string().as_bytes());
        hasher.update(r.created_at.to_rfc3339().as_bytes());
    }

    hex::encode(hasher.finalize())
}

async fn collect_unbatched_settlements(state: &AppState) -> Vec<(Uuid, DateTime<Utc>)> {
    let rows = sqlx::query!(
        r#"
        SELECT market_id, decided_at
        FROM settlements
        WHERE market_id NOT IN (
            SELECT market_id FROM batch_items
        )
        "#
    )
    .fetch_all(&state.db)
    .await
    .unwrap();

    rows.into_iter().map(|r| (r.market_id, r.decided_at)).collect()
}

async fn batcher_loop(state: AppState) {
    loop {
        create_batch(&state).await;
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

async fn create_batch(state: &AppState) {
    let rows = sqlx::query!(
        r#"
        SELECT s.market_id, s.outcome, s.decided_at
        FROM settlements s
        LEFT JOIN batch_items b
          ON s.market_id = b.market_id
        WHERE b.market_id IS NULL
        "#
    )
    .fetch_all(&state.db)
    .await
    .unwrap();

    if rows.is_empty() {
        return;
    }

    let mut leaves = Vec::new();
    for r in &rows {
        let data = format!("{}:{}:{}", r.market_id, r.outcome, r.decided_at.to_rfc3339());
        leaves.push(hash_leaf(&data));
    }

    let root = build_merkle_root(leaves);
    let root_hex = hex::encode(root);

    let batch_id = Uuid::new_v4();
    let now = Utc::now();

    let mut tx = state.db.begin().await.unwrap();

    sqlx::query(
        r#"
        INSERT INTO batches (id, merkle_root, created_at)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(batch_id)
    .bind(&root_hex)
    .bind(now)
    .execute(&mut *tx)
    .await
    .unwrap();

    for r in rows {
        sqlx::query(
            r#"
            INSERT INTO batch_items (batch_id, market_id)
            VALUES ($1, $2)
            "#,
        )
        .bind(batch_id)
        .bind(r.market_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    }

    tx.commit().await.unwrap();

    tracing::info!("Created batch {} root={}", batch_id, root_hex);
}

fn try_resolve(values: &[f64]) -> Option<f64> {
    if values.len() < 3 {
        return None;
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let min = sorted[0];
    let max = sorted[sorted.len() - 1];
    let diff = (max - min) / min;

    if diff <= 0.01 {
        let avg = sorted.iter().sum::<f64>() / sorted.len() as f64;
        Some(avg)
    } else {
        None
    }
}

async fn auto_close_markets(state: &AppState) {
    let now = Utc::now();

    let res = sqlx::query!(
        r#"
        UPDATE markets
        SET status = 'CLOSED'
        WHERE status = 'OPEN'
          AND closes_at <= $1
        "#,
        now
    )
    .execute(&state.db)
    .await
    .unwrap();

    if res.rows_affected() > 0 {
        tracing::info!("Auto-closed {} markets", res.rows_affected());
    }
}





