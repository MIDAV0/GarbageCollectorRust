use std::collections::HashMap;

mod web3Client;
mod const_types;

struct TokenData {
    chain_id: u32,
    address: String,
    name: String,
    symbol: String,
    decimals: u8,
    logo_uri: String,
}

struct NonzeroTokenData {
    address: String,
    name: String,
    symbol: String,
    decimals: u8,
    balance: u128,
}

#[derive(Default)]
struct GarbageCollector {
    signer: String,
    // Map of chains to token data
    token_lists: HashMap<String, TokenData>,
    // Map of chains to nonzero tokens that user has
    nonzero_tokens: HashMap<String, TokenData>,
    // Vector of chain IDs to exclude from the garbage collection
    chains_to_exclude: Vec<u32>,
    // Vector of token addresses to exclude from the garbage collection
    tokens_to_exclude: Vec<String>,
}

impl GarbageCollector {
    pub fn new(
        signer: String,
    ) -> Self {
        GarbageCollector {
            signer,
            ..Default::default()
        }
    }
}