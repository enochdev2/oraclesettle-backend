// backend/src/eth/client.rs

use ethers::prelude::*;
use std::sync::Arc;
use anyhow::Result;
use super::OracleSettle;

pub async fn eth_client() -> Result<OracleSettle<SignerMiddleware<Provider<Http>, Wallet<k256::ecdsa::SigningKey>>>> {

    let rpc = std::env::var("RPC_URL")?;
    let key = std::env::var("PRIVATE_KEY")?;
    let addr = std::env::var("CONTRACT_ADDRESS")?;

    let provider = Provider::<Http>::try_from(rpc)?;
    let wallet: LocalWallet = key.parse()?;

    let chain_id: u64 = std::env::var("CHAIN_ID")?.parse()?;
    let wallet = wallet.with_chain_id(chain_id);

    let client = SignerMiddleware::new(provider, wallet);
    let client = Arc::new(client);

    let address: Address = addr.parse()?;

    Ok(OracleSettle::new(address, client))
}
