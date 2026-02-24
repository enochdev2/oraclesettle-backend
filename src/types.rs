use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize)]
pub struct Market {
    pub id: Uuid,
    pub question: String,
    pub closes_at: DateTime<Utc>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Clone)]
pub struct Report {
    pub id: Uuid,
    pub market_id: Uuid,
    pub source: String,
    pub value: f64,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateReportRequest {
    pub source: String,
    pub value: f64,
    pub idempotency_key: String,
}

#[derive(Deserialize)]
pub struct CreateMarketRequest {
    pub question: String,
    // RFC3339 string from client
    pub closes_at: String,
}

#[derive(Serialize)]
pub struct SettlementView {
    pub market_id: Uuid,
    pub outcome: f64,
    pub decided_at: DateTime<Utc>,
    pub reports: Vec<Report>,
    pub hash: String,
}