use std::collections::HashMap;
use alloy::signers::local::PrivateKeySigner;
use const_types::ChainName;
use std::{fs, io};
use serde_json::Value;
use eyre::Result;

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
    token_lists: HashMap<String, Vec<TokenData>>,
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

    // Connect signer to the garbage collector
    pub fn connect_signer(&mut self, signer_: PrivateKeySigner) {
        self.signer = signer_;
    }

    // Parse JSON file with chain data
    fn parse_json_chains() -> Result<Value> {
        let file_path = "data/chains.json".to_owned();
        let contents = fs::read_to_string(file_path).expect("Couldn't find or load that file.");
        let v: Value = serde_json::from_str(&contents)?;
        Ok(v)
    }

    fn fetch_tokens(network_name: String) -> Option<Value> {
        let file_path = format!("data/token_lists/{}.json", network_name);
        // Dont panic if file is not found
        let contents = fs::read_to_string(file_path);
        if let Ok(contents) = contents {
            serde_json::from_str(&contents).ok()
        } else {
            None
        }
    }

    fn test_parser() -> Result<()> {
        let entries = fs::read_dir("data/token_lists").unwrap();

        // Extract the filenames from the directory entries and store them in a vector
        let file_names: Vec<String> = entries
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                if path.is_file() {
                    path.file_name()?.to_str().map(|s| s.to_owned())
                } else {
                    None
                }
            })
            .collect();
        println!("{:?}", file_names);
        Ok(())
    }

    pub fn get_non_zero_tokens(&mut self) -> Result<()> {
        for (k,v) in self.chain_data.as_object().unwrap() {
            let token_list = GarbageCollector::fetch_tokens(k.to_string());

            // Continue if token list is None
            if let Some(t_l) = token_list {
                let converted_token_list: Vec<TokenData> = t_l.as_array().unwrap().iter().map(|token| {
                    TokenData {
                        chain_id: token["chainId"].as_u64().unwrap() as u32,
                        address: token["address"].as_str().unwrap().to_string(),
                        name: token["name"].as_str().unwrap().to_string(),
                        symbol: token["symbol"].as_str().unwrap().to_string(),
                        decimals: token["decimals"].as_u64().unwrap() as u8,
                        logo_uri: token["logoURI"].as_str().unwrap_or("").to_string(),
                    }
                }).collect();
                self.token_lists.insert(k.to_string(), converted_token_list);
            }
        }
        Ok(())
    }

    fn get_non_zero_tokens_for_chain(&self, network_name: &str) {
        let chain_rpc = self.chain_data[network_name]["rpc"][0].as_str().unwrap();
        let mut web3_client = web3Client::Web3Client::new(network_name, Some(self.signer.clone())).unwrap();
        web3_client.set_network_rpc(chain_rpc);
    }
}


#[test]
fn test_json_parser() {
    let result = GarbageCollector::parse_json_chains();
    assert_eq!(result.is_ok(), true);
}

#[test]
fn test_get_non_zero_tokens() {
    let mut garbage_collector = GarbageCollector::new();
    garbage_collector.get_non_zero_tokens();
}

#[test]
fn test_parser() {
    let result = GarbageCollector::test_parser();
    assert_eq!(result.is_ok(), true);
}