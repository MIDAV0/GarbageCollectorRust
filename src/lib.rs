use std::collections::HashMap;
use alloy::{primitives::Address, signers::local::PrivateKeySigner};
use web3_client::Balance;
use std::fs;
use serde_json::{to_string_pretty, Value};
use eyre::Result;
use reqwest::Url;
use std::io::Write;

mod web3_client;
mod const_types;

struct TokenData {
    chain_id: u32,
    address: Address,
    name: String,
    symbol: String,
    decimals: u8,
    logo_uri: String,
}

// struct NonzeroTokenData {
//     address: String,
//     name: String,
//     symbol: String,
//     decimals: u8,
//     balance: u128,
// }

pub struct GarbageCollector {
    signer: PrivateKeySigner,
    // Map of chains to token data
    token_lists: HashMap<String, Vec<TokenData>>,
    // Map of chains to nonzero tokens that user has
    // nonzero_tokens: HashMap<String, TokenData>,
    // // Vector of chain IDs to exclude from the garbage collection
    // chains_to_exclude: Vec<u32>,
    // // Vector of token addresses to exclude from the garbage collection
    // tokens_to_exclude: Vec<String>,
    // Chain JSON data
    chain_data: Value,
}

impl Default for GarbageCollector {
    fn default() -> Self {
        GarbageCollector {
            signer: PrivateKeySigner::random(),
            token_lists: HashMap::new(),
            // nonzero_tokens: HashMap::new(),
            // chains_to_exclude: Vec::new(),
            // tokens_to_exclude: Vec::new(),
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

    fn fetch_tokens(network_name: String) -> Result<Value> {
        let file_path = format!("data/token_lists/{}.json", network_name);
        // Dont panic if file is not found
        let contents = fs::read_to_string(file_path)?;
        let v: Value = serde_json::from_str(&contents)?;
        Ok(v)
    }

    fn write_to_json_file(filename: &str, data: &HashMap<String, Vec<Balance>>) -> Result<()> {
        let json = to_string_pretty(&data)?;
        let mut file = fs::File::create(filename)?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    pub async fn get_non_zero_tokens(&mut self) -> Result<()> {
        let mut results: HashMap<String, Vec<Balance>> = HashMap::new();
        for (k, _) in self.chain_data.as_object().unwrap() {
            let token_list_result = GarbageCollector::fetch_tokens(k.to_string());

            let token_list = match token_list_result {
                Ok(t_l) =>t_l,
                Err(_) => continue,
            };
            let converted_token_list: Vec<TokenData> = token_list.as_array().unwrap().iter().map(|token| {
                TokenData {
                    chain_id: token["chainId"].as_u64().unwrap() as u32,
                    address: token["address"].as_str().unwrap().parse::<Address>().unwrap_or(Address::ZERO),
                    name: token["name"].as_str().unwrap().to_string(),
                    symbol: token["symbol"].as_str().unwrap().to_string(),
                    decimals: token["decimals"].as_u64().unwrap() as u8,
                    logo_uri: token["logoURI"].as_str().unwrap_or("").to_string(),
                }
            }).collect();
            self.token_lists.insert(k.to_string(), converted_token_list);
            let res = self.get_non_zero_tokens_for_chain(k, Some("0xBF17a4730Fe4a1ea36Cf536B8473Cc25ba146F19".to_owned())).await;
            let balance_list = match res {
                Ok(b_l) => b_l,
                Err(e) => {
                    println!("Error getting balance list: {:?}", e);
                    continue;
                }
            };
            if balance_list.len() > 0 {
                results.insert(k.to_string(), balance_list);
            }
        }
        let filename = "results/nonzero_tokens.json";
        Self::write_to_json_file(filename, &results)?;
        Ok(())
    }

    async fn get_non_zero_tokens_for_chain(&self, network_name: &String, target_wallet_: Option<String>) -> Result<Vec<Balance>> {
        println!("Getting non-zero tokens for chain {}", network_name);
        let mut web3_client = web3_client::Web3Client::new(network_name, Some(self.signer.clone())).unwrap();
        let target_wallet = match target_wallet_ {
            Some(t_w) => t_w.parse::<Address>().unwrap(),
            None => self.signer.address(),
        };
        web3_client.set_network_rpc(web3_client::Network::new( 
            self.chain_data[network_name]["id"].as_i64().unwrap() as i32,
            self.chain_data[network_name]["lz_id"].to_string(),
            Url::parse(self.chain_data[network_name]["rpc"][0].as_str().unwrap()).unwrap(),
            self.chain_data[network_name]["explorer"].to_string(),
            self.chain_data[network_name]["multicall"].as_str().unwrap().parse::<Address>().unwrap_or(Address::ZERO),
        ));
        
        let token_addresses = self.token_lists.get(network_name).unwrap_or(&vec![]).iter().map(|token| token.address).collect();
        let balance_list =  match web3_client.call_balance(target_wallet, token_addresses).await {
            Ok(b_l) => b_l,
            Err(e) => {
                println!("Error getting balance list: {:?}", e);
                return Err(eyre::eyre!("Error getting balance list"));
            }
        };
        Ok(balance_list)
    }
}


#[test]
fn test_json_parser() {
    let result = GarbageCollector::parse_json_chains();
    assert_eq!(result.is_ok(), true);
}

#[tokio::test]
async fn test_get_non_zero_tokens() {
    let mut garbage_collector = GarbageCollector::new();
    let _ = garbage_collector.get_non_zero_tokens().await.unwrap();
}