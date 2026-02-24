use axum::{extract::State, Json};
use chrono::Utc;
use sqlx::Row;
use uuid::Uuid;

use crate::state::AppState;
use crate::types::{CreateMarketRequest, Market};

pub async fn create_market(
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

pub async fn list_markets(State(state): State<AppState>) -> Json<Vec<Market>> {
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