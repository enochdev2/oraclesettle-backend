use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::State,
};
use sha2::{Sha256, Digest};
use axum::extract::Path;

use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::net::SocketAddr;
use tracing_subscriber;
use uuid::Uuid;
use chrono::Utc;
use crate::proof::{hash_leaf, build_merkle_root};
use crate::eth::submit::submit_settlement;



mod proof;
mod eth;


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

#[derive(Serialize)]
struct Settlement {
    id: String,
    market_id: String,
    outcome: f64,
    decided_at: String,
}

#[derive(Serialize)]
struct SettlementView {
    market_id: String,
    outcome: f64,
    decided_at: String,
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

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
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



    let app = Router::new()
        .route("/health", get(health))
        .route("/markets", post(create_market).get(list_markets))
        .route("/markets/:id/reports", post(create_report).get(list_reports))
        .route("/markets/:id/settlement", get(get_settlement),)
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

    let market = sqlx::query!(
        "SELECT status FROM markets WHERE id = ?",
        market_id
    )
    .fetch_one(&state.db)
    .await
    .map_err(|_| (
        axum::http::StatusCode::NOT_FOUND,
        "Market not found".to_string(),
    ))?;

    if market.status != "OPEN" {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "Market is closed".to_string(),
        ));
    }


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
    // Get up to 10 open markets
    let markets = sqlx::query!(
        r#"
        SELECT id, closes_at FROM markets
        WHERE status = 'CLOSED'
        LIMIT 10
        "#
    )
    .fetch_all(&state.db)
    .await
    .unwrap();

    let now = Utc::now();

    for market in markets {
        // Parse closes_at for THIS market (closes_at is non-nullable TEXT)
        let closes_at = chrono::DateTime::parse_from_rfc3339(&market.closes_at)
            .unwrap()
            .with_timezone(&Utc);

        // Skip if market hasn't closed yet
        if now < closes_at {
            continue;
        }

        // Fetch reports
        let reports = sqlx::query!(
            r#"
            SELECT value FROM reports
            WHERE market_id = ?
            "#,
            market.id
        )
        .fetch_all(&state.db)
        .await
        .unwrap();

        let values: Vec<f64> = reports.into_iter().map(|r| r.value).collect();

        if let Some(outcome) = try_resolve(&values) {
            if let Some(id) = market.id.as_deref() {
                finalize_market(state, id, outcome).await;
            }
        }
    }
}


async fn finalize_market(
    state: &AppState,
    market_id: &str,
    outcome: f64,
) {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let mut tx = state.db.begin().await.unwrap();

    sqlx::query(
        r#"
        INSERT INTO settlements
        (id, market_id, outcome, decided_at)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(market_id)
    .bind(outcome)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .unwrap();

    sqlx::query(
        r#"
        UPDATE markets
        SET status = 'RESOLVED'
        WHERE id = ? AND status = 'CLOSED'
        "#,
    )
    .bind(market_id)
    .execute(&mut *tx)
    .await
    .unwrap();

    tx.commit().await.unwrap();

    // use sha2::{Sha256, Digest};

    let mut hasher = Sha256::new();
    hasher.update(market_id.as_bytes());
    let market_hash: [u8; 32] = hasher.finalize().into();

    // Temporary root (single leaf for now)
    let data = format!("{}:{}:{}", market_id, outcome, now);
    let leaf = hash_leaf(&data);

    // Convert outcome
    let outcome_u64 = outcome as u64;

    // Convert timestamp
    let ts = chrono::DateTime::parse_from_rfc3339(&now)
        .unwrap()
        .timestamp() as u64;

    // Send to chain (fire and log)
    match submit_settlement(
        market_hash,
        leaf,
        outcome_u64,
        ts,
    ).await {
        Ok(_) => {
            tracing::info!("Settlement submitted on-chain");
        }
        Err(e) => {
            tracing::error!("Chain submit failed: {:?}", e);
        }
    }


    tracing::info!(
        "Market {} resolved: {}",
        market_id,
        outcome
    );
}

async fn get_settlement(
    State(state): State<AppState>,
    Path(market_id): Path<String>,
) -> Result<Json<SettlementView>, axum::http::StatusCode> {
    let settlement = sqlx::query!(
        r#"
        SELECT outcome, decided_at
        FROM settlements
        WHERE market_id = ?
        "#,
        market_id
    )
    .fetch_optional(&state.db)
    .await
    .unwrap();

    let settlement = match settlement {
        Some(s) => s,
        None => return Err(axum::http::StatusCode::NOT_FOUND),
    };

    let reports = sqlx::query!(
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

    let reports: Vec<Report> = reports
        .into_iter()
        .map(|r| Report {
            id: r.id.unwrap_or_default(),
            market_id: r.market_id,
            source: r.source,
            value: r.value,
            created_at: r.created_at,
        })
        .collect();

    let hash = settlement_hash(
        &market_id,
        settlement.outcome,
        &settlement.decided_at,
        &reports,
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

    let batch_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let root_hex = hex::encode(root);

    let mut tx = state.db.begin().await.unwrap();

    sqlx::query(
        r#"
        INSERT INTO batches
        (id, merkle_root, created_at)
        VALUES (?, ?, ?)
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
            VALUES (?, ?)
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
    let now = Utc::now().to_rfc3339();

    let res = sqlx::query!(
        r#"
        UPDATE markets
        SET status = 'CLOSED'
        WHERE status = 'OPEN'
        AND closes_at <= ?
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



// let listener = tokio::net::TcpListener::bind(&addr)
//     .await
//     .unwrap();

// axum::serve(listener, app)
//     .await
//     .unwrap();