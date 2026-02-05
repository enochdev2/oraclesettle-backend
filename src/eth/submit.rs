// backend/src/eth/submit.rs

use super::client::eth_client;
use anyhow::Result;

pub async fn submit_settlement(
    market_id: [u8; 32],
    root: [u8; 32],
    outcome: u64,
    decided_at: u64,
) -> Result<()> {
    let contract = eth_client().await?;

    let receipt = contract
        .submit_settlement(
            market_id.into(),
            root.into(),
            outcome.into(),
            decided_at.into(),
        )
        .send()
        .await?
        .await?;

    if let Some(receipt) = receipt {
        println!("TX confirmed: {:?}", receipt.transaction_hash);
    }

    Ok(())
}
