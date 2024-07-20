use std::collections::HashMap;
use alloy::signers::local::PrivateKeySigner;
use std::fs;
use serde_json::{Result, Value};

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

pub struct GarbageCollector {
    signer: PrivateKeySigner,
    // Map of chains to token data
    token_lists: HashMap<String, TokenData>,
    // Map of chains to nonzero tokens that user has
    nonzero_tokens: HashMap<String, TokenData>,
    // Vector of chain IDs to exclude from the garbage collection
    chains_to_exclude: Vec<u32>,
    // Vector of token addresses to exclude from the garbage collection
    tokens_to_exclude: Vec<String>,
    // Chain JSON data
    chain_data: Value,
}

impl Default for GarbageCollector {
    fn default() -> Self {
        GarbageCollector {
            signer: PrivateKeySigner::random(),
            token_lists: HashMap::new(),
            nonzero_tokens: HashMap::new(),
            chains_to_exclude: Vec::new(),
            tokens_to_exclude: Vec::new(),
            chain_data: Value::Null,
        }
    }
}

impl GarbageCollector {
    pub fn new() -> Self {
        let chain_data = GarbageCollector::parse_json_chains().unwrap();
        GarbageCollector {
            chain_data,
            ..Default::default()
        }
    }

    pub fn connect_signer(&mut self, signer_: PrivateKeySigner) {
        self.signer = signer_;
    }

    fn parse_json_chains() -> Result<Value> {
        let file_path = "data/chains.json".to_owned();
        let contents = fs::read_to_string(file_path).expect("Couldn't find or load that file.");
        let v: Value = serde_json::from_str(&contents)?;
        Ok(v)
    }
}


#[test]
fn test_json_parser() {
    let result = GarbageCollector::parse_json_chains();
    assert_eq!(result.is_ok(), true);
}