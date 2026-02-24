use axum::{
    extract::{Path, State},
    Json,
};
use chrono::Utc;
use uuid::Uuid;

use crate::state::AppState;
use crate::types::{CreateReportRequest, Report};

pub async fn create_report(
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

pub async fn list_reports(
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