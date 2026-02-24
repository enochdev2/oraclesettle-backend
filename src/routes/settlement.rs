use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::state::AppState;
use crate::types::{Report, SettlementView};

pub async fn get_settlement(
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