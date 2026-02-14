use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SettlementPayload {
    pub market_id: String,
    pub market_hash_hex: String,
    pub leaf_hex: String,
    pub outcome_u64: u64,
    pub ts: u64,
}
