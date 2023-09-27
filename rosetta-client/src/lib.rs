//! Rosetta client.
#![deny(missing_docs)]
use anyhow::Result;

pub use crate::wallet::Wallet;
pub use rosetta_core::{crypto, types, BlockchainConfig};

mod client;
mod mnemonic;
mod signer;
mod tx_builder;
mod wallet;

pub use signer::Signer;

/// Supported chains.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Blockchain {
    /// Bitcoin
    Bitcoin,
    /// Ethereum
    Ethereum,
    /// Astar
    Astar,
    /// Polkadot
    Polkadot,
}

impl std::str::FromStr for Blockchain {
    type Err = anyhow::Error;

    fn from_str(blockchain: &str) -> Result<Self> {
        Ok(match blockchain {
            "bitcoin" => Self::Bitcoin,
            "ethereum" => Self::Ethereum,
            "astar" => Self::Astar,
            "polkadot" => Self::Polkadot,
            _ => anyhow::bail!("unsupported blockchain {}", blockchain),
        })
    }
}
