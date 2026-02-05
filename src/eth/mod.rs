// backend/src/eth/mod.rs

use ethers::prelude::*;

pub mod submit;
pub mod client;

abigen!(
    OracleSettle,
    "./abi/OracleSettle.json"
);
